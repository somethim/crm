//! The orders workspace state: the open document tabs, the active one, and the
//! Slint models mirrored from them onto `OrdersBridge`.
//!
//! Every handler goes through [`Ws`] (`Rc<RefCell<Workspace>>`); borrows are
//! never held across a call that may come back into the workspace (dialogs,
//! the product editor).

use super::grid_nav::Pos;
use super::grid_state::{GridState, Line, price_text, qty_text};
use super::tabs::DocTab;
use super::texts;
use crate::{AppWindow, GridRow, OrdersBridge, SuggestionItem, TabItem, UiContext};
use crm_services::domain::{OrderId, OrderKind, ProductRow, format};
use slint::{ComponentHandle, Model, ModelRc, StandardListViewItem, VecModel};
use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

/// The shared workspace handle every callback holds.
pub type Ws = Rc<RefCell<Workspace>>;

/// Drafts are written this long after the last edit.
pub const DRAFT_DEBOUNCE: Duration = Duration::from_millis(500);
/// The list search text applies this long after the last keystroke.
pub const FILTER_DEBOUNCE: Duration = Duration::from_millis(200);
/// Two clicks on the same list row within this time open it.
pub const DOUBLE_CLICK: Duration = Duration::from_millis(500);
/// Suggestions fetched per query.
pub const SUGGESTION_LIMIT: usize = 50;

/// An uncommitted cell text (typed, not yet committed by Enter, Tab or focus-out).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    /// The cell.
    pub pos: Pos,
    /// Its text.
    pub text: String,
}

/// The orders workspace.
pub struct Workspace {
    /// This workspace (for timers started from `&mut self`).
    pub(crate) me: Weak<RefCell<Workspace>>,
    pub(crate) ctx: Weak<UiContext>,
    pub(crate) app: slint::Weak<AppWindow>,
    /// Open tabs, left to right.
    pub tabs: Vec<DocTab>,
    /// The selected tab.
    pub active: Option<usize>,
    pub(crate) tab_model: Rc<VecModel<TabItem>>,
    pub(crate) rows: Rc<VecModel<GridRow>>,
    pub(crate) product_items: Rc<VecModel<SuggestionItem>>,
    pub(crate) owner_items: Rc<VecModel<SuggestionItem>>,
    pub(crate) list_model: Rc<VecModel<ModelRc<StandardListViewItem>>>,
    /// The order id of each list row.
    pub list_ids: Vec<OrderId>,
    /// The list's header sort (column, ascending), kept until the program
    /// ends; `None` = the default `issued_on DESC, id DESC`.
    pub list_sort: Option<(usize, bool)>,
    /// The products behind `product_items`.
    pub(crate) product_hits: Vec<ProductRow>,
    /// The owners behind `owner_items`.
    pub(crate) owner_hits: Vec<crm_services::domain::Owner>,
    /// The focused grid cell, as reported by the grid.
    pub focus: Option<Pos>,
    /// The grid cell that had the focus last; unlike `focus` it survives the
    /// focus moving to a dialog or overlay, which then hands it back (F-008).
    pub last_focus: Option<Pos>,
    /// The typed text of a cell, until it is committed.
    pub pending: Option<Pending>,
    serial: i32,
    rev: i32,
    pub(crate) draft_timer: slint::Timer,
    pub(crate) filter_timer: slint::Timer,
    pub(crate) last_click: Option<(usize, Instant)>,
}

impl Workspace {
    /// An empty workspace with its models installed on the bridge.
    pub fn new(ctx: &Rc<UiContext>, app: &AppWindow) -> Ws {
        let ws = Workspace {
            me: Weak::new(),
            ctx: Rc::downgrade(ctx),
            app: app.as_weak(),
            tabs: Vec::new(),
            active: None,
            tab_model: Rc::new(VecModel::default()),
            rows: Rc::new(VecModel::default()),
            product_items: Rc::new(VecModel::default()),
            owner_items: Rc::new(VecModel::default()),
            list_model: Rc::new(VecModel::default()),
            list_ids: Vec::new(),
            list_sort: None,
            product_hits: Vec::new(),
            owner_hits: Vec::new(),
            focus: None,
            last_focus: None,
            pending: None,
            serial: 0,
            rev: 0,
            draft_timer: slint::Timer::default(),
            filter_timer: slint::Timer::default(),
            last_click: None,
        };
        let bridge = app.global::<OrdersBridge>();
        bridge.set_tabs(ModelRc::from(ws.tab_model.clone()));
        bridge.set_grid_rows(ModelRc::from(ws.rows.clone()));
        bridge.set_product_items(ModelRc::from(ws.product_items.clone()));
        bridge.set_owner_items(ModelRc::from(ws.owner_items.clone()));
        bridge.set_list_rows(ModelRc::from(ws.list_model.clone()));
        let ws = Rc::new(RefCell::new(ws));
        ws.borrow_mut().me = Rc::downgrade(&ws);
        ws
    }

    /// Marks the active document edited: "•" on its tab, and a draft write
    /// 500 ms after the last edit.
    pub fn touch(&mut self) {
        let Some(tab) = self.current_mut() else {
            return;
        };
        let was = tab.dirty;
        tab.dirty = true;
        tab.unsynced = true;
        if !was {
            self.sync_tabs();
        }
        let weak = self.me.clone();
        self.draft_timer
            .start(slint::TimerMode::SingleShot, DRAFT_DEBOUNCE, move || {
                if let Some(ws) = weak.upgrade()
                    && let Ok(mut w) = ws.try_borrow_mut()
                {
                    w.flush();
                }
            });
    }

    /// The UI context, while it exists.
    pub fn ctx(&self) -> Option<Rc<UiContext>> {
        self.ctx.upgrade()
    }

    /// Runs `f` with the bridge, while the main window exists.
    pub fn bridge<R>(&self, f: impl FnOnce(&OrdersBridge<'_>) -> R) -> Option<R> {
        let app = self.app.upgrade()?;
        let bridge = app.global::<OrdersBridge>();
        Some(f(&bridge))
    }

    /// The active tab.
    pub fn current(&self) -> Option<&DocTab> {
        self.active.and_then(|i| self.tabs.get(i))
    }

    /// The active tab, mutable.
    pub fn current_mut(&mut self) -> Option<&mut DocTab> {
        self.active.and_then(|i| self.tabs.get_mut(i))
    }

    /// The index of the tab with `key`.
    pub fn index_of(&self, key: &str) -> Option<usize> {
        self.tabs.iter().position(|t| t.key == key)
    }

    /// The index of the tab showing saved invoice `id`.
    pub fn index_of_order(&self, id: OrderId) -> Option<usize> {
        self.tabs.iter().position(|t| t.order_id == Some(id))
    }

    /// Asks the grid to focus `pos` (the cell focuses itself when the serial moves).
    pub fn focus_cell(&mut self, pos: Pos) {
        self.serial = self.serial.wrapping_add(1);
        let serial = self.serial;
        self.bridge(|b| {
            b.set_grid_focus_row(to_i32(pos.row));
            b.set_grid_focus_col(to_i32(pos.col));
            b.set_grid_focus_serial(serial);
        });
    }

    /// Gives the focus back to the grid cell that had it last (clamped to the
    /// rows shown), after a dialog, the busy overlay or the lock took it.
    pub fn refocus(&mut self) {
        let Some(pos) = self.last_focus else { return };
        let Some(tab) = self.current() else { return };
        let pos = Pos::new(pos.row.min(tab.grid.rows() - 1), pos.col);
        self.last_focus = Some(pos);
        self.focus_cell(pos);
    }

    /// Asks the document to focus the owner field.
    pub fn focus_owner(&mut self) {
        self.bridge(|b| b.set_owner_focus_serial(b.get_owner_focus_serial().wrapping_add(1)));
    }

    /// Ctrl+O (Kërko porosi): the order list's Kërko field takes the focus with
    /// its text selected.
    pub fn focus_search(&self) {
        self.bridge(|b| b.set_search_focus_serial(b.get_search_focus_serial().wrapping_add(1)));
    }

    // -- mirroring onto the bridge -------------------------------------------------

    /// Mirrors the tab bar and the shell flags.
    pub fn sync_tabs(&self) {
        let items: Vec<TabItem> = self
            .tabs
            .iter()
            .map(|t| TabItem {
                title: t.title.clone().into(),
                dirty: t.dirty,
            })
            .collect();
        sync_vec(&self.tab_model, items);
        let active = self.active.map_or(-1, to_i32);
        let saved = self.current().is_some_and(|t| t.order_id.is_some());
        let count = to_i32(self.tabs.len());
        let has = self.current().is_some();
        self.bridge(|b| {
            b.set_active_tab(active);
            b.set_has_document(has);
            b.set_current_is_saved(saved);
            b.set_open_tab_count(count);
        });
    }

    /// Mirrors the active document (header, rows, total) after a tab switch.
    pub fn show_current(&mut self) {
        self.focus = None;
        self.last_focus = None;
        self.pending = None;
        self.close_dropdown();
        let Some(tab) = self.current() else {
            self.rows.set_vec(Vec::new());
            return;
        };
        let header = tab.header.clone();
        let kind = tab.kind;
        let number = tab
            .order_id
            .map_or_else(|| texts::NO_NUMBER.to_owned(), |id| id.0.to_string());
        let rows = self.grid_rows(&tab.grid);
        self.rows.set_vec(rows);
        self.bridge(|b| {
            b.set_doc_kind(i32::from(kind == OrderKind::Purchase));
            b.set_doc_number(number.into());
            b.set_owner_text(header.owner_text.clone().into());
            b.set_owner_nipt(header.nipt.clone().into());
            b.set_owner_address(header.address.clone().into());
            b.set_owner_detail(texts::owner_detail(&header.nipt, &header.address).into());
            b.set_doc_description(header.description.clone().into());
            b.set_doc_date(format::date(header.date).into());
            b.set_owner_open(false);
            b.set_owner_highlighted(-1);
            b.set_focused_row(-1);
            b.set_focused_col(-1);
        });
        self.owner_items.set_vec(Vec::new());
        self.sync_total();
    }

    /// Mirrors the header's owner fields of the active document.
    pub fn sync_owner(&self) {
        let Some(tab) = self.current() else { return };
        let h = tab.header.clone();
        self.bridge(|b| {
            b.set_owner_text(h.owner_text.into());
            b.set_owner_detail(texts::owner_detail(&h.nipt, &h.address).into());
            b.set_owner_nipt(h.nipt.into());
            b.set_owner_address(h.address.into());
        });
    }

    /// Mirrors "Totali".
    pub fn sync_total(&self) {
        let total = self.current().map_or(String::new(), |t| {
            texts::total(&format::money(t.grid.total()))
        });
        self.bridge(|b| b.set_doc_total(total.into()));
    }

    /// Mirrors the active grid's rows, updating only the rows that changed;
    /// `force` re-sends one row even when equal (its cell then re-shows the
    /// committed text instead of what was typed).
    pub fn sync_rows(&mut self, force: Option<usize>) {
        let Some(tab) = self.current() else { return };
        let mut wanted = self.grid_rows(&tab.grid);
        for (i, row) in wanted.iter_mut().enumerate() {
            let old = self.rows.row_data(i);
            let same = old.as_ref().is_some_and(|o| same_row(o, row));
            if same && force != Some(i) {
                if let Some(o) = old {
                    row.rev = o.rev;
                }
                continue;
            }
            self.rev = self.rev.wrapping_add(1);
            row.rev = self.rev;
            if i < self.rows.row_count() {
                self.rows.set_row_data(i, row.clone());
            } else {
                self.rows.push(row.clone());
            }
        }
        while self.rows.row_count() > wanted.len() {
            self.rows.remove(self.rows.row_count() - 1);
        }
        self.sync_total();
    }

    fn grid_rows(&self, grid: &GridState) -> Vec<GridRow> {
        let kind = grid.kind();
        let mut rows: Vec<GridRow> = grid
            .entries()
            .iter()
            .enumerate()
            .map(|(i, line)| entry_row(kind, i, line, grid))
            .collect();
        rows.push(GridRow {
            placeholder: true,
            ..GridRow::default()
        });
        rows
    }

    /// Closes the product suggestions.
    pub fn close_dropdown(&mut self) {
        self.product_hits.clear();
        self.bridge(|b| {
            b.set_dropdown_open(false);
            b.set_product_highlighted(-1);
        });
    }

    /// Fills the product suggestions under `row`; `total` products match in all.
    pub fn show_products(&mut self, row: usize, hits: Vec<ProductRow>, total: usize) {
        let items: Vec<SuggestionItem> = hits
            .iter()
            .map(|p| SuggestionItem {
                label: p.product.name.clone().into(),
                note: slint::SharedString::new(),
                detail: format::qty(p.sku.quantity).into(),
            })
            .collect();
        let count = if hits.is_empty() {
            String::new()
        } else {
            texts::suggestion_count(hits.len(), total)
        };
        self.product_hits = hits;
        self.product_items.set_vec(items);
        self.bridge(|b| {
            b.set_product_count_text(count.into());
            b.set_dropdown_row(to_i32(row));
            b.set_dropdown_open(true);
            b.set_product_highlighted(-1);
        });
    }
}

fn same_row(a: &GridRow, b: &GridRow) -> bool {
    GridRow {
        rev: 0,
        ..a.clone()
    } == GridRow {
        rev: 0,
        ..b.clone()
    }
}

fn entry_row(kind: OrderKind, i: usize, line: &Line, grid: &GridState) -> GridRow {
    let free_text = line.product_id.is_none() && !line.name.trim().is_empty() && !line.orphan;
    let stock = match (line.stock, kind) {
        _ if line.orphan => texts::DELETED_PRODUCT.to_owned(),
        (Some(n), _) => texts::stock(n),
        (None, OrderKind::Purchase) if free_text => texts::stock(Default::default()),
        _ => String::new(),
    };
    let (price, sell) = match kind {
        // A sale price of 0 shows as 0 (free goods, F-012); an empty row blank.
        OrderKind::Sale if line.is_empty() => (String::new(), String::new()),
        OrderKind::Sale => (format::amount(line.price), String::new()),
        OrderKind::Purchase => (price_text(line.buy), price_text(line.sell)),
    };
    let value = if line.is_empty() {
        String::new()
    } else {
        format::amount(grid.value(i))
    };
    GridRow {
        number: (i + 1).to_string().into(),
        product: line.name.clone().into(),
        qty: qty_text(line.qty).into(),
        price: price.into(),
        sell: sell.into(),
        value: value.into(),
        stock: stock.into(),
        stock_link: line.product_id.is_some(),
        stock_warn: line.orphan || (free_text && kind == OrderKind::Purchase),
        locked: line.orphan,
        placeholder: false,
        rev: 0,
    }
}

/// Replaces a model's rows only where they differ.
pub fn sync_vec<T: Clone + PartialEq + 'static>(model: &VecModel<T>, items: Vec<T>) {
    for (i, item) in items.iter().enumerate() {
        match model.row_data(i) {
            Some(old) if old == *item => {}
            Some(_) => model.set_row_data(i, item.clone()),
            None => model.push(item.clone()),
        }
    }
    while model.row_count() > items.len() {
        model.remove(model.row_count() - 1);
    }
}

/// A row or column index for Slint.
pub fn to_i32(n: usize) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX)
}

/// A Slint row or column index (negative = none).
pub fn to_usize(n: i32) -> Option<usize> {
    usize::try_from(n).ok()
}

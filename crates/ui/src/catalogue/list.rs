//! The list-window controller shared by Artikujt, Klientët and Furnitorët:
//! loading (search or the full list), sorting, selection kept by id, the
//! 150 ms search debounce, the toolbar and keys, and the data-change refresh.

use crate::catalogue::models::{self, Row};
use crate::catalogue::{ListKind, owners, products, transfer};
use crate::shell::modal::{BusyOwner, ModalOwner};
use crate::shell::win;
use crate::{CatalogueList, ClientsWindow, ProductsWindow, SuppliersWindow, UiContext};
use crm_services::ServiceError;
use slint::{ComponentHandle, Model, ModelRc, SharedString, StandardListViewItem, VecModel};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

/// The search debounce.
pub const SEARCH_DEBOUNCE: Duration = Duration::from_millis(150);

/// Two presses on the same row within this time are a double click. The
/// window is a single-shot `slint::Timer`, so it runs on Slint's clock (the
/// testing backend's mock time in headless tests), not on `Instant::now()`.
pub const DOUBLE_CLICK: Duration = Duration::from_millis(500);

/// Results asked from the search index for a non-empty query.
pub const SEARCH_LIMIT: usize = 1_000;

/// The Slint side of a list window (implemented for the three window types).
/// Each window instance has its own `CatalogueList` global.
pub trait ListWindow: ModalOwner + Sized {
    /// Which list this window shows.
    const KIND: ListKind;
    /// Creates the window.
    fn create() -> Result<Self, slint::PlatformError>;
    /// The window's state and callbacks.
    fn list(&self) -> CatalogueList<'_>;
}

fn connect<W: ListWindow>(window: &W, ctrl: &Weak<ListController<W>>) {
    let list = window.list();
    let on = |f: fn(&Rc<ListController<W>>)| {
        let ctrl = ctrl.clone();
        move || {
            if let Some(c) = ctrl.upgrade() {
                f(&c);
            }
        }
    };
    list.on_add(on(ListController::add));
    list.on_edit(on(ListController::edit));
    list.on_remove(on(ListController::remove));
    list.on_import(on(ListController::import));
    list.on_export(on(ListController::export));
    list.on_refresh(on(ListController::reload));
    list.on_close_window(on(ListController::close));
    list.on_search_edited(on_text(ctrl, ListController::search_later));
    list.on_search_accepted(on_text(ctrl, |c| {
        c.debounce.stop();
        c.reload();
    }));
    let c = ctrl.clone();
    list.on_sort_ascending(move |col| {
        if let Some(c) = c.upgrade() {
            c.sort_by(col, true);
        }
    });
    let c = ctrl.clone();
    list.on_sort_descending(move |col| {
        if let Some(c) = c.upgrade() {
            c.sort_by(col, false);
        }
    });
    let c = ctrl.clone();
    list.on_row_pressed(move |row| {
        if let Some(c) = c.upgrade() {
            c.row_pressed(row);
        }
    });
}

fn on_text<W: ListWindow>(
    ctrl: &Weak<ListController<W>>,
    f: fn(&Rc<ListController<W>>),
) -> impl Fn(SharedString) + 'static {
    let ctrl = ctrl.clone();
    move |_| {
        if let Some(c) = ctrl.upgrade() {
            f(&c);
        }
    }
}

macro_rules! list_window {
    ($ty:ident, $kind:expr, $title:expr) => {
        impl ModalOwner for $ty {
            fn set_input_blocked(&self, blocked: bool) {
                self.global::<CatalogueList>().set_input_blocked(blocked);
            }

            fn native_title(&self) -> String {
                $title.to_owned()
            }

            fn set_busy(&self, busy: bool) {
                self.global::<CatalogueList>().set_busy(busy);
            }
        }

        impl ListWindow for $ty {
            const KIND: ListKind = $kind;

            fn create() -> Result<Self, slint::PlatformError> {
                $ty::new()
            }

            fn list(&self) -> CatalogueList<'_> {
                self.global::<CatalogueList>()
            }
        }
    };
}

list_window!(ProductsWindow, ListKind::Products, "Artikujt");
list_window!(ClientsWindow, ListKind::Clients, "Klientët");
list_window!(SuppliersWindow, ListKind::Suppliers, "Furnitorët");

/// One open (or hidden) list window and its rows.
pub struct ListController<W: ListWindow> {
    ctx: Weak<UiContext>,
    window: W,
    rows: RefCell<Vec<Row>>,
    model: Rc<VecModel<ModelRc<StandardListViewItem>>>,
    sort: Cell<Option<(usize, bool)>>,
    debounce: slint::Timer,
    /// The row of the last single press, while `press_window` runs.
    last_press: Cell<Option<i32>>,
    /// Runs for `DOUBLE_CLICK` after a single press.
    press_window: slint::Timer,
}

impl<W: ListWindow> ListController<W> {
    /// Creates the window (hidden), registers it with the lock and subscribes
    /// it to data changes.
    pub fn new(ctx: &Rc<UiContext>) -> Option<Rc<Self>> {
        let window = match W::create() {
            Ok(w) => w,
            Err(e) => {
                tracing::error!("list window: {e}");
                return None;
            }
        };
        let model = Rc::new(VecModel::default());
        window.list().set_rows(ModelRc::from(model.clone()));
        let ctrl = Rc::new(ListController {
            ctx: Rc::downgrade(ctx),
            window,
            rows: RefCell::new(Vec::new()),
            model,
            sort: Cell::new(None),
            debounce: slint::Timer::default(),
            last_press: Cell::new(None),
            press_window: slint::Timer::default(),
        });
        connect(&ctrl.window, &Rc::downgrade(&ctrl));
        // Hidden while locked, disabled while a background job runs (F-029).
        ctx.register_window(Box::new(BusyOwner(ctrl.window.as_weak())));
        let weak = Rc::downgrade(&ctrl);
        ctx.on_data_changed(move || {
            if let Some(c) = weak.upgrade()
                && c.window.window().is_visible()
            {
                c.reload();
            }
        });
        Some(ctrl)
    }

    /// The window.
    pub fn window(&self) -> &W {
        &self.window
    }

    fn ctx(&self) -> Option<Rc<UiContext>> {
        self.ctx.upgrade()
    }

    /// Shows the window with fresh rows and brings it to the front.
    pub fn open(self: &Rc<Self>) {
        self.reload();
        if let Err(e) = self.window.show() {
            tracing::warn!("showing a list window failed: {e}");
        }
        win::bring_to_front(&self.window.native_title());
        self.focus_table();
    }

    /// Gives the keyboard focus back to the table.
    pub fn focus_table(&self) {
        let list = self.window.list();
        list.set_focus_request(list.get_focus_request().wrapping_add(1));
    }

    /// The id of the selected row.
    pub fn selected_id(&self) -> Option<i64> {
        let row = usize::try_from(self.window.list().get_current_row()).ok()?;
        self.rows.borrow().get(row).map(|r| r.id)
    }

    /// The number of rows shown.
    pub fn row_count(&self) -> usize {
        self.model.row_count()
    }

    /// Reloads the rows for the current search, keeping the sort and the
    /// selected id.
    pub fn reload(self: &Rc<Self>) {
        let Some(ctx) = self.ctx() else { return };
        let query = self.window.list().get_search_text().trim().to_owned();
        let loaded: Result<Vec<Row>, ServiceError> = match W::KIND.owner_kind() {
            None => products::load(&ctx.crm, &query),
            Some(kind) => owners::load(&ctx.crm, kind, &query),
        };
        match loaded {
            Ok(rows) => self.put(rows, self.selected_id()),
            Err(e) => self.show_error(&e),
        }
    }

    /// Reloads and selects `id` (after a save).
    pub fn reload_selecting(self: &Rc<Self>, id: i64) {
        self.reload();
        self.select_id(Some(id));
    }

    fn put(&self, mut rows: Vec<Row>, keep: Option<i64>) {
        if let Some((col, asc)) = self.sort.get() {
            models::sort(&mut rows, col, asc);
        }
        self.model.set_vec(models::table_rows(&rows));
        let n = rows.len();
        *self.rows.borrow_mut() = rows;
        self.select_id(keep);
        self.window
            .list()
            .set_status_text(W::KIND.count_text(n).into());
    }

    fn select_id(&self, id: Option<i64>) {
        let row = id
            .and_then(|id| self.rows.borrow().iter().position(|r| r.id == id))
            .and_then(|i| i32::try_from(i).ok())
            .unwrap_or(-1);
        self.window.list().set_current_row(row);
    }

    fn search_later(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.debounce
            .start(slint::TimerMode::SingleShot, SEARCH_DEBOUNCE, move || {
                if let Some(c) = weak.upgrade() {
                    c.reload();
                }
            });
    }

    fn sort_by(self: &Rc<Self>, column: i32, ascending: bool) {
        let Ok(column) = usize::try_from(column) else {
            return;
        };
        self.sort.set(Some((column, ascending)));
        let keep = self.selected_id();
        let rows = std::mem::take(&mut *self.rows.borrow_mut());
        self.put(rows, keep);
    }

    fn row_pressed(self: &Rc<Self>, row: i32) {
        let double = self.press_window.running() && self.last_press.get() == Some(row);
        if double {
            self.press_window.stop();
            self.last_press.set(None);
            self.window.list().set_current_row(row);
            self.edit();
        } else {
            self.last_press.set(Some(row));
            self.press_window
                .start(slint::TimerMode::SingleShot, DOUBLE_CLICK, || {});
        }
    }

    /// Shows a service error over this window.
    pub fn show_error(&self, e: &ServiceError) {
        tracing::warn!("{e}");
        if let Some(ctx) = self.ctx() {
            crate::catalogue::dialogs::message_over(
                &ctx,
                &self.window,
                crate::shell::texts::ERROR_TITLE,
                &e.user_message(),
            );
        }
    }

    /// Shto (Ins).
    pub fn add(self: &Rc<Self>) {
        self.open_dialog(None);
    }

    /// Ndrysho (F2, Enter, double click).
    pub fn edit(self: &Rc<Self>) {
        if let Some(id) = self.selected_id() {
            self.open_dialog(Some(id));
        }
    }

    fn open_dialog(self: &Rc<Self>, id: Option<i64>) {
        let Some(ctx) = self.ctx() else { return };
        let weak = Rc::downgrade(self);
        let ctx_weak = Rc::downgrade(&ctx);
        let on_saved = Rc::new(move |id: i64| {
            if let Some(c) = weak.upgrade() {
                c.reload_selecting(id);
            }
            // Own writes are no longer reported by the 2 s poll (F-001):
            // tell the other windows directly.
            if let Some(ctx) = ctx_weak.upgrade() {
                ctx.notify_data_changed();
            }
        });
        let modal = match W::KIND.owner_kind() {
            None => products::open_dialog(&ctx, &self.window, id, on_saved),
            Some(kind) => owners::open_dialog(&ctx, &self.window, kind, id, on_saved),
        };
        self.refocus_after(modal);
    }

    fn refocus_after(self: &Rc<Self>, modal: Option<crate::shell::modal::Modal>) {
        if let Some(modal) = modal {
            let weak = Rc::downgrade(self);
            modal.on_closed(move || {
                if let Some(c) = weak.upgrade() {
                    c.focus_table();
                }
            });
        }
    }

    /// Fshi (Del): asks, then deletes the selected row.
    pub fn remove(self: &Rc<Self>) {
        let Some(ctx) = self.ctx() else { return };
        let Some(id) = self.selected_id() else { return };
        let weak = Rc::downgrade(self);
        crate::catalogue::dialogs::confirm_delete(&ctx, &self.window, W::KIND, id, move |result| {
            let Some(c) = weak.upgrade() else { return };
            c.focus_table();
            match result {
                Ok(true) => c.reload(),
                Ok(false) => {}
                Err(e) => {
                    c.show_error(&e);
                    c.reload();
                }
            }
        });
    }

    /// Importo…: the import dialog over this window.
    pub fn import(self: &Rc<Self>) {
        let Some(ctx) = self.ctx() else { return };
        if let Some(flow) = transfer::open_import(&ctx, &self.window, W::KIND) {
            self.refocus_after(Some(flow.modal()));
        }
    }

    /// Eksporto…: the save dialog, then the export.
    pub fn export(self: &Rc<Self>) {
        let Some(ctx) = self.ctx() else { return };
        transfer::export(&ctx, &self.window, W::KIND);
    }

    /// Mbyll (Esc): hides the window (it is reused by the next open).
    pub fn close(self: &Rc<Self>) {
        self.debounce.stop();
        if let Err(e) = self.window.hide() {
            tracing::warn!("hiding a list window failed: {e}");
        }
    }
}

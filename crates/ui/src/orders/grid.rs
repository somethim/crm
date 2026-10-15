//! The line grid's callbacks: keys (through [`grid_nav::navigate`]), commits,
//! product suggestions, the stock link and row removal.

use super::grid_nav::{self, Dropdown, Mods, NavAction, NavKey, PRODUCT_COL, Pos, QTY_COL};
use super::workspace::{Pending, SUGGESTION_LIMIT, Workspace, Ws, to_usize};
use crm_services::domain::OrderKind;
use slint::platform::Key;
use std::rc::Rc;

/// The [`NavKey`] of a Slint key text.
pub fn nav_key(text: &str) -> NavKey {
    let is = |k: Key| text == slint::SharedString::from(k).as_str();
    if is(Key::Tab) {
        NavKey::Tab
    } else if is(Key::Backtab) {
        NavKey::Backtab
    } else if is(Key::RightArrow) {
        NavKey::Right
    } else if is(Key::LeftArrow) {
        NavKey::Left
    } else if is(Key::DownArrow) {
        NavKey::Down
    } else if is(Key::UpArrow) {
        NavKey::Up
    } else if is(Key::Return) || text == "\n" {
        NavKey::Enter
    } else if is(Key::Escape) {
        NavKey::Escape
    } else if is(Key::Delete) {
        NavKey::Delete
    } else {
        NavKey::Other
    }
}

/// What a commit asks the caller to do afterwards.
enum After {
    Nothing,
    Flash(String),
}

impl Workspace {
    /// Commits the typed text of a cell, if any.
    pub fn commit_pending(&mut self) {
        if let Some(p) = self.pending.take() {
            self.commit_cell(p.pos, &p.text);
        }
    }

    /// Commits `text` into the cell at `pos` of the active document; flashes a
    /// rejected value. Returns whether the document changed.
    pub fn commit_cell(&mut self, pos: Pos, text: &str) -> bool {
        if self.pending.as_ref().is_some_and(|p| p.pos == pos) {
            self.pending = None;
        }
        let Some(tab) = self.current_mut() else {
            return false;
        };
        let grid = &mut tab.grid;
        let (changed, after) = match pos.col {
            PRODUCT_COL => {
                let changed = match grid.line(pos.row) {
                    Some(line) if line.name == text.trim() => false,
                    None if text.trim().is_empty() => false,
                    // A sale keeps only chosen products: the text reverts.
                    _ => grid.set_free_text(pos.row, text),
                };
                (changed, After::Nothing)
            }
            QTY_COL => match grid.set_qty(pos.row, text) {
                Ok(changed) => (changed, After::Nothing),
                Err((changed, msg)) => (changed, After::Flash(msg.0)),
            },
            col => match grid.set_price(pos.row, col, text) {
                Ok(changed) => (changed, After::Nothing),
                Err(msg) => (false, After::Flash(msg.0)),
            },
        };
        self.sync_rows(Some(pos.row));
        if changed {
            self.touch();
        }
        if let After::Flash(msg) = after
            && let Some(ctx) = self.ctx()
        {
            ctx.flash(msg);
        }
        changed
    }

    /// Queries the product suggestions for the Artikulli cell of `row`.
    /// `typed`: the query comes from typing; in a sale (where the text must
    /// become a product) the first hit is then highlighted, so Enter takes it.
    pub fn query_products(&mut self, row: usize, text: &str, typed: bool) {
        let Some(ctx) = self.ctx() else { return };
        let sale = self.current().is_some_and(|t| t.kind == OrderKind::Sale);
        match ctx.crm.search_products_counted(text, SUGGESTION_LIMIT) {
            Ok((hits, total)) => {
                let first = typed && sale && !text.trim().is_empty() && !hits.is_empty();
                self.show_products(row, hits, total);
                if first {
                    self.bridge(|b| b.set_product_highlighted(0));
                }
            }
            Err(e) => tracing::warn!("product search: {e}"),
        }
    }

    /// Chooses suggestion `index` for the dropdown's row; focus goes to that
    /// row's Sasia. Returns whether a product was chosen.
    pub fn choose_product(&mut self, index: usize) -> bool {
        let Some(product) = self.product_hits.get(index).cloned() else {
            return false;
        };
        let row = self
            .bridge(|b| to_usize(b.get_dropdown_row()))
            .flatten()
            .unwrap_or(0);
        if self
            .pending
            .as_ref()
            .is_some_and(|p| p.pos.row == row && p.pos.col == PRODUCT_COL)
        {
            self.pending = None;
        }
        let Some(tab) = self.current_mut() else {
            return false;
        };
        if tab.grid.is_orphan(row) {
            self.close_dropdown();
            return false;
        }
        let row = tab.grid.choose_product(row, &product);
        self.close_dropdown();
        self.sync_rows(Some(row));
        self.touch();
        self.focus_cell(Pos::new(row, QTY_COL));
        true
    }

    fn move_highlight(&mut self, forward: bool) {
        let count = self.product_hits.len();
        if count == 0 {
            return;
        }
        self.bridge(|b| {
            let h = b.get_product_highlighted();
            let next = match (to_usize(h).filter(|&h| h < count), forward) {
                (None, true) => 0,
                (None, false) => count - 1,
                (Some(h), true) => (h + 1) % count,
                (Some(h), false) => (h + count - 1) % count,
            };
            b.set_product_highlighted(super::workspace::to_i32(next));
        });
    }

    /// Removes the entry at `row` and keeps the focus in the grid when the row
    /// was focused (`col`).
    pub fn remove_row(&mut self, row: usize, refocus: Option<usize>) {
        self.pending = None;
        let Some(tab) = self.current_mut() else {
            return;
        };
        if !tab.grid.remove(row) {
            return;
        }
        let rows = tab.grid.rows();
        self.close_dropdown();
        self.sync_rows(None);
        self.touch();
        if let Some(col) = refocus {
            self.focus_cell(Pos::new(row.min(rows - 1), col));
        }
    }
}

/// `grid-key`: true when the key was handled.
pub fn key(ws: &Ws, row: usize, col: usize, text: &str, mods: Mods, cell_text: &str) -> bool {
    let key = nav_key(text);
    if key == NavKey::Other {
        return false;
    }
    let mut w = ws.borrow_mut();
    let Some(tab) = w.current() else { return false };
    let layout = grid_nav::GridLayout {
        kind: tab.kind,
        rows: tab.grid.rows(),
    };
    let hits = w.product_hits.len();
    let dropdown = w
        .bridge(|b| Dropdown {
            open: b.get_dropdown_open() && to_usize(b.get_dropdown_row()) == Some(row),
            highlighted: to_usize(b.get_product_highlighted()).is_some_and(|h| h < hits),
        })
        .unwrap_or_default();
    let pos = Pos::new(row, col);
    match grid_nav::navigate(layout, pos, key, mods, dropdown) {
        NavAction::None => return false,
        NavAction::Move(to) => {
            w.commit_cell(pos, cell_text);
            w.focus_cell(to);
        }
        NavAction::AddRowFocusProduct => {
            w.commit_cell(pos, cell_text);
            if let Some(tab) = w.current_mut() {
                let new_row = tab.grid.add_empty_row();
                w.sync_rows(None);
                w.touch();
                w.focus_cell(Pos::new(new_row, PRODUCT_COL));
            }
        }
        NavAction::SelectSuggestion => {
            let index = w
                .bridge(|b| to_usize(b.get_product_highlighted()))
                .flatten()
                .unwrap_or(0);
            w.choose_product(index);
        }
        NavAction::HighlightNext => w.move_highlight(true),
        NavAction::HighlightPrev => w.move_highlight(false),
        NavAction::CloseDropdown => w.close_dropdown(),
        NavAction::RemoveRow => w.remove_row(row, Some(col)),
        NavAction::Commit => {
            w.commit_cell(pos, cell_text);
        }
    }
    true
}

/// `cell-edited`: remembers the typed text; the Artikulli cell searches.
pub fn edited(ws: &Ws, row: usize, col: usize, text: &str) {
    let mut w = ws.borrow_mut();
    w.pending = Some(Pending {
        pos: Pos::new(row, col),
        text: text.to_owned(),
    });
    if col == PRODUCT_COL {
        w.query_products(row, text, true);
    }
}

/// `cell-focused`: commits another cell's typed text, then shows the product
/// suggestions at once on an Artikulli cell.
pub fn focused(ws: &Ws, row: usize, col: usize, text: &str) {
    let pos = Pos::new(row, col);
    let mut w = ws.borrow_mut();
    if let Some(p) = w.pending.clone()
        && p.pos != pos
    {
        w.commit_cell(p.pos, &p.text);
    }
    w.focus = Some(pos);
    w.last_focus = Some(pos);
    let orphan = w.current().is_some_and(|t| t.grid.is_orphan(row));
    if col == PRODUCT_COL && !orphan {
        w.query_products(row, text, false);
    } else {
        w.close_dropdown();
    }
}

/// `cell-committed` (focus left the cell): commits its typed text; a sale's
/// Artikulli reverts to the chosen product.
pub fn committed(ws: &Ws, row: usize, col: usize, text: &str) {
    let pos = Pos::new(row, col);
    let mut w = ws.borrow_mut();
    if w.pending.as_ref().is_some_and(|p| p.pos == pos) {
        w.commit_cell(pos, text);
    }
    if w.focus == Some(pos) {
        w.focus = None;
    }
    let dropdown_here = w
        .bridge(|b| to_usize(b.get_dropdown_row()) == Some(row))
        .unwrap_or(false);
    if col == PRODUCT_COL && dropdown_here {
        w.close_dropdown();
    }
}

/// `product-chosen` (a click on a suggestion).
pub fn product_chosen(ws: &Ws, index: usize) {
    ws.borrow_mut().choose_product(index);
}

/// `remove-row` (the ✕).
pub fn remove_clicked(ws: &Ws, row: usize) {
    ws.borrow_mut().remove_row(row, None);
}

/// `stock-clicked`: opens the product editor; after a save the stock and name
/// of that product refresh in every open document.
pub fn stock_clicked(ws: &Ws, row: usize) {
    let (ctx, id) = {
        let w = ws.borrow();
        let id = w
            .current()
            .and_then(|t| t.grid.line(row))
            .and_then(|l| l.product_id);
        (w.ctx(), id)
    };
    let (Some(ctx), Some(id)) = (ctx, id) else {
        return;
    };
    let weak = Rc::downgrade(ws);
    ctx.open_product_editor(
        id,
        Box::new(move || {
            let Some(ws) = weak.upgrade() else { return };
            let Ok(mut w) = ws.try_borrow_mut() else {
                return;
            };
            let Some(ctx) = w.ctx() else { return };
            let Ok(product) = ctx.crm.get_product(id) else {
                return;
            };
            for tab in &mut w.tabs {
                tab.grid.refresh_product(&product);
            }
            w.sync_rows(None);
        }),
    );
}

/// The kind of the active document (Shitje when none).
pub fn current_kind(ws: &Ws) -> OrderKind {
    ws.borrow().current().map_or(OrderKind::Sale, |t| t.kind)
}

//! Orders workspace (task 010): the invoice filters and list, the tabbed
//! invoice documents with their header, the custom line grid, and the drafts
//! persisted in `open_tabs`.
//!
//! Contract (008): the main window's `OrdersBridge` callbacks (`new-document`,
//! `save-current`, `print-current`, `delete-current`, `close-current`,
//! `refresh`, `flush-drafts`) and properties (`has-document`,
//! `current-is-saved`, `open-tab-count`), and the `OrdersWorkspace` component
//! inside the main window. Everything else the workspace shows is driven
//! through members added to `OrdersBridge` (ui/orders/bridge.slint).
//!
//! Layout: [`grid_nav`] and [`grid_state`] are pure (no Slint); [`workspace`]
//! holds the tabs and the models; [`tabs`], [`document`], [`grid`] and
//! [`list`] implement the callbacks.

pub mod document;
pub mod grid;
pub mod grid_nav;
pub mod grid_state;
pub mod list;
pub mod prompt;
pub mod tabs;
pub mod texts;
pub mod workspace;

#[cfg(test)]
mod tests;

pub use workspace::{Workspace, Ws};

use crate::{OrdersBridge, UiContext};
use crm_services::domain::OrderKind;
use grid_nav::Mods;
use slint::ComponentHandle;
use std::rc::Rc;
use tabs::DocTab;
use workspace::to_usize;

#[cfg(test)]
thread_local! {
    /// The workspace of the last installed main window (headless tests).
    static LAST: std::cell::RefCell<Option<std::rc::Weak<std::cell::RefCell<Workspace>>>> =
        const { std::cell::RefCell::new(None) };
}

/// The workspace of the most recently installed main window on this thread.
#[cfg(test)]
pub fn last_workspace() -> Option<Ws> {
    LAST.with(|l| l.borrow().as_ref().and_then(std::rc::Weak::upgrade))
}

/// Installs the orders handlers on the main window's `OrdersBridge`, restores
/// the persisted tabs and fills the list.
pub fn install(ctx: &Rc<UiContext>) {
    let Some(app) = ctx.app() else { return };
    let ws = Workspace::new(ctx, &app);
    let bridge = app.global::<OrdersBridge>();
    wire(&ws, &bridge);

    {
        let mut w = ws.borrow_mut();
        w.reset_filters();
        w.restore();
    }
    let weak = Rc::downgrade(&ws);
    ctx.on_data_changed(move || {
        if let Some(ws) = weak.upgrade()
            && let Ok(mut w) = ws.try_borrow_mut()
        {
            w.refresh_list();
        }
    });
    let weak = Rc::downgrade(&ws);
    ctx.on_restored(move || {
        if let Some(ws) = weak.upgrade() {
            match ws.try_borrow_mut() {
                Ok(mut w) => w.reload_from_db(),
                Err(_) => {
                    tracing::error!("restore: the orders workspace is busy; tabs not reloaded")
                }
            }
        }
    });
    #[cfg(test)]
    LAST.with(|l| *l.borrow_mut() = Some(Rc::downgrade(&ws)));
}

/// Connects every bridge callback to the workspace.
fn wire(ws: &Ws, b: &OrdersBridge<'_>) {
    // 008 contract.
    let w = ws.clone();
    b.on_new_document(move |kind| {
        let kind = if kind == 1 {
            OrderKind::Purchase
        } else {
            OrderKind::Sale
        };
        let mut x = w.borrow_mut();
        x.push_tab(DocTab::new(kind));
        x.focus_owner();
    });
    let w = ws.clone();
    b.on_save_current(move || document::save(&w));
    let w = ws.clone();
    b.on_print_current(move || document::print(&w));
    let w = ws.clone();
    b.on_delete_current(move || document::delete(&w));
    let w = ws.clone();
    b.on_close_current(move || {
        let active = w.borrow().active;
        if let Some(active) = active {
            tabs::close_with_prompt(&w, move |i, _| i == active);
        }
    });
    let w = ws.clone();
    b.on_refresh(move || w.borrow_mut().refresh_list());
    let w = ws.clone();
    b.on_flush_drafts(move || {
        let mut x = w.borrow_mut();
        x.commit_pending();
        x.flush();
    });

    // Filters and list.
    let w = ws.clone();
    b.on_filter_changed(move || w.borrow_mut().refresh_list());
    let w = ws.clone();
    b.on_filter_text_edited(move || list::text_edited(&w));
    let w = ws.clone();
    b.on_clear_filters(move || w.borrow_mut().reset_filters());
    let w = ws.clone();
    b.on_list_sort(move |column, ascending| {
        if let Some(column) = to_usize(column) {
            w.borrow_mut().sort_list(column, ascending);
        }
    });
    let w = ws.clone();
    b.on_export_list(move || list::export(&w));
    let w = ws.clone();
    b.on_focus_search(move || w.borrow().focus_search());
    let w = ws.clone();
    b.on_open_order(move |row| {
        if let Some(row) = to_usize(row) {
            list::open_row(&w, row);
        }
    });
    let w = ws.clone();
    b.on_list_clicked(move |row| {
        if let Some(row) = to_usize(row) {
            list::clicked(&w, row);
        }
    });

    // Tabs.
    let w = ws.clone();
    b.on_select_tab(move |i| {
        if let Some(i) = to_usize(i) {
            w.borrow_mut().activate(i);
        }
    });
    let w = ws.clone();
    b.on_close_tab(move |i| {
        if let Some(i) = to_usize(i) {
            tabs::close_with_prompt(&w, move |j, _| j == i);
        }
    });
    let w = ws.clone();
    b.on_close_all_tabs(move || tabs::close_with_prompt(&w, |_, _| true));
    let w = ws.clone();
    b.on_close_other_tabs(move |i| {
        if let Some(i) = to_usize(i) {
            tabs::close_with_prompt(&w, move |j, _| j != i);
        }
    });

    // Header.
    let w = ws.clone();
    b.on_owner_edited(move |text| document::owner_edited(&w, &text));
    let w = ws.clone();
    b.on_owner_focused(move |text| document::owner_focused(&w, &text));
    let w = ws.clone();
    b.on_owner_chosen(move |i| {
        if let Some(i) = to_usize(i) {
            document::owner_chosen(&w, i);
        }
    });
    let w = ws.clone();
    b.on_description_edited(move |text| document::description_edited(&w, &text));
    let w = ws.clone();
    b.on_date_edited(move |iso| document::date_edited(&w, &iso));

    // Grid.
    let w = ws.clone();
    b.on_grid_key(
        move |row, col, key, shift, ctrl, text| match (to_usize(row), to_usize(col)) {
            (Some(row), Some(col)) => grid::key(&w, row, col, &key, Mods { shift, ctrl }, &text),
            _ => false,
        },
    );
    let w = ws.clone();
    b.on_cell_edited(move |row, col, text| {
        if let (Some(row), Some(col)) = (to_usize(row), to_usize(col)) {
            grid::edited(&w, row, col, &text);
        }
    });
    let w = ws.clone();
    b.on_cell_focused(move |row, col, text| {
        if let (Some(row), Some(col)) = (to_usize(row), to_usize(col)) {
            grid::focused(&w, row, col, &text);
        }
    });
    let w = ws.clone();
    b.on_cell_committed(move |row, col, text| {
        if let (Some(row), Some(col)) = (to_usize(row), to_usize(col)) {
            grid::committed(&w, row, col, &text);
        }
    });
    let w = ws.clone();
    b.on_product_chosen(move |i| {
        if let Some(i) = to_usize(i) {
            grid::product_chosen(&w, i);
        }
    });
    let w = ws.clone();
    b.on_stock_clicked(move |row| {
        if let Some(row) = to_usize(row) {
            grid::stock_clicked(&w, row);
        }
    });
    let w = ws.clone();
    b.on_refocus(move || {
        if let Ok(mut x) = w.try_borrow_mut() {
            x.refocus();
        }
    });
    let w = ws.clone();
    b.on_header_focused(move || {
        if let Ok(mut x) = w.try_borrow_mut() {
            x.last_focus = None;
        }
    });
    let w = ws.clone();
    b.on_remove_row(move |row| {
        if let Some(row) = to_usize(row) {
            grid::remove_clicked(&w, row);
        }
    });
}

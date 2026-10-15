//! Reports export dialog (task 011): "Raporte → Eksporto raport…" and the
//! toolbar call `ReportsBridge.open-dialog()` on the main window, which opens
//! [`ReportDialog`](crate::ReportDialog) modal over it.

pub mod dialog;
pub mod texts;

#[cfg(test)]
mod tests;

use crate::{ReportsBridge, UiContext};
use slint::ComponentHandle;
use std::rc::Rc;

/// Installs the reports handlers on the main window's `ReportsBridge`.
pub fn install(ctx: &Rc<UiContext>) {
    let Some(app) = ctx.app() else { return };
    let weak = Rc::downgrade(ctx);
    app.global::<ReportsBridge>().on_open_dialog(move || {
        if let Some(ctx) = weak.upgrade() {
            dialog::open(&ctx);
        }
    });
}

//! Single-instance focus: when a second `crm.exe` starts, it signals this one,
//! which restores its main window and brings it to the front.

use crate::shell::win;
use crate::{ActivationListener, UiContext};
use slint::ComponentHandle;
use std::rc::Rc;

/// Hands the listener a callback that shows, restores and foregrounds the main
/// window on the UI thread.
pub fn install(ctx: &Rc<UiContext>, listener: ActivationListener) {
    let weak = ctx.app.clone();
    listener(Box::new(move || {
        let result = weak.upgrade_in_event_loop(|app| {
            tracing::info!("second launch: activating the main window");
            if let Err(e) = app.show() {
                tracing::warn!("showing the main window failed: {e}");
            }
            app.window().set_minimized(false);
            win::bring_to_front("CRM");
        });
        if let Err(e) = result {
            tracing::warn!("activation: event loop gone: {e}");
        }
    }));
}

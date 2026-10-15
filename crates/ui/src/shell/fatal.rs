//! Small windows shown before (or instead of) the main window: a fatal error, or
//! the corrupt-database question. Each runs its own short event loop.

use crate::{FatalWindow, UiError};
use slint::{CloseRequestResponse, ComponentHandle};
use std::cell::Cell;
use std::rc::Rc;

fn run_window(message: &str, offer_restore: bool) -> Result<bool, UiError> {
    let window = FatalWindow::new()?;
    window.set_message(message.into());
    window.set_offer_restore(offer_restore);
    let chosen = Rc::new(Cell::new(false));
    let restore = chosen.clone();
    window.on_restore(move || {
        restore.set(true);
        let _ = slint::quit_event_loop();
    });
    window.on_dismiss(|| {
        let _ = slint::quit_event_loop();
    });
    window.window().on_close_requested(|| {
        let _ = slint::quit_event_loop();
        CloseRequestResponse::HideWindow
    });
    window.show()?;
    slint::run_event_loop()?;
    window.hide()?;
    Ok(chosen.get())
}

/// Shows `message` with an OK button and waits until it is closed.
pub fn show_fatal(message: &str) -> Result<(), UiError> {
    run_window(message, false).map(|_| ())
}

/// Shows `message` with "Rikthe nga rezervimi më i fundit" and "Dil"; returns
/// true for the restore.
pub fn ask_restore(message: &str) -> Result<bool, UiError> {
    run_window(message, true)
}

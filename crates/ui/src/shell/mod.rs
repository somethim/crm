//! The main window frame: menus, toolbar, status bar, the lock screen and the
//! start-up gate, settings,
//! backups and about; plus the helpers features use (background work, modal
//! dialogs, formatting).

pub mod activation;
pub mod backup;
pub mod calendar;
pub mod fatal;
pub mod format;
pub mod gate;
pub mod idle;
pub mod menu;
pub mod modal;
pub mod password;
pub mod settings;
pub mod status;
pub mod texts;
pub mod timers;
pub(crate) mod win;
pub mod worker;

#[cfg(test)]
mod tests;

use crate::{OrdersBridge, UiContext};
use slint::ComponentHandle;
use std::rc::Rc;

/// Installs every shell handler on the main window: date parsing, status bar,
/// lock screen, menu commands, window close, timers.
pub fn install(ctx: &Rc<UiContext>) {
    let Some(app) = ctx.app() else { return };
    crate::install_date_parse!(app);
    status::install(ctx);
    password::install(ctx);
    menu::install(ctx);
    timers::install(ctx);
}

/// Dil / window close: persists the drafts (task 010), hides every window and
/// quits the event loop. The binary then closes the database (checkpoint and
/// exit backup). There is no unsaved-changes prompt: drafts are persisted.
pub fn exit(ctx: &UiContext) {
    tracing::info!("exit requested");
    let Some(app) = ctx.app() else {
        let _ = slint::quit_event_loop();
        return;
    };
    app.global::<OrdersBridge>().invoke_flush_drafts();
    for w in ctx.windows.borrow().iter() {
        if w.is_visible() {
            w.hide();
        }
    }
    if let Err(e) = app.hide() {
        tracing::warn!("hiding the main window failed: {e}");
    }
    if let Err(e) = slint::quit_event_loop() {
        tracing::warn!("quitting the event loop failed: {e}");
    }
}

/// The wipe code was accepted at the lock screen (the database is already
/// abandoned and the files are gone): stops the timers, hides every window
/// **without** flushing drafts, and quits the event loop with
/// [`crate::RunOutcome::Wiped`]. It logs nothing.
pub fn wipe_exit(ctx: &UiContext) {
    ctx.timers.borrow_mut().clear();
    ctx.outcome.set(crate::RunOutcome::Wiped);
    for w in ctx.windows.borrow().iter() {
        if w.is_alive() && w.is_visible() {
            w.hide();
        }
    }
    if let Some(app) = ctx.app() {
        let _ = app.hide();
    }
    let _ = slint::quit_event_loop();
}

/// After the event loop returned: stops the timers.
pub fn shutdown(ctx: &UiContext) {
    ctx.timers.borrow_mut().clear();
}

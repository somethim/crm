//! The shell-owned menu items (Skedari, Ndihmë) and the window close button.
//! Feature items go straight from `Commands` to the feature bridges.

use crate::shell::{backup, modal, password, settings};
use crate::{AboutDialog, ShellBridge, UiContext};
use slint::{CloseRequestResponse, ComponentHandle};
use std::rc::Rc;

/// Connects `ShellBridge`'s command callbacks and the main window's close button.
pub fn install(ctx: &Rc<UiContext>) {
    let Some(app) = ctx.app() else { return };
    let bridge = app.global::<ShellBridge>();

    let on = |f: fn(&Rc<UiContext>)| {
        let weak = Rc::downgrade(ctx);
        move || {
            if let Some(ctx) = weak.upgrade() {
                f(&ctx);
            }
        }
    };
    bridge.on_backup_now(on(backup::backup_now));
    bridge.on_backup_to_pc(on(backup::backup_to_pc));
    bridge.on_restore(on(backup::open_restore));
    bridge.on_change_password(on(password::open_change_password));
    bridge.on_settings(on(settings::open));
    bridge.on_lock(on(|ctx| ctx.lock()));
    bridge.on_about(on(open_about));
    bridge.on_exit(on(|ctx| crate::shell::exit(ctx)));

    let weak = Rc::downgrade(ctx);
    bridge.on_flash(move |text| {
        if let Some(ctx) = weak.upgrade() {
            ctx.flash(text);
        }
    });

    let weak = Rc::downgrade(ctx);
    app.window().on_close_requested(move || {
        let Some(ctx) = weak.upgrade() else {
            return CloseRequestResponse::HideWindow;
        };
        let busy = ctx
            .app()
            .is_some_and(|app| app.global::<ShellBridge>().get_busy());
        if busy {
            // A background job (import, backup, restore) is still running.
            return CloseRequestResponse::KeepWindowShown;
        }
        crate::shell::exit(&ctx);
        CloseRequestResponse::HideWindow
    });
}

/// Ndihmë → Rreth programit.
pub fn open_about(ctx: &Rc<UiContext>) {
    let dialog = match AboutDialog::new() {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("about dialog: {e}");
            return;
        }
    };
    dialog.set_version(env!("CARGO_PKG_VERSION").into());
    dialog.set_data_folder(ctx.paths.data_dir().display().to_string().into());
    let Some(modal) = modal::open_over_app(ctx, &dialog) else {
        return;
    };
    dialog.on_dismiss(move || modal.close());
}

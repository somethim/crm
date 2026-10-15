//! The lock screen over the main window and "Ndrysho fjalëkalimin…", both
//! through the [`Vault`](crm_services::Vault): every entry (password, recovery
//! key, wipe code) goes to `Vault::submit_with` on a worker thread, because
//! Argon2 must never run on the UI thread.

use crate::shell::{idle, modal, texts};
use crate::{ChangePasswordDialog, ShellBridge, UiContext};
use crm_services::{ServiceError, Unlock, VaultError, check_new_password, messages};
use slint::{ComponentHandle, SharedString};
use std::rc::Rc;
use zeroize::Zeroizing;

/// The main window is built only after the start-up gate, so it starts
/// unlocked. Connects the lock screen's callbacks.
pub fn install(ctx: &Rc<UiContext>) {
    let Some(app) = ctx.app() else { return };
    let bridge = app.global::<ShellBridge>();
    bridge.set_lock_mode("unlock".into());
    bridge.set_locked(false);
    idle::load_setting(ctx);

    let weak = Rc::downgrade(ctx);
    bridge.on_lock_submit_entry(move |entry| {
        if let Some(ctx) = weak.upgrade() {
            submit_entry(&ctx, Zeroizing::new(entry.to_string()));
        }
    });
    let weak = Rc::downgrade(ctx);
    bridge.on_lock_submit_new(move |password, repeat| {
        if let Some(ctx) = weak.upgrade() {
            submit_new(
                &ctx,
                Zeroizing::new(password.to_string()),
                Zeroizing::new(repeat.to_string()),
            );
        }
    });
}

fn bridge_do(ctx: &UiContext, f: impl FnOnce(&ShellBridge<'_>)) {
    if let Some(app) = ctx.app() {
        f(&app.global::<ShellBridge>());
    }
}

/// Shows `text` under the fields and clears them.
fn show_lock_error(ctx: &UiContext, text: &str) {
    bridge_do(ctx, |b| {
        b.set_lock_busy(false);
        b.set_lock_error(text.into());
        b.set_lock_reset(b.get_lock_reset() + 1);
    });
}

/// The text of a vault error at the lock screen.
fn vault_error_text(e: &VaultError) -> String {
    e.user_message()
}

/// The lock screen's entry: unlock, recovery (then a new password), wipe, or
/// "Fjalëkalim i gabuar" / the lockout.
pub fn submit_entry(ctx: &Rc<UiContext>, entry: Zeroizing<String>) {
    bridge_do(ctx, |b| b.set_lock_busy(true));
    let vault = ctx.vault.clone();
    ctx.run_bg(move |crm| vault.submit_with(&entry, || crm.abandon()), {
        let weak = Rc::downgrade(ctx);
        move |outcome| {
            let Some(ctx) = weak.upgrade() else { return };
            lock_outcome(&ctx, outcome);
        }
    });
}

fn lock_outcome(ctx: &Rc<UiContext>, outcome: Result<Unlock, VaultError>) {
    match outcome {
        Ok(Unlock::Unlocked(_key)) => {
            bridge_do(ctx, |b| {
                b.set_lock_busy(false);
                b.set_lock_reset(b.get_lock_reset() + 1);
            });
            ctx.unlock();
        }
        Ok(Unlock::Recovered(token)) => {
            *ctx.recovery.borrow_mut() = Some(token);
            bridge_do(ctx, |b| {
                b.set_lock_busy(false);
                b.set_lock_error(SharedString::new());
                b.set_lock_mode("new-password".into());
                b.set_lock_reset(b.get_lock_reset() + 1);
            });
        }
        Ok(Unlock::Wiped) => crate::shell::wipe_exit(ctx),
        Ok(Unlock::Wrong) if ctx.crm.is_closed() => {
            // The wipe code matched but the key file could not be erased after
            // the database was abandoned: back to the gate, which unlocks again.
            crate::shell::wipe_exit(ctx);
        }
        Ok(Unlock::Wrong) => show_lock_error(ctx, messages::PASSWORD_WRONG),
        Err(e) => show_lock_error(ctx, &vault_error_text(&e)),
    }
}

/// The lock screen's new-password form after a recovery-key unlock.
pub fn submit_new(ctx: &Rc<UiContext>, password: Zeroizing<String>, repeat: Zeroizing<String>) {
    if let Err(text) = check_pair(&password, &repeat) {
        return show_lock_error(ctx, &text);
    }
    let Some(token) = ctx.recovery.borrow_mut().take() else {
        return show_lock_error(ctx, messages::PASSWORD_WRONG);
    };
    bridge_do(ctx, |b| b.set_lock_busy(true));
    let vault = ctx.vault.clone();
    let weak = Rc::downgrade(ctx);
    ctx.run_bg(
        move |_crm| vault.set_password_after_recovery(token, &password),
        move |result| {
            let Some(ctx) = weak.upgrade() else { return };
            match result {
                Ok(_key) => {
                    bridge_do(&ctx, |b| {
                        b.set_lock_busy(false);
                        b.set_lock_mode("unlock".into());
                        b.set_lock_reset(b.get_lock_reset() + 1);
                    });
                    ctx.unlock();
                    ctx.flash(texts::PASSWORD_CHANGED);
                }
                Err(e) => {
                    // The token is spent: unlock again (the recovery key still works).
                    bridge_do(&ctx, |b| b.set_lock_mode("unlock".into()));
                    show_lock_error(&ctx, &vault_error_text(&e));
                }
            }
        },
    );
}

/// The UI-thread checks of a new password: the two fields match, at least 6
/// characters, not shaped like a code.
pub fn check_pair(password: &str, repeat: &str) -> Result<(), String> {
    if password != repeat {
        return Err(texts::PASSWORD_MISMATCH.to_owned());
    }
    check_new_password(password).map_err(|e| e.user_message())
}

/// Skedari → Ndrysho fjalëkalimin… (menu entry).
pub fn open_change_password(ctx: &Rc<UiContext>) {
    let _ = open_change_password_dialog(ctx);
}

/// Opens the change-password dialog and returns it (tests drive it). The
/// current-password field takes only the password (not the recovery key or the
/// wipe code).
pub fn open_change_password_dialog(ctx: &Rc<UiContext>) -> Option<ChangePasswordDialog> {
    let dialog = match ChangePasswordDialog::new() {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("change password dialog: {e}");
            return None;
        }
    };
    let modal = modal::open_over_app(ctx, &dialog)?;
    let weak_ctx = Rc::downgrade(ctx);
    let weak_dialog = dialog.as_weak();
    let on_submit = modal.clone();
    dialog.on_submit(move |current, new_password, repeat| {
        let (Some(ctx), Some(dialog)) = (weak_ctx.upgrade(), weak_dialog.upgrade()) else {
            return;
        };
        if let Err(text) = check_pair(&new_password, &repeat) {
            dialog.set_error(text.into());
            dialog.invoke_clear();
            return;
        }
        let current = Zeroizing::new(current.to_string());
        let new_password = Zeroizing::new(new_password.to_string());
        let vault = ctx.vault.clone();
        let weak_dialog = dialog.as_weak();
        let weak_ctx = Rc::downgrade(&ctx);
        let on_done = on_submit.clone();
        ctx.run_bg(
            move |_crm| vault.change_password(&current, &new_password),
            move |result| {
                let (Some(ctx), Some(dialog)) = (weak_ctx.upgrade(), weak_dialog.upgrade()) else {
                    return;
                };
                match result {
                    Ok(()) => {
                        dialog.set_error(SharedString::new());
                        on_done.close();
                        ctx.flash(texts::PASSWORD_CHANGED);
                    }
                    Err(e) => {
                        let text = match e {
                            VaultError::Wrong => messages::PASSWORD_WRONG.to_owned(),
                            other => ServiceError::Vault(other).user_message(),
                        };
                        dialog.set_error(text.into());
                        dialog.invoke_clear();
                    }
                }
            },
        );
    });
    dialog.on_cancel(move || modal.close());
    Some(dialog)
}

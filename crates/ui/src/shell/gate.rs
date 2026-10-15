//! The start-up gate: first-run setup (with the one-time codes screen), the
//! migration of plaintext data, unlock, recovery and the wipe, in its own
//! window before the main window exists. Argon2 runs on worker threads.
//!
//! [`GateController`] holds the state machine over an [`UnlockWindow`], so the
//! headless tests drive it without running the event loop; [`run_gate`] adds
//! the loop.

use crate::shell::{fatal, password, worker};
use crate::{UiError, UnlockWindow};
use crm_services::{
    DbKey, PendingSetup, RecoveryToken, StartState, Unlock, Vault, VaultError, messages,
};
use slint::{CloseRequestResponse, ComponentHandle, SharedString};
use std::cell::RefCell;
use std::rc::{Rc, Weak};
use zeroize::Zeroizing;

/// How the gate ended.
#[derive(Debug)]
pub enum GateOutcome {
    /// A data key was obtained: open the database with it.
    Open(DbKey),
    /// The window was closed (or the key file is missing).
    Quit,
}

/// The gate's state machine over its window.
pub struct GateController {
    window: UnlockWindow,
    vault: Vault,
    pending: RefCell<Option<PendingSetup>>,
    token: RefCell<Option<RecoveryToken>>,
    outcome: RefCell<Option<GateOutcome>>,
}

fn mode_for(state: &StartState) -> &'static str {
    match state {
        StartState::FirstRun => "setup",
        StartState::Migrate => "migrate",
        _ => "unlock",
    }
}

impl GateController {
    /// Creates the window in the mode `state` asks for and connects it.
    pub fn new(vault: Vault, state: &StartState) -> Result<Rc<GateController>, UiError> {
        let window = UnlockWindow::new()?;
        window.set_mode(mode_for(state).into());
        let gate = Rc::new(GateController {
            window,
            vault,
            pending: RefCell::new(None),
            token: RefCell::new(None),
            outcome: RefCell::new(None),
        });
        let weak = Rc::downgrade(&gate);
        gate.window.on_submit_new(move |password, repeat| {
            if let Some(gate) = weak.upgrade() {
                gate.submit_new(
                    Zeroizing::new(password.to_string()),
                    Zeroizing::new(repeat.to_string()),
                );
            }
        });
        let weak = Rc::downgrade(&gate);
        gate.window.on_submit_entry(move |entry| {
            if let Some(gate) = weak.upgrade() {
                gate.submit_entry(Zeroizing::new(entry.to_string()));
            }
        });
        let weak = Rc::downgrade(&gate);
        gate.window.on_codes_confirmed(move || {
            if let Some(gate) = weak.upgrade() {
                gate.codes_confirmed();
            }
        });
        Ok(gate)
    }

    /// The gate window.
    pub fn window(&self) -> &UnlockWindow {
        &self.window
    }

    /// The outcome, once the gate has one.
    pub fn take_outcome(&self) -> Option<GateOutcome> {
        self.outcome.borrow_mut().take()
    }

    /// Whether the gate has an outcome.
    pub fn is_done(&self) -> bool {
        self.outcome.borrow().is_some()
    }

    fn error(&self, text: &str) {
        self.window.set_busy(false);
        self.window.set_error(text.into());
        self.window.set_reset(self.window.get_reset() + 1);
    }

    fn finish(&self, outcome: GateOutcome) {
        self.window.set_busy(false);
        *self.outcome.borrow_mut() = Some(outcome);
        let _ = self.window.hide();
        let _ = slint::quit_event_loop();
    }

    /// Runs `job` on a worker, then `done` on the UI thread with the gate.
    fn spawn<T: Send + 'static>(
        self: &Rc<Self>,
        job: impl FnOnce() -> T + Send + 'static,
        done: impl FnOnce(&Rc<GateController>, T) + 'static,
    ) {
        self.window.set_busy(true);
        let weak: Weak<GateController> = Rc::downgrade(self);
        worker::run_plain(job, move |value| {
            if let Some(gate) = weak.upgrade() {
                gate.window.set_busy(false);
                done(&gate, value);
            }
        });
    }

    /// The two-field modes: setup, migrate, and the new password after recovery.
    pub fn submit_new(self: &Rc<Self>, password: Zeroizing<String>, repeat: Zeroizing<String>) {
        if let Err(text) = password::check_pair(&password, &repeat) {
            return self.error(&text);
        }
        if self.window.get_mode() == "new-password" {
            let Some(token) = self.token.borrow_mut().take() else {
                return self.error(messages::PASSWORD_WRONG);
            };
            let vault = self.vault.clone();
            self.spawn(
                move || vault.set_password_after_recovery(token, &password),
                |gate, result| match result {
                    Ok(key) => gate.finish(GateOutcome::Open(key)),
                    Err(e) => {
                        gate.window.set_mode("unlock".into());
                        gate.error(&e.user_message());
                    }
                },
            );
            return;
        }
        let vault = self.vault.clone();
        self.spawn(
            move || vault.setup(&password),
            |gate, result| match result {
                Ok(pending) => {
                    gate.window
                        .set_recovery_code(SharedString::from(pending.recovery_display().as_str()));
                    gate.window
                        .set_wipe_code(SharedString::from(pending.wipe_display().as_str()));
                    *gate.pending.borrow_mut() = Some(pending);
                    gate.window.set_error(SharedString::new());
                    gate.window.set_mode("codes".into());
                }
                Err(e) => gate.error(&e.user_message()),
            },
        );
    }

    /// "Vazhdo" on the codes screen: only now is `data/crm.key` written.
    pub fn codes_confirmed(&self) {
        let Some(pending) = self.pending.borrow_mut().take() else {
            return;
        };
        self.window.set_recovery_code(SharedString::new());
        self.window.set_wipe_code(SharedString::new());
        match pending.commit() {
            Ok(key) => {
                tracing::info!("first run setup completed");
                self.finish(GateOutcome::Open(key));
            }
            Err(e) => self.error(&e.user_message()),
        }
    }

    /// The unlock mode's entry.
    pub fn submit_entry(self: &Rc<Self>, entry: Zeroizing<String>) {
        let vault = self.vault.clone();
        self.spawn(
            move || vault.submit(&entry),
            |gate, outcome: Result<Unlock, VaultError>| match outcome {
                Ok(Unlock::Unlocked(key)) => {
                    tracing::info!("unlocked");
                    gate.finish(GateOutcome::Open(key));
                }
                Ok(Unlock::Recovered(token)) => {
                    *gate.token.borrow_mut() = Some(token);
                    gate.window.set_error(SharedString::new());
                    gate.window.set_mode("new-password".into());
                }
                Ok(Unlock::Wiped) => {
                    let state = gate.vault.probe().unwrap_or(StartState::FirstRun);
                    gate.window.set_error(SharedString::new());
                    gate.window.set_mode(mode_for(&state).into());
                    gate.window.set_reset(gate.window.get_reset() + 1);
                }
                Ok(Unlock::Wrong) => gate.error(messages::PASSWORD_WRONG),
                Err(e) => gate.error(&e.user_message()),
            },
        );
    }
}

/// Shows the gate for `state` and runs the event loop until it gives a key or
/// is closed. A missing key file shows the fatal message and quits.
pub fn run_gate(vault: &Vault, state: StartState) -> Result<GateOutcome, UiError> {
    if state == StartState::KeyMissing {
        fatal::show_fatal(messages::KEY_MISSING)?;
        return Ok(GateOutcome::Quit);
    }
    let gate = GateController::new(vault.clone(), &state)?;
    gate.window().window().on_close_requested(|| {
        let _ = slint::quit_event_loop();
        CloseRequestResponse::HideWindow
    });
    gate.window().show()?;
    slint::run_event_loop()?;
    let _ = gate.window().hide();
    Ok(gate.take_outcome().unwrap_or(GateOutcome::Quit))
}

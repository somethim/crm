//! Slint user interface: windows, dialogs and their bindings to `crm-services`.
//!
//! Boundary: depends on `crm-domain`, `crm-services` and `crm-reports`. `slint` types never
//! leave this crate (`apps/crm` only calls [`run_gate`], [`run`], [`show_fatal`]
//! and [`ask_restore`]). All user-visible text is Albanian.
//!
//! Layout:
//! - [`context`]: the [`UiContext`] every feature receives;
//! - [`shell`]: the start-up gate (password, codes, recovery, wipe), the main
//!   window frame (menus, toolbar, status bar, lock screen, settings, backups,
//!   about), background work, modal dialogs and timers;
//! - [`catalogue`], [`orders`], [`reports`]: the feature bridges (tasks 009,
//!   010 and 011), installed after the shell;
//! - [`print`]: the receipt preview, PNG save and printing (task 012), opened
//!   by the orders workspace's Printo.

slint::include_modules!();

pub mod catalogue;
pub mod context;
pub mod orders;
pub mod print;
pub mod reports;
pub mod shell;

pub use context::{SecondaryWindow, UiContext};
pub use shell::fatal::{ask_restore, show_fatal};
pub use shell::gate::{GateController, GateOutcome, run_gate};

use crm_services::{Crm, DataPaths, Vault};
use slint::ComponentHandle;

/// The single-instance hook: `apps/crm` passes a function that starts listening
/// for second launches and calls the given callback (from any thread) for each.
pub type ActivationListener = Box<dyn FnOnce(Box<dyn Fn() + Send + Sync>) + Send>;

/// What [`run`] needs from the binary.
pub struct UiConfig {
    /// The open services facade.
    pub crm: Crm,
    /// The key vault the gate unlocked (lock screen, password change, wipe).
    pub vault: Vault,
    /// The data layout (status bar, settings, about).
    pub paths: DataPaths,
    /// The second-launch listener (`None` where it is not supported).
    pub activation: Option<ActivationListener>,
}

/// A UI failure that ends [`run`] (the windowing system could not start).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiError(pub String);

impl std::fmt::Display for UiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ui: {}", self.0)
    }
}

impl std::error::Error for UiError {}

impl From<slint::PlatformError> for UiError {
    fn from(e: slint::PlatformError) -> Self {
        UiError(e.to_string())
    }
}

/// How [`run`] ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    /// The user exited: drafts were flushed; the caller closes the [`Crm`].
    Exit,
    /// The wipe code was entered at the lock screen: everything is gone and the
    /// [`Crm`] is abandoned; the caller goes back to the gate.
    Wiped,
}

/// Builds the main window (unlocked: the gate ran before) and every feature,
/// and runs the event loop until the user exits or wipes.
pub fn run(config: UiConfig) -> Result<RunOutcome, UiError> {
    let (app, ctx) = build(config.crm, config.vault, config.paths)?;
    if let Some(listener) = config.activation {
        shell::activation::install(&ctx, listener);
    }
    app.show()?;
    slint::run_event_loop()?;
    shell::shutdown(&ctx);
    Ok(ctx.outcome())
}

/// Creates the main window, the [`UiContext`], and installs the shell and the
/// features (in that order). Used by [`run`] and by the headless tests.
pub fn build(
    crm: Crm,
    vault: Vault,
    paths: DataPaths,
) -> Result<(AppWindow, std::rc::Rc<UiContext>), UiError> {
    let app = AppWindow::new()?;
    let ctx = UiContext::new(crm, vault, paths, &app);
    shell::install(&ctx);
    catalogue::install(&ctx);
    orders::install(&ctx);
    reports::install(&ctx);
    Ok((app, ctx))
}

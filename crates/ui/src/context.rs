//! [`UiContext`]: the shared state every feature receives in its `install`.
//!
//! It lives on the UI thread (`Rc`). Features:
//! - call the services through [`UiContext::crm`] (quick calls) or
//!   [`UiContext::run_bg`] (slow ones: import, export, report, backup);
//! - subscribe to [`UiContext::on_data_changed`] to refresh their lists after
//!   the 2 s `refresh_if_changed` poll or a restore;
//! - subscribe to [`UiContext::on_restored`] to drop what they keep in memory
//!   of the database a restore replaced (open documents, open editors);
//! - register their secondary windows with [`UiContext::register_window`], so
//!   the lock screen hides them (and a running background job disables the
//!   ones that can be disabled, F-029);
//! - show messages with [`UiContext::confirm`], [`UiContext::message`] and
//!   [`UiContext::flash`].

use crate::shell::{modal, texts, worker};
use crate::{AppWindow, ShellBridge};
use crm_services::domain::ProductId;
use crm_services::{Crm, DataPaths, RecoveryToken, ServiceError, Vault};
use slint::{ComponentHandle, SharedString};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

/// How long a status-bar flash stays visible.
pub const FLASH_DURATION: Duration = Duration::from_secs(5);

/// A window other than the main one (list windows, dialogs). The lock screen
/// hides the visible ones and shows them again after the unlock.
pub trait SecondaryWindow {
    /// False once the window has been dropped (the entry is then forgotten).
    fn is_alive(&self) -> bool;
    /// Whether the window is currently shown.
    fn is_visible(&self) -> bool;
    /// Shows the window.
    fn show(&self);
    /// Hides the window.
    fn hide(&self);
    /// A background job started (`true`) or the last one finished (`false`):
    /// the window must not take input meanwhile (F-029). Plain windows do
    /// nothing here; list windows register a [`crate::shell::modal::BusyOwner`].
    fn set_busy(&self, _busy: bool) {}
}

impl<T: ComponentHandle + 'static> SecondaryWindow for slint::Weak<T> {
    fn is_alive(&self) -> bool {
        self.upgrade().is_some()
    }

    fn is_visible(&self) -> bool {
        self.upgrade().is_some_and(|w| w.window().is_visible())
    }

    fn show(&self) {
        if let Some(w) = self.upgrade()
            && let Err(e) = w.show()
        {
            tracing::warn!("showing a window failed: {e}");
        }
    }

    fn hide(&self) {
        if let Some(w) = self.upgrade()
            && let Err(e) = w.hide()
        {
            tracing::warn!("hiding a window failed: {e}");
        }
    }
}

/// Opens the product editor for `ProductId`, then calls the last argument after
/// a successful save. Registered by task 009, used by task 010.
pub type ProductEditor = Box<dyn Fn(&Rc<UiContext>, ProductId, Box<dyn Fn()>)>;

/// The UI-thread state shared by the shell and the features.
pub struct UiContext {
    /// The services facade.
    pub crm: Crm,
    /// The key vault (lock screen, password change, wipe).
    pub vault: Vault,
    /// The data layout.
    pub paths: DataPaths,
    /// The main window.
    pub app: slint::Weak<AppWindow>,
    /// Secondary windows, hidden while locked.
    pub windows: RefCell<Vec<Box<dyn SecondaryWindow>>>,
    /// Called after the data changed (poll, restore).
    pub data_changed: RefCell<Vec<Box<dyn Fn()>>>,
    /// Called after a restore replaced the database (before `data_changed`).
    pub restored: RefCell<Vec<Box<dyn Fn()>>>,
    /// The product editor hook (009 registers, 010 opens).
    pub product_editor: RefCell<Option<ProductEditor>>,
    /// Indices into `windows` hidden by the current lock.
    hidden_by_lock: RefCell<Vec<usize>>,
    flash_timer: slint::Timer,
    /// Periodic timers (refresh, idle check); dropped timers stop.
    pub(crate) timers: RefCell<Vec<slint::Timer>>,
    busy_count: Cell<u32>,
    /// Idle-lock minutes from the settings (0 = off).
    pub(crate) idle_minutes: Cell<u32>,
    /// A recovery-key unlock at the lock screen, until the new password is set.
    pub(crate) recovery: RefCell<Option<RecoveryToken>>,
    /// How the event loop ended ([`crate::RunOutcome`]).
    pub(crate) outcome: Cell<crate::RunOutcome>,
}

impl UiContext {
    /// A context for `app` (no handlers installed yet).
    pub fn new(crm: Crm, vault: Vault, paths: DataPaths, app: &AppWindow) -> Rc<UiContext> {
        Rc::new(UiContext {
            crm,
            vault,
            paths,
            app: app.as_weak(),
            windows: RefCell::new(Vec::new()),
            data_changed: RefCell::new(Vec::new()),
            restored: RefCell::new(Vec::new()),
            product_editor: RefCell::new(None),
            hidden_by_lock: RefCell::new(Vec::new()),
            flash_timer: slint::Timer::default(),
            timers: RefCell::new(Vec::new()),
            busy_count: Cell::new(0),
            idle_minutes: Cell::new(0),
            recovery: RefCell::new(None),
            outcome: Cell::new(crate::RunOutcome::Exit),
        })
    }

    /// The main window, while it exists.
    pub fn app(&self) -> Option<AppWindow> {
        self.app.upgrade()
    }

    /// Shows `text` in the status bar for 5 seconds.
    pub fn flash(&self, text: impl Into<SharedString>) {
        let Some(app) = self.app() else { return };
        app.global::<ShellBridge>().set_flash_text(text.into());
        let weak = self.app.clone();
        self.flash_timer
            .start(slint::TimerMode::SingleShot, FLASH_DURATION, move || {
                if let Some(app) = weak.upgrade() {
                    app.global::<ShellBridge>()
                        .set_flash_text(SharedString::new());
                }
            });
    }

    /// Registers a secondary window (hidden while locked). Pass
    /// `Box::new(window.as_weak())`, or a [`crate::shell::modal::BusyOwner`]
    /// for a window that a running background job must disable.
    pub fn register_window(&self, window: Box<dyn SecondaryWindow>) {
        if self.is_busy() {
            window.set_busy(true);
        }
        self.windows.borrow_mut().push(window);
    }

    /// Subscribes to data changes.
    pub fn on_data_changed(&self, f: impl Fn() + 'static) {
        self.data_changed.borrow_mut().push(Box::new(f));
    }

    /// Runs every `data_changed` subscriber. Subscribers may subscribe again
    /// while running; the new ones run from the next notification on.
    pub fn notify_data_changed(&self) {
        run_subscribers(&self.data_changed);
    }

    /// Subscribes to restores: after "Rikthe nga rezervimi" every row id held in
    /// memory may name a different row (or none), so subscribers drop such
    /// state without writing it back (F-016). `data_changed` follows.
    pub fn on_restored(&self, f: impl Fn() + 'static) {
        self.restored.borrow_mut().push(Box::new(f));
    }

    /// Runs every `restored` subscriber (as [`UiContext::notify_data_changed`]).
    pub fn notify_restored(&self) {
        run_subscribers(&self.restored);
    }

    /// Registers the product editor (task 009).
    pub fn set_product_editor(&self, editor: ProductEditor) {
        *self.product_editor.borrow_mut() = Some(editor);
    }

    /// Opens the product editor (task 010), or flashes that it is unavailable.
    pub fn open_product_editor(self: &Rc<Self>, id: ProductId, on_saved: Box<dyn Fn()>) {
        let editor = self.product_editor.borrow();
        match editor.as_ref() {
            Some(open) => open(self, id, on_saved),
            None => {
                drop(editor);
                self.flash(texts::PRODUCT_EDITOR_MISSING);
            }
        }
    }

    /// Whether the lock screen is up.
    pub fn is_locked(&self) -> bool {
        self.app()
            .is_some_and(|app| app.global::<ShellBridge>().get_locked())
    }

    /// Shows the lock screen and hides every visible secondary window. The main
    /// window is natively enabled while locked, even with dialogs open (F-015).
    pub fn lock(&self) {
        let Some(app) = self.app() else { return };
        let bridge = app.global::<ShellBridge>();
        if bridge.get_locked() {
            return;
        }
        let mut windows = self.windows.borrow_mut();
        windows.retain(|w| w.is_alive());
        let mut hidden = self.hidden_by_lock.borrow_mut();
        hidden.clear();
        for (i, w) in windows.iter().enumerate() {
            if w.is_visible() {
                w.hide();
                hidden.push(i);
            }
        }
        self.recovery.borrow_mut().take();
        bridge.set_lock_mode("unlock".into());
        bridge.set_lock_busy(false);
        bridge.set_lock_error(SharedString::new());
        bridge.set_lock_reset(bridge.get_lock_reset() + 1);
        bridge.set_locked(true);
        modal::release_for_lock(&app);
        tracing::info!("locked ({} windows hidden)", hidden.len());
    }

    /// Removes the lock screen and shows the windows the lock hid; the main
    /// window is natively disabled again while a dialog is open over it.
    pub fn unlock(&self) {
        let Some(app) = self.app() else { return };
        let bridge = app.global::<ShellBridge>();
        bridge.set_lock_error(SharedString::new());
        bridge.set_locked(false);
        let windows = self.windows.borrow();
        for i in self.hidden_by_lock.borrow_mut().drain(..) {
            if let Some(w) = windows.get(i) {
                w.show();
            }
        }
        drop(windows);
        modal::reassert_after_unlock(&app);
        tracing::info!("unlocked");
    }

    /// Asks a yes/no question in a modal dialog over the main window; `done`
    /// gets `true` for `ok_label`, `false` for "Anulo" or closing the dialog.
    pub fn confirm(
        self: &Rc<Self>,
        title: &str,
        text: &str,
        ok_label: &str,
        done: impl FnOnce(bool) + 'static,
    ) {
        let _ = modal::confirm(self, title, text, ok_label, done);
    }

    /// Shows a message in a modal dialog over the main window.
    pub fn message(self: &Rc<Self>, title: &str, text: &str) {
        let _ = modal::message(self, title, text);
    }

    /// Shows a service error ("Gabim") with its Albanian text, and logs it.
    pub fn show_error(self: &Rc<Self>, error: &ServiceError) {
        tracing::warn!("{error}");
        self.message(texts::ERROR_TITLE, &error.user_message());
    }

    /// Runs `job` on a worker thread with a clone of the [`Crm`], under the busy
    /// overlay, then `done` with its result on the UI thread.
    pub fn run_bg<T, J, D>(self: &Rc<Self>, job: J, done: D)
    where
        T: Send + 'static,
        J: FnOnce(Crm) -> T + Send + 'static,
        D: FnOnce(T) + 'static,
    {
        worker::run_bg(self, job, done);
    }

    /// Whether a background job started with [`UiContext::run_bg`] is running.
    /// Writes from the secondary windows are refused meanwhile (F-029): a
    /// restore may be replacing the database under them.
    pub fn is_busy(&self) -> bool {
        self.busy_count.get() > 0
    }

    /// Counts one more running background job (busy overlay on; the
    /// registered windows are disabled from the first one on, F-029).
    pub(crate) fn begin_busy(&self) {
        let running = self.busy_count.get() + 1;
        self.busy_count.set(running);
        if let Some(app) = self.app() {
            app.global::<ShellBridge>().set_busy(true);
        }
        if running == 1 {
            self.set_windows_busy(true);
        }
    }

    /// Counts one finished background job (busy overlay off and the windows
    /// enabled again after the last).
    pub(crate) fn end_busy(&self) {
        let left = self.busy_count.get().saturating_sub(1);
        self.busy_count.set(left);
        if left == 0 {
            if let Some(app) = self.app() {
                app.global::<ShellBridge>().set_busy(false);
            }
            self.set_windows_busy(false);
        }
    }

    /// Tells every registered window that the busy state changed. Dead
    /// entries are skipped, not removed: the lock keeps indices into the list.
    fn set_windows_busy(&self, busy: bool) {
        for w in self.windows.borrow().iter() {
            w.set_busy(busy);
        }
    }

    /// How the event loop ended: [`crate::RunOutcome::Wiped`] after a wipe from
    /// the lock screen.
    pub fn outcome(&self) -> crate::RunOutcome {
        self.outcome.get()
    }

    /// Keeps a periodic timer running for the life of the context.
    pub(crate) fn keep_timer(&self, timer: slint::Timer) {
        self.timers.borrow_mut().push(timer);
    }
}

/// Runs the subscribers in `slot`; ones added while running are kept for the
/// next run.
fn run_subscribers(slot: &RefCell<Vec<Box<dyn Fn()>>>) {
    let running = std::mem::take(&mut *slot.borrow_mut());
    for f in &running {
        f();
    }
    let mut slot = slot.borrow_mut();
    let added = std::mem::replace(&mut *slot, running);
    slot.extend(added);
}

//! Modal dialogs. Slint windows are not modal by themselves, so the owner window
//! gets `input-blocked` (a dimmed overlay that swallows input) while a dialog is
//! open; on Windows the owner is also disabled with `EnableWindow`. While the
//! lock screen is up the main window is natively enabled again (F-015): the
//! lock only hides the dialogs, so the password field must still get input.
//!
//! A running background job disables the registered [`BusyOwner`] windows the
//! same way (F-029). The two blocks are tracked apart: closing the last dialog
//! does not enable a window the job still holds, and the job's end does not
//! enable a window a dialog is still open over.

use crate::context::SecondaryWindow;
use crate::shell::{texts, win};
use crate::{AppWindow, ConfirmDialog, MessageDialog, ShellBridge, UiContext};
use slint::{CloseRequestResponse, ComponentHandle};
use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// A window that dialogs can be modal over. Task 009 implements it for its list
/// windows (an `in-out property <bool> input-blocked` showing `InputBlocker`).
pub trait ModalOwner: ComponentHandle + 'static {
    /// Shows or hides the owner's input blocker.
    fn set_input_blocked(&self, blocked: bool);
    /// The owner's window title (finds the native window on Windows).
    fn native_title(&self) -> String;
    /// Shows or hides the owner's busy blocker while a background job runs
    /// (F-029; only for owners registered as a [`BusyOwner`]).
    fn set_busy(&self, _busy: bool) {}
}

impl ModalOwner for AppWindow {
    fn set_input_blocked(&self, blocked: bool) {
        self.global::<ShellBridge>().set_input_blocked(blocked);
    }

    fn native_title(&self) -> String {
        "CRM".to_owned()
    }
}

thread_local! {
    /// Open modal dialogs per owner window (nested dialogs keep the block).
    static BLOCKS: RefCell<HashMap<usize, u32>> = RefCell::new(HashMap::new());
    /// Owners natively enabled for the lock screen despite open dialogs.
    static RELEASED: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
    /// Owners disabled by a running background job (F-029).
    static BUSY: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
}

/// A secondary window that a running background job disables (F-029): its
/// busy blocker is shown and, on Windows, the native window is disabled. Pass
/// `Box::new(BusyOwner(window.as_weak()))` to [`UiContext::register_window`].
pub struct BusyOwner<O: ModalOwner>(pub slint::Weak<O>);

impl<O: ModalOwner> SecondaryWindow for BusyOwner<O> {
    fn is_alive(&self) -> bool {
        SecondaryWindow::is_alive(&self.0)
    }

    fn is_visible(&self) -> bool {
        SecondaryWindow::is_visible(&self.0)
    }

    fn show(&self) {
        SecondaryWindow::show(&self.0);
    }

    fn hide(&self) {
        SecondaryWindow::hide(&self.0);
    }

    fn set_busy(&self, busy: bool) {
        if let Some(owner) = self.0.upgrade() {
            if busy {
                hold_for_busy(&owner);
            } else {
                release_from_busy(&owner);
            }
        }
    }
}

/// Whether a background job holds `owner` disabled.
pub fn is_busy_held<O: ComponentHandle>(owner: &O) -> bool {
    BUSY.with(|b| b.borrow().contains(&owner_key(owner)))
}

fn hold_for_busy<O: ModalOwner>(owner: &O) {
    let newly = BUSY.with(|b| b.borrow_mut().insert(owner_key(owner)));
    owner.set_busy(true);
    // An owner under a dialog is natively disabled already.
    if newly && !is_blocked(owner) {
        win::set_window_enabled(&owner.native_title(), false);
    }
}

fn release_from_busy<O: ModalOwner>(owner: &O) {
    let held = BUSY.with(|b| b.borrow_mut().remove(&owner_key(owner)));
    owner.set_busy(false);
    // A dialog still open over the owner keeps it disabled.
    if held && !is_blocked(owner) {
        win::set_window_enabled(&owner.native_title(), true);
    }
}

/// Whether a modal dialog is open over `owner`.
pub fn is_blocked<O: ComponentHandle>(owner: &O) -> bool {
    BLOCKS.with(|b| b.borrow().get(&owner_key(owner)).is_some_and(|&n| n > 0))
}

/// The lock screen came up over `owner`: enable it natively even while its
/// dialogs are open (they are hidden), so the lock screen gets the input.
pub(crate) fn release_for_lock<O: ModalOwner>(owner: &O) {
    RELEASED.with(|r| r.borrow_mut().insert(owner_key(owner)));
    if is_blocked(owner) {
        win::set_window_enabled(&owner.native_title(), true);
    }
}

/// The lock screen went away (its dialogs are shown again): disable `owner`
/// natively again while a dialog is still open over it.
pub(crate) fn reassert_after_unlock<O: ModalOwner>(owner: &O) {
    RELEASED.with(|r| r.borrow_mut().remove(&owner_key(owner)));
    if is_blocked(owner) {
        win::set_window_enabled(&owner.native_title(), false);
    }
}

fn is_released<O: ComponentHandle>(owner: &O) -> bool {
    RELEASED.with(|r| r.borrow().contains(&owner_key(owner)))
}

fn owner_key<O: ComponentHandle>(owner: &O) -> usize {
    std::ptr::from_ref(owner.window()) as usize
}

fn block<O: ModalOwner>(owner: &O) {
    let count = BLOCKS.with(|b| {
        let mut b = b.borrow_mut();
        let count = b.entry(owner_key(owner)).or_insert(0);
        *count += 1;
        *count
    });
    if count == 1 {
        owner.set_input_blocked(true);
        // A dialog opened while locked (a finished background job) must not
        // disable the lock screen; the unlock disables the owner then.
        if !is_released(owner) {
            win::set_window_enabled(&owner.native_title(), false);
        }
    }
}

fn unblock<O: ModalOwner>(owner: &O) {
    let left = BLOCKS.with(|b| {
        let mut b = b.borrow_mut();
        let key = owner_key(owner);
        let left = b.get(&key).copied().unwrap_or(1).saturating_sub(1);
        if left == 0 {
            b.remove(&key);
        } else {
            b.insert(key, left);
        }
        left
    });
    if left == 0 {
        // A running background job keeps holding the owner (F-029); its end
        // enables it.
        if !is_busy_held(owner) {
            win::set_window_enabled(&owner.native_title(), true);
        }
        owner.set_input_blocked(false);
    }
}

struct ModalState {
    /// Keeps the dialog alive while it is open.
    dialog: Option<Box<dyn Any>>,
    hide: Option<Box<dyn FnOnce()>>,
    unblock: Option<Box<dyn FnOnce()>>,
    on_closed: Vec<Box<dyn FnOnce()>>,
}

/// An open modal dialog. Clone it into the dialog's callbacks and call
/// [`Modal::close`] when the dialog is done; closing twice is harmless.
#[derive(Clone)]
pub struct Modal(Rc<RefCell<ModalState>>);

impl Modal {
    /// Hides the dialog, unblocks the owner and runs the `on_closed` hooks.
    pub fn close(&self) {
        let (unblock, hide, hooks, dialog) = {
            let mut s = self.0.borrow_mut();
            (
                s.unblock.take(),
                s.hide.take(),
                std::mem::take(&mut s.on_closed),
                s.dialog.take(),
            )
        };
        // Re-enable the owner before hiding, so Windows activates it (not
        // another application).
        if let Some(unblock) = unblock {
            unblock();
        }
        if let Some(hide) = hide {
            hide();
        }
        for hook in hooks {
            hook();
        }
        drop(dialog);
    }

    /// Whether the dialog is still open.
    pub fn is_open(&self) -> bool {
        self.0.borrow().hide.is_some()
    }

    /// Runs `f` once when the dialog closes (by any route, also the title-bar
    /// close button).
    pub fn on_closed(&self, f: impl FnOnce() + 'static) {
        if self.is_open() {
            self.0.borrow_mut().on_closed.push(Box::new(f));
        } else {
            f();
        }
    }
}

/// Shows `dialog` modal over `owner` and returns its handle. The title-bar close
/// button closes it like [`Modal::close`].
pub fn open<O: ModalOwner, D: ComponentHandle + 'static>(owner: &O, dialog: &D) -> Modal {
    block(owner);
    let owner_weak = owner.as_weak();
    let dialog_weak = dialog.as_weak();
    let modal = Modal(Rc::new(RefCell::new(ModalState {
        dialog: Some(Box::new(dialog.clone_strong())),
        hide: Some(Box::new(move || {
            if let Some(d) = dialog_weak.upgrade()
                && let Err(e) = d.hide()
            {
                tracing::warn!("hiding a dialog failed: {e}");
            }
        })),
        unblock: Some(Box::new(move || {
            if let Some(o) = owner_weak.upgrade() {
                unblock(&o);
            }
        })),
        on_closed: Vec::new(),
    })));
    let on_close = modal.clone();
    dialog.window().on_close_requested(move || {
        on_close.close();
        CloseRequestResponse::HideWindow
    });
    if let Err(e) = dialog.show() {
        tracing::warn!("showing a dialog failed: {e}");
        modal.close();
    }
    modal
}

/// Opens `dialog` modal over the main window and registers it with the lock.
pub fn open_over_app<D: ComponentHandle + 'static>(ctx: &UiContext, dialog: &D) -> Option<Modal> {
    let app = ctx.app()?;
    ctx.register_window(Box::new(dialog.as_weak()));
    Some(open(&app, dialog))
}

/// The pending answer of a confirm dialog.
type Answer = Box<dyn FnOnce(bool)>;

/// The yes/no dialog behind [`UiContext::confirm`] (returned for tests).
pub fn confirm(
    ctx: &Rc<UiContext>,
    title: &str,
    text: &str,
    ok_label: &str,
    done: impl FnOnce(bool) + 'static,
) -> Option<ConfirmDialog> {
    let dialog = match ConfirmDialog::new() {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("confirm dialog: {e}");
            return None;
        }
    };
    dialog.set_caption(title.into());
    dialog.set_text(text.into());
    dialog.set_ok_label(ok_label.into());
    let modal = open_over_app(ctx, &dialog)?;
    let done: Rc<RefCell<Option<Answer>>> = Rc::new(RefCell::new(Some(Box::new(done))));
    let answer = {
        let done = done.clone();
        move |ok: bool| {
            let f = done.borrow_mut().take();
            if let Some(f) = f {
                f(ok);
            }
        }
    };
    let on_answer = modal.clone();
    let answer_ok = answer.clone();
    dialog.on_answered(move |ok| {
        answer_ok(ok);
        on_answer.close();
    });
    modal.on_closed(move || answer(false));
    Some(dialog)
}

/// The message box behind [`UiContext::message`] (returned for tests).
pub fn message(ctx: &Rc<UiContext>, title: &str, text: &str) -> Option<MessageDialog> {
    let dialog = match MessageDialog::new() {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("message dialog: {e}");
            return None;
        }
    };
    dialog.set_caption(
        if title.is_empty() {
            texts::INFO_TITLE
        } else {
            title
        }
        .into(),
    );
    dialog.set_text(text.into());
    let modal = open_over_app(ctx, &dialog)?;
    dialog.on_closed(move || modal.close());
    Some(dialog)
}

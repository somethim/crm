//! The workspace's dialogs: thin wrappers over `shell::modal` that keep the
//! last dialog for the headless tests.

use crate::UiContext;
use crate::shell::{modal, texts as shell_texts};
use crm_services::ServiceError;
use std::rc::Rc;

#[cfg(test)]
thread_local! {
    pub(crate) static LAST_CONFIRM: std::cell::RefCell<Option<crate::ConfirmDialog>> =
        const { std::cell::RefCell::new(None) };
    pub(crate) static LAST_MESSAGE: std::cell::RefCell<Option<crate::MessageDialog>> =
        const { std::cell::RefCell::new(None) };
}

/// A yes/no question ("Anulo" / `ok_label`).
pub fn confirm(
    ctx: &Rc<UiContext>,
    title: &str,
    text: &str,
    ok_label: &str,
    done: impl FnOnce(bool) + 'static,
) {
    let dialog = modal::confirm(ctx, title, text, ok_label, done);
    #[cfg(test)]
    LAST_CONFIRM.with(|d| *d.borrow_mut() = dialog);
    #[cfg(not(test))]
    drop(dialog);
}

/// A message box.
pub fn message(ctx: &Rc<UiContext>, title: &str, text: &str) {
    let dialog = modal::message(ctx, title, text);
    #[cfg(test)]
    LAST_MESSAGE.with(|d| *d.borrow_mut() = dialog);
    #[cfg(not(test))]
    drop(dialog);
}

/// A service error ("Gabim" with its Albanian text).
pub fn error(ctx: &Rc<UiContext>, e: &ServiceError) {
    tracing::warn!("{e}");
    message(ctx, shell_texts::ERROR_TITLE, &e.user_message());
}

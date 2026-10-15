//! Headless unit tests of the shell parts that need crate internals (the
//! native-window calls are recorded instead of reaching Win32).

use super::{modal, win};
use crate::{AppWindow, ShellBridge, UiContext};
use crm_services::{Crm, DataPaths};
use slint::ComponentHandle;
use std::rc::Rc;

struct Harness {
    app: AppWindow,
    ctx: Rc<UiContext>,
    _dir: tempfile::TempDir,
}

/// The app over a fresh database, unlocked.
fn harness() -> Harness {
    i_slint_backend_testing::init_no_event_loop();
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    paths.ensure_dirs().unwrap();
    let crm = Crm::open(paths.clone(), &crm_services::DbKey::for_tests(1)).unwrap();
    let (app, ctx) = crate::build(
        crm,
        crm_services::Vault::with_params(paths.clone(), crm_services::KdfParams::TEST),
        paths,
    )
    .unwrap();
    app.show().unwrap();
    ctx.unlock();
    win::take_enabled_calls();
    Harness {
        app,
        ctx,
        _dir: dir,
    }
}

fn main(enabled: bool) -> (String, bool) {
    ("CRM".to_owned(), enabled)
}

#[test]
fn the_lock_screen_enables_the_main_window_under_an_open_dialog() {
    let h = harness();
    let dialog = modal::message(&h.ctx, "", "x").unwrap();
    assert_eq!(win::take_enabled_calls(), [main(false)]);
    assert!(modal::is_blocked(&h.app));

    h.ctx.lock();
    assert!(!dialog.window().is_visible(), "the lock hides the dialog");
    assert_eq!(
        win::take_enabled_calls(),
        [main(true)],
        "the lock screen must get the input"
    );

    h.ctx.unlock();
    assert!(dialog.window().is_visible(), "the dialog is back");
    assert_eq!(win::take_enabled_calls(), [main(false)]);
    assert!(h.app.global::<ShellBridge>().get_input_blocked());

    dialog.invoke_closed();
    assert_eq!(win::take_enabled_calls(), [main(true)]);
    assert!(!modal::is_blocked(&h.app));
}

#[test]
fn lock_and_unlock_without_dialogs_leave_the_main_window_alone() {
    let h = harness();
    h.ctx.lock();
    h.ctx.unlock();
    assert_eq!(win::take_enabled_calls(), []);
}

#[test]
fn a_dialog_opened_while_locked_keeps_the_lock_screen_usable() {
    let h = harness();
    h.ctx.lock();
    // A background job finishing while locked shows its message.
    let dialog = modal::message(&h.ctx, "", "x").unwrap();
    assert_eq!(win::take_enabled_calls(), [], "not disabled while locked");
    h.ctx.unlock();
    assert_eq!(win::take_enabled_calls(), [main(false)]);
    dialog.invoke_closed();
    assert_eq!(win::take_enabled_calls(), [main(true)]);

    // Closed while still locked: enabled, and the unlock changes nothing.
    h.ctx.lock();
    let dialog = modal::message(&h.ctx, "", "y").unwrap();
    dialog.invoke_closed();
    assert_eq!(win::take_enabled_calls(), [main(true)]);
    h.ctx.unlock();
    assert_eq!(win::take_enabled_calls(), []);
}

#[test]
fn the_lock_screen_enables_the_main_window_under_the_print_preview() {
    use crate::print::{preview, texts as print_texts};
    use crm_services::domain::OrderId;
    let h = harness();
    // The preview of a missing invoice: a message box over the preview.
    let p = preview::open_preview(&h.ctx, OrderId(999)).unwrap();
    p.apply(preview::prepare(
        &h.ctx.crm,
        OrderId(999),
        preview::preview_dpi(1.0),
    ));
    assert!(
        p.window().get_input_blocked(),
        "the message blocks the preview"
    );
    let title = print_texts::PREVIEW_TITLE.to_owned();
    assert_eq!(
        win::take_enabled_calls(),
        [main(false), (title.clone(), false)]
    );

    h.ctx.lock();
    assert!(
        !p.window().window().is_visible(),
        "the lock hides the preview"
    );
    assert_eq!(win::take_enabled_calls(), [main(true)]);

    h.ctx.unlock();
    assert!(p.window().window().is_visible());
    assert_eq!(
        win::take_enabled_calls(),
        [main(false)],
        "the preview (still under its message) and the main window stay blocked"
    );
    assert!(p.window().get_input_blocked());
}

// -- F-029: a running background job disables the list windows --------------

fn products(enabled: bool) -> (String, bool) {
    ("Artikujt".to_owned(), enabled)
}

/// The Artikujt window, open, with no native calls recorded.
fn products_window(
    h: &Harness,
) -> Rc<crate::catalogue::list::ListController<crate::ProductsWindow>> {
    crate::catalogue::open_list(&h.ctx, crate::catalogue::ListKind::Products);
    let ctrl = crate::catalogue::products_list(&h.ctx).unwrap();
    win::take_enabled_calls();
    ctrl
}

#[test]
fn a_background_job_disables_the_list_windows_until_the_last_one_ends() {
    use crate::CatalogueList;
    let h = harness();
    let ctrl = products_window(&h);
    let list = ctrl.window().global::<CatalogueList>();

    h.ctx.begin_busy();
    assert!(h.ctx.is_busy());
    assert!(list.get_busy(), "the list shows its blocker");
    assert_eq!(win::take_enabled_calls(), [products(false)]);
    h.ctx.begin_busy();
    h.ctx.end_busy();
    assert!(list.get_busy(), "the second job still runs");
    assert_eq!(win::take_enabled_calls(), []);

    h.ctx.end_busy();
    assert!(!h.ctx.is_busy());
    assert!(!list.get_busy());
    assert_eq!(win::take_enabled_calls(), [products(true)]);
}

#[test]
fn the_end_of_a_job_keeps_a_list_window_under_its_dialog_disabled() {
    let h = harness();
    let ctrl = products_window(&h);
    ctrl.add();
    let dialog = crate::catalogue::dialogs::open_product_dialog().expect("create dialog");
    assert_eq!(win::take_enabled_calls(), [products(false)]);

    h.ctx.begin_busy();
    assert_eq!(win::take_enabled_calls(), [], "disabled already");
    h.ctx.end_busy();
    assert_eq!(win::take_enabled_calls(), [], "the dialog is still open");
    assert!(modal::is_blocked(ctrl.window()));

    dialog.invoke_cancel();
    assert_eq!(win::take_enabled_calls(), [products(true)]);
}

#[test]
fn closing_a_dialog_during_a_job_keeps_the_list_window_disabled() {
    let h = harness();
    let ctrl = products_window(&h);
    h.ctx.begin_busy();
    assert_eq!(win::take_enabled_calls(), [products(false)]);

    // A job's message over the list window, shown and closed while another runs.
    let message = crate::catalogue::dialogs::message_over(&h.ctx, ctrl.window(), "", "x").unwrap();
    win::take_enabled_calls();
    message.invoke_closed();
    assert_eq!(
        win::take_enabled_calls(),
        [],
        "the job still holds the window"
    );
    assert!(modal::is_busy_held(ctrl.window()));

    h.ctx.end_busy();
    assert_eq!(win::take_enabled_calls(), [products(true)]);
    assert!(!modal::is_busy_held(ctrl.window()));
}

#[test]
fn a_list_window_opened_during_a_job_starts_disabled() {
    use crate::CatalogueList;
    let h = harness();
    h.ctx.begin_busy();
    let ctrl = products_window(&h);
    assert!(ctrl.window().global::<CatalogueList>().get_busy());
    h.ctx.end_busy();
    assert!(!ctrl.window().global::<CatalogueList>().get_busy());
    assert_eq!(win::take_enabled_calls(), [products(true)]);
}

#[test]
fn the_main_window_is_not_natively_disabled_by_a_job() {
    let h = harness();
    h.ctx.begin_busy();
    h.ctx.end_busy();
    assert_eq!(win::take_enabled_calls(), [], "only the overlay covers it");
    assert!(!h.app.global::<ShellBridge>().get_busy());
}

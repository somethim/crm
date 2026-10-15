//! F-028: a background job that panics ends with the standard error message
//! and the busy overlay removed, instead of the action just stopping. One test
//! per file: the event-loop backend can be initialised once per process.

use crm_services::{Crm, DataPaths};
use crm_ui::ShellBridge;
use crm_ui::shell::{texts, worker};
use slint::ComponentHandle;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::mpsc;

/// Re-checks on the event loop until the failure message is shown (the
/// panicked job's completion is queued by its own thread), then quits.
fn quit_when_shown(tries: u32) {
    if worker::last_job_failure().is_some() || tries == 0 {
        slint::quit_event_loop().unwrap();
        return;
    }
    std::thread::sleep(std::time::Duration::from_millis(2));
    slint::invoke_from_event_loop(move || quit_when_shown(tries - 1)).unwrap();
}

#[test]
fn a_panicking_job_shows_the_error_and_clears_the_busy_overlay() {
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    paths.ensure_dirs().unwrap();
    let crm = Crm::open(paths.clone(), &crm_services::DbKey::for_tests(1)).unwrap();
    let (app, ctx) = crm_ui::build(
        crm,
        crm_services::Vault::with_params(paths.clone(), crm_services::KdfParams::TEST),
        paths,
    )
    .unwrap();
    app.show().unwrap();
    assert!(worker::last_job_failure().is_none());

    // The second job waits until the first one has unwound (its sender drops).
    let (tx, rx) = mpsc::channel::<()>();
    let done_ran = Rc::new(Cell::new(false));
    let d = done_ran.clone();
    ctx.run_bg(
        move |_crm| -> i32 {
            let _tx = tx;
            panic!("test panic");
        },
        move |_| d.set(true),
    );
    ctx.run_bg(move |_crm| rx.recv().is_err(), |_| quit_when_shown(2000));
    assert!(app.global::<ShellBridge>().get_busy());

    slint::run_event_loop_until_quit().unwrap();

    assert!(!done_ran.get(), "done of a panicked job never runs");
    let dialog = worker::last_job_failure().expect("the failure message is shown");
    assert_eq!(dialog.get_caption(), texts::ERROR_TITLE);
    assert_eq!(dialog.get_text(), texts::JOB_FAILED);
    assert!(
        !app.global::<ShellBridge>().get_busy(),
        "busy cleared after both jobs"
    );
}

//! `run_bg` with a real (testing) event loop: the job runs off the UI thread,
//! `done` runs on it afterwards, and the busy overlay covers the wait. One test
//! per file: the event-loop backend can be initialised once per process.

use crm_services::{Crm, DataPaths};
use crm_ui::ShellBridge;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;

#[test]
fn run_bg_runs_the_job_off_the_ui_thread_then_done_on_it() {
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
    let ui_thread = thread::current().id();
    let events = Rc::new(RefCell::new(Vec::new()));

    // The second job waits until the first one finished on the UI thread.
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let e = events.clone();
    let weak = app.as_weak();
    ctx.run_bg(
        move |crm| {
            assert_ne!(thread::current().id(), ui_thread);
            crm.idle_lock_minutes().unwrap()
        },
        move |minutes| {
            assert_eq!(thread::current().id(), ui_thread);
            let busy = weak.upgrade().unwrap().global::<ShellBridge>().get_busy();
            e.borrow_mut()
                .push(format!("first done: minutes={minutes} busy={busy}"));
            release_tx.send(()).unwrap();
        },
    );
    let e = events.clone();
    let weak = app.as_weak();
    ctx.run_bg(
        move |_crm| {
            release_rx.recv().unwrap();
            42
        },
        move |value| {
            let busy = weak.upgrade().unwrap().global::<ShellBridge>().get_busy();
            e.borrow_mut()
                .push(format!("second done: {value} busy={busy}"));
            slint::quit_event_loop().unwrap();
        },
    );
    assert!(
        app.global::<ShellBridge>().get_busy(),
        "busy while jobs run"
    );
    events.borrow_mut().push("scheduled".into());

    slint::run_event_loop_until_quit().unwrap();

    assert_eq!(
        *events.borrow(),
        [
            "scheduled",
            // The second job still runs, so the overlay stays.
            "first done: minutes=0 busy=true",
            "second done: 42 busy=false",
        ]
    );
    assert!(!app.global::<ShellBridge>().get_busy());
}

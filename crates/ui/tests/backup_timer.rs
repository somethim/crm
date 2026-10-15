//! The periodic daily-backup check (F-018) with a real (testing) event loop:
//! it takes the day's copy off the UI thread, without the busy overlay, and
//! refreshes the status bar. One test per file: the event-loop backend can be
//! initialised once per process.

use crm_services::{Crm, DataPaths};
use crm_ui::ShellBridge;
use crm_ui::shell::timers;
use slint::ComponentHandle;
use std::cell::Cell;
use std::rc::Rc;

#[test]
fn the_backup_check_takes_the_missing_daily_copy_and_updates_the_status_bar() {
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    paths.ensure_dirs().unwrap();
    let crm = Crm::open(paths.clone(), &crm_services::DbKey::for_tests(1)).unwrap();
    let (app, ctx) = crm_ui::build(
        crm,
        crm_services::Vault::with_params(paths.clone(), crm_services::KdfParams::TEST),
        paths.clone(),
    )
    .unwrap();
    let daily = |crm: &Crm| {
        crm.list_backups()
            .unwrap()
            .into_iter()
            .filter(|b| b.name.reason.is_none())
            .count()
    };
    // Opening took today's copy; as if the day changed since: remove it.
    assert_eq!(daily(&ctx.crm), 1);
    for entry in std::fs::read_dir(&paths.backups).unwrap() {
        std::fs::remove_file(entry.unwrap().path()).unwrap();
    }
    app.global::<ShellBridge>().set_last_backup("".into());

    let busy_seen = Rc::new(Cell::new(false));
    let seen = busy_seen.clone();
    let weak = app.as_weak();
    timers::backup_check(&ctx, move || {
        seen.set(weak.upgrade().unwrap().global::<ShellBridge>().get_busy());
        slint::quit_event_loop().unwrap();
    });
    assert!(!app.global::<ShellBridge>().get_busy(), "no busy overlay");
    slint::run_event_loop_until_quit().unwrap();

    assert!(!busy_seen.get());
    assert_eq!(daily(&ctx.crm), 1, "a new daily copy");
    let status = app.global::<ShellBridge>().get_last_backup();
    assert!(!status.is_empty(), "the status bar shows the new copy");
    // Due no more: a second check takes nothing.
    timers::backup_check(&ctx, || slint::quit_event_loop().unwrap());
    slint::run_event_loop_until_quit().unwrap();
    assert_eq!(daily(&ctx.crm), 1);
}

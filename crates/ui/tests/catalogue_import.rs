//! The import dialog with a real (testing) event loop: KLIENTET.csv through the
//! dialog's Rust handler, on the worker thread, then the result view and the
//! refreshed Klientët list. One test per file: the event-loop backend can be
//! initialised once per process.

use crm_services::{Crm, DataPaths};
use crm_ui::catalogue::{self, ListKind};
use crm_ui::{CatalogueList, ShellBridge};
use slint::{ComponentHandle, Model};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[test]
fn catalogue_import_of_klientet_reports_the_counts_and_the_backup() {
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
    ctx.unlock();

    catalogue::open_list(&ctx, ListKind::Clients);
    let ctrl = catalogue::clients_list(&ctx).unwrap();
    assert_eq!(ctrl.row_count(), 0);
    // F-001: an own import is not reported by the poll, so it notifies the
    // other windows itself.
    let notified = std::rc::Rc::new(std::cell::Cell::new(0));
    let n = notified.clone();
    ctx.on_data_changed(move || n.set(n.get() + 1));

    let flow = catalogue::transfer::open_import(&ctx, ctrl.window(), ListKind::Clients).unwrap();
    let dialog = flow.dialog().unwrap();
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../io/tests/fixtures/KLIENTET.csv");
    flow.start_with(&fixture);
    assert!(dialog.get_busy(), "the import runs in the background");

    wait_for(dialog.as_weak(), |d| d.get_finished() && !d.get_busy());

    let counts = dialog.get_counts_text().to_string();
    assert!(counts.contains("U krijuan: 2931"), "{counts}");
    assert!(counts.contains("U përditësuan: 0"), "{counts}");
    assert!(counts.contains("U anashkaluan: 0"), "{counts}");
    assert!(counts.contains("Dështuan: 0"), "{counts}");
    assert_eq!(dialog.get_errors().row_count(), 0);
    let backup = dialog.get_backup_text().to_string();
    assert!(
        backup.starts_with("Para importimit u krijua një rezervim: crm-")
            && backup.ends_with("-pre-import-clients.db"),
        "{backup}"
    );
    assert!(!app.global::<ShellBridge>().get_busy());

    assert_eq!(notified.get(), 1, "the import notified the other windows");
    // The open Klientët list was refreshed.
    assert_eq!(ctrl.row_count(), 2931);
    assert_eq!(
        ctrl.window().global::<CatalogueList>().get_status_text(),
        "2931 klientë"
    );

    // Mbyll closes the dialog and unblocks the list.
    dialog.invoke_dismiss();
    assert!(!ctrl.window().global::<CatalogueList>().get_input_blocked());

    // Export the clients again (the save dialog's chosen-path handler).
    let out = dir.path().join("klientet-eksport.csv");
    catalogue::transfer::export_to(&ctx, ctrl.window(), ListKind::Clients, out.clone());
    assert!(
        ctrl.window().global::<CatalogueList>().get_input_blocked(),
        "the list is covered while the file is written"
    );
    wait_for(app.as_weak(), |a| {
        !a.global::<ShellBridge>().get_flash_text().is_empty()
    });
    assert_eq!(
        app.global::<ShellBridge>().get_flash_text(),
        "Eksportimi u ruajt: klientet-eksport.csv"
    );
    assert!(!ctrl.window().global::<CatalogueList>().get_input_blocked());
    let written = std::fs::read_to_string(&out).unwrap();
    assert_eq!(written.lines().count(), 2932, "header + 2931 clients");
}

/// Runs the event loop until `done` holds for the component (checked from a
/// helper thread every 20 ms).
fn wait_for<T: ComponentHandle + 'static>(weak: slint::Weak<T>, done: fn(&T) -> bool) {
    let stop = Arc::new(AtomicBool::new(false));
    let poller = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let stop = stop.clone();
                let _ = weak.upgrade_in_event_loop(move |c| {
                    if done(&c) && !stop.swap(true, Ordering::SeqCst) {
                        let _ = slint::quit_event_loop();
                    }
                });
                std::thread::sleep(Duration::from_millis(20));
            }
        })
    };
    slint::run_event_loop_until_quit().unwrap();
    stop.store(true, Ordering::SeqCst);
    poller.join().unwrap();
}

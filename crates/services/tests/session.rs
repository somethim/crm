//! Settings, tabs/drafts and backups through the facade (007 step 5, AC 7–8).
//! The password lock is tested in `vault.rs`.

mod common;

use common::{buy_line, draft, line, open, product, stock};
use crm_services::domain::{OrderKind, Qty};
use crm_services::{Fault, OpenTab, ReceiptSettings, ServiceError};

#[test]
fn settings_ranges() {
    let (_dir, crm) = open();
    assert_eq!(crm.idle_lock_minutes().unwrap(), 0);
    crm.set_idle_lock_minutes(120).unwrap();
    assert_eq!(crm.idle_lock_minutes().unwrap(), 120);
    let err = crm.set_idle_lock_minutes(121).unwrap_err();
    assert_eq!(err.user_message(), "Vlera duhet të jetë nga 0 deri në 120.");

    assert_eq!(
        crm.receipt_settings().unwrap(),
        ReceiptSettings {
            width_mm: 72,
            font_pt: 9
        }
    );
    let set = ReceiptSettings {
        width_mm: 80,
        font_pt: 10,
    };
    crm.set_receipt_settings(set).unwrap();
    assert_eq!(crm.receipt_settings().unwrap(), set);
    let err = crm
        .set_receipt_settings(ReceiptSettings {
            width_mm: 47,
            font_pt: 15,
        })
        .unwrap_err();
    let ServiceError::Validation(errors) = &err else {
        panic!("{err:?}")
    };
    assert_eq!(
        errors.messages_for("width_mm"),
        ["Vlera duhet të jetë nga 48 deri në 110."]
    );
    assert_eq!(
        errors.messages_for("font_pt"),
        ["Vlera duhet të jetë nga 6 deri në 14."]
    );
    assert_eq!(crm.receipt_settings().unwrap(), set);
}

/// Writes raw receipt settings through a second storage handle, bypassing the
/// facade's range check (a database edited outside the app, F-021).
fn store_raw_receipt_settings(root: &std::path::Path, width_mm: u32, font_pt: u32) {
    use crm_storage::repo::settings;
    let db = crm_storage::Db::open(
        &crm_storage::DataPaths::at(root),
        &crm_services::DbKey::for_tests(1),
    )
    .unwrap();
    db.write(move |tx| -> Result<(), crm_storage::StorageError> {
        settings::set_receipt_width_mm(tx, width_mm)?;
        settings::set_receipt_font_pt(tx, font_pt)
    })
    .unwrap();
}

#[test]
fn out_of_range_stored_receipt_settings_read_as_the_defaults() {
    let (dir, crm) = open();
    let defaults = ReceiptSettings {
        width_mm: 72,
        font_pt: 9,
    };
    for (width, font) in [
        (999_999, 0),
        (0, 999_999),
        (u32::MAX, 15),
        (47, 5),
        (111, u32::MAX),
    ] {
        store_raw_receipt_settings(dir.path(), width, font);
        assert_eq!(
            crm.receipt_settings().unwrap(),
            defaults,
            "{width} / {font}"
        );
    }
    // Each field falls back on its own; the range edges stay as stored.
    store_raw_receipt_settings(dir.path(), 4_000_000_000, 14);
    assert_eq!(
        crm.receipt_settings().unwrap(),
        ReceiptSettings {
            width_mm: 72,
            font_pt: 14
        }
    );
    store_raw_receipt_settings(dir.path(), 48, 6);
    assert_eq!(
        crm.receipt_settings().unwrap(),
        ReceiptSettings {
            width_mm: 48,
            font_pt: 6
        }
    );
    store_raw_receipt_settings(dir.path(), 110, 0);
    assert_eq!(
        crm.receipt_settings().unwrap(),
        ReceiptSettings {
            width_mm: 110,
            font_pt: 9
        }
    );
}

fn tab(key: &str, position: i64, active: bool) -> OpenTab {
    OpenTab {
        tab_key: key.into(),
        position,
        kind: OrderKind::Purchase,
        order_id: None,
        title: OrderKind::Purchase.new_tab_title().into(),
        active,
        draft: None,
    }
}

#[test]
fn tabs_and_drafts_round_trip() {
    let (_dir, crm) = open();
    let mut d = draft(
        OrderKind::Purchase,
        "Furnitori Alfa",
        vec![buy_line(None, "Raki", 3, 800, 1_000), Default::default()],
    );
    d.owner_description = "Tiranë".into();
    d.issued_on = jiff::civil::date(2026, 10, 14);
    let first = OpenTab {
        draft: Some(d.clone()),
        ..tab("aaaa", 0, true)
    };
    crm.save_tab(first.clone()).unwrap();
    crm.save_tab(tab("bbbb", 1, true)).unwrap();
    crm.save_tab(tab("cccc", 2, false)).unwrap();

    let tabs = crm.list_tabs().unwrap();
    assert_eq!(tabs.len(), 3);
    assert_eq!(tabs[0].draft.as_ref(), Some(&d), "draft JSON round trip");
    // Only the last saved active tab stays active.
    let active: Vec<_> = tabs
        .iter()
        .filter(|t| t.active)
        .map(|t| t.tab_key.as_str())
        .collect();
    assert_eq!(active, ["bbbb"]);

    assert!(crm.remove_tab("cccc").unwrap());
    assert!(!crm.remove_tab("cccc").unwrap());
    crm.clear_tabs_except("aaaa").unwrap();
    let tabs = crm.list_tabs().unwrap();
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0].tab_key, "aaaa");
    crm.clear_tabs().unwrap();
    assert!(crm.list_tabs().unwrap().is_empty());
}

#[test]
fn restore_round_trip_through_the_facade() {
    let (dir, crm) = open();
    let p = product(&crm, "P1", "BRIZO", 10, 100, 200);
    let backup = crm.backup_now().unwrap();
    assert!(backup.starts_with(dir.path().join("backups")));

    // Changes after the backup.
    crm.save_order(
        draft(OrderKind::Sale, "K", vec![line(p.product.id, 4, 0)]),
        None,
        None,
    )
    .unwrap();
    product(&crm, "P2", "NEW AFTER BACKUP", 1, 1, 1);
    assert_eq!(stock(&crm, p.product.id), 6);

    let safety = crm.restore(&backup).unwrap().expect("before-restore copy");
    assert!(safety.exists());
    assert_eq!(stock(&crm, p.product.id), 10);
    assert!(crm.search_products("after backup", 5).unwrap().is_empty());
    assert_eq!(crm.search_products("brizo", 5).unwrap().len(), 1);
    assert!(
        !crm.refresh_if_changed(),
        "restore resets the change marker"
    );

    // The facade keeps working after the restore.
    crm.save_order(
        draft(OrderKind::Sale, "K", vec![line(p.product.id, 1, 0)]),
        None,
        None,
    )
    .unwrap();
    assert_eq!(crm.get_product(p.product.id).unwrap().sku.quantity, Qty(9));

    // A broken copy is refused and leaves the data as it was.
    let bad = dir.path().join("bad.db");
    std::fs::write(&bad, b"not a database").unwrap();
    assert!(crm.restore(&bad).is_err());
    assert_eq!(stock(&crm, p.product.id), 9);

    // Back up to "this PC".
    let pc = dir.path().join("pc");
    let copy = crm.backup_to_pc(&pc).unwrap();
    assert!(copy.starts_with(&pc) && copy.exists());
    let listed = crm.list_backups().unwrap();
    assert!(listed.iter().any(|b| b.path == backup));
    assert!(
        listed
            .iter()
            .any(|b| b.name.reason.as_deref() == Some("before-restore"))
    );
    crm.close().unwrap();
    assert!(matches!(crm.close(), Err(ServiceError::Closed)));
}

#[test]
fn draft_autosaves_do_not_count_as_data_changes() {
    // F-001: the 500 ms draft autosave must not make the 2 s poll rebuild the
    // index and reload every list.
    let (_dir, crm) = open();
    assert!(!crm.refresh_if_changed());
    crm.save_tab(tab("t1", 0, true)).unwrap();
    assert!(!crm.refresh_if_changed(), "own draft write");
    crm.set_idle_lock_minutes(3).unwrap();
    assert!(!crm.refresh_if_changed(), "own settings write");
    crm.remove_tab("t1").unwrap();
    assert!(!crm.refresh_if_changed(), "own tab removal");
}

#[test]
fn a_finished_restore_is_ok_when_the_index_rebuild_fails() {
    // F-013 variant: the data was already replaced when `rebuild_index` failed.
    let (_dir, crm) = open();
    let p = product(&crm, "P1", "BRIZO", 10, 100, 200);
    let backup = crm.backup_to_pc(&crm.paths().root.join("copy")).unwrap();
    product(&crm, "P2", "WEWEWE", 1, 1, 1);
    crm.inject_fault(Fault::IndexRead);
    crm.restore(&backup).expect("restored, so Ok");
    assert_eq!(stock(&crm, p.product.id), 10);
    // The index missed the restore: the next poll rebuilds it (no foreign commit).
    assert!(crm.refresh_if_changed(), "index left dirty");
    assert!(!crm.refresh_if_changed());
    assert!(crm.search_products("wewewe", 5).unwrap().is_empty());
    assert_eq!(crm.search_products("brizo", 5).unwrap().len(), 1);
}

#[test]
fn a_restore_whose_reopen_fails_closes_the_facade_cleanly() {
    // F-002: the copy is in place but cannot be reopened: every later call
    // fails with `Closed` (no half-open facade), and a fresh open works.
    let (_dir, crm) = open();
    let p = product(&crm, "P1", "BRIZO", 10, 100, 200);
    let backup = crm.backup_to_pc(&crm.paths().root.join("copy")).unwrap();
    product(&crm, "P2", "AFTER", 1, 1, 1);
    crm.inject_fault(Fault::ReopenAfterRestore);
    assert!(crm.restore(&backup).is_err());
    assert!(matches!(
        crm.get_product(p.product.id),
        Err(ServiceError::Closed)
    ));
    assert!(matches!(crm.backup_now(), Err(ServiceError::Closed)));
    assert!(!crm.refresh_if_changed());
    assert!(matches!(crm.close(), Err(ServiceError::Closed)));

    let paths = crm.paths().clone();
    drop(crm);
    let crm = crm_services::Crm::open(paths, &crm_services::DbKey::for_tests(1)).unwrap();
    assert_eq!(stock(&crm, p.product.id), 10, "the restored data");
    assert_eq!(
        crm.list_products("", crm_services::Page::default())
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn close_with_a_failing_exit_backup_still_closes() {
    // F-002 variant: the exit backup fails; the database is still checkpointed
    // and closed, the error is reported, and the data survives.
    let (_dir, crm) = open();
    let p = product(&crm, "P1", "BRIZO", 10, 100, 200);
    let backups = crm.paths().backups.clone();
    std::fs::remove_dir_all(&backups).unwrap();
    std::fs::write(&backups, b"not a folder").unwrap();
    assert!(crm.close().is_err());
    assert!(matches!(
        crm.get_product(p.product.id),
        Err(ServiceError::Closed)
    ));
    assert!(matches!(crm.close(), Err(ServiceError::Closed)));

    std::fs::remove_file(&backups).unwrap();
    let paths = crm.paths().clone();
    drop(crm);
    let crm = crm_services::Crm::open(paths, &crm_services::DbKey::for_tests(1)).unwrap();
    assert_eq!(stock(&crm, p.product.id), 10);
}

#[test]
fn a_stored_appearance_is_ignored_and_the_other_settings_still_read() {
    let (dir, crm) = open();
    crm.set_idle_lock_minutes(15).unwrap();
    crm.set_receipt_settings(ReceiptSettings {
        width_mm: 80,
        font_pt: 10,
    })
    .unwrap();
    // A row left by a build that still had dark mode (task 022): kept, never read.
    let db = crm_storage::Db::open(
        &crm_storage::DataPaths::at(dir.path()),
        &crm_services::DbKey::for_tests(1),
    )
    .unwrap();
    db.write(|tx| crm_storage::repo::settings::set(tx, "appearance", "dark"))
        .unwrap();
    assert_eq!(crm.idle_lock_minutes().unwrap(), 15);
    assert_eq!(
        crm.receipt_settings().unwrap(),
        ReceiptSettings {
            width_mm: 80,
            font_pt: 10,
        }
    );
    assert_eq!(
        db.read(|c| crm_storage::repo::settings::get(c, "appearance"))
            .unwrap()
            .as_deref(),
        Some("dark")
    );
}

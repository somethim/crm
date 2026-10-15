//! AC 6 and step 5: backups, rotation and restore.

mod common;

use crm_storage::repo::settings;
use crm_storage::{
    BackupName, BackupReason, DataPaths, Db, DbKey, StorageError, backup, plan_rotation,
    restore_from,
};
use std::collections::BTreeSet;

fn open(dir: &std::path::Path) -> (DataPaths, Db) {
    let paths = DataPaths::at(dir);
    let db = Db::open(&paths, &DbKey::for_tests(1)).unwrap();
    (paths, db)
}

#[test]
fn backup_now_writes_a_checked_copy_with_the_rows() {
    let dir = tempfile::tempdir().unwrap();
    let (paths, db) = open(dir.path());
    db.write(|tx| settings::set(tx, "row", "A")).unwrap();
    assert_eq!(db.read(settings::last_backup_at).unwrap(), None);

    let path = db.backup_now(BackupReason::PreImport("products")).unwrap();
    let name = path.file_name().unwrap().to_str().unwrap().to_owned();
    assert!(path.starts_with(&paths.backups));
    let parsed = BackupName::parse(&name).unwrap();
    assert_eq!(
        parsed.reason.as_deref(),
        Some("pre-import-products"),
        "{name}"
    );
    assert!(name.starts_with("crm-") && name.ends_with("-pre-import-products.db"));

    backup::verify(&path, &common::key()).unwrap();
    let copy = common::raw(&path);
    let value: String = copy
        .query_row("SELECT value FROM settings WHERE key = 'row'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(value, "A");
    assert!(db.read(settings::last_backup_at).unwrap().is_some());

    // Same-second copies never collide (rotation then keeps the newest of the day).
    let again = db.backup_now(BackupReason::Manual).unwrap();
    let third = db.backup_now(BackupReason::Manual).unwrap();
    assert_ne!(again, third);
    assert!(third.exists());
}

#[test]
fn daily_backup_is_due_until_one_is_taken_today() {
    let dir = tempfile::tempdir().unwrap();
    let (_paths, db) = open(dir.path());
    let now = crm_domain::time::now();
    assert!(db.daily_backup_due(now).unwrap());
    db.backup_now(BackupReason::Daily).unwrap();
    assert!(!db.daily_backup_due(crm_domain::time::now()).unwrap());
}

#[test]
fn backup_to_writes_into_another_folder_without_rotation() {
    let dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let (paths, db) = open(dir.path());
    db.write(|tx| settings::set(tx, "row", "A")).unwrap();
    let path = db.backup_to(target.path()).unwrap();
    assert!(path.starts_with(target.path()));
    backup::verify(&path, &common::key()).unwrap();
    assert_eq!(std::fs::read_dir(&paths.backups).unwrap().count(), 0);
}

#[test]
fn verify_rejects_a_broken_copy() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("crm-20261014-100000.db");
    std::fs::write(&bad, vec![0x42; 8192]).unwrap();
    // Opened with a key, a file that is no database reads as "wrong key".
    assert_eq!(
        backup::verify(&bad, &common::key()),
        Err(StorageError::WrongKey)
    );
}

#[test]
fn restore_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let (paths, db) = open(dir.path());
    db.write(|tx| settings::set(tx, "A", "1")).unwrap();
    let copy = db.backup_now(BackupReason::Manual).unwrap();
    db.write(|tx| settings::set(tx, "B", "1")).unwrap();
    db.checkpoint_and_close().unwrap();

    let saved = restore_from(&copy, &paths, &common::key())
        .unwrap()
        .unwrap();
    assert!(
        saved
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .ends_with("-before-restore.db")
    );

    let db = Db::open(&paths, &DbKey::for_tests(1)).unwrap();
    assert_eq!(
        db.read(|c| settings::get(c, "A")).unwrap().as_deref(),
        Some("1")
    );
    assert_eq!(db.read(|c| settings::get(c, "B")).unwrap(), None);
    drop(db);

    // The data from before the restore is kept, including row B.
    let before = common::raw(&saved);
    let b: String = before
        .query_row("SELECT value FROM settings WHERE key = 'B'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(b, "1");
}

#[test]
fn restore_without_a_clean_close_keeps_all_committed_rows_in_the_safety_copy() {
    let dir = tempfile::tempdir().unwrap();
    let (paths, db) = open(dir.path());
    db.write(|tx| settings::set(tx, "A", "1")).unwrap();
    let copy = db.backup_now(BackupReason::Manual).unwrap();
    db.write(|tx| settings::set(tx, "B", "1")).unwrap();
    drop(db); // no checkpoint
    let saved = restore_from(&copy, &paths, &common::key())
        .unwrap()
        .unwrap();
    let before = common::raw(&saved);
    let n: i64 = before
        .query_row(
            "SELECT count(*) FROM settings WHERE key IN ('A', 'B')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 2);
    let db = Db::open(&paths, &DbKey::for_tests(1)).unwrap();
    assert_eq!(db.read(|c| settings::get(c, "B")).unwrap(), None);
}

#[test]
fn restore_refuses_a_broken_backup_and_keeps_the_data() {
    let dir = tempfile::tempdir().unwrap();
    let (paths, db) = open(dir.path());
    db.write(|tx| settings::set(tx, "A", "1")).unwrap();
    db.checkpoint_and_close().unwrap();
    let bad = dir.path().join("bad.db");
    std::fs::write(&bad, vec![0x42; 8192]).unwrap();
    assert!(restore_from(&bad, &paths, &common::key()).is_err());
    let db = Db::open(&paths, &DbKey::for_tests(1)).unwrap();
    assert_eq!(
        db.read(|c| settings::get(c, "A")).unwrap().as_deref(),
        Some("1")
    );
}

/// 40 synthetic names across 14 months (2025-09 … 2026-10; 2026-10-14 is a Wednesday).
fn synthetic_names() -> Vec<String> {
    let mut names = Vec::new();
    // Monthly copies on the 1st, 2025-09 … 2026-09 (13), plus 2025-09-15.
    for (y, m) in (9..=12)
        .map(|m| (2025, m))
        .chain((1..=9).map(|m| (2026, m)))
    {
        names.push(format!("crm-{y}{m:02}01-120000.db"));
    }
    names.push("crm-20250915-120000.db".into());
    // Daily copies 2026-10-01 … 14 (14), an exit copy on the 14th, a manual one on the 13th.
    for d in 1..=14 {
        names.push(format!("crm-202610{d:02}-080000.db"));
    }
    names.push("crm-20261014-180000-exit.db".into());
    names.push("crm-20261013-100000-manual.db".into());
    // Seven pre-import copies, one pre-migrate copy.
    for d in 2..=8 {
        names.push(format!("crm-202610{d:02}-090000-pre-import-products.db"));
    }
    names.push("crm-20251201-090000-pre-migrate-v1.db".into());
    // Never deleted: unrecognised names.
    names.push("crm-20250901-000000-before-restore.db".into());
    names.push("notes.txt".into());
    names
}

fn expected_deletions() -> BTreeSet<String> {
    [
        // Daily: only the newest per day survives, for the 7 most recent days
        // (10-08 … 10-14); 10-04 survives as the newest of its ISO week.
        "crm-20261014-080000.db",
        "crm-20261013-080000.db",
        "crm-20261007-080000.db",
        "crm-20261006-080000.db",
        "crm-20261005-080000.db",
        "crm-20261003-080000.db",
        "crm-20261002-080000.db",
        "crm-20261001-080000.db",
        // Monthly: 12 months (2025-11 … 2026-10) are kept; older ones go.
        "crm-20251001-120000.db",
        "crm-20250915-120000.db",
        "crm-20250901-120000.db",
        // pre-import: the 5 newest stay.
        "crm-20261002-090000-pre-import-products.db",
        "crm-20261003-090000-pre-import-products.db",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

#[test]
fn rotation_planner_keeps_the_expected_set() {
    let names = synthetic_names();
    assert_eq!(names.len(), 40);
    let plan = plan_rotation(names.iter().map(String::as_str));
    let deleted: BTreeSet<String> = plan.delete.iter().cloned().collect();
    assert_eq!(deleted, expected_deletions());
    assert_eq!(plan.keep.len(), 27);
    for kept in [
        "crm-20261014-180000-exit.db",
        "crm-20261013-100000-manual.db",
        "crm-20261004-080000.db",
        "crm-20260901-120000.db",
        "crm-20251101-120000.db",
        "crm-20251201-090000-pre-migrate-v1.db",
        "crm-20250901-000000-before-restore.db",
        "notes.txt",
    ] {
        assert!(plan.keep.contains(&kept.to_owned()), "{kept}");
    }
    // Planning the kept set again deletes nothing.
    let again = plan_rotation(plan.keep.iter().map(String::as_str));
    assert!(again.delete.is_empty(), "{:?}", again.delete);
}

#[test]
fn rotate_deletes_only_the_planned_files() {
    let dir = tempfile::tempdir().unwrap();
    for name in synthetic_names() {
        std::fs::write(dir.path().join(name), b"x").unwrap();
    }
    let deleted = backup::rotate(dir.path()).unwrap();
    let deleted: BTreeSet<String> = deleted
        .iter()
        .map(|p| p.file_name().unwrap().to_str().unwrap().to_owned())
        .collect();
    assert_eq!(deleted, expected_deletions());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 27);
}

/// F-019: copies made in the same second must rotate the same way whatever
/// order the directory lists them in; an exit or manual copy outranks the
/// daily copy of that second.
#[test]
fn same_second_copies_keep_the_exit_or_manual_one_in_any_listing_order() {
    let cases: [(&[&str], &str); 4] = [
        (
            &["crm-20261014-100000.db", "crm-20261014-100000-exit.db"],
            "crm-20261014-100000-exit.db",
        ),
        (
            &["crm-20261014-100000.db", "crm-20261014-100000-manual.db"],
            "crm-20261014-100000-manual.db",
        ),
        (
            &[
                "crm-20261014-100000.db",
                "crm-20261014-100000-2.db",
                "crm-20261014-100000-exit.db",
            ],
            "crm-20261014-100000-exit.db",
        ),
        (
            &[
                "crm-20261014-100000-exit.db",
                "crm-20261014-100000-manual.db",
            ],
            "crm-20261014-100000-manual.db",
        ),
    ];
    for (names, kept) in cases {
        let forward = plan_rotation(names.iter().copied());
        let backward = plan_rotation(names.iter().rev().copied());
        assert_eq!(forward, backward, "{names:?}");
        assert_eq!(forward.keep, vec![kept.to_owned()], "{names:?}");
    }
    // A later second still wins over an exit copy of an earlier one.
    let names = ["crm-20261014-100000-exit.db", "crm-20261014-100001.db"];
    for order in [names, [names[1], names[0]]] {
        let plan = plan_rotation(order);
        assert_eq!(plan.keep, vec!["crm-20261014-100001.db".to_owned()]);
    }
}

/// F-019 (variant): the copy a backup call just wrote is never deleted by the
/// rotation run of that same call, even when the folder already holds copies
/// that rank above it (here: copies dated in the future, as after a clock change).
#[test]
fn backup_now_never_deletes_the_copy_it_returns() {
    let dir = tempfile::tempdir().unwrap();
    let (paths, db) = open(dir.path());
    std::fs::create_dir_all(&paths.backups).unwrap();
    // 12 future months on distinct days and weeks fill every daily, weekly and
    // monthly slot; 5 future pre-import copies fill that reason's slots.
    for month in 1..=12 {
        let name = format!("crm-2099{month:02}{month:02}-120000-manual.db");
        std::fs::write(paths.backups.join(name), b"x").unwrap();
    }
    for day in 1..=5 {
        let name = format!("crm-209901{day:02}-120000-pre-import-products.db");
        std::fs::write(paths.backups.join(name), b"x").unwrap();
    }
    for reason in [
        BackupReason::Manual,
        BackupReason::Exit,
        BackupReason::PreImport("products"),
    ] {
        let path = db.backup_now(reason).unwrap();
        assert!(path.exists(), "{path:?}");
        backup::verify(&path, &common::key()).unwrap();
    }
}

// Plan 014 step 6: keyed backups, sibling `.key` files, rotation and restore.

mod keyed {
    use super::*;
    use std::path::Path;

    const KEY_FILE: &[u8] = b"CRMKEY dummy key file bytes";

    fn open_keyed(dir: &Path, key: &DbKey) -> (DataPaths, Db) {
        let paths = DataPaths::at(dir);
        let db = Db::open(&paths, key).unwrap();
        std::fs::write(&paths.key, KEY_FILE).unwrap();
        (paths, db)
    }

    fn assert_encrypted_with_sibling(path: &Path, key: &DbKey) {
        let bytes = std::fs::read(path).unwrap();
        assert!(!bytes.starts_with(b"SQLite format 3"), "{}", path.display());
        backup::verify(path, key).unwrap();
        assert_eq!(
            std::fs::read(path.with_extension("key")).unwrap(),
            KEY_FILE,
            "{}",
            path.display()
        );
    }

    #[test]
    fn every_reason_and_backup_to_are_encrypted_with_a_sibling() {
        let dir = tempfile::tempdir().unwrap();
        let pc = tempfile::tempdir().unwrap();
        let key = DbKey::for_tests(1);
        let (_paths, db) = open_keyed(dir.path(), &key);
        db.write(|tx| settings::set(tx, "row", "A")).unwrap();
        for reason in [
            BackupReason::Daily,
            BackupReason::Exit,
            BackupReason::Manual,
            BackupReason::PreImport("products"),
            BackupReason::PreMigrate(1),
        ] {
            let path = db.backup_now(reason).unwrap();
            assert_encrypted_with_sibling(&path, &key);
            assert_eq!(
                backup::verify(&path, &DbKey::for_tests(2)),
                Err(StorageError::WrongKey)
            );
        }
        let copy = db.backup_to(pc.path()).unwrap();
        assert_encrypted_with_sibling(&copy, &key);
    }

    #[test]
    fn no_key_file_means_no_sibling() {
        let dir = tempfile::tempdir().unwrap();
        let key = DbKey::for_tests(1);
        let paths = DataPaths::at(dir.path());
        let db = Db::open(&paths, &key).unwrap();
        let path = db.backup_now(BackupReason::Manual).unwrap();
        backup::verify(&path, &key).unwrap();
        assert!(!path.with_extension("key").exists());
    }

    #[test]
    fn rotation_removes_siblings_and_orphans_and_parse_ignores_keys() {
        let dir = tempfile::tempdir().unwrap();
        // 10 daily copies on 10 days: rotation keeps the newest 7 of them (and
        // the weekly/monthly picks), deleting some with their siblings.
        let mut names = Vec::new();
        for day in 1..=20 {
            let name = format!("crm-202609{day:02}-120000.db");
            std::fs::write(dir.path().join(&name), b"x").unwrap();
            std::fs::write(dir.path().join(&name).with_extension("key"), b"k").unwrap();
            names.push(name);
        }
        std::fs::write(dir.path().join("crm-20260101-120000.key"), b"orphan").unwrap();
        let deleted = backup::rotate(dir.path()).unwrap();
        assert!(!deleted.is_empty());
        for path in &deleted {
            assert!(!path.exists());
            assert!(!path.with_extension("key").exists(), "{}", path.display());
            assert_eq!(path.extension().unwrap(), "db");
        }
        assert!(!dir.path().join("crm-20260101-120000.key").exists());
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_str().unwrap().to_owned();
            if name.ends_with(".db") {
                assert!(path.with_extension("key").exists(), "{name}");
            } else {
                assert!(path.with_extension("db").exists(), "{name}");
                assert_eq!(BackupName::parse(&name), None, "{name}");
            }
        }
    }

    #[test]
    fn restore_refuses_a_copy_with_another_key_and_keeps_everything() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let key = DbKey::for_tests(1);
        let (paths, db) = open_keyed(dir.path(), &key);
        db.write(|tx| settings::set(tx, "A", "1")).unwrap();
        db.checkpoint_and_close().unwrap();
        let (_other_paths, other_db) = open_keyed(other.path(), &DbKey::for_tests(2));
        let foreign = other_db.backup_now(BackupReason::Manual).unwrap();
        drop(other_db);

        let before = std::fs::read(&paths.db).unwrap();
        let backups_before = std::fs::read_dir(&paths.backups).unwrap().count();
        assert_eq!(
            restore_from(&foreign, &paths, &key),
            Err(StorageError::WrongKey)
        );
        assert_eq!(std::fs::read(&paths.db).unwrap(), before);
        assert_eq!(std::fs::read(&paths.key).unwrap(), KEY_FILE);
        assert_eq!(
            std::fs::read_dir(&paths.backups).unwrap().count(),
            backups_before
        );
    }

    #[test]
    fn keyed_restore_round_trip_keeps_an_encrypted_before_restore_copy() {
        let dir = tempfile::tempdir().unwrap();
        let key = DbKey::for_tests(1);
        let (paths, db) = open_keyed(dir.path(), &key);
        db.write(|tx| settings::set(tx, "A", "1")).unwrap();
        let copy = db.backup_now(BackupReason::Manual).unwrap();
        db.write(|tx| settings::set(tx, "B", "1")).unwrap();
        drop(db); // no checkpoint: B lives in the WAL
        let key_file_before = std::fs::read(&paths.key).unwrap();

        let saved = restore_from(&copy, &paths, &key).unwrap().unwrap();
        assert!(saved.to_str().unwrap().ends_with("-before-restore.db"));
        assert_encrypted_with_sibling(&saved, &key);
        assert_eq!(std::fs::read(&paths.key).unwrap(), key_file_before);

        let db = Db::open(&paths, &key).unwrap();
        assert_eq!(
            db.read(|c| settings::get(c, "A")).unwrap().as_deref(),
            Some("1")
        );
        assert_eq!(db.read(|c| settings::get(c, "B")).unwrap(), None);
    }
}

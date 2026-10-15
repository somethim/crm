//! Plan 014 step 7: plaintext → encrypted, in place and per copy, resumable
//! after an interruption at every stage.

use crm_storage::repo::settings;
use crm_storage::{
    CopyStats, DataPaths, Db, DbKey, FileKind, Stage, database_stats, encrypt_copy,
    encrypt_in_place, encrypt_in_place_until, file_kind,
};
use std::path::Path;

/// A plaintext root with rows: some checkpointed into crm.db, some only in
/// the WAL (the files are copied while the writer is open, like a crash).
fn plaintext_root(root: &Path) {
    let build = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(build.path());
    let mut seed = String::new();
    for i in 0..50 {
        seed.push_str(&format!(
            "INSERT INTO settings VALUES ('a{i}', 'checkpointed');"
        ));
    }
    Db::create_legacy_plaintext(&paths, &seed).unwrap();
    let db = rusqlite::Connection::open(&paths.db).unwrap();
    db.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    for i in 0..30 {
        db.execute(
            "INSERT INTO settings VALUES (?1, 'in the wal Lekë')",
            [format!("w{i}")],
        )
        .unwrap();
    }
    let target = DataPaths::at(root);
    std::fs::create_dir_all(target.data_dir()).unwrap();
    std::fs::copy(&paths.db, &target.db).unwrap();
    std::fs::copy(
        paths.data_dir().join("crm.db-wal"),
        target.data_dir().join("crm.db-wal"),
    )
    .unwrap();
    assert!(
        std::fs::metadata(target.data_dir().join("crm.db-wal"))
            .unwrap()
            .len()
            > 0
    );
    drop(db);
}

fn assert_clean_encrypted(root: &Path, key: &DbKey, expected: &CopyStats) {
    let paths = DataPaths::at(root);
    assert_eq!(file_kind(&paths.db).unwrap(), FileKind::Encrypted);
    let data = paths.data_dir();
    for leftover in ["crm.db.enc", "crm.db.plain", "crm.db-wal", "crm.db-shm"] {
        assert!(!data.join(leftover).exists(), "{leftover} left in data/");
    }
    let raw = std::fs::read(&paths.db).unwrap();
    assert!(!raw.windows(12).any(|w| w == b"checkpointed"));
    let stats = database_stats(&paths.db, Some(key)).unwrap();
    assert_eq!(&stats, expected);
    assert_eq!(stats.user_version, 1);
    let db = Db::open(&paths, key).unwrap();
    assert_eq!(
        db.read(|c| settings::get(c, "w29")).unwrap().as_deref(),
        Some("in the wal Lekë")
    );
}

/// The stats of the plaintext root, read from a scratch copy (opening the
/// original would checkpoint its WAL).
fn plaintext_stats(root: &Path) -> CopyStats {
    let scratch = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(root);
    let db = scratch.path().join("crm.db");
    std::fs::copy(&paths.db, &db).unwrap();
    std::fs::copy(
        paths.data_dir().join("crm.db-wal"),
        scratch.path().join("crm.db-wal"),
    )
    .unwrap();
    let stats = encrypt_copy(&db, &scratch.path().join("x.db"), &DbKey::for_tests(9)).unwrap();
    assert!(
        stats
            .tables
            .iter()
            .any(|(t, n)| t == "settings" && *n == 80)
    );
    stats
}

#[test]
fn encrypt_in_place_keeps_every_row_and_leaves_no_plaintext() {
    let dir = tempfile::tempdir().unwrap();
    plaintext_root(dir.path());
    let expected = plaintext_stats(dir.path());
    let key = DbKey::for_tests(4);
    encrypt_in_place(&DataPaths::at(dir.path()), &key).unwrap();
    assert_clean_encrypted(dir.path(), &key, &expected);
    // Idempotent.
    encrypt_in_place(&DataPaths::at(dir.path()), &key).unwrap();
    assert_clean_encrypted(dir.path(), &key, &expected);
}

#[test]
fn every_interruption_resumes() {
    for stop in [
        Stage::Copied,
        Stage::PlainSetAside,
        Stage::EncryptedInPlace,
        Stage::Done,
    ] {
        let dir = tempfile::tempdir().unwrap();
        plaintext_root(dir.path());
        let expected = plaintext_stats(dir.path());
        let key = DbKey::for_tests(4);
        let paths = DataPaths::at(dir.path());
        encrypt_in_place_until(&paths, &key, stop).unwrap();
        encrypt_in_place(&paths, &key).unwrap();
        assert_clean_encrypted(dir.path(), &key, &expected);
    }
}

#[test]
fn step_five_interrupted_before_the_sidecar_erase_resumes() {
    let dir = tempfile::tempdir().unwrap();
    plaintext_root(dir.path());
    let expected = plaintext_stats(dir.path());
    let key = DbKey::for_tests(4);
    let paths = DataPaths::at(dir.path());
    encrypt_in_place_until(&paths, &key, Stage::Copied).unwrap();
    // The rename happened, the erase of the sidecars did not.
    std::fs::rename(&paths.db, paths.data_dir().join("crm.db.plain")).unwrap();
    std::fs::write(paths.data_dir().join("crm.db-shm"), b"plaintext shm").unwrap();
    encrypt_in_place(&paths, &key).unwrap();
    assert_clean_encrypted(dir.path(), &key, &expected);
}

#[test]
fn awkward_root_names_work() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("a b#c%d?e ë");
    plaintext_root(&root);
    let expected = plaintext_stats(&root);
    let key = DbKey::for_tests(5);
    encrypt_in_place(&DataPaths::at(&root), &key).unwrap();
    assert_clean_encrypted(&root, &key, &expected);
}

#[test]
fn encrypt_copy_encrypts_a_plaintext_backup() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    Db::create_legacy_plaintext(&paths, "INSERT INTO settings VALUES ('a', '1');").unwrap();
    let copy = paths.backups.join("crm-20261014-100000-manual.db");
    std::fs::copy(&paths.db, &copy).unwrap();
    assert_eq!(file_kind(&copy).unwrap(), FileKind::Plaintext);
    let target = paths.backups.join("copy #1 ë.db");
    let key = DbKey::for_tests(6);
    let stats = encrypt_copy(&copy, &target, &key).unwrap();
    assert_eq!(stats.user_version, 1);
    assert!(stats.tables.iter().any(|(t, n)| t == "settings" && *n >= 1));
    assert_eq!(file_kind(&target).unwrap(), FileKind::Encrypted);
    crm_storage::backup::verify(&target, &key).unwrap();
    assert_eq!(
        crm_storage::backup::verify(&target, &DbKey::for_tests(7)),
        Err(crm_storage::StorageError::WrongKey)
    );
}

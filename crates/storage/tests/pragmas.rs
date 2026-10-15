//! AC 1: every connection (writer, each pooled reader, the data-version
//! connection) reports the 001 pragmas.

use crm_storage::{DataPaths, Db, DbKey, JOURNAL_SIZE_LIMIT};

#[test]
fn every_connection_reports_the_pragmas() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&DataPaths::at(dir.path()), &DbKey::for_tests(1)).unwrap();
    let report = db.connection_pragmas().unwrap();
    let roles: Vec<&str> = report.iter().map(|p| p.role.as_str()).collect();
    assert_eq!(
        roles,
        ["writer", "reader-0", "reader-1", "reader-2", "data-version"]
    );
    for p in &report {
        assert_eq!(p.journal_mode, "wal", "{}", p.role);
        assert_eq!(p.synchronous, 2, "{}", p.role);
        assert_eq!(p.foreign_keys, 1, "{}", p.role);
        assert_eq!(p.temp_store, 2, "{}", p.role);
        assert_eq!(p.busy_timeout, 5000, "{}", p.role);
        assert_eq!(p.mmap_size, 0, "{}", p.role);
        assert_eq!(p.journal_size_limit, JOURNAL_SIZE_LIMIT, "{}", p.role);
        assert_eq!(p.journal_size_limit, 67_108_864, "{}", p.role);
        assert_eq!(p.cipher, "chacha20", "{}", p.role);
    }
}

#[test]
fn reopening_keeps_the_pragmas() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    Db::open(&paths, &DbKey::for_tests(1))
        .unwrap()
        .checkpoint_and_close()
        .unwrap();
    let db = Db::open(&paths, &DbKey::for_tests(1)).unwrap();
    assert!(
        db.connection_pragmas()
            .unwrap()
            .iter()
            .all(|p| p.journal_mode == "wal" && p.synchronous == 2)
    );
}

//! Plan 014 step 4: keyed opens. Every connection is keyed, nothing readable
//! reaches the disk, another key is `WrongKey` and a flipped bit is `Corrupt`.

use crm_storage::repo::settings;
use crm_storage::{DataPaths, Db, DbKey, StorageError};
use std::sync::Arc;

const MARKER: &str = "secret-marker-XYZ Lekë";

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn assert_opaque(path: &std::path::Path) {
    let bytes = std::fs::read(path).unwrap();
    assert!(!bytes.is_empty(), "{} is empty", path.display());
    assert!(
        !bytes.starts_with(b"SQLite format 3"),
        "{} has a plaintext header",
        path.display()
    );
    assert!(
        !contains(&bytes, MARKER.as_bytes()),
        "{} holds the marker",
        path.display()
    );
    assert!(
        !contains(&bytes, b"secret-marker"),
        "{} holds the marker",
        path.display()
    );
}

fn flip_bit(path: &std::path::Path, offset: usize) {
    let mut bytes = std::fs::read(path).unwrap();
    bytes[offset] ^= 0x01;
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn nothing_readable_reaches_the_disk() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    let key = DbKey::for_tests(1);
    let db = Db::open(&paths, &key).unwrap();
    db.write(|tx| settings::set(tx, "marker", MARKER)).unwrap();
    // A second handle keeps the WAL in place while the files are read.
    let second = Db::open(&paths, &key).unwrap();
    let wal = paths.data_dir().join("crm.db-wal");
    assert!(std::fs::metadata(&wal).unwrap().len() > 0);
    assert_opaque(&paths.db);
    assert_opaque(&wal);
    assert_eq!(
        second
            .read(|c| settings::get(c, "marker"))
            .unwrap()
            .as_deref(),
        Some(MARKER)
    );
    drop(second);
    db.checkpoint_and_close().unwrap();
    assert_opaque(&paths.db);
}

#[test]
fn every_connection_is_keyed_and_reads() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    let db = Db::open(&paths, &DbKey::for_tests(1)).unwrap();
    db.write(|tx| settings::set(tx, "a", "1")).unwrap();
    let report = db.connection_pragmas().unwrap();
    let roles: Vec<&str> = report.iter().map(|p| p.role.as_str()).collect();
    assert_eq!(
        roles,
        ["writer", "reader-0", "reader-1", "reader-2", "data-version"]
    );
    for p in &report {
        assert_eq!(p.cipher, "chacha20", "{}", p.role);
        assert_eq!(p.journal_mode, "wal", "{}", p.role);
    }
    let rows = db.settings_rows_per_connection().unwrap();
    assert_eq!(rows.len(), 5);
    for (role, n) in rows {
        assert_eq!(n, 1, "{role}");
    }
}

#[test]
fn another_key_is_wrong_key() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    let db = Db::open(&paths, &DbKey::for_tests(1)).unwrap();
    db.write(|tx| settings::set(tx, "a", "1")).unwrap();
    db.checkpoint_and_close().unwrap();

    assert_eq!(
        Db::open(&paths, &DbKey::for_tests(2)).unwrap_err(),
        StorageError::WrongKey
    );
    // A plaintext database opened with a key is refused the same way.
    let legacy = tempfile::tempdir().unwrap();
    let legacy = DataPaths::at(legacy.path());
    Db::create_legacy_plaintext(&legacy, "").unwrap();
    assert_eq!(
        Db::open(&legacy, &DbKey::for_tests(1)).unwrap_err(),
        StorageError::WrongKey
    );
    // The right key still opens it.
    let db = Db::open(&paths, &DbKey::for_tests(1)).unwrap();
    assert_eq!(
        db.read(|c| settings::get(c, "a")).unwrap().as_deref(),
        Some("1")
    );
}

#[test]
fn a_flipped_bit_in_a_page_is_corrupt() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    let key = DbKey::for_tests(1);
    let db = Db::open(&paths, &key).unwrap();
    db.write(|tx| settings::set(tx, "a", "1")).unwrap();
    db.checkpoint_and_close().unwrap();
    flip_bit(&paths.db, 4096 + 100);
    assert!(matches!(
        Db::open(&paths, &key).unwrap_err(),
        StorageError::Corrupt { .. }
    ));
}

#[test]
fn a_flipped_bit_in_a_wal_frame_is_corrupt() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    let key = DbKey::for_tests(1);
    let a = Db::open(&paths, &key).unwrap();
    let b = Db::open(&paths, &key).unwrap();
    for i in 0..20 {
        b.write(move |tx| settings::set(tx, &format!("k{i}"), MARKER))
            .unwrap();
    }
    let wal = paths.data_dir().join("crm.db-wal");
    let len = std::fs::metadata(&wal).unwrap().len() as usize;
    assert!(len > 4096);
    flip_bit(&wal, len - 200);
    assert!(matches!(
        Db::open(&paths, &key).unwrap_err(),
        StorageError::Corrupt { .. }
    ));
    drop(b);
    drop(a);
}

#[test]
fn readers_keep_reading_during_keyed_writes() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    let db = Arc::new(Db::open(&paths, &DbKey::for_tests(3)).unwrap());
    let writer = {
        let db = db.clone();
        std::thread::spawn(move || {
            for i in 0..200 {
                db.write(move |tx| settings::set(tx, &format!("k{i}"), "v"))
                    .unwrap();
            }
        })
    };
    let mut last = 0;
    let mut reads = 0;
    while !writer.is_finished() || reads == 0 {
        let n = db
            .read(|c| {
                let mut n = 0;
                for i in 0..200 {
                    if settings::get(c, &format!("k{i}"))?.is_some() {
                        n += 1;
                    }
                }
                Ok::<_, StorageError>(n)
            })
            .unwrap();
        assert!(n >= last, "went back from {last} to {n}");
        last = n;
        reads += 1;
    }
    writer.join().unwrap();
    assert_eq!(
        db.read(|c| settings::get(c, "k199")).unwrap().as_deref(),
        Some("v")
    );
}

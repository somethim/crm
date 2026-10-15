//! Plan 015 step 7: the keyed `Crm`, `secure_storage` and `abandon`.

mod common;

use common::{draft, line, product};
use crm_services::domain::OrderKind;
use crm_services::{
    Crm, DataPaths, DbKey, KdfParams, ServiceError, StorageError, Unlock, Vault, secure_storage,
};
use std::path::Path;

fn header_is_plain(path: &Path) -> bool {
    std::fs::read(path)
        .unwrap()
        .starts_with(b"SQLite format 3\0")
}

/// A committed vault and its key on a fresh root.
fn keyed_root(dir: &Path) -> (Vault, DbKey, String) {
    let vault = Vault::with_params(DataPaths::at(dir), KdfParams::TEST);
    let pending = vault.setup("fjalëkalim").unwrap();
    let wipe = pending.wipe_display().to_string();
    (vault, pending.commit().unwrap(), wipe)
}

#[test]
fn keyed_round_trip_with_use_cases() {
    let dir = tempfile::tempdir().unwrap();
    let (_vault, key, _) = keyed_root(dir.path());
    let paths = DataPaths::at(dir.path());
    let crm = Crm::open(paths.clone(), &key).unwrap();
    let p = product(&crm, "L0009", "BRIZO PJESHKË", 10, 6_000, 6_500);
    crm.save_order(
        draft(OrderKind::Sale, "Klienti", vec![line(p.product.id, 2, 0)]),
        None,
        None,
    )
    .unwrap();
    assert_eq!(crm.search_products("brizo", 10).unwrap().len(), 1);
    crm.close().unwrap();
    assert!(!header_is_plain(&paths.db));
    let crm = Crm::open(paths.clone(), &key).unwrap();
    assert_eq!(crm.get_product(p.product.id).unwrap().sku.quantity.0, 8);
    crm.close().unwrap();
}

#[test]
fn backups_are_encrypted_with_their_key_sibling() {
    let dir = tempfile::tempdir().unwrap();
    let pc = tempfile::tempdir().unwrap();
    let (_vault, key, _) = keyed_root(dir.path());
    let paths = DataPaths::at(dir.path());
    let crm = Crm::open(paths.clone(), &key).unwrap();
    let key_file = std::fs::read(&paths.key).unwrap();
    for copy in [
        crm.backup_now().unwrap(),
        crm.backup_to_pc(pc.path()).unwrap(),
    ] {
        assert!(!header_is_plain(&copy));
        assert_eq!(std::fs::read(copy.with_extension("key")).unwrap(), key_file);
    }
    crm.close().unwrap();
}

#[test]
fn a_copy_of_other_data_is_refused_and_nothing_changes() {
    let dir = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let (_vault, key, _) = keyed_root(dir.path());
    let (_other_vault, other_key, _) = keyed_root(other.path());
    let foreign = {
        let crm = Crm::open(DataPaths::at(other.path()), &other_key).unwrap();
        let copy = crm.backup_now().unwrap();
        crm.close().unwrap();
        copy
    };
    let crm = Crm::open(DataPaths::at(dir.path()), &key).unwrap();
    let p = product(&crm, "A1", "Artikull", 1, 1, 2);
    let err = crm.restore(&foreign).unwrap_err();
    assert!(
        matches!(err, ServiceError::Storage(StorageError::WrongKey)),
        "{err:?}"
    );
    assert_eq!(
        crm.get_product(p.product.id).unwrap().product.name,
        "Artikull"
    );
    crm.close().unwrap();
}

#[test]
fn a_keyed_restore_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let (_vault, key, _) = keyed_root(dir.path());
    let crm = Crm::open(DataPaths::at(dir.path()), &key).unwrap();
    let a = product(&crm, "A1", "Artikull A", 1, 1, 2);
    let copy = crm.backup_now().unwrap();
    let b = product(&crm, "B1", "Artikull B", 1, 1, 2);
    let saved = crm.restore(&copy).unwrap().unwrap();
    assert!(!header_is_plain(&saved));
    assert!(saved.with_extension("key").exists());
    assert!(crm.get_product(a.product.id).is_ok());
    assert!(crm.get_product(b.product.id).is_err());
    crm.close().unwrap();
}

#[test]
fn secure_storage_encrypts_plaintext_data_and_backups() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    // Plaintext data and backups from an older version, with the legacy lock row.
    let copy = common::legacy_root(&paths, 5);
    // Three plaintext copies (rotation keeps one manual copy per day).
    for day in ["20261001", "20261002"] {
        std::fs::copy(
            &copy,
            paths.backups.join(format!("crm-{day}-120000-manual.db")),
        )
        .unwrap();
    }
    let all_db: Vec<_> = std::fs::read_dir(&paths.backups)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "db"))
        .collect();
    assert!(all_db.len() >= 3);
    let junk = paths.backups.join("crm-20200101-000000-manual.db");
    let mut bytes = b"SQLite format 3\0".to_vec();
    bytes.extend_from_slice(&[0x42; 5000]);
    std::fs::write(&junk, bytes).unwrap();
    let sidecar = paths.backups.join("crm-20200101-000000-manual.db-wal");
    std::fs::write(&sidecar, b"plaintext wal").unwrap();

    let vault = Vault::with_params(paths.clone(), KdfParams::TEST);
    assert_eq!(vault.probe().unwrap(), crm_services::StartState::Migrate);
    let key = vault.setup("fjalëkalim").unwrap().commit().unwrap();
    secure_storage(&paths, &key).unwrap();

    assert!(!header_is_plain(&paths.db));
    assert!(!junk.exists());
    assert!(!sidecar.exists());
    let key_file = std::fs::read(&paths.key).unwrap();
    for db in &all_db {
        assert!(!header_is_plain(db), "{}", db.display());
        assert_eq!(std::fs::read(db.with_extension("key")).unwrap(), key_file);
        crm_services::KeyFile::parse(&std::fs::read(db.with_extension("key")).unwrap()).unwrap();
    }
    for entry in std::fs::read_dir(&paths.backups).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        assert!(name.ends_with(".db") || name.ends_with(".key"), "{name}");
    }
    // A second run is a no-op.
    let before: Vec<_> = all_db.iter().map(|p| std::fs::read(p).unwrap()).collect();
    secure_storage(&paths, &key).unwrap();
    let after: Vec<_> = all_db.iter().map(|p| std::fs::read(p).unwrap()).collect();
    assert_eq!(before, after);

    // The data is all there, and the legacy lock row is gone after the first open.
    let stats = crm_storage::database_stats(&paths.db, Some(&key)).unwrap();
    assert!(stats.tables.contains(&("products".to_owned(), 5)));
    let crm = Crm::open(paths.clone(), &key).unwrap();
    assert_eq!(crm.search_products("artikull", 50).unwrap().len(), 5);
    crm.close().unwrap();
    let raw = crm_storage::database_stats(&paths.db, Some(&key)).unwrap();
    let settings_before = stats
        .tables
        .iter()
        .find(|(t, _)| t == "settings")
        .unwrap()
        .1;
    let settings_after = raw.tables.iter().find(|(t, _)| t == "settings").unwrap().1;
    // pin_hash went; last_backup_at (daily copy) came.
    assert_eq!(settings_before, 1);
    assert_eq!(settings_after, 1);
}

#[test]
fn the_wipe_from_inside_the_app_abandons_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let (vault, key, wipe) = keyed_root(dir.path());
    let paths = DataPaths::at(dir.path());
    let crm = Crm::open(paths.clone(), &key).unwrap();
    product(&crm, "A1", "Artikull", 1, 1, 2);
    assert_eq!(crm.search_products("artikull", 5).unwrap().len(), 1);
    let _ = crm.list_tabs().unwrap(); // a pooled reader was used
    let outcome = vault.submit_with(&wipe, || crm.abandon()).unwrap();
    assert!(matches!(outcome, Unlock::Wiped));
    assert!(crm.is_closed());
    assert!(matches!(crm.list_tabs(), Err(ServiceError::Closed)));
    assert!(matches!(crm.close(), Err(ServiceError::Closed)));
    assert!(!crm.refresh_if_changed());
    assert!(!paths.db.exists());
    assert!(!paths.key.exists());
}

//! AC 5: `data_version` changes after a commit and only then.

use crm_storage::repo::settings;
use crm_storage::{DataPaths, Db, DbKey, StorageError};

#[test]
fn data_version_changes_only_after_a_commit() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&DataPaths::at(dir.path()), &DbKey::for_tests(1)).unwrap();
    let v0 = db.data_version().unwrap();
    assert_eq!(db.data_version().unwrap(), v0, "no commit, no change");

    db.write(|tx| settings::set(tx, "k", "1")).unwrap();
    let v1 = db.data_version().unwrap();
    assert_ne!(v1, v0, "a commit changes it");
    assert_eq!(db.data_version().unwrap(), v1);

    // A rolled-back write commits nothing.
    let err = db
        .write(|tx| -> Result<(), StorageError> {
            settings::set(tx, "k", "2")?;
            Err(StorageError::NotFound)
        })
        .unwrap_err();
    assert_eq!(err, StorageError::NotFound);
    assert_eq!(db.data_version().unwrap(), v1);

    // Reads change nothing either.
    db.read(|c| settings::get(c, "k")).unwrap();
    assert_eq!(db.data_version().unwrap(), v1);
}

#[test]
fn a_commit_from_another_handle_is_seen() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    let a = Db::open(&paths, &DbKey::for_tests(1)).unwrap();
    let b = Db::open(&paths, &DbKey::for_tests(1)).unwrap();
    let v0 = a.data_version().unwrap();
    b.write(|tx| settings::set(tx, "k", "1")).unwrap();
    assert_ne!(a.data_version().unwrap(), v0);
}

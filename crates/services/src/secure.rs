//! Bringing plaintext data under the key (013, "Plaintext migration"): run after
//! the unlock and before [`Crm::open`](crate::Crm::open).

use crate::error::Result;
use crm_storage::{
    DataPaths, DbKey, FileKind, StorageError, encrypt_copy, encrypt_in_place, file_kind,
    overwrite_and_delete,
};
use std::path::{Path, PathBuf};

const ENC_TMP: &str = ".enc-tmp";

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Copies `data/crm.key` next to `db` as its `.key` sibling (when it exists).
fn copy_key_sibling(key_file: &Path, db: &Path) -> Result<()> {
    if !key_file.exists() {
        return Ok(());
    }
    let sibling = db.with_extension("key");
    std::fs::copy(key_file, &sibling).map_err(StorageError::from)?;
    std::fs::OpenOptions::new()
        .write(true)
        .open(&sibling)
        .and_then(|f| f.sync_all())
        .map_err(StorageError::from)?;
    Ok(())
}

/// Encrypts whatever is still plaintext, idempotently:
/// 1. `data/crm.db` (resuming an interrupted [`encrypt_in_place`]);
/// 2. every plaintext `backups/*.db`: a copy that passes `quick_check` is
///    encrypted through `<name>.enc-tmp` (verified), the plaintext is overwritten
///    and deleted, the tmp file takes its name, and it gets its `.key` sibling;
///    a copy that fails the check is overwritten and deleted; `-wal`/`-shm`
///    sidecars of plaintext copies are overwritten and deleted.
///
/// Call it while no database is open. It logs counts only.
pub fn secure_storage(paths: &DataPaths, key: &DbKey) -> Result<()> {
    encrypt_in_place(paths, key)?;
    let entries = match std::fs::read_dir(&paths.backups) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(StorageError::from(e).into()),
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    files.sort();

    // An interrupted run may have left `X.db.enc-tmp` (verified) after erasing `X.db`.
    for tmp in files
        .iter()
        .filter(|p| p.to_string_lossy().ends_with(ENC_TMP))
    {
        let original = PathBuf::from(tmp.to_string_lossy().trim_end_matches(ENC_TMP).to_owned());
        if original.exists() {
            std::fs::remove_file(tmp).map_err(StorageError::from)?;
        } else {
            std::fs::rename(tmp, &original).map_err(StorageError::from)?;
            copy_key_sibling(&paths.key, &original)?;
        }
    }

    let (mut secured, mut removed) = (0usize, 0usize);
    for db in files
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "db"))
    {
        if file_kind(db)? != FileKind::Plaintext {
            continue;
        }
        let tmp = with_suffix(db, ENC_TMP);
        match encrypt_copy(db, &tmp, key) {
            Ok(_) => {
                overwrite_and_delete(db)?;
                std::fs::rename(&tmp, db).map_err(StorageError::from)?;
                copy_key_sibling(&paths.key, db)?;
                secured += 1;
            }
            // Fails `quick_check` (or is no database at all): not worth keeping.
            Err(StorageError::Corrupt { .. }) => {
                overwrite_and_delete(db)?;
                removed += 1;
            }
            Err(e) => return Err(e.into()),
        }
        for side in ["-wal", "-shm", "-journal"] {
            overwrite_and_delete(&with_suffix(db, side))?;
        }
    }
    // Sidecars whose copy is gone (or was encrypted by an earlier run).
    for side in files.iter().filter(|p| {
        let name = p.to_string_lossy();
        name.ends_with(".db-wal") || name.ends_with(".db-shm")
    }) {
        let name = side.to_string_lossy();
        let db = PathBuf::from(name[..name.len() - 4].to_owned());
        if file_kind(&db)? != FileKind::Encrypted || !db.exists() {
            overwrite_and_delete(side)?;
        }
    }
    if secured + removed > 0 {
        tracing::info!("secured {secured} plaintext backups, removed {removed} unreadable ones");
    }
    Ok(())
}

/// Creates a plaintext database as the versions before encryption left it and
/// runs `seed_sql` on it: only for the migration tests of the UI and the app.
#[doc(hidden)]
pub fn create_legacy_plaintext(paths: &DataPaths, seed_sql: &str) -> Result<()> {
    Ok(crm_storage::Db::create_legacy_plaintext(paths, seed_sql)?)
}

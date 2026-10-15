//! Backups and restore through the facade.

use crate::crm::{Crm, Fault};
use crate::error::{Result, ServiceError};
use crm_storage::repo::settings;
use crm_storage::{BackupName, BackupReason, Db, backup};
use jiff::Timestamp;
use std::path::{Path, PathBuf};

/// One file in `backups/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupEntry {
    /// Full path.
    pub path: PathBuf,
    /// The parsed name (date, time, reason).
    pub name: BackupName,
    /// File size in bytes.
    pub size: u64,
}

impl Crm {
    /// "Rezervo tani": a `-manual` copy in `backups/` (rotated).
    pub fn backup_now(&self) -> Result<PathBuf> {
        self.with_db_committing(|db| Ok(db.backup_now(BackupReason::Manual)?))
    }

    /// "Rezervo në këtë kompjuter…": a copy into a folder the user picked.
    pub fn backup_to_pc(&self, dir: &Path) -> Result<PathBuf> {
        self.with_db_committing(|db| Ok(db.backup_to(dir)?))
    }

    /// When the last backup was taken (status bar), `None` = never.
    pub fn last_backup_at(&self) -> Result<Option<Timestamp>> {
        self.read(|c| Ok(settings::last_backup_at(c)?))
    }

    /// The backup copies in `backups/`, newest first. Other files are ignored.
    pub fn list_backups(&self) -> Result<Vec<BackupEntry>> {
        let dir = &self.paths().backups;
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(crm_storage::StorageError::from(e).into()),
        };
        let mut out = Vec::new();
        for entry in entries {
            let entry = entry.map_err(crm_storage::StorageError::from)?;
            let file_name = entry.file_name();
            let Some(name) = file_name.to_str().and_then(BackupName::parse) else {
                continue;
            };
            let meta = entry.metadata().map_err(crm_storage::StorageError::from)?;
            if !meta.is_file() {
                continue;
            }
            out.push(BackupEntry {
                path: entry.path(),
                name,
                size: meta.len(),
            });
        }
        out.sort_by(|a, b| b.name.cmp(&a.name));
        Ok(out)
    }

    /// Replaces the data with `backup` in one call: checks the copy, closes the
    /// database, restores (the current data is saved as a `-before-restore`
    /// copy, whose path is returned), reopens and rebuilds the search index. A
    /// copy that fails its check leaves everything as it was.
    pub fn restore(&self, backup_path: &Path) -> Result<Option<PathBuf>> {
        backup::verify(backup_path, self.key())?;
        let mut slot = self.db_slot();
        let db = slot.take().ok_or(ServiceError::Closed)?;
        if let Err(e) = db.checkpoint_and_close() {
            tracing::warn!("checkpoint before restore failed: {e}");
        }
        let restored = backup::restore_from(backup_path, self.paths(), self.key());
        // Reopen whatever is in place now (the restored copy, or the old data
        // when the restore failed before replacing it).
        let reopened = if self.take_fault(Fault::ReopenAfterRestore) {
            Err(crm_storage::StorageError::DeviceUnavailable(
                "injected reopen failure".into(),
            ))
        } else {
            Db::open(self.paths(), self.key())
        };
        let (safety, db) = match (restored, reopened) {
            (Ok(safety), Ok(db)) => (safety, db),
            (Err(e), Ok(db)) => {
                *slot = Some(db);
                return Err(e.into());
            }
            (_, Err(e)) => return Err(e.into()),
        };
        // The restore is done: from here on nothing may fail the call (F-013),
        // and the reopened database goes into the slot first.
        let version = db.data_version();
        *slot = Some(db);
        drop(slot);
        match version {
            Ok(version) => self.reset_data_version(version),
            Err(e) => tracing::warn!("data_version after restore failed: {e}"),
        }
        self.rebuild_index_after_commit();
        Ok(safety)
    }
}

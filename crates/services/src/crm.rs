//! The [`Crm`] facade: the one handle the UI holds.

use crate::error::{Result, ServiceError};
use crate::index_sync;
use crm_domain::time;
use crm_search::SearchIndex;
use crm_storage::repo::settings;
use crm_storage::{BackupReason, Conn, DataPaths, Db, DbKey, Tx};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// The application services. Cheap to clone (one `Arc`), `Send + Sync`: UI
/// worker threads may each hold a clone.
///
/// Every write use case runs in exactly one storage transaction, then updates
/// the search index in-process. Long operations (import, export, report, backup,
/// restore) are synchronous; run them off the UI thread.
#[derive(Clone)]
pub struct Crm(Arc<Inner>);

struct Inner {
    /// `None` after [`Crm::close`] (or a failed restore). The write lock is taken
    /// only by `close` and `restore`, which replace the database.
    db: RwLock<Option<Db>>,
    index: RwLock<SearchIndex>,
    paths: DataPaths,
    last_data_version: AtomicI64,
    /// Set when an index update after a commit failed (F-013): the next poll
    /// rebuilds the whole index.
    index_dirty: AtomicBool,
    /// Test-only failures armed through [`Crm::inject_fault`].
    faults: Mutex<Vec<Fault>>,
    /// The data key (every connection, backup and restore uses it).
    key: DbKey,
}

/// A failure the integration tests can arm with [`Crm::inject_fault`]; each
/// one fires once. Not part of the application API.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// The next search-index read after a commit fails.
    IndexRead,
    /// The import transaction fails after every row was applied (so it rolls back).
    ImportAfterApply,
    /// Reopening the database at the end of a restore fails.
    ReopenAfterRestore,
}

impl std::fmt::Debug for Crm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Crm").field("paths", &self.0.paths).finish()
    }
}

fn read_lock<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn write_lock<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Crm {
    /// Opens the encrypted database under `paths` with `key` (creating it on
    /// first run; the storage layer takes the pre-migration backup when an older
    /// schema is found), builds the search index, deletes the legacy `pin_hash`
    /// setting (a no-op when absent), and takes the daily backup when one is due.
    /// Call [`secure_storage`](crate::secure_storage) first.
    ///
    /// The caller holds the [`InstanceLock`](crm_storage::InstanceLock).
    pub fn open(paths: DataPaths, key: &DbKey) -> Result<Crm> {
        let db = Db::open(&paths, key)?;
        let index = db.read(index_sync::load)?;
        let version = db.data_version()?;
        let crm = Crm(Arc::new(Inner {
            db: RwLock::new(Some(db)),
            index: RwLock::new(index),
            paths,
            last_data_version: AtomicI64::new(version),
            index_dirty: AtomicBool::new(false),
            faults: Mutex::new(Vec::new()),
            key: key.clone(),
        }));
        // The UI-lock hash of the versions before encryption; the password replaces it.
        if let Err(e) = crm.write(|tx| Ok(settings::delete(tx, "pin_hash")?)) {
            tracing::warn!("removing the legacy lock setting failed: {e}");
        }
        crm.daily_backup_if_due()?;
        Ok(crm)
    }

    /// Takes the daily backup when today's is missing. Returns whether it ran.
    pub fn daily_backup_if_due(&self) -> Result<bool> {
        self.with_db_committing(|db| {
            if db.daily_backup_due(time::now())? {
                db.backup_now(BackupReason::Daily)?;
                Ok(true)
            } else {
                Ok(false)
            }
        })
    }

    /// The data layout (for the status bar and the settings dialog).
    pub fn paths(&self) -> &DataPaths {
        &self.0.paths
    }

    /// Polls `PRAGMA data_version`; when the database changed since the last
    /// poll through a commit not made by this facade (another process or tool),
    /// rebuilds the search index and returns `true`, so the UI refreshes its
    /// lists. The facade's own commits (every use case, draft autosaves, backups)
    /// record the version they produced, so they do not count (F-001): their
    /// callers update the index and their views themselves. Task 008 calls it
    /// every 2 s. Errors are logged and reported as "no change".
    pub fn refresh_if_changed(&self) -> bool {
        let version = match self.with_db(|db| Ok(db.data_version()?)) {
            Ok(v) => v,
            // Closed or abandoned: nothing to poll, and nothing to log.
            Err(ServiceError::Closed) => return false,
            Err(e) => {
                tracing::warn!("data_version poll failed: {e}");
                return false;
            }
        };
        let changed = self.0.last_data_version.swap(version, Ordering::SeqCst) != version;
        let dirty = self.0.index_dirty.swap(false, Ordering::SeqCst);
        if !changed && !dirty {
            return false;
        }
        if let Err(e) = self.rebuild_index() {
            tracing::warn!("search index rebuild failed: {e}");
            self.mark_index_dirty();
        }
        true
    }

    /// Arms a one-shot test failure. Not part of the application API.
    #[doc(hidden)]
    pub fn inject_fault(&self, fault: Fault) {
        self.faults().push(fault);
    }

    /// Clean shutdown: the exit backup, then `wal_checkpoint(TRUNCATE)`. Later
    /// calls fail with [`ServiceError::Closed`].
    pub fn close(&self) -> Result<()> {
        let mut guard = write_lock(&self.0.db);
        let db = guard.take().ok_or(ServiceError::Closed)?;
        let backup = db.backup_now(BackupReason::Exit);
        let closed = db.checkpoint_and_close();
        backup?;
        closed?;
        Ok(())
    }

    /// Drops the database **without** the exit backup and without a checkpoint
    /// (the wipe path closes every SQLite handle before deleting files). Waits
    /// for running jobs through the slot's write lock. Later calls fail with
    /// [`ServiceError::Closed`].
    pub fn abandon(&self) {
        let db = self.db_slot().take();
        drop(db);
    }

    /// Whether the database is closed (after [`Crm::close`], [`Crm::abandon`]
    /// or a failed restore).
    pub fn is_closed(&self) -> bool {
        read_lock(&self.0.db).is_none()
    }

    // -- internals ---------------------------------------------------------

    /// The data key.
    pub(crate) fn key(&self) -> &DbKey {
        &self.0.key
    }

    /// Runs `f` with the open database.
    pub(crate) fn with_db<T>(&self, f: impl FnOnce(&Db) -> Result<T>) -> Result<T> {
        let guard = read_lock(&self.0.db);
        let db = guard.as_ref().ok_or(ServiceError::Closed)?;
        f(db)
    }

    /// One read transaction.
    pub(crate) fn read<T>(&self, f: impl FnOnce(&Conn<'_>) -> Result<T>) -> Result<T> {
        self.with_db(|db| db.read(f))
    }

    /// One `BEGIN IMMEDIATE` write transaction; an `Err` rolls it back. The
    /// commit is this facade's own: the next poll ignores it.
    pub(crate) fn write<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Tx<'_>) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        self.with_db_committing(|db| db.write(f))
    }

    /// Runs `f`, which may commit through `db` (a write, or a backup recording
    /// `last_backup_at`), then records the `data_version` it left behind, so
    /// [`Crm::refresh_if_changed`] does not report this facade's own commits.
    /// The version is read under the same read lock, so a restore cannot swap
    /// the database in between.
    pub(crate) fn with_db_committing<T>(&self, f: impl FnOnce(&Db) -> Result<T>) -> Result<T> {
        self.with_db(|db| {
            let out = f(db);
            match db.data_version() {
                Ok(version) => self.0.last_data_version.store(version, Ordering::SeqCst),
                Err(e) => tracing::warn!("data_version read after a commit failed: {e}"),
            }
            out
        })
    }

    /// Exclusive access to the database slot (close and restore).
    pub(crate) fn db_slot(&self) -> RwLockWriteGuard<'_, Option<Db>> {
        write_lock(&self.0.db)
    }

    pub(crate) fn index(&self) -> RwLockReadGuard<'_, SearchIndex> {
        read_lock(&self.0.index)
    }

    pub(crate) fn index_mut(&self) -> RwLockWriteGuard<'_, SearchIndex> {
        write_lock(&self.0.index)
    }

    fn faults(&self) -> MutexGuard<'_, Vec<Fault>> {
        self.0
            .faults
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Consumes an armed [`Fault`]; `true` when it was armed.
    pub(crate) fn take_fault(&self, fault: Fault) -> bool {
        let mut faults = self.faults();
        match faults.iter().position(|f| *f == fault) {
            Some(at) => {
                faults.remove(at);
                true
            }
            None => false,
        }
    }

    /// The index may miss a committed change: the next poll rebuilds it.
    pub(crate) fn mark_index_dirty(&self) {
        self.0.index_dirty.store(true, Ordering::SeqCst);
    }

    /// Remembers the current `data_version` (after a restore reopened the file).
    pub(crate) fn reset_data_version(&self, version: i64) {
        self.0.last_data_version.store(version, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crm_is_clone_send_and_sync() {
        fn check<T: Clone + Send + Sync + 'static>() {}
        check::<Crm>();
    }
}

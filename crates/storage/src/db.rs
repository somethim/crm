//! The database handle: one writer thread plus a pool of readers.

use crate::backup;
use crate::conn::{self, BUSY_TIMEOUT, Conn, ConnPragmas, Tx};
use crate::error::{Result, StorageError, to_sqlite};
use crate::guard;
use crate::key::DbKey;
use crate::migrations;
use crate::paths::DataPaths;
use r2d2::{Pool, PooledConnection};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, OpenFlags, TransactionBehavior};
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

/// Number of pooled reader connections.
const READERS: u32 = 3;

type Job = Box<dyn FnOnce(&mut Connection) + Send>;

/// The open database. `Send + Sync`: share it (for example in an `Arc`) across
/// UI worker threads. Every write runs on one dedicated writer thread inside
/// `BEGIN IMMEDIATE`; reads use a pool of WAL readers and never wait for it.
///
/// Calling [`Db::write`] from inside a write closure deadlocks, and [`Db::read`]
/// there sees the snapshot from before the transaction; call the repository read
/// functions on the `Tx` instead (it derefs to [`Conn`]).
pub struct Db {
    paths: DataPaths,
    sender: Option<mpsc::Sender<Job>>,
    thread: Option<JoinHandle<()>>,
    pool: Pool<SqliteConnectionManager>,
    version_conn: Mutex<Connection>,
    key: DbKey,
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Db").field("db", &self.paths.db).finish()
    }
}

impl Db {
    /// Opens (creating if needed) the encrypted `paths.db` with `key`:
    /// 1. refuses network drives and read-only locations;
    /// 2. opens the writer connection, runs `PRAGMA key` before anything else
    ///    (another key gives [`StorageError::WrongKey`]) and configures it;
    /// 3. runs `PRAGMA quick_check` (≠ `ok`, e.g. a tampered page →
    ///    [`StorageError::Corrupt`]; the caller decides on a restore);
    /// 4. migrates to the latest schema, taking a `pre-migrate-v{N}` backup first
    ///    when an existing database is behind;
    /// 5. builds the keyed reader pool and the keyed data-version connection, and
    ///    spawns the writer thread.
    ///
    /// A new database is created encrypted. Backups are encrypted with the same
    /// key and get a sibling `.key` copy of `paths.key` when it exists.
    ///
    /// The caller holds the [`InstanceLock`](crate::InstanceLock).
    pub fn open(paths: &DataPaths, key: &DbKey) -> Result<Db> {
        Self::open_with(paths, key, BUSY_TIMEOUT)
    }

    /// [`Db::open`] with a custom `busy_timeout`. Meant for tests (AC 3 uses 200 ms).
    #[doc(hidden)]
    pub fn open_with(paths: &DataPaths, key: &DbKey, busy_timeout: Duration) -> Result<Db> {
        guard::check_local_drive(&paths.db)?;
        paths.ensure_dirs()?;
        let key = key.clone();

        let mut writer = conn::open_keyed_connection(&paths.db, OpenFlags::default(), Some(&key))?;
        conn::configure(&writer, busy_timeout)?;
        conn::quick_check(&writer)?;
        migrations::migrate(&mut writer, |c| {
            backup::pre_migrate(c, paths, migrations::LATEST_VERSION, &key).map(|_| ())
        })?;

        let pool_key = key.clone();
        let manager = SqliteConnectionManager::file(&paths.db)
            .with_flags(
                // WAL readers need write access to the -shm file.
                OpenFlags::SQLITE_OPEN_READ_WRITE
                    | OpenFlags::SQLITE_OPEN_URI
                    | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
            .with_init(move |c| {
                conn::apply_key(c, Some(&pool_key)).map_err(to_sqlite)?;
                conn::configure(c, busy_timeout)
            });
        let pool = Pool::builder()
            .max_size(READERS)
            .connection_timeout(BUSY_TIMEOUT)
            .build(manager)?;

        let version_conn = conn::open_keyed_connection(
            &paths.db,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            Some(&key),
        )?;
        conn::configure(&version_conn, busy_timeout)?;

        let (sender, receiver) = mpsc::channel::<Job>();
        let thread = std::thread::Builder::new()
            .name("crm-db-writer".into())
            .spawn(move || {
                for job in receiver {
                    job(&mut writer);
                }
            })?;

        Ok(Db {
            paths: paths.clone(),
            sender: Some(sender),
            thread: Some(thread),
            pool,
            version_conn: Mutex::new(version_conn),
            key,
        })
    }

    /// The key this database was opened with.
    pub(crate) fn key(&self) -> &DbKey {
        &self.key
    }

    /// Creates a **plaintext** schema-v1 database at `paths.db`, as the versions
    /// before encryption left it, and runs `seed_sql` on it. Only for tests of
    /// the plaintext → encrypted migration; nothing else opens a database
    /// without a key.
    #[doc(hidden)]
    pub fn create_legacy_plaintext(paths: &DataPaths, seed_sql: &str) -> Result<()> {
        paths.ensure_dirs()?;
        let mut conn = Connection::open(&paths.db)?;
        conn::configure(&conn, BUSY_TIMEOUT)?;
        migrations::migrate(&mut conn, |_| Ok(()))?;
        conn.execute_batch(seed_sql)?;
        Ok(())
    }

    /// The layout this database was opened from.
    pub fn paths(&self) -> &DataPaths {
        &self.paths
    }

    /// Runs `f` on the writer thread inside `BEGIN IMMEDIATE`. Ok commits, Err
    /// (or a panic) rolls back. Waits up to `busy_timeout` for another process'
    /// write lock, then fails with [`StorageError::Busy`].
    ///
    /// `E` is any error that can carry a [`StorageError`], so services can abort a
    /// transaction with their own error (for example a stock error).
    pub fn write<T, E, F>(&self, f: F) -> Result<T, E>
    where
        F: FnOnce(&Tx<'_>) -> Result<T, E> + Send + 'static,
        T: Send + 'static,
        E: From<StorageError> + Send + 'static,
    {
        self.on_writer(move |conn| Ok(run_transaction(conn, f)))
            .map_err(E::from)?
    }

    /// Runs `f` on a pooled reader, inside one read transaction (one consistent
    /// snapshot). Never blocked by the writer.
    pub fn read<T, E, F>(&self, f: F) -> Result<T, E>
    where
        F: FnOnce(&Conn<'_>) -> Result<T, E>,
        E: From<StorageError>,
    {
        let reader = self.reader().map_err(E::from)?;
        let snapshot = reader
            .unchecked_transaction()
            .map_err(|e| E::from(StorageError::from(e)))?;
        f(&Conn::new(&snapshot))
        // `snapshot` drops here and ends the read transaction.
    }

    /// `PRAGMA data_version` on a dedicated connection: it changes after any other
    /// connection (this writer or another process) commits. Poll it to notice changes.
    pub fn data_version(&self) -> Result<i64> {
        let conn = self
            .version_conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Ok(conn.pragma_query_value(None, "data_version", |row| row.get(0))?)
    }

    /// The pragmas reported by the writer, every pooled reader and the
    /// data-version connection (diagnostics; AC 1).
    pub fn connection_pragmas(&self) -> Result<Vec<ConnPragmas>> {
        self.on_every_connection(conn::read_pragmas)
    }

    /// `count(*)` of `settings` on the writer, every pooled reader and the
    /// data-version connection: proves each one decrypts (tests).
    #[doc(hidden)]
    pub fn settings_rows_per_connection(&self) -> Result<Vec<(String, i64)>> {
        self.on_every_connection(|c, role| {
            let rows = c.query_row("SELECT count(*) FROM settings", [], |row| row.get(0))?;
            Ok((role, rows))
        })
    }

    fn on_every_connection<T: Send + 'static>(
        &self,
        f: fn(&Connection, String) -> Result<T>,
    ) -> Result<Vec<T>> {
        let mut out = vec![self.on_writer(move |c| f(c, "writer".into()))?];
        let readers = (0..READERS)
            .map(|_| self.reader())
            .collect::<Result<Vec<_>>>()?;
        for (i, reader) in readers.iter().enumerate() {
            out.push(f(reader, format!("reader-{i}"))?);
        }
        let version = self
            .version_conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        out.push(f(&version, "data-version".into())?);
        Ok(out)
    }

    /// Clean shutdown: `wal_checkpoint(TRUNCATE)` and `PRAGMA optimize` on the
    /// writer, then the writer thread stops.
    pub fn checkpoint_and_close(mut self) -> Result<()> {
        let result = self.on_writer(|c| {
            c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
            c.execute_batch("PRAGMA optimize")?;
            Ok(())
        });
        self.shutdown();
        result
    }

    /// Runs `f` on the writer connection outside any transaction.
    pub(crate) fn on_writer<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let sender = self
            .sender
            .as_ref()
            .ok_or_else(|| StorageError::other("the database is closed"))?;
        let (reply, answer) = mpsc::sync_channel(1);
        let job: Job = Box::new(move |conn| {
            let outcome = panic::catch_unwind(AssertUnwindSafe(|| f(conn)))
                .unwrap_or_else(|_| Err(StorageError::other("a database job panicked")));
            let _ = reply.send(outcome);
        });
        sender
            .send(job)
            .map_err(|_| StorageError::other("the writer thread stopped"))?;
        answer
            .recv()
            .map_err(|_| StorageError::other("the writer thread stopped"))?
    }

    pub(crate) fn reader(&self) -> Result<PooledConnection<SqliteConnectionManager>> {
        Ok(self.pool.get()?)
    }

    fn shutdown(&mut self) {
        self.sender.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Db {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run_transaction<T, E, F>(conn: &mut Connection, f: F) -> Result<T, E>
where
    F: FnOnce(&Tx<'_>) -> Result<T, E>,
    E: From<StorageError>,
{
    let txn = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| E::from(StorageError::from(e)))?;
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| f(&Tx::new(&txn))));
    match outcome {
        Ok(Ok(value)) => {
            txn.commit().map_err(|e| E::from(StorageError::from(e)))?;
            Ok(value)
        }
        // Dropping `txn` rolls back.
        Ok(Err(err)) => Err(err),
        Err(_) => Err(E::from(StorageError::other("a write closure panicked"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn db_is_send_and_sync() {
        assert_send_sync::<Db>();
    }

    #[test]
    fn a_panicking_write_rolls_back_and_the_writer_survives() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&DataPaths::at(dir.path()), &DbKey::for_tests(1)).unwrap();
        let err = db
            .write(|tx| -> Result<()> {
                tx.raw()
                    .execute("INSERT INTO settings VALUES ('a', '1')", [])?;
                panic!("boom");
            })
            .unwrap_err();
        assert!(matches!(err, StorageError::Other(_)));
        let count: i64 = db
            .read(|c| {
                Ok::<_, StorageError>(c.raw().query_row(
                    "SELECT count(*) FROM settings",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(count, 0);
        db.write(|tx| -> Result<()> {
            tx.raw()
                .execute("INSERT INTO settings VALUES ('a', '1')", [])?;
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn an_err_rolls_back() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&DataPaths::at(dir.path()), &DbKey::for_tests(1)).unwrap();
        let err = db
            .write(|tx| -> Result<()> {
                tx.raw()
                    .execute("INSERT INTO settings VALUES ('a', '1')", [])?;
                Err(StorageError::NotFound)
            })
            .unwrap_err();
        assert_eq!(err, StorageError::NotFound);
        let count: i64 = db
            .read(|c| {
                Ok::<_, StorageError>(c.raw().query_row(
                    "SELECT count(*) FROM settings",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(count, 0);
    }
}

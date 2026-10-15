//! Per-connection setup and the wrappers repositories take instead of rusqlite types.

use crate::error::{Result, StorageError, to_sqlite};
use crate::key::DbKey;
use rusqlite::{Connection, ErrorCode, OpenFlags};
use std::ops::Deref;
use std::path::Path;
use std::time::Duration;

/// The default `busy_timeout` (001): a writer waits up to 5 s for the lock.
pub const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// `journal_size_limit` in bytes (64 MiB).
pub const JOURNAL_SIZE_LIMIT: i64 = 67_108_864;

/// Applies the 001 pragmas to one connection. `journal_mode` must come back as
/// `wal`, else the connection is refused.
pub(crate) fn configure(conn: &Connection, busy_timeout: Duration) -> rusqlite::Result<()> {
    let mode: String =
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(to_sqlite(StorageError::Other(format!(
            "journal_mode is {mode}, expected wal"
        ))));
    }
    conn.pragma_update(None, "synchronous", "FULL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(busy_timeout)?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    conn.pragma_update(None, "mmap_size", 0)?;
    conn.pragma_update(None, "journal_size_limit", JOURNAL_SIZE_LIMIT)?;
    conn.pragma_update(None, "cache_size", -16_000)?;
    conn.pragma_update(None, "wal_autocheckpoint", 1_000)?;
    Ok(())
}

/// Opens `path` with `flags`, then [`apply_key`]: with a key, `PRAGMA key` runs
/// before any other statement.
pub(crate) fn open_keyed_connection(
    path: &Path,
    flags: OpenFlags,
    key: Option<&DbKey>,
) -> Result<Connection> {
    let conn = Connection::open_with_flags(path, flags)?;
    apply_key(&conn, key)?;
    Ok(conn)
}

/// On a freshly opened connection: `PRAGMA key` with [`DbKey::pragma_text`] (when `key` is
/// given), then `SELECT count(*) FROM sqlite_schema` to force the key check.
/// With a key, `SQLITE_NOTADB` becomes [`StorageError::WrongKey`]; without one
/// it stays [`StorageError::Corrupt`] (a file that is not a database).
pub(crate) fn apply_key(conn: &Connection, key: Option<&DbKey>) -> Result<()> {
    if let Some(key) = key {
        conn.pragma_update(None, "key", &*key.pragma_text())?;
    }
    match conn.query_row("SELECT count(*) FROM sqlite_schema", [], |_| Ok(())) {
        Ok(()) => Ok(()),
        Err(rusqlite::Error::SqliteFailure(e, _))
            if key.is_some() && e.code == ErrorCode::NotADatabase =>
        {
            Err(StorageError::WrongKey)
        }
        Err(e) => Err(e.into()),
    }
}

/// The pragma values one connection reports (diagnostics and AC 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnPragmas {
    /// `writer`, `reader-N` or `data-version`.
    pub role: String,
    /// `journal_mode` (`wal`).
    pub journal_mode: String,
    /// `synchronous` (2 = FULL).
    pub synchronous: i64,
    /// `foreign_keys` (1).
    pub foreign_keys: i64,
    /// `temp_store` (2 = MEMORY).
    pub temp_store: i64,
    /// `busy_timeout` in ms.
    pub busy_timeout: i64,
    /// `mmap_size` (0).
    pub mmap_size: i64,
    /// `journal_size_limit` in bytes.
    pub journal_size_limit: i64,
    /// `PRAGMA cipher`: `chacha20` on a keyed connection (sqlite3mc reports its
    /// default cipher, also `chacha20`, on an unkeyed one).
    pub cipher: String,
}

pub(crate) fn read_pragmas(conn: &Connection, role: String) -> Result<ConnPragmas> {
    let int =
        |name: &str| -> Result<i64> { Ok(conn.pragma_query_value(None, name, |row| row.get(0))?) };
    Ok(ConnPragmas {
        role,
        journal_mode: conn.pragma_query_value(None, "journal_mode", |row| row.get(0))?,
        synchronous: int("synchronous")?,
        foreign_keys: int("foreign_keys")?,
        temp_store: int("temp_store")?,
        busy_timeout: int("busy_timeout")?,
        mmap_size: int("mmap_size")?,
        journal_size_limit: int("journal_size_limit")?,
        cipher: conn
            .pragma_query_value(None, "cipher", |row| row.get::<_, Option<String>>(0))?
            .unwrap_or_default(),
    })
}

/// Runs `PRAGMA quick_check`; anything but `ok` is [`StorageError::Corrupt`].
pub(crate) fn quick_check(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA quick_check")?;
    let rows: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    if rows.len() == 1 && rows[0] == "ok" {
        Ok(())
    } else {
        Err(StorageError::Corrupt {
            details: rows.join("; "),
        })
    }
}

/// A read connection handed to [`Db::read`](crate::Db::read) closures and to
/// repository read functions. Opaque outside this crate.
pub struct Conn<'a> {
    raw: &'a Connection,
}

impl<'a> Conn<'a> {
    pub(crate) fn new(raw: &'a Connection) -> Self {
        Self { raw }
    }

    pub(crate) fn raw(&self) -> &'a Connection {
        self.raw
    }
}

/// The open `BEGIN IMMEDIATE` transaction handed to [`Db::write`](crate::Db::write)
/// closures. It derefs to [`Conn`], so every repository read also works inside
/// a write. Opaque outside this crate.
pub struct Tx<'a> {
    conn: Conn<'a>,
}

impl<'a> Tx<'a> {
    pub(crate) fn new(raw: &'a Connection) -> Self {
        Self {
            conn: Conn::new(raw),
        }
    }
}

impl<'a> Deref for Tx<'a> {
    type Target = Conn<'a>;

    fn deref(&self) -> &Conn<'a> {
        &self.conn
    }
}

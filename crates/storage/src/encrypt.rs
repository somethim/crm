//! Plaintext → encrypted: the file-kind probe, the keyed `VACUUM INTO` copy,
//! the overwrite-then-delete of plaintext files, and the resumable in-place
//! migration of `data/crm.db` (013, "Plaintext migration").

use crate::conn;
use crate::error::{Result, StorageError};
use crate::key::DbKey;
use crate::paths::DataPaths;
use rusqlite::{Connection, OpenFlags};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";

/// What a database file looks like on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// No file.
    Missing,
    /// A zero-length file (SQLite treats it as a new database).
    Empty,
    /// Starts with `SQLite format 3\0`.
    Plaintext,
    /// Any other content: an encrypted database (or something that is not one).
    Encrypted,
}

/// Reads the first 16 bytes of `path` and classifies the file.
pub fn file_kind(path: &Path) -> Result<FileKind> {
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(FileKind::Missing),
        Err(e) => return Err(e.into()),
    };
    let mut head = [0u8; 16];
    let mut filled = 0;
    while filled < head.len() {
        let n = file.read(&mut head[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    Ok(match filled {
        0 => FileKind::Empty,
        16 if &head == SQLITE_HEADER => FileKind::Plaintext,
        _ => FileKind::Encrypted,
    })
}

/// What [`encrypt_copy`] found in the source (and checked in the copy).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyStats {
    /// `PRAGMA user_version`.
    pub user_version: i64,
    /// `count(*)` of every table, by name, sorted.
    pub tables: Vec<(String, i64)>,
}

fn stats(conn: &Connection) -> Result<CopyStats> {
    let user_version = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let names: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_schema WHERE type = 'table' ORDER BY name")?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut tables = Vec::with_capacity(names.len());
    for name in names {
        let sql = format!("SELECT count(*) FROM \"{}\"", name.replace('"', "\"\""));
        let count = conn.query_row(&sql, [], |row| row.get(0))?;
        tables.push((name, count));
    }
    Ok(CopyStats {
        user_version,
        tables,
    })
}

/// `user_version` and every table's row count of the database at `path`, opened
/// read-only (with `key` when given). Tests and migration checks.
#[doc(hidden)]
pub fn database_stats(path: &Path, key: Option<&DbKey>) -> Result<CopyStats> {
    let conn = conn::open_keyed_connection(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        key,
    )?;
    stats(&conn)
}

/// Percent-encodes a path for a `file:` URI: `\` becomes `/`, a Windows drive
/// becomes `/E:/…`, and every byte outside `A–Z a–z 0–9 - . _ ~ / :` is `%XX`.
fn encode_uri_path(path: &str) -> String {
    let path = path.strip_prefix(r"\\?\").unwrap_or(path);
    let mut text = path.replace('\\', "/");
    let bytes = text.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        text.insert(0, '/');
    }
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/:".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// `file://<absolute path, %-encoded>?cipher=chacha20&key=raw:<hex>`.
fn keyed_uri(path: &Path, key: &DbKey) -> Result<Zeroizing<String>> {
    let absolute = std::path::absolute(path)?;
    let text = absolute
        .to_str()
        .ok_or_else(|| StorageError::other("database path is not valid UTF-8"))?;
    let mut uri = Zeroizing::new(String::with_capacity(text.len() * 3 + 96));
    uri.push_str("file://");
    uri.push_str(&encode_uri_path(text));
    uri.push_str("?cipher=chacha20&key=");
    uri.push_str(&key.pragma_text());
    Ok(uri)
}

/// Writes an encrypted copy of the plaintext database `src` to `dst`:
/// `quick_check` and count the source, `VACUUM INTO` the keyed URI of `dst`,
/// checkpoint and close the source, then open `dst` with `key`, `quick_check`
/// it and compare every table count and `user_version`. A stale `dst` is
/// replaced; a copy that differs is deleted.
pub fn encrypt_copy(src: &Path, dst: &Path, key: &DbKey) -> Result<CopyStats> {
    let source = conn::open_keyed_connection(src, OpenFlags::default(), None)?;
    conn::quick_check(&source)?;
    let expected = stats(&source)?;
    remove_if_exists(dst)?;
    let uri = keyed_uri(dst, key)?;
    let copied = source
        .execute("VACUUM INTO ?1", [uri.as_str()])
        .map_err(StorageError::from);
    drop(uri);
    if let Err(err) = copied {
        let _ = std::fs::remove_file(dst);
        return Err(err);
    }
    source.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    drop(source);

    let checked = (|| {
        let copy = conn::open_keyed_connection(
            dst,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            Some(key),
        )?;
        conn::quick_check(&copy)?;
        if stats(&copy)? != expected {
            return Err(StorageError::other("encrypted copy differs"));
        }
        Ok(())
    })();
    if let Err(err) = checked {
        let _ = std::fs::remove_file(dst);
        return Err(err);
    }
    Ok(expected)
}

/// Overwrites `path` with random bytes over its full length, syncs it, then
/// deletes it. A missing file is not an error.
pub fn overwrite_and_delete(path: &Path) -> Result<()> {
    let mut file = match std::fs::OpenOptions::new().write(true).open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    let mut left = file.metadata()?.len();
    let mut buf = vec![0u8; 64 * 1024];
    while left > 0 {
        let n = usize::try_from(left).map_or(buf.len(), |l| l.min(buf.len()));
        getrandom::fill(&mut buf[..n])
            .map_err(|e| StorageError::other(format!("random bytes: {e}")))?;
        file.write_all(&buf[..n])?;
        left -= n as u64;
    }
    file.sync_all()?;
    drop(file);
    std::fs::remove_file(path)?;
    Ok(())
}

fn remove_if_exists(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// The steps of [`encrypt_in_place`] after which a test can stop it, to prove
/// that a rerun resumes from any interruption.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Steps 1–4: `crm.db.enc` written and verified; crm.db still plaintext.
    Copied,
    /// Step 5: crm.db renamed to `crm.db.plain`, the plaintext `-wal`/`-shm` erased.
    PlainSetAside,
    /// Step 6: `crm.db.enc` renamed to crm.db.
    EncryptedInPlace,
    /// Step 7: `crm.db.plain` overwritten and deleted.
    Done,
}

struct WorkFiles {
    enc: PathBuf,
    plain: PathBuf,
    wal: PathBuf,
    shm: PathBuf,
}

impl WorkFiles {
    fn of(paths: &DataPaths) -> Self {
        let side = |suffix: &str| {
            let mut name = paths.db.clone().into_os_string();
            name.push(suffix);
            PathBuf::from(name)
        };
        Self {
            enc: side(".enc"),
            plain: side(".plain"),
            wal: side("-wal"),
            shm: side("-shm"),
        }
    }
}

/// Turns a plaintext `data/crm.db` (WAL content included) into an encrypted
/// one, idempotently. Call it while no [`Db`](crate::Db) is open. Steps:
/// 1–4. [`encrypt_copy`] into `crm.db.enc`;
/// 5. rename crm.db to `crm.db.plain`, overwrite and delete the plaintext `-wal`/`-shm`;
/// 6. rename `crm.db.enc` to crm.db;
/// 7. overwrite and delete `crm.db.plain`.
///
/// Leftovers of an interrupted run resume it: a plaintext crm.db restarts at
/// step 1; a missing crm.db with `.enc` resumes at step 5's erase, then 6; an
/// encrypted crm.db with `.plain` resumes at step 7. An encrypted, missing or
/// empty crm.db without leftovers is left as it is.
pub fn encrypt_in_place(paths: &DataPaths, key: &DbKey) -> Result<()> {
    encrypt_in_place_until(paths, key, Stage::Done)
}

/// [`encrypt_in_place`], stopping after `stop` (interruption tests).
#[doc(hidden)]
pub fn encrypt_in_place_until(paths: &DataPaths, key: &DbKey, stop: Stage) -> Result<()> {
    let work = WorkFiles::of(paths);
    match file_kind(&paths.db)? {
        FileKind::Plaintext => {
            encrypt_copy(&paths.db, &work.enc, key)?;
            if stop == Stage::Copied {
                return Ok(());
            }
            std::fs::rename(&paths.db, &work.plain)?;
            overwrite_and_delete(&work.wal)?;
            overwrite_and_delete(&work.shm)?;
            if stop == Stage::PlainSetAside {
                return Ok(());
            }
        }
        FileKind::Missing => {
            if !work.enc.exists() {
                return Ok(());
            }
            if work.plain.exists() {
                // Step 5 may have stopped between the rename and the erase.
                overwrite_and_delete(&work.wal)?;
                overwrite_and_delete(&work.shm)?;
            }
        }
        FileKind::Empty | FileKind::Encrypted => {}
    }
    if !paths.db.exists() && work.enc.exists() {
        std::fs::rename(&work.enc, &paths.db)?;
    }
    if stop == Stage::EncryptedInPlace {
        return Ok(());
    }
    // Never erase `.plain` unless the encrypted database is in place.
    if file_kind(&paths.db)? == FileKind::Encrypted {
        overwrite_and_delete(&work.plain)?;
        remove_if_exists(&work.enc)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_paths_are_percent_encoded() {
        assert_eq!(
            encode_uri_path("/tmp/a b#c%d?e/ë.db"),
            "/tmp/a%20b%23c%25d%3Fe/%C3%AB.db"
        );
        assert_eq!(
            encode_uri_path(r"E:\CRM Porosi\data\crm.db.enc"),
            "/E:/CRM%20Porosi/data/crm.db.enc"
        );
        assert_eq!(encode_uri_path(r"\\?\E:\data\crm.db"), "/E:/data/crm.db");
    }

    #[test]
    fn keyed_uri_has_the_cipher_and_the_raw_key() {
        let key = DbKey::for_tests(1);
        let uri = keyed_uri(Path::new("/x y/crm.db"), &key).unwrap();
        assert!(uri.starts_with("file:///"), "{}", uri.as_str());
        assert!(uri.contains("/x%20y/crm.db?cipher=chacha20&key=raw:"));
        assert!(uri.ends_with(&key.pragma_text()[4..]));
    }

    #[test]
    fn file_kind_classifies_by_header() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.db");
        assert_eq!(file_kind(&p).unwrap(), FileKind::Missing);
        std::fs::write(&p, b"").unwrap();
        assert_eq!(file_kind(&p).unwrap(), FileKind::Empty);
        let mut plain = SQLITE_HEADER.to_vec();
        plain.extend_from_slice(&[0; 100]);
        std::fs::write(&p, &plain).unwrap();
        assert_eq!(file_kind(&p).unwrap(), FileKind::Plaintext);
        std::fs::write(&p, b"SQLite format 3").unwrap();
        assert_eq!(file_kind(&p).unwrap(), FileKind::Encrypted);
        std::fs::write(&p, [7u8; 4096]).unwrap();
        assert_eq!(file_kind(&p).unwrap(), FileKind::Encrypted);
    }

    #[test]
    fn overwrite_and_delete_removes_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("plain");
        std::fs::write(&p, vec![b'x'; 200_000]).unwrap();
        overwrite_and_delete(&p).unwrap();
        assert!(!p.exists());
        overwrite_and_delete(&p).unwrap();
    }
}

//! Shared helpers for the storage integration tests.

#![allow(dead_code)]

use crm_storage::DbKey;
use rusqlite::Connection;
use std::path::Path;

/// The key every test database uses unless a test needs another one.
pub fn key() -> DbKey {
    DbKey::for_tests(1)
}

/// A raw rusqlite connection on an encrypted file, keyed with `key`.
pub fn raw_with(path: &Path, key: &DbKey) -> Connection {
    let conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "key", format!("raw:{}", key.to_hex_for_tests()))
        .unwrap();
    conn
}

/// [`raw_with`] the default [`key`].
pub fn raw(path: &Path) -> Connection {
    raw_with(path, &key())
}

//! Schema migrations, tracked in `PRAGMA user_version`.

use crate::error::Result;
use rusqlite::Connection;
use rusqlite_migration::{M, Migrations, SchemaVersion};

/// The schema version this build migrates to.
pub const LATEST_VERSION: u32 = 1;

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![M::up(include_str!("../migrations/001_initial.sql"))])
}

/// Checks that the migration set is well formed (each script applies cleanly
/// to an empty in-memory database).
pub fn validate() -> Result<()> {
    Ok(migrations().validate()?)
}

/// The current `user_version` (0 when none is set).
pub(crate) fn current_version(conn: &Connection) -> Result<u32> {
    Ok(match migrations().current_version(conn)? {
        SchemaVersion::NoneSet => 0,
        SchemaVersion::Inside(v) | SchemaVersion::Outside(v) => {
            u32::try_from(v.get()).unwrap_or(u32::MAX)
        }
    })
}

/// Migrates to [`LATEST_VERSION`]. When the database is behind AND already holds
/// tables, `pre_migrate_backup` runs first (the `pre-migrate-v{N}` copy); a
/// fresh, empty database needs no backup. A no-op when already current.
pub(crate) fn migrate(
    conn: &mut Connection,
    pre_migrate_backup: impl FnOnce(&Connection) -> Result<()>,
) -> Result<()> {
    if current_version(conn)? >= LATEST_VERSION {
        return Ok(());
    }
    let tables: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE type = 'table'",
        [],
        |row| row.get(0),
    )?;
    if tables > 0 {
        pre_migrate_backup(conn)?;
    }
    migrations().to_latest(conn)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_set_is_valid() {
        validate().unwrap();
    }

    #[test]
    fn fresh_db_skips_the_backup_hook_and_reaches_latest() {
        let mut conn = Connection::open_in_memory().unwrap();
        let mut called = false;
        migrate(&mut conn, |_| {
            called = true;
            Ok(())
        })
        .unwrap();
        assert!(!called);
        assert_eq!(current_version(&conn).unwrap(), LATEST_VERSION);
    }

    #[test]
    fn behind_db_with_tables_runs_the_backup_hook_once() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE legacy (x INTEGER)")
            .unwrap();
        let mut calls = 0;
        migrate(&mut conn, |_| {
            calls += 1;
            Ok(())
        })
        .unwrap();
        migrate(&mut conn, |_| {
            calls += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(calls, 1);
        assert_eq!(current_version(&conn).unwrap(), LATEST_VERSION);
    }

    #[test]
    fn failing_backup_hook_aborts_the_migration() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE legacy (x INTEGER)")
            .unwrap();
        let err = migrate(&mut conn, |_| Err(crate::StorageError::Busy)).unwrap_err();
        assert_eq!(err, crate::StorageError::Busy);
        assert_eq!(current_version(&conn).unwrap(), 0);
    }
}

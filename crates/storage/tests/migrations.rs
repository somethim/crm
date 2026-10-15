//! AC 2 and step 3: schema v1 migrations are idempotent and match the DDL.

mod common;

use crm_storage::{DataPaths, Db, DbKey, migrations};
use rusqlite::Connection;

fn schema(conn: &Connection) -> Vec<(String, Option<String>)> {
    let mut stmt = conn
        .prepare("SELECT name, sql FROM sqlite_schema ORDER BY name")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn user_version(conn: &Connection) -> i64 {
    conn.pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}

#[test]
fn migration_set_validates() {
    migrations::validate().unwrap();
    assert_eq!(migrations::LATEST_VERSION, 1);
}

#[test]
fn migrating_twice_is_a_no_op() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    Db::open(&paths, &DbKey::for_tests(1))
        .unwrap()
        .checkpoint_and_close()
        .unwrap();
    let first = {
        let conn = common::raw(&paths.db);
        assert_eq!(user_version(&conn), 1);
        schema(&conn)
    };
    Db::open(&paths, &DbKey::for_tests(1))
        .unwrap()
        .checkpoint_and_close()
        .unwrap();
    let conn = common::raw(&paths.db);
    assert_eq!(user_version(&conn), 1);
    assert_eq!(schema(&conn), first);
    // No pre-migrate backup for a fresh or current database.
    assert_eq!(std::fs::read_dir(&paths.backups).unwrap().count(), 0);
}

#[test]
fn a_migrated_db_equals_the_ddl() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    Db::open(&paths, &DbKey::for_tests(1))
        .unwrap()
        .checkpoint_and_close()
        .unwrap();
    let migrated = schema(&common::raw(&paths.db));

    let fresh = Connection::open_in_memory().unwrap();
    fresh
        .execute_batch(include_str!("../migrations/001_initial.sql"))
        .unwrap();
    assert_eq!(schema(&fresh), migrated);
    let tables: Vec<&str> = migrated.iter().map(|(n, _)| n.as_str()).collect();
    for table in [
        "settings",
        "skus",
        "products",
        "owners",
        "orders",
        "order_lines",
        "open_tabs",
    ] {
        assert!(tables.contains(&table), "{table}");
    }
}

#[test]
fn foreign_keys_and_checks_are_enforced() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    Db::open(&paths, &DbKey::for_tests(1))
        .unwrap()
        .checkpoint_and_close()
        .unwrap();
    let conn = common::raw(&paths.db);
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    let ts = "2026-10-14T00:00:00Z";

    let missing_sku = conn.execute(
        "INSERT INTO products (sku_id, name, name_key, created_at, updated_at) VALUES (99, 'x', 'x', ?1, ?1)",
        [ts],
    );
    assert!(missing_sku.is_err(), "a product needs an existing sku");

    conn.execute(
        "INSERT INTO skus (code, unit, buy_price, sell_price, quantity, created_at, updated_at)
         VALUES ('L0001', 'COPE', 1, 2, 3, ?1, ?1)",
        [ts],
    )
    .unwrap();
    let negative = conn.execute(
        "UPDATE skus SET quantity = quantity - 4 WHERE code = 'L0001'",
        [],
    );
    assert!(negative.is_err(), "CHECK (quantity >= 0)");
    let quantity: i64 = conn
        .query_row("SELECT quantity FROM skus WHERE code = 'L0001'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(quantity, 3);
}

#[test]
fn an_unmigrated_db_with_tables_gets_a_pre_migrate_backup() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    paths.ensure_dirs().unwrap();
    {
        let conn = common::raw(&paths.db);
        conn.execute_batch("CREATE TABLE legacy (x INTEGER); INSERT INTO legacy VALUES (7);")
            .unwrap();
    }
    Db::open(&paths, &DbKey::for_tests(1))
        .unwrap()
        .checkpoint_and_close()
        .unwrap();
    let names: Vec<String> = std::fs::read_dir(&paths.backups)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names.len(), 1, "{names:?}");
    assert!(names[0].ends_with("-pre-migrate-v1.db"), "{names:?}");
    let copy = common::raw(&paths.backups.join(&names[0]));
    assert_eq!(user_version(&copy), 0);
    let x: i64 = copy
        .query_row("SELECT x FROM legacy", [], |r| r.get(0))
        .unwrap();
    assert_eq!(x, 7);
}

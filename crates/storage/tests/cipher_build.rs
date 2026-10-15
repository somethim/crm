//! Plan 014 step 2: the build links SQLite3 Multiple Ciphers 2.6.1 (SQLite
//! 3.54), not the stock SQLite bundled with libsqlite3-sys.

#[test]
fn sqlite_is_3_54() {
    let version = rusqlite::version();
    assert!(version.starts_with("3.54."), "{version}");
}

#[test]
fn sqlite3mc_is_2_6_1() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    let version: String = conn
        .query_row("SELECT sqlite3mc_version()", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, "SQLite3 Multiple Ciphers 2.6.1");
}

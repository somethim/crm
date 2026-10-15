//! AC 3: a second handle on the same file (a second process) waits for the
//! write lock instead of failing; a hold beyond `busy_timeout` gives `Busy`;
//! readers are never blocked by a write.

use crm_storage::repo::settings;
use crm_storage::{DataPaths, Db, DbKey, StorageError};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const HOLD: Duration = Duration::from_secs(1);

/// Starts a write on `db` that holds `BEGIN IMMEDIATE` for [`HOLD`], and returns
/// once the transaction is open.
fn hold_write_lock(db: std::sync::Arc<Db>) -> thread::JoinHandle<()> {
    let (started, wait) = mpsc::channel();
    let handle = thread::spawn(move || {
        db.write(move |tx| {
            settings::set(tx, "holder", "a")?;
            started.send(()).unwrap();
            thread::sleep(HOLD);
            Ok::<_, StorageError>(())
        })
        .unwrap();
    });
    wait.recv().unwrap();
    handle
}

#[test]
fn second_handle_waits_for_the_lock_then_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    let a = std::sync::Arc::new(Db::open(&paths, &DbKey::for_tests(1)).unwrap());
    let b = Db::open(&paths, &DbKey::for_tests(1)).unwrap();

    let started = Instant::now();
    let holder = hold_write_lock(a.clone());
    b.write(|tx| settings::set(tx, "waiter", "b")).unwrap();
    let waited = started.elapsed();
    holder.join().unwrap();

    assert!(
        waited >= Duration::from_millis(900),
        "waited only {waited:?}"
    );
    let (holder_value, waiter_value) = b
        .read(|c| Ok::<_, StorageError>((settings::get(c, "holder")?, settings::get(c, "waiter")?)))
        .unwrap();
    assert_eq!(holder_value.as_deref(), Some("a"));
    assert_eq!(waiter_value.as_deref(), Some("b"));
}

#[test]
fn a_hold_beyond_busy_timeout_gives_busy() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    let a = std::sync::Arc::new(Db::open(&paths, &DbKey::for_tests(1)).unwrap());
    let b = Db::open_with(&paths, &DbKey::for_tests(1), Duration::from_millis(200)).unwrap();

    let holder = hold_write_lock(a.clone());
    let started = Instant::now();
    let result = b.write(|tx| settings::set(tx, "waiter", "b"));
    let waited = started.elapsed();
    holder.join().unwrap();

    assert_eq!(result, Err(StorageError::Busy));
    assert!(
        waited >= Duration::from_millis(150),
        "gave up after {waited:?}"
    );
    assert!(waited < HOLD, "should not wait for the holder: {waited:?}");
    assert_eq!(b.read(|c| settings::get(c, "waiter")).unwrap(), None);
}

#[test]
fn readers_keep_reading_during_a_long_write() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    let a = std::sync::Arc::new(Db::open(&paths, &DbKey::for_tests(1)).unwrap());
    a.write(|tx| settings::set(tx, "k", "before")).unwrap();

    let holder = hold_write_lock(a.clone());
    for _ in 0..5 {
        let started = Instant::now();
        let value = a.read(|c| settings::get(c, "k")).unwrap();
        assert!(
            started.elapsed() < Duration::from_millis(200),
            "read waited {:?}",
            started.elapsed()
        );
        assert_eq!(value.as_deref(), Some("before"));
        // The uncommitted write is invisible.
        assert_eq!(a.read(|c| settings::get(c, "holder")).unwrap(), None);
    }
    holder.join().unwrap();
    assert_eq!(
        a.read(|c| settings::get(c, "holder")).unwrap().as_deref(),
        Some("a")
    );
}

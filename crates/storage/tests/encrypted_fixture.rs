//! Plan 014 step 8: a committed encrypted database (schema v1, key
//! `DbKey::for_tests(7)`), embedded in the test binary so the cross-built
//! Windows `.exe` needs no path. CI runs this file on Linux, from the cargo-xwin
//! (clang-cl) build and from the native MSVC build.
//!
//! Regenerate the fixture only on purpose:
//! `cargo test -p crm-storage --test encrypted_fixture -- --ignored regenerate_fixture`.

use crm_domain::{
    Money, Order, OrderId, OrderKind, OrderLine, Owner, OwnerId, OwnerKind, OwnerSnapshot, Qty,
    Sku, SkuId,
};
use crm_storage::repo::{orders, owners, products, settings, skus};
use crm_storage::{
    BackupReason, DataPaths, Db, DbKey, FileKind, StorageError, backup, database_stats,
    encrypt_copy, file_kind,
};
use std::path::Path;

const FIXTURE: &[u8] = include_bytes!("fixtures/encrypted-v1.db");
const NAMES: [&str; 3] = ["UJË NATYRAL 1.5L", "ÇAJ I FTOHTË LIMON", "BRIZO PJESHKË"];

fn key() -> DbKey {
    DbKey::for_tests(7)
}

/// A temp root whose `data/crm.db` is the fixture.
fn root() -> (tempfile::TempDir, DataPaths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    std::fs::create_dir_all(paths.data_dir()).unwrap();
    std::fs::write(&paths.db, FIXTURE).unwrap();
    (dir, paths)
}

fn count(stats: &crm_storage::CopyStats, table: &str) -> i64 {
    stats
        .tables
        .iter()
        .find(|(t, _)| t == table)
        .map_or(-1, |(_, n)| *n)
}

#[test]
fn the_fixture_is_encrypted_and_opens_with_its_key() {
    assert!(FIXTURE.len() >= 4096, "fixture missing: regenerate it");
    assert!(!FIXTURE.starts_with(b"SQLite format 3"));
    let (_dir, paths) = root();
    let stats = database_stats(&paths.db, Some(&key())).unwrap();
    assert_eq!(stats.user_version, 1);
    for (table, n) in [
        ("skus", 3),
        ("products", 3),
        ("owners", 1),
        ("orders", 1),
        ("order_lines", 2),
    ] {
        assert_eq!(count(&stats, table), n, "{table}");
    }
    let db = Db::open(&paths, &key()).unwrap();
    let mut names: Vec<String> = db
        .read(products::all_for_index)
        .unwrap()
        .into_iter()
        .map(|p| p.name)
        .collect();
    names.sort();
    let mut expected: Vec<String> = NAMES.iter().map(|n| n.to_string()).collect();
    expected.sort();
    assert_eq!(names, expected);
    assert_eq!(
        db.read(|c| settings::get(c, "appearance"))
            .unwrap()
            .as_deref(),
        Some("light")
    );
}

#[test]
fn writes_reach_the_readers_and_a_backup_verifies() {
    let (_dir, paths) = root();
    let db = Db::open(&paths, &key()).unwrap();
    db.write(|tx| settings::set(tx, "fixture-write", "Lekë"))
        .unwrap();
    assert_eq!(
        db.read(|c| settings::get(c, "fixture-write"))
            .unwrap()
            .as_deref(),
        Some("Lekë")
    );
    let copy = db.backup_now(BackupReason::Manual).unwrap();
    assert_eq!(file_kind(&copy).unwrap(), FileKind::Encrypted);
    backup::verify(&copy, &key()).unwrap();
}

#[test]
fn every_connection_reports_chacha20() {
    let (_dir, paths) = root();
    let db = Db::open(&paths, &key()).unwrap();
    let report = db.connection_pragmas().unwrap();
    assert_eq!(report.len(), 5);
    for p in report {
        assert_eq!(p.cipher, "chacha20", "{}", p.role);
    }
}

#[test]
fn the_wrong_key_is_refused() {
    let (_dir, paths) = root();
    assert_eq!(
        Db::open(&paths, &DbKey::for_tests(8)).unwrap_err(),
        StorageError::WrongKey
    );
}

#[test]
fn a_flipped_bit_on_page_two_is_corrupt() {
    let (_dir, paths) = root();
    let mut bytes = std::fs::read(&paths.db).unwrap();
    bytes[4096 + 100] ^= 0x01;
    std::fs::write(&paths.db, bytes).unwrap();
    assert!(matches!(
        Db::open(&paths, &key()).unwrap_err(),
        StorageError::Corrupt { .. }
    ));
}

#[test]
fn encrypt_copy_of_a_fresh_plaintext_database() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    Db::create_legacy_plaintext(&paths, "INSERT INTO settings VALUES ('a', 'ë');").unwrap();
    let target = dir.path().join("encrypted copy ë.db");
    let stats = encrypt_copy(&paths.db, &target, &key()).unwrap();
    assert_eq!(stats.user_version, 1);
    assert_eq!(file_kind(&target).unwrap(), FileKind::Encrypted);
    assert_eq!(database_stats(&target, Some(&key())).unwrap(), stats);
}

fn build(dir: &Path) -> std::path::PathBuf {
    let paths = DataPaths::at(dir);
    let db = Db::open(&paths, &key()).unwrap();
    db.write(|tx| {
        let mut sku_ids = Vec::new();
        for (i, name) in NAMES.iter().enumerate() {
            let sku = Sku {
                id: SkuId(0),
                code: format!("L000{i}"),
                unit: "COPE".into(),
                buy_price: Money(6_200),
                sell_price: Money(6_500),
                quantity: Qty(100),
            };
            let id = skus::insert(tx, &sku)?;
            products::insert(tx, id, name)?;
            sku_ids.push(id);
        }
        let owner_id = owners::insert(
            tx,
            &Owner {
                id: OwnerId(0),
                kind: OwnerKind::Client,
                name: "Klienti Alfa".into(),
                description: Some("Dyqani Qendër".into()),
                nipt: Some("M02317046H".into()),
                address: Some("Rr. Pishave".into()),
                account_code: None,
            },
        )?;
        let line = |position: i64, sku: SkuId, qty: i64| OrderLine {
            position,
            product_id: None,
            sku_id: Some(sku),
            name: NAMES[position as usize].into(),
            code: format!("L000{position}"),
            unit: "COPE".into(),
            quantity: Qty(qty),
            price: Money(6_500),
            value: Money(6_500 * qty),
        };
        let lines = vec![line(0, sku_ids[0], 2), line(1, sku_ids[1], 3)];
        let at: jiff::Timestamp = "2026-10-14T08:00:00Z".parse().unwrap();
        orders::insert(
            tx,
            &Order {
                id: OrderId(0),
                kind: OrderKind::Sale,
                owner_id: Some(owner_id),
                owner: OwnerSnapshot {
                    name: "Klienti Alfa".into(),
                    description: Some("Dyqani Qendër".into()),
                    nipt: Some("M02317046H".into()),
                    address: Some("Rr. Pishave".into()),
                },
                issued_on: jiff::civil::date(2026, 10, 14),
                total: lines.iter().map(|l| l.value).sum(),
                created_at: at,
                updated_at: at,
                lines,
            },
        )?;
        settings::set(tx, "appearance", "light")?;
        Ok::<_, StorageError>(())
    })
    .unwrap();
    db.checkpoint_and_close().unwrap();
    assert!(!paths.data_dir().join("crm.db-wal").exists());
    paths.db
}

#[test]
#[ignore = "rewrites the committed fixture"]
fn regenerate_fixture() {
    let dir = tempfile::tempdir().unwrap();
    let db = build(dir.path());
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    std::fs::create_dir_all(&fixtures).unwrap();
    std::fs::copy(db, fixtures.join("encrypted-v1.db")).unwrap();
}

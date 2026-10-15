//! Plan 015 step 9: a committed `crm.key` v1 (`KdfParams::TEST`) that wraps the
//! storage fixture's data key (`DbKey::for_tests(7)`). Embedded, so the
//! cross-built Windows `.exe` needs no path. CI runs it on Linux, from the
//! cargo-xwin (clang-cl) build and from the native MSVC build.
//!
//! Regenerate it only on purpose:
//! `cargo test -p crm-services --test keyfile_fixture -- --ignored regenerate_fixture`.

use crm_services::{
    Crm, DataKey, DataPaths, DbKey, Entry, KdfParams, KeyFile, RecoveryKey, Unlock, Vault,
    WipeCode, classify,
};
use std::path::Path;
use zeroize::Zeroizing;

const KEY_FIXTURE: &[u8] = include_bytes!("fixtures/crm-v1.key");
const DB_FIXTURE: &[u8] = include_bytes!("../../storage/tests/fixtures/encrypted-v1.db");
const PASSWORD: &str = "fjalëkalim-test";
/// Test-only codes, public on purpose.
const RECOVERY: &str = "1QE6-3YYP-E8FA-5J9Q-F2R1-61SE-T21F-QJKV";
const WIPE: &str = "CNPY-RZ3Y-E4EC";

fn recovery() -> RecoveryKey {
    match classify(RECOVERY) {
        Entry::Recovery(rk) => rk,
        other => panic!("{other:?}"),
    }
}

fn wipe() -> WipeCode {
    match classify(WIPE) {
        Entry::Wipe(code) => code,
        other => panic!("{other:?}"),
    }
}

/// The bytes of `DbKey::for_tests(7)`.
fn storage_fixture_key() -> DataKey {
    let hex = DbKey::for_tests(7).to_hex_for_tests();
    let mut key = Zeroizing::new([0u8; 32]);
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap();
    }
    key
}

#[test]
fn both_secrets_open_the_storage_fixture_key() {
    assert_eq!(KEY_FIXTURE.len(), 260, "fixture missing: regenerate it");
    let file = KeyFile::parse(KEY_FIXTURE).unwrap();
    assert_eq!(file.params(), KdfParams::TEST);
    let expected = storage_fixture_key();
    assert_eq!(*file.open_password(PASSWORD).unwrap(), *expected);
    assert_eq!(*file.open_recovery(&recovery()).unwrap(), *expected);
    assert!(file.open_password("fjalëkalim-tesT").is_none());
    assert!(file.verify_wipe(&wipe()));
    let typo = WIPE.replacen('C', "D", 1);
    // Either the checksum rejects the typo, or it is another code that fails.
    if let Entry::Wipe(other) = classify(&typo) {
        assert!(!file.verify_wipe(&other));
    }
}

#[test]
fn the_vault_unlocks_the_storage_fixture() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    std::fs::create_dir_all(paths.data_dir()).unwrap();
    std::fs::write(&paths.key, KEY_FIXTURE).unwrap();
    std::fs::write(&paths.db, DB_FIXTURE).unwrap();
    let vault = Vault::with_params(paths.clone(), KdfParams::TEST);
    let Unlock::Unlocked(key) = vault.submit(PASSWORD).unwrap() else {
        panic!("not unlocked");
    };
    let crm = Crm::open(paths, &key).unwrap();
    let all = crm
        .list_products("", crm_services::Page::default())
        .unwrap();
    assert_eq!(all.len(), 3);
    assert!(all.iter().any(|p| p.product.name == "ÇAJ I FTOHTË LIMON"));
    crm.close().unwrap();
}

#[test]
#[ignore = "rewrites the committed fixture"]
fn regenerate_fixture() {
    let file = KeyFile::create(
        &storage_fixture_key(),
        PASSWORD,
        &recovery(),
        &wipe(),
        KdfParams::TEST,
    )
    .unwrap();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/crm-v1.key");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, file.to_bytes()).unwrap();
}

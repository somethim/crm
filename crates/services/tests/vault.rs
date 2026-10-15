//! Plan 015: the vault (setup, unlock, recovery, password change, probe, wipe).

use crm_services::{
    Crm, DataPaths, KdfParams, KeyFile, StartState, Unlock, Vault, VaultError, check_new_password,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const PASSWORD: &str = "fjalëkalim";

fn vault(dir: &Path) -> Vault {
    Vault::with_params(DataPaths::at(dir), KdfParams::TEST)
}

struct Setup {
    vault: Vault,
    paths: DataPaths,
    recovery: String,
    wipe: String,
}

fn set_up(dir: &Path) -> Setup {
    let vault = vault(dir);
    let paths = DataPaths::at(dir);
    let pending = vault.setup(PASSWORD).unwrap();
    let recovery = pending.recovery_display().to_string();
    let wipe = pending.wipe_display().to_string();
    let key = pending.commit().unwrap();
    // A database with data and a backup with its sibling key.
    let crm = Crm::open(paths.clone(), &key).unwrap();
    crm.backup_now().unwrap();
    crm.close().unwrap();
    Setup {
        vault,
        paths,
        recovery,
        wipe,
    }
}

fn unlocked(outcome: Unlock) -> crm_services::DbKey {
    match outcome {
        Unlock::Unlocked(key) => key,
        other => panic!("expected Unlocked, got {other:?}"),
    }
}

/// Every file under `root` with its size and mtime.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, (u64, SystemTime)> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            let meta = entry.metadata().unwrap();
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                out.insert(entry.path(), (meta.len(), meta.modified().unwrap()));
            }
        }
    }
    out
}

fn one_char_changed(code: &str) -> String {
    let mut chars: Vec<char> = code.chars().collect();
    let i = chars
        .iter()
        .position(|c| c.is_ascii_alphanumeric())
        .unwrap();
    chars[i] = if chars[i] == '7' { '8' } else { '7' };
    chars.into_iter().collect()
}

#[test]
fn setup_writes_nothing_until_commit() {
    let dir = tempfile::tempdir().unwrap();
    let v = vault(dir.path());
    let paths = DataPaths::at(dir.path());
    let pending = v.setup(PASSWORD).unwrap();
    let rk = pending.recovery_display();
    let wipe = pending.wipe_display();
    assert_eq!(rk.len(), 39);
    assert!(rk.split('-').all(|g| g.len() == 4));
    assert_eq!(wipe.len(), 14);
    assert!(!paths.key.exists());
    assert!(
        !paths.data_dir().exists() || std::fs::read_dir(paths.data_dir()).unwrap().count() == 0
    );
    drop(pending);
    assert!(!paths.key.exists());
    let key = v.setup(PASSWORD).unwrap().commit().unwrap();
    assert_eq!(std::fs::read(&paths.key).unwrap().len(), 260);
    assert_eq!(v.probe().unwrap(), StartState::Unlock);
    assert_eq!(
        unlocked(v.submit(PASSWORD).unwrap()).to_hex_for_tests(),
        key.to_hex_for_tests()
    );
}

#[test]
fn passwords_are_checked() {
    let dir = tempfile::tempdir().unwrap();
    let v = vault(dir.path());
    assert_eq!(v.setup("12345").unwrap_err(), VaultError::TooShort);
    let s = set_up(&tempfile::tempdir().unwrap().keep());
    assert_eq!(
        v.setup(&s.wipe).unwrap_err(),
        VaultError::PasswordNotAllowed
    );
    assert_eq!(
        check_new_password(&s.recovery),
        Err(VaultError::PasswordNotAllowed)
    );
    assert_eq!(
        VaultError::TooShort.user_message(),
        "Fjalëkalimi duhet të ketë të paktën 6 karaktere."
    );
    assert_eq!(
        VaultError::PasswordNotAllowed.user_message(),
        "Ky fjalëkalim nuk lejohet. Zgjidhni një tjetër."
    );
    std::fs::remove_dir_all(s.paths.root).unwrap();
}

#[test]
fn wrong_passwords_lock_out_after_five() {
    let dir = tempfile::tempdir().unwrap();
    let s = set_up(dir.path());
    for _ in 0..4 {
        assert!(matches!(
            s.vault.submit("gabim-gabim").unwrap(),
            Unlock::Wrong
        ));
    }
    assert_eq!(
        s.vault.submit("gabim-gabim").unwrap_err(),
        VaultError::LockedFor(30)
    );
    // Even the right password is refused during the lockout.
    assert!(matches!(
        s.vault.submit(PASSWORD).unwrap_err(),
        VaultError::LockedFor(_)
    ));
    assert_eq!(
        VaultError::LockedFor(30).user_message(),
        "Provoni përsëri pas 30 sekondash"
    );
}

#[test]
fn the_recovery_key_in_any_form_then_a_new_password() {
    let dir = tempfile::tempdir().unwrap();
    let s = set_up(dir.path());
    let key = unlocked(s.vault.submit(PASSWORD).unwrap());
    let sloppy = format!(" {} ", s.recovery.to_lowercase().replace('-', " "));
    let token = match s.vault.submit(&sloppy).unwrap() {
        Unlock::Recovered(token) => token,
        other => panic!("{other:?}"),
    };
    assert_eq!(token.db_key().to_hex_for_tests(), key.to_hex_for_tests());
    assert_eq!(
        s.vault
            .set_password_after_recovery(token, "123")
            .map(|_| ())
            .unwrap_err(),
        VaultError::TooShort
    );
    let token = match s.vault.submit(&s.recovery.replace('-', "")).unwrap() {
        Unlock::Recovered(token) => token,
        other => panic!("{other:?}"),
    };
    let again = s
        .vault
        .set_password_after_recovery(token, "i-ri-fjalëkalim")
        .unwrap();
    assert_eq!(again.to_hex_for_tests(), key.to_hex_for_tests());
    assert!(matches!(s.vault.submit(PASSWORD).unwrap(), Unlock::Wrong));
    let key2 = unlocked(s.vault.submit("i-ri-fjalëkalim").unwrap());
    let crm = Crm::open(s.paths.clone(), &key2).unwrap();
    crm.close().unwrap();
}

#[test]
fn a_password_change_rewraps_every_key_file_on_the_stick() {
    let dir = tempfile::tempdir().unwrap();
    let s = set_up(dir.path());
    let foreign = s.paths.backups.join("crm-20200101-000000-manual.key");
    let other = tempfile::tempdir().unwrap();
    let other_setup = set_up(other.path());
    std::fs::copy(&other_setup.paths.key, &foreign).unwrap();
    let foreign_before = std::fs::read(&foreign).unwrap();

    assert_eq!(
        s.vault.change_password("gabim-gabim", "i-ri-fjalëkalim"),
        Err(VaultError::Wrong)
    );
    assert_eq!(
        s.vault.change_password(PASSWORD, "12"),
        Err(VaultError::TooShort)
    );
    s.vault
        .change_password(PASSWORD, "i-ri-fjalëkalim")
        .unwrap();
    let mut siblings = 0;
    for entry in std::fs::read_dir(&s.paths.backups).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "key") && path != foreign {
            let file = KeyFile::parse(&std::fs::read(&path).unwrap()).unwrap();
            assert!(file.open_password(PASSWORD).is_none(), "{}", path.display());
            assert!(file.open_password("i-ri-fjalëkalim").is_some());
            siblings += 1;
        }
    }
    assert!(siblings >= 2, "{siblings}");
    assert_eq!(std::fs::read(&foreign).unwrap(), foreign_before);
    let file = KeyFile::parse(&std::fs::read(&s.paths.key).unwrap()).unwrap();
    assert!(file.open_password(PASSWORD).is_none());
    let key = unlocked(s.vault.submit("i-ri-fjalëkalim").unwrap());
    Crm::open(s.paths.clone(), &key).unwrap().close().unwrap();
}

#[test]
fn typos_of_the_codes_never_wipe() {
    let dir = tempfile::tempdir().unwrap();
    let s = set_up(dir.path());
    let before = snapshot(dir.path());
    let swapped = {
        let mut c: Vec<char> = s.wipe.chars().collect();
        let i = (0..c.len() - 1)
            .find(|&i| c[i] != c[i + 1] && c[i] != '-' && c[i + 1] != '-')
            .unwrap();
        c.swap(i, i + 1);
        c.into_iter().collect::<String>()
    };
    for entry in [
        one_char_changed(&s.wipe),
        swapped,
        one_char_changed(&s.recovery),
    ] {
        assert!(
            matches!(s.vault.submit(&entry).unwrap(), Unlock::Wrong),
            "{entry}"
        );
    }
    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn the_wipe_code_wipes_everything_on_the_stick() {
    let dir = tempfile::tempdir().unwrap();
    let pc = tempfile::tempdir().unwrap();
    let s = set_up(dir.path());
    let key = unlocked(s.vault.submit(PASSWORD).unwrap());
    let crm = Crm::open(s.paths.clone(), &key).unwrap();
    let pc_copy = crm.backup_to_pc(pc.path()).unwrap();
    crm.backup_now().unwrap();
    crm.close().unwrap();
    std::fs::write(s.paths.logs.join("crm.log.2026-10-14"), b"log line").unwrap();
    std::fs::create_dir_all(s.paths.backups.join("sub")).unwrap();
    std::fs::write(s.paths.lock.clone(), b"").unwrap();
    std::fs::write(s.paths.data_dir().join("crm.db.plain"), b"x").unwrap();

    let mut hook_ran = false;
    let outcome = s.vault.submit_with(&s.wipe, || hook_ran = true).unwrap();
    assert!(matches!(outcome, Unlock::Wiped));
    assert!(hook_ran);
    let data: Vec<_> = std::fs::read_dir(s.paths.data_dir())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(data, ["crm.lock"]);
    assert_eq!(std::fs::read_dir(&s.paths.backups).unwrap().count(), 0);
    assert_eq!(std::fs::read_dir(&s.paths.logs).unwrap().count(), 0);
    assert!(pc_copy.exists(), "the PC copy is out of reach (documented)");
    assert_eq!(s.vault.probe().unwrap(), StartState::FirstRun);
}

#[test]
fn probe_follows_the_start_state_table() {
    let dir = tempfile::tempdir().unwrap();
    let v = vault(dir.path());
    let paths = DataPaths::at(dir.path());
    assert_eq!(v.probe().unwrap(), StartState::FirstRun);

    // Plaintext data from an older version.
    crm_storage::Db::create_legacy_plaintext(&paths, "").unwrap();
    assert_eq!(v.probe().unwrap(), StartState::Migrate);
    std::fs::remove_dir_all(dir.path()).unwrap();

    let s = set_up(dir.path());
    assert_eq!(s.vault.probe().unwrap(), StartState::Unlock);
    let key_bytes = std::fs::read(&s.paths.key).unwrap();

    std::fs::remove_file(&s.paths.key).unwrap();
    let state = s.vault.probe().unwrap();
    let StartState::UnlockWithBackupKey(path) = state else {
        panic!("{state:?}");
    };
    assert!(path.starts_with(&s.paths.backups));
    unlocked(s.vault.submit(PASSWORD).unwrap());
    assert_eq!(std::fs::read(&s.paths.key).unwrap(), key_bytes);
    assert_eq!(s.vault.probe().unwrap(), StartState::Unlock);

    // An unreadable key file falls back the same way.
    std::fs::write(&s.paths.key, b"garbage").unwrap();
    assert!(matches!(
        s.vault.probe().unwrap(),
        StartState::UnlockWithBackupKey(_)
    ));

    std::fs::remove_file(&s.paths.key).unwrap();
    for entry in std::fs::read_dir(&s.paths.backups).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "key") {
            std::fs::remove_file(path).unwrap();
        }
    }
    assert_eq!(s.vault.probe().unwrap(), StartState::KeyMissing);
    assert!(matches!(
        s.vault.submit(PASSWORD).unwrap_err(),
        VaultError::KeyMissing
    ));
}

#[test]
fn a_tampered_key_file_opens_with_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let s = set_up(dir.path());
    let mut bytes = std::fs::read(&s.paths.key).unwrap();
    bytes[25] ^= 0x01; // db_id, part of both AADs
    std::fs::write(&s.paths.key, &bytes).unwrap();
    assert!(matches!(s.vault.submit(PASSWORD).unwrap(), Unlock::Wrong));
    assert!(matches!(
        s.vault.submit(&s.recovery).unwrap(),
        Unlock::Wrong
    ));
    bytes[25] ^= 0x01;
    bytes[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    std::fs::write(&s.paths.key, &bytes).unwrap();
    assert!(matches!(
        s.vault.submit(PASSWORD).unwrap_err(),
        VaultError::KeyFileInvalid
    ));
}

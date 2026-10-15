//! The key vault: `data/crm.key`, the password, the recovery key, the wipe code,
//! the start state, the lockout and the wipe (plans 013 and 015).
//!
//! The vault never logs a secret, and holds no data key after handing it out:
//! the [`Crm`](crate::Crm), through its database, holds the only long-lived copy.

mod codes;
mod guard;
mod kdf;
mod keyfile;
mod wipe;

pub use codes::{
    Entry, PASSWORD_MAX_CHARS, PASSWORD_MIN_CHARS, RecoveryKey, WipeCode, check_new_password,
    classify,
};
pub(crate) use guard::AttemptGuard;
pub use guard::{LOCKOUT, MAX_ATTEMPTS};
pub use kdf::KdfParams;
#[doc(hidden)]
pub use keyfile::{DataKey, KEY_FILE_LEN, KeyFile};

use crate::error::VaultError;
use crm_storage::{BackupName, DataPaths, DbKey, FileKind, file_kind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;
use zeroize::Zeroizing;

/// What the start-up gate shows (013, "Start states").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartState {
    /// No key file, and crm.db is missing or empty: set a password.
    FirstRun,
    /// No key file, and crm.db is plaintext: set a password, then migrate.
    Migrate,
    /// `data/crm.key` is present.
    Unlock,
    /// `data/crm.key` is missing or unreadable, crm.db is encrypted, and this
    /// backup key file (the newest that parses) is used instead.
    UnlockWithBackupKey(PathBuf),
    /// As above, with no backup key file: the data cannot be opened.
    KeyMissing,
}

/// The outcome of [`Vault::submit`].
#[derive(Debug)]
pub enum Unlock {
    /// The password opened the key file.
    Unlocked(DbKey),
    /// The recovery key opened it: a new password is mandatory
    /// ([`Vault::set_password_after_recovery`]).
    Recovered(RecoveryToken),
    /// The wipe code matched and everything is gone.
    Wiped,
    /// Nothing matched (also a failed erase after a wipe-code match).
    Wrong,
}

/// Proof of a recovery-key unlock: the data key and its key file. Opaque;
/// zeroized on drop.
pub struct RecoveryToken {
    data_key: DataKey,
    file: Box<KeyFile>,
}

impl RecoveryToken {
    /// The data key, for opening the database before the new password is set.
    pub fn db_key(&self) -> DbKey {
        DbKey::from_bytes(*self.data_key)
    }
}

impl std::fmt::Debug for RecoveryToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RecoveryToken(<redacted>)")
    }
}

/// A first-run setup whose key file is not written yet (013 A-08): show the
/// codes, then [`PendingSetup::commit`]. Dropping it leaves no trace.
pub struct PendingSetup {
    key_path: PathBuf,
    file: KeyFile,
    data_key: DataKey,
    recovery: RecoveryKey,
    wipe: WipeCode,
}

impl std::fmt::Debug for PendingSetup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PendingSetup(<redacted>)")
    }
}

impl PendingSetup {
    /// The recovery key, `XXXX-XXXX-…` (8 groups of 4).
    pub fn recovery_display(&self) -> Zeroizing<String> {
        self.recovery.display_groups()
    }

    /// The wipe code, `XXXX-XXXX-XXXX`.
    pub fn wipe_display(&self) -> Zeroizing<String> {
        self.wipe.display_groups()
    }

    /// The data key this setup will commit (tests only).
    #[doc(hidden)]
    pub fn db_key_for_tests(&self) -> DbKey {
        DbKey::from_bytes(*self.data_key)
    }

    /// Writes `data/crm.key` and returns the data key.
    pub fn commit(self) -> Result<DbKey, VaultError> {
        keyfile::write_atomic(&self.key_path, &self.file.to_bytes())?;
        Ok(DbKey::from_bytes(*self.data_key))
    }
}

/// The vault. Cheap to clone; one instance serves the gate and the in-app lock,
/// so the lockout spans both.
#[derive(Clone)]
pub struct Vault(Arc<Inner>);

struct Inner {
    paths: DataPaths,
    params: KdfParams,
    guard: Mutex<AttemptGuard>,
    /// The key file `submit` reads, resolved by `probe`.
    source: Mutex<Option<PathBuf>>,
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("key", &self.0.paths.key)
            .finish_non_exhaustive()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn io(e: impl std::fmt::Display) -> VaultError {
    VaultError::Io(e.to_string())
}

impl Vault {
    /// The vault for `paths`, creating key files with [`KdfParams::PRODUCTION`].
    pub fn new(paths: DataPaths) -> Self {
        Self::with_params(paths, KdfParams::PRODUCTION)
    }

    /// [`Vault::new`] with other KDF parameters (tests use [`KdfParams::TEST`]).
    #[doc(hidden)]
    pub fn with_params(paths: DataPaths, params: KdfParams) -> Self {
        Vault(Arc::new(Inner {
            paths,
            params,
            guard: Mutex::new(AttemptGuard::default()),
            source: Mutex::new(None),
        }))
    }

    /// The data layout.
    pub fn paths(&self) -> &DataPaths {
        &self.0.paths
    }

    /// Decides the start state from `data/crm.key`, crm.db's header and the
    /// backup key files, and remembers which key file [`Vault::submit`] reads.
    pub fn probe(&self) -> Result<StartState, VaultError> {
        let paths = &self.0.paths;
        let (state, source) = if KeyFile::read(&paths.key).is_ok() {
            (StartState::Unlock, Some(paths.key.clone()))
        } else {
            let key_present = paths.key.exists();
            let kind = file_kind(&paths.db).map_err(io)?;
            match (key_present, kind) {
                (false, FileKind::Missing | FileKind::Empty) => (StartState::FirstRun, None),
                (false, FileKind::Plaintext) => (StartState::Migrate, None),
                _ => match newest_backup_key(&paths.backups) {
                    Some(path) => (StartState::UnlockWithBackupKey(path.clone()), Some(path)),
                    None => (StartState::KeyMissing, None),
                },
            }
        };
        *lock(&self.0.source) = source;
        Ok(state)
    }

    fn key_source(&self) -> Result<PathBuf, VaultError> {
        if let Some(path) = lock(&self.0.source).clone() {
            return Ok(path);
        }
        match self.probe()? {
            StartState::Unlock => Ok(self.0.paths.key.clone()),
            StartState::UnlockWithBackupKey(path) => Ok(path),
            _ => Err(VaultError::KeyMissing),
        }
    }

    /// Starts a first-run setup: checks the password, draws the data key and
    /// both codes, and builds the key file in memory (three Argon2 runs).
    /// Nothing is written.
    pub fn setup(&self, password: &str) -> Result<PendingSetup, VaultError> {
        check_new_password(password)?;
        let mut data_key: DataKey = Zeroizing::new([0u8; 32]);
        getrandom::fill(data_key.as_mut()).map_err(|_| VaultError::Crypto)?;
        let recovery = RecoveryKey::generate()?;
        let wipe = WipeCode::generate()?;
        let file = KeyFile::create(&data_key, password, &recovery, &wipe, self.0.params)?;
        Ok(PendingSetup {
            key_path: self.0.paths.key.clone(),
            file,
            data_key,
            recovery,
            wipe,
        })
    }

    /// [`Vault::submit_with`] with no hook.
    pub fn submit(&self, entry: &str) -> Result<Unlock, VaultError> {
        self.submit_with(entry, || {})
    }

    /// Checks one typed entry: a password, the recovery key or the wipe code.
    /// `before_wipe` runs after a wipe-code match and before any file is
    /// touched (the app closes every database handle there). Every miss counts
    /// toward the lockout; the fifth gives `Err(LockedFor(30))`.
    pub fn submit_with(
        &self,
        entry: &str,
        before_wipe: impl FnOnce(),
    ) -> Result<Unlock, VaultError> {
        let mut guard = lock(&self.0.guard);
        guard.check(Instant::now()).map_err(VaultError::LockedFor)?;
        let source = self.key_source()?;
        let file = KeyFile::read(&source)?;
        let outcome = match classify(entry) {
            Entry::Wipe(code) => {
                if file.verify_wipe(&code) {
                    before_wipe();
                    match wipe::wipe(&self.0.paths) {
                        Ok(_) => {
                            guard.succeed();
                            *lock(&self.0.source) = None;
                            return Ok(Unlock::Wiped);
                        }
                        Err(_) => None,
                    }
                } else {
                    None
                }
            }
            Entry::Recovery(rk) => file.open_recovery(&rk).map(|data_key| {
                Unlock::Recovered(RecoveryToken {
                    data_key,
                    file: Box::new(file.clone()),
                })
            }),
            Entry::Password(pw) => file
                .open_password(&pw)
                .map(|data_key| Unlock::Unlocked(DbKey::from_bytes(*data_key))),
        };
        match outcome {
            Some(unlock) => {
                guard.succeed();
                if source != self.0.paths.key {
                    // Unlocked through a backup key file: it becomes data/crm.key.
                    keyfile::write_atomic(&self.0.paths.key, &file.to_bytes())?;
                    *lock(&self.0.source) = Some(self.0.paths.key.clone());
                }
                Ok(unlock)
            }
            None => match guard.fail(Instant::now()) {
                Some(secs) => Err(VaultError::LockedFor(secs)),
                None => Ok(Unlock::Wrong),
            },
        }
    }

    /// After a recovery-key unlock: sets the new password (mandatory), writes
    /// `data/crm.key` and refreshes the matching backup key files.
    pub fn set_password_after_recovery(
        &self,
        token: RecoveryToken,
        new_password: &str,
    ) -> Result<DbKey, VaultError> {
        check_new_password(new_password)?;
        let file = token.file.rewrap_password(&token.data_key, new_password)?;
        self.store(&file)?;
        Ok(DbKey::from_bytes(*token.data_key))
    }

    /// Changes the password: the current one is required (a miss counts toward
    /// the lockout). Rewrites `data/crm.key` and every `backups/*.key` of the
    /// same install, so the old password opens none of them.
    pub fn change_password(&self, old: &str, new: &str) -> Result<(), VaultError> {
        let mut guard = lock(&self.0.guard);
        guard.check(Instant::now()).map_err(VaultError::LockedFor)?;
        check_new_password(new)?;
        let file = KeyFile::read(&self.0.paths.key)?;
        let Some(data_key) = file.open_password(old) else {
            return Err(match guard.fail(Instant::now()) {
                Some(secs) => VaultError::LockedFor(secs),
                None => VaultError::Wrong,
            });
        };
        guard.succeed();
        drop(guard);
        let file = file.rewrap_password(&data_key, new)?;
        self.store(&file)
    }

    /// Writes `data/crm.key`, then every parseable `backups/*.key` with the same
    /// `db_id` (others are left alone).
    fn store(&self, file: &KeyFile) -> Result<(), VaultError> {
        let bytes = file.to_bytes();
        keyfile::write_atomic(&self.0.paths.key, &bytes)?;
        *lock(&self.0.source) = Some(self.0.paths.key.clone());
        for path in backup_key_files(&self.0.paths.backups) {
            if KeyFile::read(&path).is_ok_and(|other| other.db_id() == file.db_id()) {
                keyfile::write_atomic(&path, &bytes)?;
            }
        }
        Ok(())
    }
}

/// Every `*.key` in `dir`.
fn backup_key_files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "key") && p.is_file())
        .collect()
}

/// The newest `backups/X.key` (by the backup name of `X.db`) that parses.
fn newest_backup_key(dir: &Path) -> Option<PathBuf> {
    let mut found: Vec<(BackupName, PathBuf)> = backup_key_files(dir)
        .into_iter()
        .filter_map(|path| {
            let stem = path.file_stem()?.to_str()?;
            let name = BackupName::parse(&format!("{stem}.db"))?;
            Some((name, path))
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found
        .into_iter()
        .map(|(_, path)| path)
        .find(|path| KeyFile::read(path).is_ok())
}

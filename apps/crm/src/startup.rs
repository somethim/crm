//! The startup sequence: paths, network guard, folders, logs, panic hook,
//! instance lock, then the loop: password gate, securing plaintext data,
//! database, UI, clean close (or, after a wipe, back to the gate).

use crate::{instance, logging};
use crm_services::{Crm, DataPaths, DbKey, ServiceError, StorageError, Vault};
use crm_storage::{BackupName, InstanceLock, check_local_drive, restore_from};
use crm_ui::shell::texts;
use crm_ui::{GateOutcome, RunOutcome, UiConfig};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Runs the application; the exit code of the process.
pub fn run() -> ExitCode {
    let paths = match DataPaths::from_exe() {
        Ok(paths) => paths,
        Err(e) => return fatal(&e.user_message()),
    };
    if let Err(e) = check_local_drive(&paths.root) {
        return fatal(&e.user_message());
    }
    if let Err(e) = paths.ensure_dirs() {
        return fatal(&e.user_message());
    }
    let _log_guard = match logging::init(&paths.logs) {
        Ok(guard) => Some(guard),
        Err(e) => {
            eprintln!("logging unavailable: {e}");
            None
        }
    };
    logging::install_panic_hook();
    tracing::info!(
        "crm {} starting in {}",
        env!("CARGO_PKG_VERSION"),
        paths.root.display()
    );

    let _lock = match InstanceLock::acquire(&paths.data_dir()) {
        Ok(lock) => lock,
        Err(StorageError::AlreadyRunning) => {
            if instance::signal_existing(&paths.root) {
                tracing::info!("already running: activated the other instance");
            } else {
                tracing::info!("already running");
                show_message(&StorageError::AlreadyRunning.user_message());
            }
            return ExitCode::SUCCESS;
        }
        Err(e) => return fatal_logged(&e),
    };
    let mut activation = instance::listener(&paths.root);
    let vault = Vault::new(paths.clone());

    loop {
        let state = match vault.probe() {
            Ok(state) => state,
            Err(e) => {
                tracing::error!("{e}");
                return fatal(&e.user_message());
            }
        };
        let key = match crm_ui::run_gate(&vault, state) {
            Ok(GateOutcome::Open(key)) => key,
            Ok(GateOutcome::Quit) => return ExitCode::SUCCESS,
            Err(e) => {
                tracing::error!("{e}");
                native_message(texts::UNEXPECTED);
                return ExitCode::FAILURE;
            }
        };
        // The migration of plaintext data (and plaintext backups), resumable.
        if let Err(e) = crm_services::secure_storage(&paths, &key) {
            tracing::error!("securing the data failed: {e}");
            return fatal(&e.user_message());
        }
        let Some(crm) = open_crm(&paths, &key) else {
            return ExitCode::FAILURE;
        };
        drop(key);

        let config = UiConfig {
            crm: crm.clone(),
            vault: vault.clone(),
            paths: paths.clone(),
            // The listener is a one-shot: only the first window gets it (013 A-11).
            activation: activation.take(),
        };
        match crm_ui::run(config) {
            Ok(RunOutcome::Exit) => {
                match crm.close() {
                    Ok(()) | Err(ServiceError::Closed) => tracing::info!("closed"),
                    Err(e) => tracing::error!("close failed: {e}"),
                }
                return ExitCode::SUCCESS;
            }
            // The database was abandoned before the wipe: nothing to close.
            Ok(RunOutcome::Wiped) => drop(crm),
            Err(e) => {
                tracing::error!("{e}");
                native_message(texts::UNEXPECTED);
                match crm.close() {
                    Ok(()) | Err(ServiceError::Closed) => tracing::info!("closed"),
                    Err(e) => tracing::error!("close failed: {e}"),
                }
                return ExitCode::FAILURE;
            }
        }
    }
}

/// `Crm::open`; a corrupt database, or one the key does not open (the key file
/// unwrapped fine, so the database file was swapped or damaged), offers the
/// newest backup copy that the key opens.
fn open_crm(paths: &DataPaths, key: &DbKey) -> Option<Crm> {
    let error = match Crm::open(paths.clone(), key) {
        Ok(crm) => return Some(crm),
        Err(e) => e,
    };
    tracing::error!("open failed: {error}");
    if !matches!(
        error,
        ServiceError::Storage(StorageError::Corrupt { .. } | StorageError::WrongKey)
    ) {
        show_message(&error.user_message());
        return None;
    }
    match crm_ui::ask_restore(&error.user_message()) {
        Ok(true) => {}
        Ok(false) => return None,
        Err(e) => {
            tracing::error!("{e}");
            native_message(&error.user_message());
            return None;
        }
    }
    if let Err(message) = restore_latest(paths, key) {
        show_message(&message);
        return None;
    }
    match Crm::open(paths.clone(), key) {
        Ok(crm) => Some(crm),
        Err(e) => {
            tracing::error!("open after restore failed: {e}");
            show_message(&e.user_message());
            None
        }
    }
}

/// The backup copies in `backups/`, newest first.
fn backups_newest_first(dir: &Path) -> Vec<(BackupName, PathBuf)> {
    let mut found: Vec<(BackupName, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = BackupName::parse(entry.file_name().to_str()?)?;
            Some((name, entry.path()))
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found
}

/// Restores the newest copy that passes its check with `key` (copies of other
/// data give `WrongKey` and are skipped; no `Crm` exists here, so this calls
/// crm-storage directly). Returns the message to show on failure.
fn restore_latest(paths: &DataPaths, key: &DbKey) -> Result<(), String> {
    let candidates = backups_newest_first(&paths.backups);
    if candidates.is_empty() {
        return Err(texts::NO_BACKUP_TO_RESTORE.to_owned());
    }
    let mut last_error = None;
    for (_, path) in candidates {
        match restore_from(&path, paths, key) {
            Ok(safety) => {
                tracing::info!(
                    "restored {} (previous data: {:?})",
                    path.display(),
                    safety.map(|p| p.display().to_string())
                );
                return Ok(());
            }
            Err(e) => {
                tracing::warn!("restore from {} failed: {e}", path.display());
                last_error = Some(e);
            }
        }
    }
    let reason = last_error.map(|e| e.user_message()).unwrap_or_default();
    Err(texts::startup_restore_failed(&reason))
}

fn fatal_logged(error: &StorageError) -> ExitCode {
    tracing::error!("{error}");
    fatal(&error.user_message())
}

fn fatal(message: &str) -> ExitCode {
    show_message(message);
    ExitCode::FAILURE
}

/// A message window (Slint), or the native message box when Slint fails.
fn show_message(message: &str) {
    if let Err(e) = crm_ui::show_fatal(message) {
        tracing::error!("{e}");
        native_message(message);
    }
}

/// `MessageBoxW` on Windows; stderr elsewhere.
pub fn native_message(message: &str) {
    #[cfg(windows)]
    native::message_box(message);
    #[cfg(not(windows))]
    eprintln!("{message}");
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod native {
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    use windows::core::HSTRING;

    pub(super) fn message_box(message: &str) {
        let text = HSTRING::from(message);
        let caption = HSTRING::from("CRM");
        // SAFETY: both strings are valid NUL-terminated wide strings that outlive
        // the call; no owner window.
        unsafe {
            MessageBoxW(None, &text, &caption, MB_OK | MB_ICONERROR);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder under the system temp dir (this crate has no tempfile).
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            let dir =
                std::env::temp_dir().join(format!("crm-startup-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn backups_are_listed_newest_first_and_others_ignored() {
        let dir = TempDir::new("list");
        for name in [
            "crm-20261001-080000.db",
            "crm-20261012-090000-manual.db",
            "crm-20261005-100000-exit.db",
            "notes.txt",
        ] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        let names: Vec<String> = backups_newest_first(dir.path())
            .into_iter()
            .map(|(_, p)| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            [
                "crm-20261012-090000-manual.db",
                "crm-20261005-100000-exit.db",
                "crm-20261001-080000.db",
            ]
        );
    }

    #[test]
    fn restore_latest_without_backups_says_so() {
        let dir = TempDir::new("none");
        let paths = DataPaths::at(dir.path());
        assert_eq!(
            restore_latest(&paths, &DbKey::for_tests(1)),
            Err(texts::NO_BACKUP_TO_RESTORE.to_owned())
        );
    }

    #[test]
    fn a_damaged_database_is_replaced_by_the_newest_copy_the_key_opens() {
        let dir = TempDir::new("restore");
        let paths = DataPaths::at(dir.path());
        paths.ensure_dirs().unwrap();
        let vault = Vault::with_params(paths.clone(), crm_services::KdfParams::TEST);
        let key = vault.setup("fjalëkalim").unwrap().commit().unwrap();
        let crm = Crm::open(paths.clone(), &key).unwrap();
        crm.set_idle_lock_minutes(7).unwrap();
        crm.backup_now().unwrap();
        crm.close().unwrap();
        // A newer copy that is not a database is skipped, and so is a copy of
        // other data (another key).
        std::fs::write(paths.backups.join("crm-29991231-235959-manual.db"), b"junk").unwrap();
        let other = TempDir::new("restore-other");
        let other_paths = DataPaths::at(other.path());
        let other_crm = Crm::open(other_paths.clone(), &DbKey::for_tests(9)).unwrap();
        let foreign = other_crm.backup_now().unwrap();
        other_crm.close().unwrap();
        std::fs::copy(
            &foreign,
            paths.backups.join("crm-29991230-235959-manual.db"),
        )
        .unwrap();
        std::fs::write(&paths.db, b"not a database at all").unwrap();
        assert!(matches!(
            Crm::open(paths.clone(), &key),
            Err(ServiceError::Storage(
                StorageError::Corrupt { .. } | StorageError::WrongKey
            ))
        ));

        restore_latest(&paths, &key).unwrap();
        let crm = Crm::open(paths.clone(), &key).unwrap();
        assert_eq!(crm.idle_lock_minutes().unwrap(), 7);
        crm.close().unwrap();
    }
}

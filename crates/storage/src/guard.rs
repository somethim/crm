//! Startup guards: refuse network drives, and allow one running instance.

use crate::error::{Result, StorageError};
use fs4::FileExt;
use std::fs::{File, OpenOptions};
use std::path::Path;

/// True for a `\\server\share\…` or `\\?\UNC\server\…` path. Pure, so it is
/// tested on every platform. `\\?\C:\…` (what `std::fs::canonicalize` returns on
/// Windows) and `\\.\…` device paths are local.
pub fn is_network_path(path: &Path) -> bool {
    let text = path.to_string_lossy();
    let upper = text.to_ascii_uppercase();
    if upper.starts_with(r"\\?\UNC\") {
        return true;
    }
    if upper.starts_with(r"\\?\") || upper.starts_with(r"\\.\") {
        return false;
    }
    text.starts_with(r"\\")
}

/// Refuses a database on a network share or a mapped network drive
/// (`GetDriveTypeW == DRIVE_REMOTE`). Always Ok outside Windows.
pub fn check_local_drive(path: &Path) -> Result<()> {
    if is_network_path(path) {
        return Err(StorageError::NetworkDrive);
    }
    #[cfg(windows)]
    {
        if win::is_remote_drive(path) {
            return Err(StorageError::NetworkDrive);
        }
    }
    Ok(())
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod win {
    use std::path::{Component, Path, Prefix};
    use windows::Win32::Storage::FileSystem::GetDriveTypeW;
    use windows::core::HSTRING;

    /// `DRIVE_REMOTE` from `Win32::System::WindowsProgramming` (not an enabled
    /// feature of the `windows` crate here); the value is fixed by the Win32 ABI.
    const DRIVE_REMOTE: u32 = 4;

    pub(super) fn is_remote_drive(path: &Path) -> bool {
        let Some(Component::Prefix(prefix)) = path.components().next() else {
            return false;
        };
        let letter = match prefix.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => letter,
            Prefix::UNC(..) | Prefix::VerbatimUNC(..) => return true,
            _ => return false,
        };
        let root = HSTRING::from(format!("{}:\\", letter as char));
        // SAFETY: `root` is a valid NUL-terminated wide string that outlives the
        // call; GetDriveTypeW only reads it.
        let kind = unsafe { GetDriveTypeW(&root) };
        kind == DRIVE_REMOTE
    }
}

/// The single-instance lock: an exclusive lock on `data/crm.lock`, held until
/// dropped (the process lifetime in the app).
#[derive(Debug)]
pub struct InstanceLock {
    file: File,
}

impl InstanceLock {
    /// Takes the lock in `data_dir`, or fails with [`StorageError::AlreadyRunning`]
    /// while another holder (process or handle) has it.
    pub fn acquire(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(data_dir.join("crm.lock"))?;
        match FileExt::try_lock(&file) {
            Ok(()) => Ok(Self { file }),
            Err(fs4::TryLockError::WouldBlock) => Err(StorageError::AlreadyRunning),
            Err(fs4::TryLockError::Error(err)) => Err(err.into()),
        }
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_paths() {
        assert!(is_network_path(Path::new(r"\\srv\x\crm.db")));
        assert!(is_network_path(Path::new(r"\\?\UNC\srv\x")));
        assert!(is_network_path(Path::new(r"\\?\unc\srv\x")));
        assert!(!is_network_path(Path::new(r"\\?\C:\x")));
        assert!(!is_network_path(Path::new(r"\\.\C:\x")));
        assert!(!is_network_path(Path::new(r"C:\x")));
        assert!(!is_network_path(Path::new("/media/usb")));
        assert!(!is_network_path(Path::new("data/crm.db")));
    }

    #[test]
    fn local_drive_check() {
        assert_eq!(
            check_local_drive(Path::new(r"\\srv\share\data\crm.db")),
            Err(StorageError::NetworkDrive)
        );
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(check_local_drive(dir.path()), Ok(()));
    }

    #[test]
    fn second_lock_fails_until_the_first_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let first = InstanceLock::acquire(&data).unwrap();
        assert_eq!(
            InstanceLock::acquire(&data).unwrap_err(),
            StorageError::AlreadyRunning
        );
        drop(first);
        let again = InstanceLock::acquire(&data).unwrap();
        drop(again);
    }
}

//! Where everything lives on disk: next to the executable, never in APPDATA.

use crate::error::{Result, StorageError};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// The data layout under one root folder (the folder holding `crm.exe`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPaths {
    /// The root folder.
    pub root: PathBuf,
    /// `root/data/crm.db`.
    pub db: PathBuf,
    /// `root/backups`.
    pub backups: PathBuf,
    /// `root/logs`.
    pub logs: PathBuf,
    /// `root/data/crm.lock`, the single-instance lock file.
    pub lock: PathBuf,
    /// `root/data/crm.key`, the wrapped data key (plan 015). Every backup copy
    /// gets a byte copy of it as its `.key` sibling when it exists.
    pub key: PathBuf,
}

impl DataPaths {
    /// The layout next to the running executable.
    pub fn from_exe() -> Result<Self> {
        let exe = std::env::current_exe()?;
        let root = exe
            .parent()
            .ok_or_else(|| StorageError::other("the executable has no parent folder"))?;
        Ok(Self::at(root))
    }

    /// The layout under an explicit root (tests, tools).
    pub fn at(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref().to_path_buf();
        let data = root.join("data");
        Self {
            db: data.join("crm.db"),
            lock: data.join("crm.lock"),
            key: data.join("crm.key"),
            backups: root.join("backups"),
            logs: root.join("logs"),
            root,
        }
    }

    /// `root/data`.
    pub fn data_dir(&self) -> PathBuf {
        self.root.join("data")
    }

    /// Creates `data/`, `backups/` and `logs/`, then proves `data/` is writable
    /// by creating and deleting `data/.probe`. A refused write gives
    /// [`StorageError::ReadOnlyLocation`].
    pub fn ensure_dirs(&self) -> Result<()> {
        for dir in [self.data_dir(), self.backups.clone(), self.logs.clone()] {
            std::fs::create_dir_all(&dir).map_err(write_error)?;
        }
        let probe = self.data_dir().join(".probe");
        std::fs::write(&probe, b"probe").map_err(write_error)?;
        std::fs::remove_file(&probe).map_err(write_error)?;
        Ok(())
    }
}

fn write_error(err: std::io::Error) -> StorageError {
    match err.kind() {
        ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem => {
            StorageError::ReadOnlyLocation
        }
        _ => err.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_under_root() {
        let paths = DataPaths::at("/x");
        assert_eq!(paths.db, Path::new("/x/data/crm.db"));
        assert_eq!(paths.lock, Path::new("/x/data/crm.lock"));
        assert_eq!(paths.key, Path::new("/x/data/crm.key"));
        assert_eq!(paths.backups, Path::new("/x/backups"));
        assert_eq!(paths.logs, Path::new("/x/logs"));
    }

    #[test]
    fn ensure_dirs_creates_the_tree() {
        let dir = tempfile::tempdir().unwrap();
        let paths = DataPaths::at(dir.path());
        paths.ensure_dirs().unwrap();
        assert!(paths.data_dir().is_dir());
        assert!(paths.backups.is_dir());
        assert!(paths.logs.is_dir());
        assert!(!paths.data_dir().join(".probe").exists());
    }

    #[cfg(unix)]
    #[test]
    fn read_only_data_dir_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let paths = DataPaths::at(dir.path());
        paths.ensure_dirs().unwrap();
        let data = paths.data_dir();
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o555)).unwrap();
        let result = paths.ensure_dirs();
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(result, Err(StorageError::ReadOnlyLocation));
    }
}

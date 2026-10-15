//! The emergency wipe. This module never logs.

use crate::error::VaultError;
use crm_storage::{DataPaths, overwrite_and_delete};
use std::path::Path;

/// What the bulk deletion could not remove (best effort; the key files are
/// already gone when this is returned).
#[derive(Debug, Default)]
pub(crate) struct WipeReport {
    pub(crate) failures: usize,
}

/// 1. overwrite and delete `data/crm.key` (an error here aborts: the caller
///    reports a wrong entry);
/// 2. overwrite and delete every `backups/*.key`;
/// 3. remove everything in `data/` except `crm.lock`, then everything in
///    `backups/` and `logs/`, file by file, best effort;
/// 4. recreate the empty folders.
///
/// Every SQLite handle must be closed first (Windows refuses to delete open files).
pub(crate) fn wipe(paths: &DataPaths) -> Result<WipeReport, VaultError> {
    overwrite_and_delete(&paths.key).map_err(|e| VaultError::Io(e.to_string()))?;
    let mut report = WipeReport::default();
    if let Ok(entries) = std::fs::read_dir(&paths.backups) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "key")
                && path.is_file()
                && overwrite_and_delete(&path).is_err()
            {
                report.failures += 1;
            }
        }
    }
    let data = paths.data_dir();
    clear_dir(&data, Some(&paths.lock), &mut report);
    clear_dir(&paths.backups, None, &mut report);
    clear_dir(&paths.logs, None, &mut report);
    if paths.ensure_dirs().is_err() {
        report.failures += 1;
    }
    Ok(report)
}

fn clear_dir(dir: &Path, keep: Option<&Path>, report: &mut WipeReport) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if keep.is_some_and(|k| k == path) {
            continue;
        }
        let removed = match entry.file_type() {
            Ok(t) if t.is_dir() => std::fs::remove_dir_all(&path),
            _ => std::fs::remove_file(&path),
        };
        if removed.is_err() {
            report.failures += 1;
        }
    }
}

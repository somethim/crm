//! `VACUUM INTO` backups, their rotation, and restore.
//!
//! Names: `crm-YYYYMMDD-HHMMSS[-N][-reason].db`, in Tirana local time. `-N`
//! (2, 3, …) only appears on a same-second collision. Reasons: none (daily),
//! `exit`, `manual`, `pre-import-<kind>`, `pre-migrate-v<N>`, `before-restore`.
//!
//! A keyed database's copies are encrypted with the same key (`VACUUM INTO`
//! inherits it). Each copy `X.db` gets a sibling `X.key`, a byte copy of
//! `data/crm.key`, whenever that file exists; rotation deletes the sibling with
//! its copy, and deletes orphan `.key` files.

use crate::conn;
use crate::db::Db;
use crate::error::{Result, StorageError};
use crate::key::DbKey;
use crate::paths::DataPaths;
use crate::repo::settings;
use crm_domain::time;
use jiff::Timestamp;
use jiff::civil::{Date, Time};
use rusqlite::{Connection, OpenFlags};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Daily copies kept: the newest per day, for this many most recent days with copies.
pub const KEEP_DAILY: usize = 7;
/// Weekly copies kept: the newest per ISO week, for this many most recent weeks.
pub const KEEP_WEEKLY: usize = 4;
/// Monthly copies kept: the newest per month, for this many most recent months.
pub const KEEP_MONTHLY: usize = 12;
/// Copies kept per `pre-*` reason.
pub const KEEP_PER_PRE_REASON: usize = 5;

/// Why a backup is taken; it becomes the file-name suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupReason {
    /// The once-a-day copy (no suffix).
    Daily,
    /// Taken on clean exit (`-exit`).
    Exit,
    /// "Rezervo tani" (`-manual`).
    Manual,
    /// Before an import of the given kind (`-pre-import-<kind>`, e.g. `products`).
    PreImport(&'static str),
    /// Before migrating to the given schema version (`-pre-migrate-v<N>`).
    PreMigrate(u32),
}

impl BackupReason {
    fn suffix(self) -> String {
        match self {
            BackupReason::Daily => String::new(),
            BackupReason::Exit => "-exit".into(),
            BackupReason::Manual => "-manual".into(),
            BackupReason::PreImport(kind) => format!("-pre-import-{kind}"),
            BackupReason::PreMigrate(version) => format!("-pre-migrate-v{version}"),
        }
    }
}

/// A parsed backup file name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BackupName {
    /// Local (Tirana) date of the copy.
    pub date: Date,
    /// Local (Tirana) time of the copy.
    pub time: Time,
    /// Same-second collision counter (1 when absent).
    pub seq: u32,
    /// The suffix after the time, without the leading `-` (`None` = daily).
    pub reason: Option<String>,
}

impl BackupName {
    /// Parses `crm-YYYYMMDD-HHMMSS[-N][-reason].db`; `None` for anything else.
    pub fn parse(name: &str) -> Option<BackupName> {
        let body = name.strip_prefix("crm-")?.strip_suffix(".db")?;
        let (stamp, rest) = body.split_at_checked(15)?;
        let bytes = stamp.as_bytes();
        if bytes[8] != b'-'
            || !bytes[..8].iter().all(u8::is_ascii_digit)
            || !bytes[9..].iter().all(u8::is_ascii_digit)
        {
            return None;
        }
        let num = |range: std::ops::Range<usize>| stamp[range].parse::<i16>().ok();
        let date = Date::new(num(0..4)?, num(4..6)? as i8, num(6..8)? as i8).ok()?;
        let time = Time::new(num(9..11)? as i8, num(11..13)? as i8, num(13..15)? as i8, 0).ok()?;
        let mut rest = rest;
        let mut seq = 1;
        if let Some(after) = rest.strip_prefix('-') {
            let digits = after.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0 {
                seq = after[..digits].parse().ok()?;
                rest = &after[digits..];
            }
        }
        let reason = match rest {
            "" => None,
            r => {
                let reason = r.strip_prefix('-')?;
                if reason.is_empty() {
                    return None;
                }
                Some(reason.to_owned())
            }
        };
        Some(BackupName {
            date,
            time,
            seq,
            reason,
        })
    }

    /// True for the copies that take part in the daily/weekly/monthly rotation:
    /// daily, `exit` and `manual`.
    pub fn is_regular(&self) -> bool {
        matches!(self.reason.as_deref(), None | Some("exit") | Some("manual"))
    }

    fn pre_reason(&self) -> Option<&str> {
        self.reason.as_deref().filter(|r| r.starts_with("pre-"))
    }

    /// The rotation order (larger = newer), total so the outcome never depends
    /// on the directory listing order. `seq` counts per suffix, so a daily and an
    /// `exit`/`manual` copy of the same second both have `seq` 1; within a second
    /// the deliberate copy (`exit`, `manual`) ranks above the daily one, then
    /// `seq`, then the reason text breaks any remaining tie.
    fn key(&self) -> (Date, Time, bool, u32, Option<&str>) {
        let deliberate = self.reason.is_some();
        (
            self.date,
            self.time,
            deliberate,
            self.seq,
            self.reason.as_deref(),
        )
    }
}

/// A rotation bucket key (day, ISO week or month) of a copy.
type Bucket = fn(&BackupName) -> (i16, i8, i8);

fn day_bucket(n: &BackupName) -> (i16, i8, i8) {
    (n.date.year(), n.date.month(), n.date.day())
}

fn week_bucket(n: &BackupName) -> (i16, i8, i8) {
    let iso = n.date.iso_week_date();
    (iso.year(), iso.week(), 0)
}

fn month_bucket(n: &BackupName) -> (i16, i8, i8) {
    (n.date.year(), n.date.month(), 0)
}

/// What [`plan_rotation`] decided.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RotationPlan {
    /// Names kept (sorted).
    pub keep: Vec<String>,
    /// Names to delete (sorted).
    pub delete: Vec<String>,
}

/// The pure rotation planner over file names. It keeps:
/// - the newest regular copy per day, for the [`KEEP_DAILY`] most recent days with copies;
/// - the newest per ISO week for [`KEEP_WEEKLY`] weeks, and per month for [`KEEP_MONTHLY`] months;
/// - the [`KEEP_PER_PRE_REASON`] newest per `pre-*` reason (e.g. `pre-import-products`).
///
/// Names it does not recognise (including `before-restore` copies) are always kept.
pub fn plan_rotation<'a>(names: impl IntoIterator<Item = &'a str>) -> RotationPlan {
    let mut keep = BTreeSet::new();
    let mut regular = Vec::new();
    let mut pre: BTreeMap<String, Vec<(BackupName, &str)>> = BTreeMap::new();
    let mut all = Vec::new();
    for name in names {
        all.push(name);
        match BackupName::parse(name) {
            Some(parsed) if parsed.is_regular() => regular.push((parsed, name)),
            Some(parsed) if parsed.pre_reason().is_some() => {
                let reason = parsed.pre_reason().unwrap_or_default().to_owned();
                pre.entry(reason).or_default().push((parsed, name));
            }
            _ => {
                keep.insert(name);
            }
        }
    }

    regular.sort_by(|a, b| b.0.key().cmp(&a.0.key()));
    let buckets: [(Bucket, usize); 3] = [
        (day_bucket, KEEP_DAILY),
        (week_bucket, KEEP_WEEKLY),
        (month_bucket, KEEP_MONTHLY),
    ];
    for (bucket, limit) in buckets {
        let mut seen = Vec::new();
        for (parsed, name) in &regular {
            let b = bucket(parsed);
            if seen.contains(&b) {
                continue;
            }
            if seen.len() == limit {
                break;
            }
            seen.push(b);
            keep.insert(name);
        }
    }

    for entries in pre.values_mut() {
        entries.sort_by(|a, b| b.0.key().cmp(&a.0.key()));
        for (_, name) in entries.iter().take(KEEP_PER_PRE_REASON) {
            keep.insert(name);
        }
    }

    let mut plan = RotationPlan::default();
    for name in all {
        if keep.contains(name) {
            plan.keep.push(name.to_owned());
        } else {
            plan.delete.push(name.to_owned());
        }
    }
    plan.keep.sort();
    plan.keep.dedup();
    plan.delete.sort();
    plan
}

/// Applies [`plan_rotation`] to the files in `dir`; returns the deleted paths.
pub fn rotate(dir: &Path) -> Result<Vec<PathBuf>> {
    rotate_keeping(dir, None)
}

/// [`rotate`], but never deletes `fresh`: the copy the calling backup just wrote
/// (and returns) survives its own rotation even when other copies outrank it,
/// e.g. copies dated in the future after a clock change.
///
/// Each deleted `X.db` takes its sibling `X.key` with it, and every `*.key`
/// whose `X.db` is gone (an orphan) is deleted too. The returned list holds the
/// deleted copies only, not their `.key` files.
fn rotate_keeping(dir: &Path, fresh: Option<&Path>) -> Result<Vec<PathBuf>> {
    let names = file_names(dir)?;
    let plan = plan_rotation(
        names
            .iter()
            .map(String::as_str)
            .filter(|name| !is_key_file(name)),
    );
    let fresh = fresh.and_then(Path::file_name);
    let mut deleted = Vec::new();
    for name in plan.delete {
        if fresh == Some(std::ffi::OsStr::new(&name)) {
            continue;
        }
        let path = dir.join(&name);
        std::fs::remove_file(&path)?;
        remove_if_exists(&key_sibling(&path))?;
        deleted.push(path);
    }
    for name in names.iter().filter(|name| is_key_file(name)) {
        let key = dir.join(name);
        if key.exists() && !key.with_extension("db").exists() {
            std::fs::remove_file(&key)?;
        }
    }
    Ok(deleted)
}

fn is_key_file(name: &str) -> bool {
    name.ends_with(".key")
}

/// `X.db` → `X.key`.
fn key_sibling(db: &Path) -> PathBuf {
    db.with_extension("key")
}

fn remove_if_exists(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Copies `key_file` (`data/crm.key`) to the sibling `.key` of `target_db` and
/// syncs it, when `key_file` exists (a no-op otherwise). If the copy fails, the
/// copy `target_db` is deleted too, so a backup never sits without its key.
fn copy_key_sibling(key_file: &Path, target_db: &Path) -> Result<()> {
    if !key_file.exists() {
        return Ok(());
    }
    let sibling = key_sibling(target_db);
    let copied = (|| -> Result<()> {
        std::fs::copy(key_file, &sibling)?;
        std::fs::OpenOptions::new()
            .write(true)
            .open(&sibling)?
            .sync_all()?;
        Ok(())
    })();
    if let Err(err) = copied {
        let _ = std::fs::remove_file(&sibling);
        let _ = std::fs::remove_file(target_db);
        return Err(err);
    }
    Ok(())
}

/// True when `dir` holds no regular (daily, exit or manual) copy dated today
/// (Tirana) for `now`.
pub fn daily_backup_due(dir: &Path, now: Timestamp) -> Result<bool> {
    let today = now.to_zoned(time::tirane()).date();
    let names = match file_names(dir) {
        Ok(names) => names,
        Err(_) if !dir.exists() => return Ok(true),
        Err(err) => return Err(err),
    };
    Ok(!names
        .iter()
        .filter_map(|n| BackupName::parse(n))
        .any(|n| n.date == today && n.is_regular()))
}

fn file_names(dir: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && let Ok(name) = entry.file_name().into_string()
        {
            names.push(name);
        }
    }
    Ok(names)
}

/// A free target `dir/crm-YYYYMMDD-HHMMSS[-N]<suffix>.db` (`VACUUM INTO` needs
/// a path that does not exist yet).
fn unique_target(dir: &Path, now: Timestamp, suffix: &str) -> PathBuf {
    let stamp = now
        .to_zoned(time::tirane())
        .strftime("%Y%m%d-%H%M%S")
        .to_string();
    let mut seq = 1;
    loop {
        let name = if seq == 1 {
            format!("crm-{stamp}{suffix}.db")
        } else {
            format!("crm-{stamp}-{seq}{suffix}.db")
        };
        let path = dir.join(name);
        if !path.exists() {
            return path;
        }
        seq += 1;
    }
}

/// `VACUUM INTO target` (a keyed source writes a copy encrypted with the same
/// key), then `quick_check` on the copy opened with `key`; a bad copy is deleted.
fn snapshot(conn: &Connection, target: &Path, key: &DbKey) -> Result<()> {
    let text = target
        .to_str()
        .ok_or_else(|| StorageError::other("backup path is not valid UTF-8"))?;
    conn.execute("VACUUM INTO ?1", [text])?;
    if let Err(err) = verify(target, key) {
        let _ = std::fs::remove_file(target);
        return Err(err);
    }
    Ok(())
}

/// Opens `path` read-only with `key` and runs `quick_check`. A copy encrypted
/// with another key (or a file that is no database) gives
/// [`StorageError::WrongKey`].
pub fn verify(path: &Path, key: &DbKey) -> Result<()> {
    let conn = conn::open_keyed_connection(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        Some(key),
    )?;
    conn::quick_check(&conn)
}

/// The `pre-migrate-v{N}` copy, taken on the writer connection before migrating.
pub(crate) fn pre_migrate(
    conn: &Connection,
    paths: &DataPaths,
    version: u32,
    key: &DbKey,
) -> Result<PathBuf> {
    let dir = &paths.backups;
    std::fs::create_dir_all(dir)?;
    let target = unique_target(
        dir,
        time::now(),
        &BackupReason::PreMigrate(version).suffix(),
    );
    snapshot(conn, &target, key)?;
    copy_key_sibling(&paths.key, &target)?;
    rotate_keeping(dir, Some(&target))?;
    Ok(target)
}

impl Db {
    /// Writes `backups/crm-YYYYMMDD-HHMMSS[-reason].db` from a consistent reader
    /// snapshot (the writer is not blocked), checks it, records `last_backup_at`
    /// and rotates the folder; the rotation never deletes the copy returned.
    pub fn backup_now(&self, reason: BackupReason) -> Result<PathBuf> {
        let dir = self.paths().backups.clone();
        let path = self.snapshot_into(&dir, &reason.suffix())?;
        rotate_keeping(&dir, Some(&path))?;
        Ok(path)
    }

    /// Like [`Db::backup_now`], into a folder the user picked (with the sibling
    /// `.key`); no rotation there.
    pub fn backup_to(&self, dir: &Path) -> Result<PathBuf> {
        self.snapshot_into(dir, "")
    }

    /// [`daily_backup_due`] for this database's backup folder.
    pub fn daily_backup_due(&self, now: Timestamp) -> Result<bool> {
        daily_backup_due(&self.paths().backups, now)
    }

    fn snapshot_into(&self, dir: &Path, suffix: &str) -> Result<PathBuf> {
        std::fs::create_dir_all(dir)?;
        let now = time::now();
        let target = unique_target(dir, now, suffix);
        let reader = self.reader()?;
        snapshot(&reader, &target, self.key())?;
        drop(reader);
        copy_key_sibling(&self.paths().key, &target)?;
        self.write(move |tx| settings::set_last_backup_at(tx, now))?;
        Ok(target)
    }
}

/// Replaces `paths.db` with `backup`. Call it only while no [`Db`] is open (the
/// caller holds the [`InstanceLock`](crate::InstanceLock)):
/// 1. `quick_check` the backup, opened with `key` (a copy encrypted with
///    another key gives [`StorageError::WrongKey`]: it belongs to other data);
/// 2. save the current data as `backups/crm-…-before-restore.db` (through
///    `VACUUM INTO` with `key`, so committed WAL content is included; a raw file
///    copy when the current database cannot be read), plus its sibling `.key`;
/// 3. copy the backup next to `crm.db`, then delete `crm.db-wal`/`crm.db-shm` and
///    move the copy over `crm.db`.
///
/// `paths.key` is never read for the data nor written: a restore keeps the
/// current key file.
///
/// Returns the `before-restore` path, if there was a database to save.
pub fn restore_from(backup: &Path, paths: &DataPaths, key: &DbKey) -> Result<Option<PathBuf>> {
    verify(backup, key)?;
    std::fs::create_dir_all(&paths.backups)?;
    std::fs::create_dir_all(paths.data_dir())?;

    let safety = if paths.db.exists() {
        let target = unique_target(&paths.backups, time::now(), "-before-restore");
        if save_current(&paths.db, &target, key).is_err() {
            // The current file may be the corrupt one being replaced: keep its raw
            // bytes (and its WAL, under the name SQLite looks for) instead.
            std::fs::copy(&paths.db, &target)?;
            let wal = paths.data_dir().join("crm.db-wal");
            if wal.exists() {
                let mut side = target.clone().into_os_string();
                side.push("-wal");
                std::fs::copy(&wal, PathBuf::from(side))?;
            }
        }
        copy_key_sibling(&paths.key, &target)?;
        Some(target)
    } else {
        None
    };

    let staged = paths.data_dir().join("crm.db.restore");
    std::fs::copy(backup, &staged)?;
    std::fs::File::open(&staged)?.sync_all()?;
    for side in ["crm.db-wal", "crm.db-shm"] {
        let path = paths.data_dir().join(side);
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
    }
    std::fs::rename(&staged, &paths.db)?;
    Ok(safety)
}

fn save_current(db: &Path, target: &Path, key: &DbKey) -> Result<()> {
    let current = conn::open_keyed_connection(db, OpenFlags::default(), Some(key))?;
    snapshot(&current, target, key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_names() {
        let n = BackupName::parse("crm-20261014-093005.db").unwrap();
        assert_eq!(n.date, jiff::civil::date(2026, 10, 14));
        assert_eq!(n.time, jiff::civil::time(9, 30, 5, 0));
        assert_eq!((n.seq, n.reason.as_deref()), (1, None));
        let n = BackupName::parse("crm-20261014-093005-2-pre-import-products.db").unwrap();
        assert_eq!(
            (n.seq, n.reason.as_deref()),
            (2, Some("pre-import-products"))
        );
        let n = BackupName::parse("crm-20261014-093005-pre-migrate-v1.db").unwrap();
        assert_eq!(n.reason.as_deref(), Some("pre-migrate-v1"));
        assert!(
            BackupName::parse("crm-20261014-093005-exit.db")
                .unwrap()
                .is_regular()
        );
        for bad in [
            "crm-20261314-093005.db",
            "crm-2026101-093005.db",
            "crm-20261014-093005-.db",
            "crm-20261014-093005.db-wal",
            "notes.txt",
            "crm-20261014093005.db",
        ] {
            assert_eq!(BackupName::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn reason_suffixes() {
        assert_eq!(BackupReason::Daily.suffix(), "");
        assert_eq!(BackupReason::Exit.suffix(), "-exit");
        assert_eq!(BackupReason::Manual.suffix(), "-manual");
        assert_eq!(
            BackupReason::PreImport("products").suffix(),
            "-pre-import-products"
        );
        assert_eq!(BackupReason::PreMigrate(1).suffix(), "-pre-migrate-v1");
    }

    #[test]
    fn unique_target_appends_a_counter() {
        let dir = tempfile::tempdir().unwrap();
        let now: Timestamp = "2026-10-14T08:00:00Z".parse().unwrap();
        let first = unique_target(dir.path(), now, "-manual");
        assert_eq!(first.file_name().unwrap(), "crm-20261014-100000-manual.db");
        std::fs::write(&first, b"x").unwrap();
        let second = unique_target(dir.path(), now, "-manual");
        assert_eq!(
            second.file_name().unwrap(),
            "crm-20261014-100000-2-manual.db"
        );
        assert_eq!(
            BackupName::parse("crm-20261014-100000-2-manual.db")
                .unwrap()
                .seq,
            2
        );
    }

    #[test]
    fn daily_due_only_counts_regular_copies_of_today() {
        let dir = tempfile::tempdir().unwrap();
        let now: Timestamp = "2026-10-14T08:00:00Z".parse().unwrap();
        assert!(daily_backup_due(dir.path(), now).unwrap());
        assert!(daily_backup_due(&dir.path().join("missing"), now).unwrap());
        std::fs::write(dir.path().join("crm-20261013-235900.db"), b"x").unwrap();
        std::fs::write(
            dir.path()
                .join("crm-20261014-070000-pre-import-products.db"),
            b"x",
        )
        .unwrap();
        assert!(daily_backup_due(dir.path(), now).unwrap());
        std::fs::write(dir.path().join("crm-20261014-071500-exit.db"), b"x").unwrap();
        assert!(!daily_backup_due(dir.path(), now).unwrap());
    }
}

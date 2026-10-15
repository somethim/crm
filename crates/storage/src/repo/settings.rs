//! `settings`: key/value pairs with typed helpers.

use super::ts_text;
use crate::conn::{Conn, Tx};
use crate::error::Result;
use jiff::Timestamp;
use rusqlite::{OptionalExtension, params};

/// Minutes of inactivity before the lock screen (`0` = off).
pub const IDLE_LOCK_MINUTES: &str = "idle_lock_minutes";
/// Receipt paper width in mm.
pub const RECEIPT_WIDTH_MM: &str = "receipt_width_mm";
/// Receipt font size in points.
pub const RECEIPT_FONT_PT: &str = "receipt_font_pt";
/// Time of the last successful backup (RFC 3339).
pub const LAST_BACKUP_AT: &str = "last_backup_at";

/// Default for [`IDLE_LOCK_MINUTES`].
pub const DEFAULT_IDLE_LOCK_MINUTES: u32 = 0;
/// Default for [`RECEIPT_WIDTH_MM`].
pub const DEFAULT_RECEIPT_WIDTH_MM: u32 = 72;
/// Default for [`RECEIPT_FONT_PT`].
pub const DEFAULT_RECEIPT_FONT_PT: u32 = 9;

/// The raw value of `key`.
pub fn get(c: &Conn<'_>, key: &str) -> Result<Option<String>> {
    Ok(c.raw()
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?)
}

/// Sets `key` to `value`.
pub fn set(tx: &Tx<'_>, key: &str, value: &str) -> Result<()> {
    tx.raw().execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// Deletes `key`; `true` when a row was removed.
pub fn delete(tx: &Tx<'_>, key: &str) -> Result<bool> {
    Ok(tx
        .raw()
        .execute("DELETE FROM settings WHERE key = ?1", [key])?
        > 0)
}

fn number(c: &Conn<'_>, key: &str, default: u32) -> Result<u32> {
    Ok(get(c, key)?
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default))
}

/// Idle-lock minutes (default 0 = off).
pub fn idle_lock_minutes(c: &Conn<'_>) -> Result<u32> {
    number(c, IDLE_LOCK_MINUTES, DEFAULT_IDLE_LOCK_MINUTES)
}

/// Sets the idle-lock minutes.
pub fn set_idle_lock_minutes(tx: &Tx<'_>, minutes: u32) -> Result<()> {
    set(tx, IDLE_LOCK_MINUTES, &minutes.to_string())
}

/// Receipt width in mm (default 72).
pub fn receipt_width_mm(c: &Conn<'_>) -> Result<u32> {
    number(c, RECEIPT_WIDTH_MM, DEFAULT_RECEIPT_WIDTH_MM)
}

/// Sets the receipt width in mm.
pub fn set_receipt_width_mm(tx: &Tx<'_>, mm: u32) -> Result<()> {
    set(tx, RECEIPT_WIDTH_MM, &mm.to_string())
}

/// Receipt font size in points (default 9).
pub fn receipt_font_pt(c: &Conn<'_>) -> Result<u32> {
    number(c, RECEIPT_FONT_PT, DEFAULT_RECEIPT_FONT_PT)
}

/// Sets the receipt font size in points.
pub fn set_receipt_font_pt(tx: &Tx<'_>, pt: u32) -> Result<()> {
    set(tx, RECEIPT_FONT_PT, &pt.to_string())
}

/// When the last backup was taken (`None` = never, or unreadable).
pub fn last_backup_at(c: &Conn<'_>) -> Result<Option<Timestamp>> {
    Ok(get(c, LAST_BACKUP_AT)?.and_then(|v| v.parse().ok()))
}

/// Records the time of a backup.
pub fn set_last_backup_at(tx: &Tx<'_>, at: Timestamp) -> Result<()> {
    set(tx, LAST_BACKUP_AT, &ts_text(at))
}

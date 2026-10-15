//! Repositories: free functions over [`Conn`](crate::Conn) (reads) and
//! [`Tx`](crate::Tx) (writes) that speak domain types.
//!
//! Storage formats: timestamps as RFC 3339 UTC text, `issued_on` as
//! `YYYY-MM-DD`, money and quantities as integers. `name_key`/`search_key` are
//! computed here with [`crm_domain::text::fold`].
//!
//! Reads return `Err(NotFound)` from `get`-style lookups and `Ok(None)` from
//! `by_*` lookups. `insert` functions ignore the `id` of the value passed in.

pub mod orders;
pub mod owners;
pub mod products;
pub mod settings;
pub mod skus;
pub mod tabs;

use crate::error::{Result, StorageError};
use crm_domain::{OrderKind, OwnerKind};
use jiff::Timestamp;
use jiff::civil::Date;
use rusqlite::Row;
use rusqlite::types::Type;

pub(crate) fn ts_text(ts: Timestamp) -> String {
    ts.to_string()
}

pub(crate) fn date_text(date: Date) -> String {
    date.to_string()
}

fn conversion(idx: usize, err: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(idx, Type::Text, Box::new(err))
}

pub(crate) fn col_ts(row: &Row<'_>, idx: usize) -> rusqlite::Result<Timestamp> {
    let text: String = row.get(idx)?;
    text.parse().map_err(|e| conversion(idx, e))
}

pub(crate) fn col_date(row: &Row<'_>, idx: usize) -> rusqlite::Result<Date> {
    let text: String = row.get(idx)?;
    text.parse().map_err(|e| conversion(idx, e))
}

pub(crate) fn col_order_kind(row: &Row<'_>, idx: usize) -> rusqlite::Result<OrderKind> {
    let raw: i64 = row.get(idx)?;
    OrderKind::try_from(raw).map_err(|e| conversion(idx, e))
}

pub(crate) fn col_owner_kind(row: &Row<'_>, idx: usize) -> rusqlite::Result<OwnerKind> {
    let raw: i64 = row.get(idx)?;
    OwnerKind::try_from(raw).map_err(|e| conversion(idx, e))
}

/// `Err(NotFound)` when an UPDATE/DELETE touched no row.
pub(crate) fn expect_one(changed: usize) -> Result<()> {
    if changed == 0 {
        Err(StorageError::NotFound)
    } else {
        Ok(())
    }
}

/// `%token%` LIKE patterns for a filter (folded tokens; they hold no `%`/`_`).
pub(crate) fn like_patterns(filter: &str) -> Vec<String> {
    crm_domain::text::tokens(filter)
        .into_iter()
        .map(|t| format!("%{t}%"))
        .collect()
}

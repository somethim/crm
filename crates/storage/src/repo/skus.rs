//! `skus`: one stock pool, its code, unit and prices.

use super::{expect_one, ts_text};
use crate::conn::{Conn, Tx};
use crate::error::Result;
use crm_domain::{Money, Qty, Sku, SkuId, time};
use rusqlite::{OptionalExtension, Row, params};

const COLUMNS: &str = "id, code, unit, buy_price, sell_price, quantity";

pub(crate) fn map(row: &Row<'_>, at: usize) -> rusqlite::Result<Sku> {
    Ok(Sku {
        id: SkuId(row.get(at)?),
        code: row.get(at + 1)?,
        unit: row.get(at + 2)?,
        buy_price: Money(row.get(at + 3)?),
        sell_price: Money(row.get(at + 4)?),
        quantity: Qty(row.get(at + 5)?),
    })
}

/// Changes for [`update_fields`]; `None` leaves a field as it is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkuPatch {
    /// New base code.
    pub code: Option<String>,
    /// New unit.
    pub unit: Option<String>,
    /// New buy price.
    pub buy_price: Option<Money>,
    /// New sell price.
    pub sell_price: Option<Money>,
    /// New stock quantity (absolute).
    pub quantity: Option<Qty>,
}

/// The SKU with this id.
pub fn get(c: &Conn<'_>, id: SkuId) -> Result<Sku> {
    Ok(c.raw().query_row(
        &format!("SELECT {COLUMNS} FROM skus WHERE id = ?1"),
        [id.0],
        |r| map(r, 0),
    )?)
}

/// The SKU with this exact base code.
pub fn by_code(c: &Conn<'_>, code: &str) -> Result<Option<Sku>> {
    Ok(c.raw()
        .query_row(
            &format!("SELECT {COLUMNS} FROM skus WHERE code = ?1"),
            [code],
            |r| map(r, 0),
        )
        .optional()?)
}

/// Every SKU code, in byte order (input for `sku::next_auto_code`).
pub fn all_codes(c: &Conn<'_>) -> Result<Vec<String>> {
    let mut stmt = c.raw().prepare("SELECT code FROM skus ORDER BY code")?;
    let codes = stmt
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(codes)
}

/// Inserts a SKU (its `id` is ignored) and returns the new id.
pub fn insert(tx: &Tx<'_>, sku: &Sku) -> Result<SkuId> {
    let now = ts_text(time::now());
    tx.raw().execute(
        "INSERT INTO skus (code, unit, buy_price, sell_price, quantity, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![
            sku.code,
            sku.unit,
            sku.buy_price.0,
            sku.sell_price.0,
            sku.quantity.0,
            now
        ],
    )?;
    Ok(SkuId(tx.raw().last_insert_rowid()))
}

/// Overwrites the fields set in `patch`.
pub fn update_fields(tx: &Tx<'_>, id: SkuId, patch: &SkuPatch) -> Result<()> {
    let changed = tx.raw().execute(
        "UPDATE skus SET
            code = COALESCE(?1, code),
            unit = COALESCE(?2, unit),
            buy_price = COALESCE(?3, buy_price),
            sell_price = COALESCE(?4, sell_price),
            quantity = COALESCE(?5, quantity),
            updated_at = ?6
         WHERE id = ?7",
        params![
            patch.code,
            patch.unit,
            patch.buy_price.map(|m| m.0),
            patch.sell_price.map(|m| m.0),
            patch.quantity.map(|q| q.0),
            ts_text(time::now()),
            id.0
        ],
    )?;
    expect_one(changed)
}

/// `quantity = quantity + delta`. The `CHECK (quantity >= 0)` rejects a result
/// below zero with [`StorageError::Constraint`](crate::StorageError::Constraint).
pub fn apply_delta(tx: &Tx<'_>, id: SkuId, delta: Qty) -> Result<()> {
    let changed = tx.raw().execute(
        "UPDATE skus SET quantity = quantity + ?1, updated_at = ?2 WHERE id = ?3",
        params![delta.0, ts_text(time::now()), id.0],
    )?;
    expect_one(changed)
}

/// Deletes the SKU when no product uses it any more (001 A11). Returns whether
/// it was deleted.
pub fn delete_if_orphan(tx: &Tx<'_>, id: SkuId) -> Result<bool> {
    let changed = tx.raw().execute(
        "DELETE FROM skus WHERE id = ?1 AND NOT EXISTS (SELECT 1 FROM products WHERE sku_id = ?1)",
        [id.0],
    )?;
    Ok(changed > 0)
}

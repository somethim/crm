//! `products`: a named flavour of a SKU.

use super::{expect_one, like_patterns, skus, ts_text};
use crate::conn::{Conn, Tx};
use crate::error::Result;
use crm_domain::text::fold;
use crm_domain::{Money, Product, ProductId, ProductRow, Qty, SkuId, time};
use rusqlite::{OptionalExtension, Row, params, params_from_iter};

const ROW_COLUMNS: &str =
    "p.id, p.sku_id, p.name, s.id, s.code, s.unit, s.buy_price, s.sell_price, s.quantity";
const ROW_FROM: &str = "products p JOIN skus s ON s.id = p.sku_id";

fn map_row(row: &Row<'_>) -> rusqlite::Result<ProductRow> {
    Ok(ProductRow {
        product: Product {
            id: ProductId(row.get(0)?),
            sku_id: SkuId(row.get(1)?),
            name: row.get(2)?,
        },
        sku: skus::map(row, 3)?,
    })
}

/// One product as the search index needs it, with the derived popularity
/// (`order_count` = number of distinct orders with a line for it; 001 Q2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductIndexRow {
    /// Product id.
    pub id: ProductId,
    /// Product name.
    pub name: String,
    /// SKU code.
    pub code: String,
    /// SKU unit.
    pub unit: String,
    /// SKU sell price.
    pub sell_price: Money,
    /// SKU stock.
    pub quantity: Qty,
    /// Distinct orders referencing the product.
    pub order_count: i64,
}

/// Inserts a product of `sku_id` and returns its id.
pub fn insert(tx: &Tx<'_>, sku_id: SkuId, name: &str) -> Result<ProductId> {
    let now = ts_text(time::now());
    tx.raw().execute(
        "INSERT INTO products (sku_id, name, name_key, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?4)",
        params![sku_id.0, name, fold(name), now],
    )?;
    Ok(ProductId(tx.raw().last_insert_rowid()))
}

/// Renames a product (and refreshes its `name_key`).
pub fn update_name(tx: &Tx<'_>, id: ProductId, name: &str) -> Result<()> {
    let changed = tx.raw().execute(
        "UPDATE products SET name = ?1, name_key = ?2, updated_at = ?3 WHERE id = ?4",
        params![name, fold(name), ts_text(time::now()), id.0],
    )?;
    expect_one(changed)
}

/// Deletes a product and returns its SKU id (for [`skus::delete_if_orphan`]).
/// Order lines keep their snapshot; their `product_id` becomes NULL.
pub fn delete(tx: &Tx<'_>, id: ProductId) -> Result<SkuId> {
    let sku: i64 =
        tx.raw()
            .query_row("SELECT sku_id FROM products WHERE id = ?1", [id.0], |r| {
                r.get(0)
            })?;
    tx.raw()
        .execute("DELETE FROM products WHERE id = ?1", [id.0])?;
    Ok(SkuId(sku))
}

/// The product joined with its SKU.
pub fn get_row(c: &Conn<'_>, id: ProductId) -> Result<ProductRow> {
    Ok(c.raw().query_row(
        &format!("SELECT {ROW_COLUMNS} FROM {ROW_FROM} WHERE p.id = ?1"),
        [id.0],
        map_row,
    )?)
}

/// A page of products whose folded name or code contains every token of
/// `filter` (all when blank), ordered by code, then name (byte order).
pub fn list_rows(c: &Conn<'_>, filter: &str, offset: i64, limit: i64) -> Result<Vec<ProductRow>> {
    let patterns = like_patterns(filter);
    let mut sql = format!("SELECT {ROW_COLUMNS} FROM {ROW_FROM} WHERE 1 = 1");
    for i in 1..=patterns.len() {
        sql.push_str(&format!(
            " AND (p.name_key LIKE ?{i} OR lower(s.code) LIKE ?{i})"
        ));
    }
    sql.push_str(&format!(
        " ORDER BY s.code, p.name, p.id LIMIT {limit} OFFSET {offset}",
        limit = limit.max(0),
        offset = offset.max(0)
    ));
    let mut stmt = c.raw().prepare(&sql)?;
    let rows = stmt
        .query_map(params_from_iter(patterns), map_row)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// The first product (lowest id) whose folded name equals `fold(key)`.
pub fn by_name_key(c: &Conn<'_>, key: &str) -> Result<Option<ProductRow>> {
    Ok(c.raw()
        .query_row(
            &format!(
                "SELECT {ROW_COLUMNS} FROM {ROW_FROM} WHERE p.name_key = ?1 ORDER BY p.id LIMIT 1"
            ),
            [fold(key)],
            map_row,
        )
        .optional()?)
}

/// The product of `sku_id` with exactly this name.
pub fn by_sku_and_name(c: &Conn<'_>, sku_id: SkuId, name: &str) -> Result<Option<Product>> {
    Ok(c.raw()
        .query_row(
            "SELECT id, sku_id, name FROM products WHERE sku_id = ?1 AND name = ?2",
            params![sku_id.0, name],
            |r| {
                Ok(Product {
                    id: ProductId(r.get(0)?),
                    sku_id: SkuId(r.get(1)?),
                    name: r.get(2)?,
                })
            },
        )
        .optional()?)
}

/// Every product for the search index, ordered by id.
pub fn all_for_index(c: &Conn<'_>) -> Result<Vec<ProductIndexRow>> {
    let mut stmt = c.raw().prepare(
        "SELECT p.id, p.name, s.code, s.unit, s.sell_price, s.quantity, COALESCE(oc.n, 0)
         FROM products p
         JOIN skus s ON s.id = p.sku_id
         LEFT JOIN (
             SELECT product_id, COUNT(DISTINCT order_id) AS n
             FROM order_lines WHERE product_id IS NOT NULL GROUP BY product_id
         ) oc ON oc.product_id = p.id
         ORDER BY p.id",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(ProductIndexRow {
                id: ProductId(r.get(0)?),
                name: r.get(1)?,
                code: r.get(2)?,
                unit: r.get(3)?,
                sell_price: Money(r.get(4)?),
                quantity: Qty(r.get(5)?),
                order_count: r.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

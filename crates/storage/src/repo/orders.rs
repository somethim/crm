//! `orders` and `order_lines`: invoices with owner and line snapshots.

use super::{col_date, col_order_kind, col_ts, date_text, expect_one, ts_text};
use crate::conn::{Conn, Tx};
use crate::error::Result;
use crm_domain::text::{fold, tokens};
use crm_domain::{
    Money, Order, OrderId, OrderKind, OrderLine, OwnerId, OwnerSnapshot, ProductId, Qty, SkuId,
};
use jiff::Timestamp;
use jiff::civil::Date;
use rusqlite::types::Value;
use rusqlite::{Row, params, params_from_iter};

const HEADER_COLUMNS: &str = "o.id, o.kind, o.owner_id, o.owner_name, o.owner_description, \
     o.owner_nipt, o.owner_address, o.issued_on, o.total, o.created_at, o.updated_at";
const LINE_COLUMNS: &str =
    "l.position, l.product_id, l.sku_id, l.name, l.code, l.unit, l.quantity, l.price, l.value";

/// An order without its lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderHeader {
    /// "Nr. Porosie".
    pub id: OrderId,
    /// Shitje / Blerje.
    pub kind: OrderKind,
    /// The owner, unless deleted.
    pub owner_id: Option<OwnerId>,
    /// Owner details as saved on the order.
    pub owner: OwnerSnapshot,
    /// Issue date (filters and reports use it).
    pub issued_on: Date,
    /// Order total.
    pub total: Money,
    /// Creation time.
    pub created_at: Timestamp,
    /// Last change.
    pub updated_at: Timestamp,
}

/// One row of the order list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderSummary {
    /// "Nr. Porosie".
    pub id: OrderId,
    /// Shitje / Blerje.
    pub kind: OrderKind,
    /// Owner name snapshot.
    pub owner_name: String,
    /// Owner description snapshot.
    pub owner_description: Option<String>,
    /// Order total.
    pub total: Money,
    /// Issue date.
    pub issued_on: Date,
    /// Last change ("Modifikuar").
    pub updated_at: Timestamp,
}

/// The order list filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderFilter {
    /// Only this kind, or both.
    pub kind: Option<OrderKind>,
    /// First `issued_on` included.
    pub from: Date,
    /// Last `issued_on` included.
    pub to: Date,
    /// Search text: all digits = the order number (dates ignored); otherwise
    /// every folded token must occur in owner name or description.
    pub text: Option<String>,
}

fn map_header(row: &Row<'_>) -> rusqlite::Result<OrderHeader> {
    Ok(OrderHeader {
        id: OrderId(row.get(0)?),
        kind: col_order_kind(row, 1)?,
        owner_id: row.get::<_, Option<i64>>(2)?.map(OwnerId),
        owner: OwnerSnapshot {
            name: row.get(3)?,
            description: row.get(4)?,
            nipt: row.get(5)?,
            address: row.get(6)?,
        },
        issued_on: col_date(row, 7)?,
        total: Money(row.get(8)?),
        created_at: col_ts(row, 9)?,
        updated_at: col_ts(row, 10)?,
    })
}

fn map_line(row: &Row<'_>, at: usize) -> rusqlite::Result<OrderLine> {
    Ok(OrderLine {
        position: row.get(at)?,
        product_id: row.get::<_, Option<i64>>(at + 1)?.map(ProductId),
        sku_id: row.get::<_, Option<i64>>(at + 2)?.map(SkuId),
        name: row.get(at + 3)?,
        code: row.get(at + 4)?,
        unit: row.get(at + 5)?,
        quantity: Qty(row.get(at + 6)?),
        price: Money(row.get(at + 7)?),
        value: Money(row.get(at + 8)?),
    })
}

fn search_key(owner: &OwnerSnapshot) -> String {
    fold(&format!(
        "{} {}",
        owner.name,
        owner.description.as_deref().unwrap_or_default()
    ))
}

fn insert_lines(tx: &Tx<'_>, id: OrderId, lines: &[OrderLine]) -> Result<()> {
    let mut stmt = tx.raw().prepare_cached(
        "INSERT INTO order_lines (order_id, position, product_id, sku_id, name, code, unit,
                                  quantity, price, value)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )?;
    for line in lines {
        stmt.execute(params![
            id.0,
            line.position,
            line.product_id.map(|p| p.0),
            line.sku_id.map(|s| s.0),
            line.name,
            line.code,
            line.unit,
            line.quantity.0,
            line.price.0,
            line.value.0
        ])?;
    }
    Ok(())
}

/// Inserts the order with its lines (`order.id` is ignored) and returns the new
/// number. `created_at`/`updated_at` are written as given.
pub fn insert(tx: &Tx<'_>, order: &Order) -> Result<OrderId> {
    tx.raw().execute(
        "INSERT INTO orders (kind, owner_id, owner_name, owner_description, owner_nipt,
                             owner_address, search_key, issued_on, total, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            order.kind.as_i64(),
            order.owner_id.map(|o| o.0),
            order.owner.name,
            order.owner.description,
            order.owner.nipt,
            order.owner.address,
            search_key(&order.owner),
            date_text(order.issued_on),
            order.total.0,
            ts_text(order.created_at),
            ts_text(order.updated_at)
        ],
    )?;
    let id = OrderId(tx.raw().last_insert_rowid());
    insert_lines(tx, id, &order.lines)?;
    Ok(id)
}

/// Overwrites the header of `order.id` (keeping `created_at`) and replaces its lines.
pub fn replace(tx: &Tx<'_>, order: &Order) -> Result<()> {
    let changed = tx.raw().execute(
        "UPDATE orders SET kind = ?1, owner_id = ?2, owner_name = ?3, owner_description = ?4,
                           owner_nipt = ?5, owner_address = ?6, search_key = ?7, issued_on = ?8,
                           total = ?9, updated_at = ?10
         WHERE id = ?11",
        params![
            order.kind.as_i64(),
            order.owner_id.map(|o| o.0),
            order.owner.name,
            order.owner.description,
            order.owner.nipt,
            order.owner.address,
            search_key(&order.owner),
            date_text(order.issued_on),
            order.total.0,
            ts_text(order.updated_at),
            order.id.0
        ],
    )?;
    expect_one(changed)?;
    tx.raw()
        .execute("DELETE FROM order_lines WHERE order_id = ?1", [order.id.0])?;
    insert_lines(tx, order.id, &order.lines)
}

/// Deletes the order, its lines and any open tab of it. The number is never reused.
pub fn delete(tx: &Tx<'_>, id: OrderId) -> Result<()> {
    let changed = tx
        .raw()
        .execute("DELETE FROM orders WHERE id = ?1", [id.0])?;
    expect_one(changed)
}

/// The order with its lines, by position.
pub fn get(c: &Conn<'_>, id: OrderId) -> Result<Order> {
    let header = c.raw().query_row(
        &format!("SELECT {HEADER_COLUMNS} FROM orders o WHERE o.id = ?1"),
        [id.0],
        map_header,
    )?;
    let mut stmt = c.raw().prepare(&format!(
        "SELECT {LINE_COLUMNS} FROM order_lines l WHERE l.order_id = ?1 ORDER BY l.position, l.id"
    ))?;
    let lines = stmt
        .query_map([id.0], |r| map_line(r, 0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Order {
        id: header.id,
        kind: header.kind,
        owner_id: header.owner_id,
        owner: header.owner,
        issued_on: header.issued_on,
        total: header.total,
        created_at: header.created_at,
        updated_at: header.updated_at,
        lines,
    })
}

/// The order list, newest issue date first (`issued_on DESC, id DESC`).
pub fn list(c: &Conn<'_>, filter: &OrderFilter) -> Result<Vec<OrderSummary>> {
    let mut sql = String::from(
        "SELECT id, kind, owner_name, owner_description, total, issued_on, updated_at
         FROM orders WHERE 1 = 1",
    );
    let mut values: Vec<Value> = Vec::new();
    let mut bind = |sql: &mut String, clause: &str, value: Value| {
        values.push(value);
        sql.push_str(&clause.replace('?', &format!("?{}", values.len())));
    };
    if let Some(kind) = filter.kind {
        bind(&mut sql, " AND kind = ?", Value::Integer(kind.as_i64()));
    }
    let text = filter.text.as_deref().map(str::trim).unwrap_or_default();
    if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) {
        // The order number; an out-of-range number matches nothing.
        let id = text.parse::<i64>().unwrap_or(-1);
        bind(&mut sql, " AND id = ?", Value::Integer(id));
    } else {
        bind(
            &mut sql,
            " AND issued_on >= ?",
            Value::Text(date_text(filter.from)),
        );
        bind(
            &mut sql,
            " AND issued_on <= ?",
            Value::Text(date_text(filter.to)),
        );
        for token in tokens(text) {
            bind(
                &mut sql,
                " AND search_key LIKE ?",
                Value::Text(format!("%{token}%")),
            );
        }
    }
    sql.push_str(" ORDER BY issued_on DESC, id DESC");
    let mut stmt = c.raw().prepare(&sql)?;
    let rows = stmt
        .query_map(params_from_iter(values), |r| {
            Ok(OrderSummary {
                id: OrderId(r.get(0)?),
                kind: col_order_kind(r, 1)?,
                owner_name: r.get(2)?,
                owner_description: r.get(3)?,
                total: Money(r.get(4)?),
                issued_on: col_date(r, 5)?,
                updated_at: col_ts(r, 6)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// Every line of the orders issued in `from..=to` (of `kind`, or both), with its
/// order header, ordered by `issued_on, id, position` (reports, Q3).
pub fn lines_for_report(
    c: &Conn<'_>,
    from: Date,
    to: Date,
    kind: Option<OrderKind>,
) -> Result<Vec<(OrderHeader, OrderLine)>> {
    let mut stmt = c.raw().prepare(&format!(
        "SELECT {HEADER_COLUMNS}, {LINE_COLUMNS}
         FROM orders o JOIN order_lines l ON l.order_id = o.id
         WHERE o.issued_on >= ?1 AND o.issued_on <= ?2 AND (?3 IS NULL OR o.kind = ?3)
         ORDER BY o.issued_on, o.id, l.position, l.id"
    ))?;
    let rows = stmt
        .query_map(
            params![date_text(from), date_text(to), kind.map(OrderKind::as_i64)],
            |r| Ok((map_header(r)?, map_line(r, 11)?)),
        )?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

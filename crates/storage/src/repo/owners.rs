//! `owners`: clients (Klientët) and suppliers (Furnitorët).

use super::{col_owner_kind, expect_one, like_patterns, ts_text};
use crate::conn::{Conn, Tx};
use crate::error::Result;
use crm_domain::text::fold;
use crm_domain::{Owner, OwnerId, OwnerKind, time};
use rusqlite::{OptionalExtension, Row, params, params_from_iter};

const COLUMNS: &str = "id, kind, name, description, nipt, address, account_code";

fn map(row: &Row<'_>) -> rusqlite::Result<Owner> {
    Ok(Owner {
        id: OwnerId(row.get(0)?),
        kind: col_owner_kind(row, 1)?,
        name: row.get(2)?,
        description: row.get(3)?,
        nipt: row.get(4)?,
        address: row.get(5)?,
        account_code: row.get(6)?,
    })
}

fn many(c: &Conn<'_>, sql: &str, params: impl rusqlite::Params) -> Result<Vec<Owner>> {
    let mut stmt = c.raw().prepare(sql)?;
    let owners = stmt
        .query_map(params, map)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(owners)
}

/// Inserts an owner (its `id` is ignored) and returns the new id.
pub fn insert(tx: &Tx<'_>, owner: &Owner) -> Result<OwnerId> {
    let now = ts_text(time::now());
    tx.raw().execute(
        "INSERT INTO owners (kind, name, name_key, description, nipt, address, account_code,
                             created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
        params![
            owner.kind.as_i64(),
            owner.name,
            fold(&owner.name),
            owner.description,
            owner.nipt,
            owner.address,
            owner.account_code,
            now
        ],
    )?;
    Ok(OwnerId(tx.raw().last_insert_rowid()))
}

/// Overwrites every field of the owner with `owner.id`.
pub fn update(tx: &Tx<'_>, owner: &Owner) -> Result<()> {
    let changed = tx.raw().execute(
        "UPDATE owners SET kind = ?1, name = ?2, name_key = ?3, description = ?4, nipt = ?5,
                           address = ?6, account_code = ?7, updated_at = ?8
         WHERE id = ?9",
        params![
            owner.kind.as_i64(),
            owner.name,
            fold(&owner.name),
            owner.description,
            owner.nipt,
            owner.address,
            owner.account_code,
            ts_text(time::now()),
            owner.id.0
        ],
    )?;
    expect_one(changed)
}

/// Deletes an owner. Its orders keep their snapshot; `orders.owner_id` becomes NULL.
pub fn delete(tx: &Tx<'_>, id: OwnerId) -> Result<()> {
    let changed = tx
        .raw()
        .execute("DELETE FROM owners WHERE id = ?1", [id.0])?;
    expect_one(changed)
}

/// The owner with this id.
pub fn get(c: &Conn<'_>, id: OwnerId) -> Result<Owner> {
    Ok(c.raw().query_row(
        &format!("SELECT {COLUMNS} FROM owners WHERE id = ?1"),
        [id.0],
        map,
    )?)
}

/// A page of owners of `kind` whose folded name, NIPT or account code contains
/// every token of `filter` (all when blank), in byte order of the name (the old
/// `COLLATE BINARY` order: `*KLIENTE` < `1A` < `A`).
pub fn list(
    c: &Conn<'_>,
    kind: OwnerKind,
    filter: &str,
    offset: i64,
    limit: i64,
) -> Result<Vec<Owner>> {
    let patterns = like_patterns(filter);
    let mut sql = format!(
        "SELECT {COLUMNS} FROM owners WHERE kind = {}",
        kind.as_i64()
    );
    for i in 1..=patterns.len() {
        sql.push_str(&format!(
            " AND (name_key LIKE ?{i} OR lower(COALESCE(nipt, '')) LIKE ?{i}
                   OR lower(COALESCE(account_code, '')) LIKE ?{i})"
        ));
    }
    sql.push_str(&format!(
        " ORDER BY name COLLATE BINARY, id LIMIT {} OFFSET {}",
        limit.max(0),
        offset.max(0)
    ));
    many(c, &sql, params_from_iter(patterns))
}

/// The first owner (lowest id) of `kind` whose folded name equals `fold(key)`.
pub fn by_name_key(c: &Conn<'_>, kind: OwnerKind, key: &str) -> Result<Option<Owner>> {
    Ok(c.raw()
        .query_row(
            &format!(
                "SELECT {COLUMNS} FROM owners WHERE kind = ?1 AND name_key = ?2 ORDER BY id LIMIT 1"
            ),
            params![kind.as_i64(), fold(key)],
            map,
        )
        .optional()?)
}

/// The owner of `kind` with this account code ("Llogari"; unique per kind).
pub fn by_account(c: &Conn<'_>, kind: OwnerKind, code: &str) -> Result<Option<Owner>> {
    Ok(c.raw()
        .query_row(
            &format!("SELECT {COLUMNS} FROM owners WHERE kind = ?1 AND account_code = ?2"),
            params![kind.as_i64(), code],
            map,
        )
        .optional()?)
}

/// Every owner of `kind` with this NIPT, by id.
pub fn by_nipt(c: &Conn<'_>, kind: OwnerKind, nipt: &str) -> Result<Vec<Owner>> {
    many(
        c,
        &format!("SELECT {COLUMNS} FROM owners WHERE kind = ?1 AND nipt = ?2 ORDER BY id"),
        params![kind.as_i64(), nipt],
    )
}

/// Every owner, by id (for the search index).
pub fn all_for_index(c: &Conn<'_>) -> Result<Vec<Owner>> {
    many(c, &format!("SELECT {COLUMNS} FROM owners ORDER BY id"), [])
}

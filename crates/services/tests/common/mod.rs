//! Shared helpers for the services integration tests (temp-dir databases).

#![allow(dead_code)]

use crm_services::domain::{
    DraftLine, Money, OrderDraft, OrderKind, OwnerId, ProductId, ProductInput, ProductRow, Qty,
    time,
};
use crm_services::{Crm, DataPaths};
use std::path::PathBuf;
use tempfile::TempDir;

/// A facade on a fresh temp folder; keep the `TempDir` alive.
pub fn open() -> (TempDir, Crm) {
    let dir = tempfile::tempdir().unwrap();
    let crm = Crm::open(
        DataPaths::at(dir.path()),
        &crm_services::DbKey::for_tests(1),
    )
    .unwrap();
    (dir, crm)
}

/// The io fixtures (ARTIKUJ, KLIENTET, FURNITORET).
pub fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../io/tests/fixtures")
        .join(name)
}

pub fn product(crm: &Crm, code: &str, name: &str, qty: i64, buy: i64, sell: i64) -> ProductRow {
    crm.create_product(ProductInput {
        code: code.into(),
        name: name.into(),
        unit: "COPE".into(),
        buy_price: Money(buy),
        sell_price: Money(sell),
        quantity: Qty(qty),
    })
    .unwrap()
}

pub fn stock(crm: &Crm, id: ProductId) -> i64 {
    crm.get_product(id).unwrap().sku.quantity.0
}

/// A picked-product line; `price` 0 = blank.
pub fn line(id: ProductId, qty: i64, price: i64) -> DraftLine {
    DraftLine {
        product_id: Some(id),
        name: String::new(),
        quantity: Qty(qty),
        price: Money(price),
        buy_price: None,
        sell_price: None,
    }
}

/// A purchase line with buy/sell prices; `id: None` = free text `name`.
pub fn buy_line(id: Option<ProductId>, name: &str, qty: i64, buy: i64, sell: i64) -> DraftLine {
    DraftLine {
        product_id: id,
        name: name.into(),
        quantity: Qty(qty),
        price: Money(buy),
        buy_price: Some(Money(buy)),
        sell_price: Some(Money(sell)),
    }
}

pub fn draft(kind: OrderKind, owner: &str, lines: Vec<DraftLine>) -> OrderDraft {
    OrderDraft {
        kind,
        owner_id: None,
        owner_name: owner.into(),
        owner_description: String::new(),
        issued_on: time::today(),
        lines,
    }
}

pub fn draft_for(kind: OrderKind, owner: OwnerId, name: &str, lines: Vec<DraftLine>) -> OrderDraft {
    OrderDraft {
        owner_id: Some(owner),
        ..draft(kind, name, lines)
    }
}

/// Builds a plaintext root as the versions before encryption left it: `count`
/// products ("Artikull 0", …), a legacy `pin_hash` row, and one plaintext
/// backup copy of it in `backups/`. Returns the copy's path.
pub fn legacy_root(paths: &DataPaths, count: usize) -> PathBuf {
    let ts = "2026-10-14T00:00:00Z";
    let mut seed = String::from("INSERT INTO settings VALUES ('pin_hash', '$argon2id$legacy');");
    for i in 0..count {
        seed.push_str(&format!(
            "INSERT INTO skus (code, unit, buy_price, sell_price, quantity, created_at, updated_at)
             VALUES ('K{i}', 'COPE', 1, 2, 1, '{ts}', '{ts}');
             INSERT INTO products (sku_id, name, name_key, created_at, updated_at)
             VALUES (last_insert_rowid(), 'Artikull {i}', 'artikull {i}', '{ts}', '{ts}');"
        ));
    }
    crm_storage::Db::create_legacy_plaintext(paths, &seed).unwrap();
    let copy = paths.backups.join("crm-20261013-120000-manual.db");
    std::fs::copy(&paths.db, &copy).unwrap();
    copy
}

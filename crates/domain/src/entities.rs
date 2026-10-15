//! Entities (as stored) and input DTOs (as typed or drafted).

use crate::ids::{OrderId, OwnerId, ProductId, SkuId};
use crate::kind::{OrderKind, OwnerKind};
use crate::money::Money;
use crate::qty::Qty;
use crate::stock::StockLine;
use jiff::Timestamp;
use jiff::civil::Date;
use serde::{Deserialize, Serialize};

/// A stock and price pool. Several products (flavours) may share one SKU.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sku {
    /// Row id.
    pub id: SkuId,
    /// Base code, never `X/n` ("Kodi").
    pub code: String,
    /// Unit, e.g. `SHISHE` ("Njësia").
    pub unit: String,
    /// "Çmimi i blerjes".
    pub buy_price: Money,
    /// "Çmimi i shitjes".
    pub sell_price: Money,
    /// Stock on hand ("Gjendja"); never negative.
    pub quantity: Qty,
}

/// A named product ("Artikull") belonging to one SKU.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Product {
    /// Row id.
    pub id: ProductId,
    /// The SKU whose stock and prices it uses.
    pub sku_id: SkuId,
    /// "Emri".
    pub name: String,
}

/// A product joined with its SKU, as listed in the catalogue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductRow {
    /// The product.
    pub product: Product,
    /// Its SKU.
    pub sku: Sku,
}

/// A client or supplier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Owner {
    /// Row id.
    pub id: OwnerId,
    /// Klient or Furnitor.
    pub kind: OwnerKind,
    /// "Emri".
    pub name: String,
    /// "Përshkrimi".
    pub description: Option<String>,
    /// "NIPT".
    pub nipt: Option<String>,
    /// "Adresa".
    pub address: Option<String>,
    /// "Llogari" (account code from the client's other system).
    pub account_code: Option<String>,
}

/// The owner details copied onto an order when it is saved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerSnapshot {
    /// "Emri".
    pub name: String,
    /// "Përshkrimi".
    pub description: Option<String>,
    /// "NIPT".
    pub nipt: Option<String>,
    /// "Adresa".
    pub address: Option<String>,
}

/// A saved invoice ("Porosi Shitje" / "Porosi Blerje").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Order {
    /// "Nr. Porosie".
    pub id: OrderId,
    /// Shitje or Blerje.
    pub kind: OrderKind,
    /// The owner, unless it was deleted since.
    pub owner_id: Option<OwnerId>,
    /// Owner details as of the last save.
    pub owner: OwnerSnapshot,
    /// Issue date ("Data"; Q3/Q5).
    #[serde(with = "crate::serde_jiff::date")]
    pub issued_on: Date,
    /// "Totali": the sum of line values.
    pub total: Money,
    /// When the order was created.
    #[serde(with = "crate::serde_jiff::timestamp")]
    pub created_at: Timestamp,
    /// When the order was last saved ("Modifikuar").
    #[serde(with = "crate::serde_jiff::timestamp")]
    pub updated_at: Timestamp,
    /// Lines in display order.
    pub lines: Vec<OrderLine>,
}

/// A saved order line. Name, code and unit are snapshots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderLine {
    /// 0-based display position.
    pub position: i64,
    /// The product, unless it was deleted since.
    pub product_id: Option<ProductId>,
    /// The SKU whose stock moved, unless it was deleted since.
    pub sku_id: Option<SkuId>,
    /// Product name at save time.
    pub name: String,
    /// SKU code at save time.
    pub code: String,
    /// Unit at save time.
    pub unit: String,
    /// "Sasia" (≥ 1).
    pub quantity: Qty,
    /// "Çmimi" per unit.
    pub price: Money,
    /// "Vlera" = quantity × price.
    pub value: Money,
}

impl StockLine for OrderLine {
    fn sku_id(&self) -> Option<SkuId> {
        self.sku_id
    }
    fn product_name(&self) -> &str {
        &self.name
    }
    fn quantity(&self) -> Qty {
        self.quantity
    }
}

/// An order being edited (persisted as JSON in `open_tabs.draft_json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderDraft {
    /// Shitje or Blerje.
    pub kind: OrderKind,
    /// The selected owner, if one was picked from the list.
    pub owner_id: Option<OwnerId>,
    /// Typed owner name.
    pub owner_name: String,
    /// Typed owner description ("Përshkrimi").
    pub owner_description: String,
    /// Issue date ("Data").
    #[serde(with = "crate::serde_jiff::date")]
    pub issued_on: Date,
    /// Lines, including empty placeholder rows.
    pub lines: Vec<DraftLine>,
}

/// One line of an [`OrderDraft`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftLine {
    /// The picked product; `None` for a free-text purchase line.
    pub product_id: Option<ProductId>,
    /// Product name (shown, or typed for a free-text purchase line).
    pub name: String,
    /// "Sasia".
    pub quantity: Qty,
    /// "Çmimi" per unit (sale: defaults to the sell price; purchase: the buy price).
    pub price: Money,
    /// Purchase: the SKU buy price to set (Q11).
    pub buy_price: Option<Money>,
    /// Purchase: the SKU sell price to set (Q11).
    pub sell_price: Option<Money>,
}

impl DraftLine {
    /// An untouched placeholder row: no product, no name, quantity 0.
    pub fn is_placeholder(&self) -> bool {
        self.product_id.is_none() && self.name.trim().is_empty() && self.quantity == Qty::ZERO
    }
}

/// Owner fields as typed in the property dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerInput {
    /// Klient or Furnitor.
    pub kind: OwnerKind,
    /// "Emri" (required, ≤ 50).
    pub name: String,
    /// "Përshkrimi" (≤ 255).
    pub description: String,
    /// "NIPT" (≤ 20).
    pub nipt: String,
    /// "Adresa" (≤ 255).
    pub address: String,
    /// "Llogari".
    pub account_code: String,
}

/// Product and SKU fields as typed in the property dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductInput {
    /// "Kodi" (≤ 50); empty means "generate the next `A####`".
    pub code: String,
    /// "Emri" (required, ≤ 255).
    pub name: String,
    /// "Njësia" (required, ≤ 20).
    pub unit: String,
    /// "Çmimi i blerjes" (≥ 0).
    pub buy_price: Money,
    /// "Çmimi i shitjes" (≥ 0).
    pub sell_price: Money,
    /// "Gjendja" (≥ 0).
    pub quantity: Qty,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_value;
    use jiff::civil::date;

    fn draft() -> OrderDraft {
        OrderDraft {
            kind: OrderKind::Purchase,
            owner_id: Some(OwnerId(4)),
            owner_name: "Furnitori Alfa".into(),
            owner_description: "Tiranë".into(),
            issued_on: date(2026, 10, 14),
            lines: vec![
                DraftLine {
                    product_id: Some(ProductId(1)),
                    name: "BRIZO PJESHKË".into(),
                    quantity: Qty(3),
                    price: Money(6_200),
                    buy_price: Some(Money(6_200)),
                    sell_price: Some(Money(6_500)),
                },
                DraftLine::default(),
            ],
        }
    }

    #[test]
    fn draft_serde_round_trip() {
        let original = draft();
        let value = test_value::to_value(&original);
        let text = format!("{value:?}");
        assert!(
            text.contains(r#"("issued_on", Str("2026-10-14"))"#),
            "{text}"
        );
        assert!(text.contains(r#"("kind", I64(1))"#), "{text}");
        assert!(text.contains(r#"("price", I64(6200))"#), "{text}");
        let back: OrderDraft = test_value::from_value(value);
        assert_eq!(back, original);
    }

    #[test]
    fn order_serde_round_trip() {
        let order = Order {
            id: OrderId(12),
            kind: OrderKind::Sale,
            owner_id: None,
            owner: OwnerSnapshot {
                name: "Klienti".into(),
                description: None,
                nipt: Some("M02317046H".into()),
                address: None,
            },
            issued_on: date(2026, 1, 31),
            total: Money(13_000),
            created_at: "2026-01-31T09:00:00Z".parse().unwrap(),
            updated_at: "2026-02-01T10:15:30Z".parse().unwrap(),
            lines: vec![OrderLine {
                position: 0,
                product_id: Some(ProductId(1)),
                sku_id: Some(SkuId(2)),
                name: "BRIZO".into(),
                code: "L0009".into(),
                unit: "COPE".into(),
                quantity: Qty(2),
                price: Money(6_500),
                value: Money(13_000),
            }],
        };
        let value = test_value::to_value(&order);
        assert!(format!("{value:?}").contains(r#"Str("2026-02-01T10:15:30Z")"#));
        assert_eq!(test_value::from_value::<Order>(value), order);
    }

    #[test]
    fn placeholder_rows() {
        assert!(DraftLine::default().is_placeholder());
        let named = DraftLine {
            name: "x".into(),
            ..DraftLine::default()
        };
        assert!(!named.is_placeholder());
        let counted = DraftLine {
            quantity: Qty(1),
            ..DraftLine::default()
        };
        assert!(!counted.is_placeholder());
    }
}

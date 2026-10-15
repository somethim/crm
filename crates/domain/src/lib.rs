//! Domain types, business rules and formatting for the CRM.
//!
//! Boundary: pure logic with no IO. Depends only on `thiserror`, `rust_decimal`,
//! `jiff` and `serde`. Every other crate depends on this one; it depends on no
//! other workspace crate. Money is integer qindarka, quantities are integers, and
//! all user-visible formatting (`1.234,56 Lekë`, `dd.MM.yyyy`) lives here.
//!
//! Modules:
//! - [`ids`], [`money`], [`qty`], [`kind`], [`entities`]: core types;
//! - [`sku`], [`validate`]: SKU code helpers and input validation;
//! - [`stock`]: the per-SKU stock delta math (edit/delete reversal, Q6);
//! - [`text`], [`format`](mod@format), [`time`]: folding, display/parse formatters, clock;
//! - [`messages`]: every Albanian user-facing string.

#![warn(missing_docs)]

pub mod entities;
pub mod format;
pub mod ids;
pub mod kind;
pub mod messages;
pub mod money;
pub mod qty;
mod serde_jiff;
pub mod sku;
pub mod stock;
#[cfg(test)]
mod test_value;
pub mod text;
pub mod time;
pub mod validate;

pub use entities::{
    DraftLine, Order, OrderDraft, OrderLine, Owner, OwnerInput, OwnerSnapshot, Product,
    ProductInput, ProductRow, Sku,
};
pub use format::ParseError;
pub use ids::{OrderId, OwnerId, ProductId, SkuId};
pub use kind::{KindError, OrderKind, OwnerKind};
pub use money::{Money, ParseMode};
pub use qty::Qty;
pub use stock::{LineEffect, PurchaseAction, StockError, StockLine};
pub use validate::{FieldError, ValidationErrors};

/// Unit given to SKUs created without one (old `config/crm.php` `default_unit`).
pub const DEFAULT_UNIT: &str = "SHISHE";

/// Prefix of automatically generated SKU codes (`A0001`, `A0002`, …; A10).
pub const AUTO_CODE_PREFIX: &str = "A";

/// Field length limits in characters (001 A9).
pub mod limits {
    /// Owner (client/supplier) name.
    pub const OWNER_NAME: usize = 50;
    /// Owner description.
    pub const OWNER_DESCRIPTION: usize = 255;
    /// Owner address.
    pub const OWNER_ADDRESS: usize = 255;
    /// Owner NIPT (tax id).
    pub const OWNER_NIPT: usize = 20;
    /// Product name.
    pub const PRODUCT_NAME: usize = 255;
    /// SKU code.
    pub const SKU_CODE: usize = 50;
    /// SKU unit.
    pub const SKU_UNIT: usize = 20;
}

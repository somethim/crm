//! Order and owner kinds, stored as `0`/`1` in SQLite and in draft JSON.

use crate::messages;
use serde::{Deserialize, Serialize};

/// A kind number that is neither 0 nor 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{}", messages::unknown_kind(*.0))]
pub struct KindError(pub i64);

impl KindError {
    /// The Albanian message shown to the user.
    pub fn user_message(&self) -> String {
        self.to_string()
    }
}

/// The kind of an invoice: a sale to a client or a purchase from a supplier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "i64", into = "i64")]
pub enum OrderKind {
    /// "Shitje": stock goes out.
    Sale = 0,
    /// "Blerje": stock comes in.
    Purchase = 1,
}

impl OrderKind {
    /// Both kinds, in storage order.
    pub const ALL: [OrderKind; 2] = [OrderKind::Sale, OrderKind::Purchase];

    /// Label: `Shitje` / `Blerje` (Q10); also the report's Lloji column.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Sale => messages::SALE,
            Self::Purchase => messages::PURCHASE,
        }
    }

    /// Document title: `Porosi Shitje` / `Porosi Blerje`.
    pub const fn document_title(self) -> &'static str {
        match self {
            Self::Sale => messages::SALE_DOCUMENT,
            Self::Purchase => messages::PURCHASE_DOCUMENT,
        }
    }

    /// Title of a new, unsaved tab: `Porosi Shitje (e re)` / `Porosi Blerje (e re)`.
    pub const fn new_tab_title(self) -> &'static str {
        match self {
            Self::Sale => messages::SALE_NEW_TAB,
            Self::Purchase => messages::PURCHASE_NEW_TAB,
        }
    }

    /// The owner kind on the other side: clients buy, suppliers sell.
    pub const fn owner_kind(self) -> OwnerKind {
        match self {
            Self::Sale => OwnerKind::Client,
            Self::Purchase => OwnerKind::Supplier,
        }
    }

    /// The stored number (`0` / `1`).
    pub const fn as_i64(self) -> i64 {
        self as i64
    }
}

impl TryFrom<i64> for OrderKind {
    type Error = KindError;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Sale),
            1 => Ok(Self::Purchase),
            other => Err(KindError(other)),
        }
    }
}

impl From<OrderKind> for i64 {
    fn from(kind: OrderKind) -> Self {
        kind.as_i64()
    }
}

/// The kind of an owner: a client (buys) or a supplier (sells).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "i64", into = "i64")]
pub enum OwnerKind {
    /// "Klient".
    Client = 0,
    /// "Furnitor".
    Supplier = 1,
}

impl OwnerKind {
    /// Both kinds, in storage order.
    pub const ALL: [OwnerKind; 2] = [OwnerKind::Client, OwnerKind::Supplier];

    /// Label: `Klient` / `Furnitor`.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Client => messages::CLIENT,
            Self::Supplier => messages::SUPPLIER,
        }
    }

    /// Plural: `Klientët` / `Furnitorët`.
    pub const fn plural(self) -> &'static str {
        match self {
            Self::Client => messages::CLIENTS,
            Self::Supplier => messages::SUPPLIERS,
        }
    }

    /// The order kind this owner takes part in.
    pub const fn order_kind(self) -> OrderKind {
        match self {
            Self::Client => OrderKind::Sale,
            Self::Supplier => OrderKind::Purchase,
        }
    }

    /// The stored number (`0` / `1`).
    pub const fn as_i64(self) -> i64 {
        self as i64
    }
}

impl TryFrom<i64> for OwnerKind {
    type Error = KindError;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Client),
            1 => Ok(Self::Supplier),
            other => Err(KindError(other)),
        }
    }
}

impl From<OwnerKind> for i64 {
    fn from(kind: OwnerKind) -> Self {
        kind.as_i64()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_kind_texts() {
        assert_eq!(OrderKind::Sale.label(), "Shitje");
        assert_eq!(OrderKind::Purchase.label(), "Blerje");
        assert_eq!(OrderKind::Sale.document_title(), "Porosi Shitje");
        assert_eq!(OrderKind::Purchase.document_title(), "Porosi Blerje");
        assert_eq!(OrderKind::Sale.new_tab_title(), "Porosi Shitje (e re)");
        assert_eq!(OrderKind::Purchase.new_tab_title(), "Porosi Blerje (e re)");
        assert_eq!(OrderKind::Sale.owner_kind(), OwnerKind::Client);
        assert_eq!(OrderKind::Purchase.owner_kind(), OwnerKind::Supplier);
    }

    #[test]
    fn owner_kind_texts() {
        assert_eq!(OwnerKind::Client.label(), "Klient");
        assert_eq!(OwnerKind::Supplier.label(), "Furnitor");
        assert_eq!(OwnerKind::Client.plural(), "Klientët");
        assert_eq!(OwnerKind::Supplier.plural(), "Furnitorët");
        assert_eq!(OwnerKind::Supplier.order_kind(), OrderKind::Purchase);
    }

    #[test]
    fn kinds_round_trip_through_i64() {
        for kind in OrderKind::ALL {
            assert_eq!(OrderKind::try_from(kind.as_i64()), Ok(kind));
        }
        for kind in OwnerKind::ALL {
            assert_eq!(OwnerKind::try_from(i64::from(kind)), Ok(kind));
        }
        assert_eq!(OrderKind::try_from(2), Err(KindError(2)));
        assert_eq!(OwnerKind::try_from(-1), Err(KindError(-1)));
        assert_eq!(KindError(2).user_message(), "Lloj i panjohur: 2.");
    }
}

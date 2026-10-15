//! The documents the index is built from.

use crm_domain::{Owner, OwnerId, OwnerKind, ProductId};

/// A product as the index sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductDoc {
    /// Row id.
    pub id: ProductId,
    /// "Emri"; matched with typo tolerance.
    pub name: String,
    /// The SKU code ("Kodi", e.g. `L0009/1`); matched exact or prefix only.
    pub code: String,
    /// Number of distinct orders that contain the product (popularity tiebreak).
    pub order_count: u32,
}

/// A client or supplier as the index sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerDoc {
    /// Row id.
    pub id: OwnerId,
    /// Klient or Furnitor; searches run within one kind.
    pub kind: OwnerKind,
    /// "Emri"; matched with typo tolerance.
    pub name: String,
    /// "Përshkrimi"; matched with typo tolerance.
    pub description: Option<String>,
    /// "NIPT"; matched trimmed and case-insensitively, exact or prefix only.
    pub nipt: Option<String>,
    /// "Adresa"; matched with typo tolerance.
    pub address: Option<String>,
}

impl From<&Owner> for OwnerDoc {
    fn from(owner: &Owner) -> Self {
        Self {
            id: owner.id,
            kind: owner.kind,
            name: owner.name.clone(),
            description: owner.description.clone(),
            nipt: owner.nipt.clone(),
            address: owner.address.clone(),
        }
    }
}

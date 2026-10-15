//! Index structures: interned, pre-folded tokens per document.

use std::collections::HashMap;
use std::hash::Hash;

use crm_domain::text::tokens;
use crm_domain::{OwnerId, OwnerKind, ProductId};

use crate::doc::{OwnerDoc, ProductDoc};

/// Interned folded tokens of one collection. Each distinct token is scored once
/// per query token, however many documents contain it.
#[derive(Debug, Clone, Default)]
pub(crate) struct Vocab {
    ids: HashMap<String, u32>,
    pub(crate) tokens: Vec<String>,
    /// Whether the token occurs in a typo-tolerant field (else typo matching is skipped).
    pub(crate) fuzzy: Vec<bool>,
}

impl Vocab {
    fn intern(&mut self, token: String, fuzzy: bool) -> u32 {
        if let Some(&id) = self.ids.get(&token) {
            self.fuzzy[id as usize] |= fuzzy;
            return id;
        }
        let id = u32::try_from(self.tokens.len()).expect("fewer than 2^32 distinct tokens");
        self.ids.insert(token.clone(), id);
        self.tokens.push(token);
        self.fuzzy.push(fuzzy);
        id
    }
}

/// One indexed document.
#[derive(Debug, Clone)]
pub(crate) struct Entry<Id, M> {
    pub(crate) id: Id,
    /// The original name; the last tiebreak (byte order).
    pub(crate) name: String,
    /// Tokens matched exact, prefix or with typos (names, descriptions, addresses).
    pub(crate) fuzzy: Vec<u32>,
    /// Tokens matched exact or prefix only (codes, NIPTs).
    pub(crate) exact: Vec<u32>,
    /// `order_count` for products, the kind for owners.
    pub(crate) meta: M,
}

/// The documents of one type, addressable by id.
#[derive(Debug, Clone)]
pub(crate) struct Collection<Id, M> {
    pub(crate) vocab: Vocab,
    pub(crate) entries: Vec<Entry<Id, M>>,
    pos: HashMap<Id, usize>,
}

impl<Id, M> Default for Collection<Id, M> {
    fn default() -> Self {
        Self {
            vocab: Vocab::default(),
            entries: Vec::new(),
            pos: HashMap::new(),
        }
    }
}

impl<Id: Copy + Eq + Hash, M> Collection<Id, M> {
    fn upsert(&mut self, id: Id, name: String, fuzzy: &[&str], exact: &[&str], meta: M) {
        let mut intern = |texts: &[&str], is_fuzzy: bool| {
            let mut ids: Vec<u32> = texts
                .iter()
                .flat_map(|text| tokens(text))
                .map(|t| self.vocab.intern(t, is_fuzzy))
                .collect();
            ids.sort_unstable();
            ids.dedup();
            ids
        };
        let entry = Entry {
            id,
            fuzzy: intern(fuzzy, true),
            exact: intern(exact, false),
            name,
            meta,
        };
        match self.pos.get(&id) {
            Some(&i) => self.entries[i] = entry,
            None => {
                self.pos.insert(id, self.entries.len());
                self.entries.push(entry);
            }
        }
    }

    fn remove(&mut self, id: Id) -> bool {
        let Some(i) = self.pos.remove(&id) else {
            return false;
        };
        self.entries.swap_remove(i);
        if let Some(moved) = self.entries.get(i) {
            self.pos.insert(moved.id, i);
        }
        true
    }
}

/// The in-memory search index over products and owners.
///
/// Every field is stored as folded tokens ([`crm_domain::text::tokens`]), so
/// `pjeshkë`, `PJESHKE` and `pjeshke` match alike. It is `Send + Sync`; wrap it
/// in `Arc<RwLock<_>>` to share it. Tokens of removed or replaced documents stay
/// interned until the next [`SearchIndex::build`]; they cost a little scoring
/// time but never match a document.
#[derive(Debug, Clone, Default)]
pub struct SearchIndex {
    pub(crate) products: Collection<ProductId, u32>,
    pub(crate) owners: Collection<OwnerId, OwnerKind>,
}

impl SearchIndex {
    /// An empty index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds the index from all products and owners. A later doc with the same
    /// id replaces an earlier one.
    pub fn build(
        products: impl IntoIterator<Item = ProductDoc>,
        owners: impl IntoIterator<Item = OwnerDoc>,
    ) -> Self {
        let mut index = Self::new();
        for doc in products {
            index.upsert_product(doc);
        }
        for doc in owners {
            index.upsert_owner(doc);
        }
        index
    }

    /// Adds the product, or replaces the one with the same id.
    pub fn upsert_product(&mut self, doc: ProductDoc) {
        let ProductDoc {
            id,
            name,
            code,
            order_count,
        } = doc;
        let fuzzy = [name.as_str()];
        self.products
            .upsert(id, name.clone(), &fuzzy, &[code.as_str()], order_count);
    }

    /// Removes the product; `false` when it was not indexed.
    pub fn remove_product(&mut self, id: ProductId) -> bool {
        self.products.remove(id)
    }

    /// Adds the owner, or replaces the one with the same id (its kind may change).
    pub fn upsert_owner(&mut self, doc: OwnerDoc) {
        let OwnerDoc {
            id,
            kind,
            name,
            description,
            nipt,
            address,
        } = doc;
        let fuzzy = [
            Some(name.as_str()),
            description.as_deref(),
            address.as_deref(),
        ];
        let fuzzy: Vec<&str> = fuzzy.into_iter().flatten().collect();
        let nipt: Vec<&str> = nipt.as_deref().map(str::trim).into_iter().collect();
        self.owners.upsert(id, name.clone(), &fuzzy, &nipt, kind);
    }

    /// Removes the owner; `false` when it was not indexed.
    pub fn remove_owner(&mut self, id: OwnerId) -> bool {
        self.owners.remove(id)
    }

    /// Replaces every product's `order_count`. The counts are derived wholesale,
    /// so products missing from `counts` get 0.
    pub fn set_order_counts(&mut self, counts: HashMap<ProductId, u32>) {
        for entry in &mut self.products.entries {
            entry.meta = counts.get(&entry.id).copied().unwrap_or(0);
        }
    }

    /// Number of indexed products.
    pub fn product_count(&self) -> usize {
        self.products.entries.len()
    }

    /// Number of indexed owners (both kinds).
    pub fn owner_count(&self) -> usize {
        self.owners.entries.len()
    }

    /// Whether no product and no owner is indexed.
    pub fn is_empty(&self) -> bool {
        self.product_count() == 0 && self.owner_count() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn product(id: i64, name: &str, code: &str) -> ProductDoc {
        ProductDoc {
            id: ProductId(id),
            name: name.to_owned(),
            code: code.to_owned(),
            order_count: 0,
        }
    }

    fn owner(id: i64, kind: OwnerKind, name: &str) -> OwnerDoc {
        OwnerDoc {
            id: OwnerId(id),
            kind,
            name: name.to_owned(),
            description: None,
            nipt: None,
            address: None,
        }
    }

    fn token_texts<Id, M>(c: &Collection<Id, M>, ids: &[u32]) -> Vec<String> {
        ids.iter()
            .map(|&i| c.vocab.tokens[i as usize].clone())
            .collect()
    }

    #[test]
    fn index_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SearchIndex>();
    }

    #[test]
    fn build_folds_and_interns_tokens() {
        let index = SearchIndex::build(
            [
                product(1, "BRIZO PJESHKË 24*0.33LIT", "L0009/1"),
                product(2, "BRIZO MOLLE", "L0009/2"),
            ],
            [OwnerDoc {
                nipt: Some("K47120593B ".to_owned()),
                address: Some("Livadhet, Kamëz".to_owned()),
                ..owner(7, OwnerKind::Client, "3 YJE")
            }],
        );
        assert_eq!(index.product_count(), 2);
        assert_eq!(index.owner_count(), 1);
        assert!(!index.is_empty());
        let p = &index.products.entries[0];
        let mut fuzzy = token_texts(&index.products, &p.fuzzy);
        fuzzy.sort();
        assert_eq!(fuzzy, ["0", "24", "33lit", "brizo", "pjeshke"]);
        assert_eq!(token_texts(&index.products, &p.exact), ["l0009", "1"]);
        // "brizo" is interned once for both products.
        assert_eq!(index.products.vocab.tokens.len(), 9);
        let o = &index.owners.entries[0];
        assert_eq!(token_texts(&index.owners, &o.exact), ["k47120593b"]);
        assert_eq!(o.meta, OwnerKind::Client);
    }

    #[test]
    fn upsert_replaces_and_remove_keeps_positions() {
        let mut index = SearchIndex::new();
        assert!(index.is_empty());
        for i in 1..=3 {
            index.upsert_product(product(i, &format!("P{i}"), &format!("A000{i}")));
        }
        index.upsert_product(ProductDoc {
            order_count: 5,
            ..product(2, "RENAMED", "A0002")
        });
        assert_eq!(index.product_count(), 3);
        assert_eq!(index.products.entries[1].name, "RENAMED");
        assert_eq!(index.products.entries[1].meta, 5);

        assert!(index.remove_product(ProductId(1)));
        assert!(!index.remove_product(ProductId(1)));
        assert_eq!(index.product_count(), 2);
        // The swapped-in entry is still addressable.
        index.upsert_product(product(3, "THREE", "A0003"));
        assert_eq!(index.product_count(), 2);
        assert!(index.products.entries.iter().any(|e| e.name == "THREE"));
        assert!(index.remove_product(ProductId(3)));
        assert!(index.remove_product(ProductId(2)));
        assert_eq!(index.product_count(), 0);

        index.upsert_owner(owner(1, OwnerKind::Client, "X"));
        index.upsert_owner(owner(1, OwnerKind::Supplier, "X"));
        assert_eq!(index.owner_count(), 1);
        assert_eq!(index.owners.entries[0].meta, OwnerKind::Supplier);
        assert!(index.remove_owner(OwnerId(1)));
        assert!(!index.remove_owner(OwnerId(1)));
        assert!(index.is_empty());
    }

    #[test]
    fn set_order_counts_resets_missing_products() {
        let mut index = SearchIndex::build(
            [
                ProductDoc {
                    order_count: 9,
                    ..product(1, "A", "A0001")
                },
                product(2, "B", "A0002"),
            ],
            [],
        );
        index.set_order_counts(HashMap::from([(ProductId(2), 4)]));
        let counts: Vec<u32> = index.products.entries.iter().map(|e| e.meta).collect();
        assert_eq!(counts, [0, 4]);
    }
}

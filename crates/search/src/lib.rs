//! In-memory fuzzy search index for products and owners.
//!
//! Boundary: depends on `crm-domain` only and performs no IO. The index is built
//! from domain values handed to it by `crm-services`.
//!
//! - [`SearchIndex`] holds pre-folded tokens ([`crm_domain::text::tokens`]) for
//!   products ([`ProductDoc`]) and owners ([`OwnerDoc`]). It is `Send + Sync`;
//!   the services layer wraps it in `Arc<RwLock<_>>`.
//! - Matching is per query token: exact (3), prefix (2) or a typo match (1, up to
//!   two edits for long tokens). Product codes and NIPTs match exact or prefix only.
//! - Ranking is by match quality, then `order_count`, then name in byte order.
//!   When fewer than 10 candidates match every token, candidates matching fewer
//!   tokens are appended (the rightmost token is dropped first).
//! - [`classify_query`] recognises a digits-only query as an invoice number.

#![warn(missing_docs)]

mod doc;
mod index;
mod matcher;
mod query;
mod rank;

pub use doc::{OwnerDoc, ProductDoc};
pub use index::SearchIndex;
pub use query::{Query, classify_query};
pub use rank::DROP_TOKENS_THRESHOLD;

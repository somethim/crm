//! Candidate scoring, ordering and token dropping.

use std::cmp::Ordering;
use std::hash::Hash;

use crm_domain::text::tokens;
use crm_domain::{OwnerId, OwnerKind, ProductId};

use crate::index::{Collection, Entry, SearchIndex};
use crate::matcher::{PREFIX, QueryToken};

/// When fewer candidates than this match every query token, candidates that
/// match fewer tokens are appended (old Typesense `drop_tokens_threshold: 10`).
pub const DROP_TOKENS_THRESHOLD: usize = 10;

/// Query tokens beyond this many are ignored (one bit each in [`Scored::mask`]).
const MAX_QUERY_TOKENS: usize = 64;

/// A candidate's rank key, compared descending field by field.
#[derive(Debug, Clone, Copy)]
struct Scored<'a, Id> {
    /// How many query tokens matched.
    matched: u32,
    /// Bit `63 - i` is set when query token `i` matched, so among candidates
    /// that match the same number of tokens, those missing the rightmost
    /// tokens come first ("rightmost token dropped first").
    mask: u64,
    /// Sum of the per-token scores (3 exact, 2 prefix, 1 typo).
    sum: u32,
    /// `order_count` for products, 0 for owners.
    popularity: u32,
    /// Ascending, in byte order.
    name: &'a str,
    /// Ascending; makes the order total.
    id: Id,
}

fn rank_order<Id: Ord>(a: &Scored<'_, Id>, b: &Scored<'_, Id>) -> Ordering {
    b.matched
        .cmp(&a.matched)
        .then(b.mask.cmp(&a.mask))
        .then(b.sum.cmp(&a.sum))
        .then(b.popularity.cmp(&a.popularity))
        .then(a.name.cmp(b.name))
        .then(a.id.cmp(&b.id))
}

/// Folded query tokens; empty for a blank query.
fn query_tokens(query: &str) -> Vec<QueryToken> {
    tokens(query)
        .into_iter()
        .take(MAX_QUERY_TOKENS)
        .map(QueryToken::new)
        .collect()
}

/// Scores every candidate that matches at least one query token, sorts them,
/// keeps the all-token matches and, while fewer than [`DROP_TOKENS_THRESHOLD`]
/// are kept, appends each next level (one token fewer), then cuts to `limit`.
/// Also returns how many were kept before that cut (the total match count).
fn search<Id, M>(
    collection: &Collection<Id, M>,
    query: &[QueryToken],
    limit: usize,
    keep: impl Fn(&Entry<Id, M>) -> bool,
    popularity: impl Fn(&M) -> u32,
) -> (Vec<Id>, usize)
where
    Id: Copy + Ord + Hash,
{
    let vocab = &collection.vocab;
    // scores[i][v]: query token i against vocabulary token v.
    let scores: Vec<Vec<u8>> = query
        .iter()
        .map(|q| {
            vocab
                .tokens
                .iter()
                .zip(&vocab.fuzzy)
                .map(|(t, &fuzzy)| {
                    if fuzzy {
                        q.score_fuzzy(t)
                    } else {
                        q.score_exact(t)
                    }
                })
                .collect()
        })
        .collect();

    let mut candidates: Vec<Scored<'_, Id>> = collection
        .entries
        .iter()
        .filter(|e| keep(e))
        .filter_map(|e| {
            let mut scored = Scored {
                matched: 0,
                mask: 0,
                sum: 0,
                popularity: popularity(&e.meta),
                name: &e.name,
                id: e.id,
            };
            for (i, row) in scores.iter().enumerate() {
                let fuzzy = e.fuzzy.iter().map(|&v| row[v as usize]);
                // Typo scores never count for exact-only fields.
                let exact = e
                    .exact
                    .iter()
                    .map(|&v| row[v as usize])
                    .filter(|&s| s >= PREFIX);
                let best = fuzzy.chain(exact).max().unwrap_or(0);
                if best > 0 {
                    scored.matched += 1;
                    scored.mask |= 1 << (63 - i);
                    scored.sum += u32::from(best);
                }
            }
            (scored.matched > 0).then_some(scored)
        })
        .collect();
    candidates.sort_unstable_by(rank_order);

    let all = u32::try_from(query.len()).unwrap_or(u32::MAX);
    let mut kept = 0;
    let mut level = all;
    for (i, c) in candidates.iter().enumerate() {
        if c.matched != level {
            // A new, lower level starts: append it only below the threshold.
            if i >= DROP_TOKENS_THRESHOLD {
                break;
            }
            level = c.matched;
        }
        kept = i + 1;
    }
    let ids = candidates
        .iter()
        .take(kept.min(limit))
        .map(|c| c.id)
        .collect();
    (ids, kept)
}

impl SearchIndex {
    /// Products ranked for `query`, at most `limit`.
    ///
    /// Each query token matches a name token exact (3), as a prefix (2) or
    /// within the allowed typos (1), or a code token exact or as a prefix.
    /// Order: matched tokens, then (for partial matches) leftmost tokens
    /// matched, then summed scores, then `order_count` descending, then name
    /// in byte order. Partial matches are appended only while fewer than
    /// [`DROP_TOKENS_THRESHOLD`] results are listed. A blank query lists every product
    /// by `order_count` descending, then name.
    pub fn search_products(&self, query: &str, limit: usize) -> Vec<ProductId> {
        self.search_products_counted(query, limit).0
    }

    /// [`SearchIndex::search_products`], plus the number of products that
    /// match before the `limit` cut ("50/124 sugjerime"). For a blank query
    /// that is every product.
    pub fn search_products_counted(&self, query: &str, limit: usize) -> (Vec<ProductId>, usize) {
        let query = query_tokens(query);
        if query.is_empty() {
            let mut all: Vec<&Entry<ProductId, u32>> = self.products.entries.iter().collect();
            all.sort_unstable_by(|a, b| {
                b.meta
                    .cmp(&a.meta)
                    .then(a.name.cmp(&b.name))
                    .then(a.id.cmp(&b.id))
            });
            let total = all.len();
            return (all.into_iter().take(limit).map(|e| e.id).collect(), total);
        }
        search(&self.products, &query, limit, |_| true, |&count| count)
    }

    /// Owners of `kind` ranked for `query`, at most `limit`.
    ///
    /// Name, description and address tokens match like product names; the
    /// NIPT (trimmed, case-insensitive) matches exact or as a prefix only.
    /// A blank query lists every owner of `kind` by name in byte order
    /// (special characters, then digits, then letters).
    pub fn search_owners(&self, kind: OwnerKind, query: &str, limit: usize) -> Vec<OwnerId> {
        self.search_owners_counted(kind, query, limit).0
    }

    /// [`SearchIndex::search_owners`], plus the number of owners of `kind`
    /// that match before the `limit` cut. For a blank query that is every
    /// owner of `kind`.
    pub fn search_owners_counted(
        &self,
        kind: OwnerKind,
        query: &str,
        limit: usize,
    ) -> (Vec<OwnerId>, usize) {
        let query = query_tokens(query);
        if query.is_empty() {
            let mut all: Vec<&Entry<OwnerId, OwnerKind>> = self
                .owners
                .entries
                .iter()
                .filter(|e| e.meta == kind)
                .collect();
            all.sort_unstable_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
            let total = all.len();
            return (all.into_iter().take(limit).map(|e| e.id).collect(), total);
        }
        search(&self.owners, &query, limit, |e| e.meta == kind, |_| 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OwnerDoc, ProductDoc};

    fn product(id: i64, name: &str, order_count: u32) -> ProductDoc {
        ProductDoc {
            id: ProductId(id),
            name: name.to_owned(),
            code: format!("A{id:04}"),
            order_count,
        }
    }

    fn ids(found: Vec<ProductId>) -> Vec<i64> {
        found.into_iter().map(ProductId::get).collect()
    }

    #[test]
    fn relevance_then_popularity_then_name() {
        // Port of ProductPopularityTest "ranks by relevance then popularity".
        let index = SearchIndex::build(
            [
                product(1, "COCA COLA 0.5", 1),
                product(2, "COCA COLA ZERO", 7),
                product(3, "COLA FANTA", 3),
                product(4, "COCA LIGHT", 7),
                product(5, "COCA", 0),
            ],
            [],
        );
        // "coca": exact for 2, 4, 1, 5 (popularity 7, 7, 1, 0; 2 and 4 by
        // name), then the one-typo "cola" of 3.
        assert_eq!(ids(index.search_products("coca", 10)), [2, 4, 1, 5, 3]);
        // Relevance beats popularity: exact "cola" (2, 3, 1) before the
        // one-typo "coca" of 4 (7 orders) and 5.
        assert_eq!(ids(index.search_products("cola", 10)), [2, 3, 1, 4, 5]);
        // Exact beats prefix even with fewer orders.
        assert_eq!(ids(index.search_products("coca zero", 10))[0], 2);
    }

    #[test]
    fn order_counts_change_the_ranking() {
        let mut index = SearchIndex::build([product(1, "UJE A", 0), product(2, "UJE B", 0)], []);
        assert_eq!(ids(index.search_products("uje", 10)), [1, 2]);
        index.set_order_counts([(ProductId(2), 3)].into());
        assert_eq!(ids(index.search_products("uje", 10)), [2, 1]);
    }

    #[test]
    fn empty_query_lists_by_popularity_then_name() {
        let index = SearchIndex::build(
            [product(1, "B", 2), product(2, "A", 2), product(3, "C", 5)],
            [],
        );
        assert_eq!(ids(index.search_products("  ", 10)), [3, 2, 1]);
        assert_eq!(ids(index.search_products("", 2)), [3, 2]);
        assert_eq!(ids(index.search_products("***", 10)), [3, 2, 1]);
    }

    #[test]
    fn tokens_are_dropped_rightmost_first_below_the_threshold() {
        let index = SearchIndex::build(
            [
                product(1, "MOLLE PJESHKE", 0),
                product(2, "MOLLE", 9),
                product(3, "PJESHKE", 9),
                product(4, "DARDHE", 9),
            ],
            [],
        );
        // One full match, then "molle" (rightmost dropped), then "pjeshke".
        assert_eq!(ids(index.search_products("molle pjeshke", 10)), [1, 2, 3]);
        assert_eq!(ids(index.search_products("molle pjeshke", 2)), [1, 2]);
    }

    #[test]
    fn no_dropping_at_or_above_the_threshold() {
        let mut docs: Vec<ProductDoc> = (1..=10)
            .map(|i| product(i, &format!("MOLLE PJESHKE {i}"), 0))
            .collect();
        docs.push(product(11, "MOLLE", 99));
        let index = SearchIndex::build(docs, []);
        let found = ids(index.search_products("molle pjeshke", 50));
        assert_eq!(found.len(), 10);
        assert!(!found.contains(&11));
        // Nine full matches leave room for the dropped level.
        let index = SearchIndex::build(
            (2..=10)
                .map(|i| product(i, &format!("MOLLE PJESHKE {i}"), 0))
                .chain([product(11, "MOLLE", 99)]),
            [],
        );
        assert_eq!(index.search_products("molle pjeshke", 50).len(), 10);
    }

    #[test]
    fn codes_match_exact_or_prefix_only() {
        let index = SearchIndex::build(
            [
                ProductDoc {
                    code: "L0009".to_owned(),
                    ..product(1, "BRIZO", 0)
                },
                ProductDoc {
                    code: "L0009/1".to_owned(),
                    ..product(2, "BRIZO PJESHKE", 0)
                },
                ProductDoc {
                    code: "L0019".to_owned(),
                    ..product(3, "SUKO", 0)
                },
            ],
            [],
        );
        assert_eq!(ids(index.search_products("L0009", 10)), [1, 2]);
        assert_eq!(ids(index.search_products("l0009/1", 10))[0], 2);
        assert_eq!(ids(index.search_products("L001", 10)), [3]);
        // One edit away from L0009 is not a code match.
        assert!(index.search_products("L0008", 10).is_empty());
    }

    #[test]
    fn owners_filter_by_kind_and_sort_by_name_when_empty() {
        let owner = |id, kind, name: &str| OwnerDoc {
            id: OwnerId(id),
            kind,
            name: name.to_owned(),
            description: Some("Pershkrim".to_owned()),
            nipt: None,
            address: None,
        };
        let index = SearchIndex::build(
            [],
            [
                owner(1, OwnerKind::Client, "ALFA"),
                owner(2, OwnerKind::Client, "3 YJE"),
                owner(3, OwnerKind::Client, "*KLIENTE"),
                owner(4, OwnerKind::Supplier, "ALFA SH.P.K"),
            ],
        );
        let found = |kind, q| -> Vec<i64> {
            index
                .search_owners(kind, q, 10)
                .into_iter()
                .map(OwnerId::get)
                .collect()
        };
        assert_eq!(found(OwnerKind::Client, ""), [3, 2, 1]);
        assert_eq!(found(OwnerKind::Supplier, ""), [4]);
        assert_eq!(found(OwnerKind::Client, "alfa"), [1]);
        assert_eq!(found(OwnerKind::Supplier, "alfa"), [4]);
        assert_eq!(found(OwnerKind::Client, "përshkrim"), [3, 2, 1]);
        assert!(found(OwnerKind::Client, "zzzzzz").is_empty());
    }

    #[test]
    fn counted_search_reports_the_matches_before_the_limit() {
        let index = SearchIndex::build(
            [
                product(1, "COCA COLA", 0),
                product(2, "COCA LIGHT", 0),
                product(3, "COCA ZERO", 0),
                product(4, "FANTA", 0),
            ],
            [],
        );
        let (found, total) = index.search_products_counted("coca", 2);
        assert_eq!((found.len(), total), (2, 3));
        assert_eq!(ids(found), ids(index.search_products("coca", 2)));
        // A blank query: every product.
        assert_eq!(index.search_products_counted("", 2).1, 4);
        assert_eq!(index.search_products_counted("", 2).0.len(), 2);
    }

    #[test]
    fn counted_owner_search_counts_one_kind_and_skips_dropped_levels() {
        let owner = |id, kind, name: &str| OwnerDoc {
            id: OwnerId(id),
            kind,
            name: name.to_owned(),
            description: None,
            nipt: None,
            address: None,
        };
        let mut owners = vec![
            owner(1, OwnerKind::Supplier, "BAR ALFA"),
            owner(2, OwnerKind::Client, "QYTETI"),
        ];
        // DROP_TOKENS_THRESHOLD clients match both tokens; one matches one only.
        for i in 0..DROP_TOKENS_THRESHOLD {
            owners.push(owner(
                10 + i as i64,
                OwnerKind::Client,
                &format!("BAR ALFA {i}"),
            ));
        }
        owners.push(owner(99, OwnerKind::Client, "BAR TJETER"));
        let index = SearchIndex::build([], owners);
        let (found, total) = index.search_owners_counted(OwnerKind::Client, "bar alfa", 3);
        assert_eq!(found.len(), 3);
        assert_eq!(
            total, DROP_TOKENS_THRESHOLD,
            "the one-token level is not kept"
        );
        // A blank query: every owner of the kind.
        assert_eq!(
            index.search_owners_counted(OwnerKind::Client, "", 5).1,
            DROP_TOKENS_THRESHOLD + 2
        );
        assert_eq!(index.search_owners_counted(OwnerKind::Supplier, "", 5).1, 1);
    }
}

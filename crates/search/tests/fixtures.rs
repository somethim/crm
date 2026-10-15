//! Ranking tests over the sample import files (ARTIKUJ, KLIENTET, FURNITORET).

use std::time::Instant;

use crm_domain::{OwnerId, OwnerKind, ProductId};
use crm_search::{OwnerDoc, ProductDoc, SearchIndex};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../io/tests/fixtures/");

/// Rows of a fixture as `(header → value)` lookups, in file order.
fn rows(file: &str) -> Vec<Vec<(String, String)>> {
    let mut reader = csv::Reader::from_path(format!("{FIXTURES}{file}")).expect("fixture opens");
    let headers: Vec<String> = reader
        .headers()
        .expect("header row")
        .iter()
        .map(str::to_owned)
        .collect();
    reader
        .records()
        .map(|r| {
            let r = r.expect("valid row");
            headers
                .iter()
                .cloned()
                .zip(r.iter().map(str::to_owned))
                .collect()
        })
        .collect()
}

fn field<'a>(row: &'a [(String, String)], name: &str) -> &'a str {
    row.iter()
        .find(|(h, _)| h == name)
        .map(|(_, v)| v.as_str())
        .unwrap_or_else(|| panic!("column {name}"))
}

fn non_empty(s: &str) -> Option<String> {
    (!s.trim().is_empty()).then(|| s.to_owned())
}

fn products() -> Vec<ProductDoc> {
    rows("ARTIKUJ.csv")
        .iter()
        .zip(1..)
        .map(|(row, id)| ProductDoc {
            id: ProductId(id),
            name: field(row, "Përshkrimi").to_owned(),
            code: field(row, "Kodi").to_owned(),
            order_count: 0,
        })
        .collect()
}

fn owners() -> Vec<OwnerDoc> {
    let clients = rows("KLIENTET.csv")
        .into_iter()
        .map(|row| (OwnerKind::Client, row));
    let suppliers = rows("FURNITORET.csv")
        .into_iter()
        .map(|row| (OwnerKind::Supplier, row));
    clients
        .chain(suppliers)
        .zip(1..)
        .map(|((kind, row), id)| OwnerDoc {
            id: OwnerId(id),
            kind,
            name: field(&row, "Emër").to_owned(),
            description: None,
            nipt: non_empty(field(&row, "NIPT")),
            address: non_empty(field(&row, "Adresa")),
        })
        .collect()
}

struct Fixture {
    index: SearchIndex,
    products: Vec<ProductDoc>,
    owners: Vec<OwnerDoc>,
}

impl Fixture {
    fn load() -> Self {
        let products = products();
        let owners = owners();
        let index = SearchIndex::build(products.clone(), owners.clone());
        Self {
            index,
            products,
            owners,
        }
    }

    fn product_names(&self, query: &str, limit: usize) -> Vec<&str> {
        self.index
            .search_products(query, limit)
            .into_iter()
            .map(|id| {
                self.products[usize::try_from(id.get() - 1).unwrap()]
                    .name
                    .as_str()
            })
            .collect()
    }

    fn owner_names(&self, kind: OwnerKind, query: &str, limit: usize) -> Vec<&str> {
        self.index
            .search_owners(kind, query, limit)
            .into_iter()
            .map(|id| {
                self.owners[usize::try_from(id.get() - 1).unwrap()]
                    .name
                    .as_str()
            })
            .collect()
    }
}

#[test]
fn fixture_sizes() {
    let f = Fixture::load();
    assert_eq!(f.index.product_count(), 776);
    assert_eq!(f.index.owner_count(), 3_117);
}

#[test]
fn br_pj_ranks_brizo_pjeshke_first() {
    let f = Fixture::load();
    assert_eq!(f.product_names("br pj", 10)[0], "BRIZO PJESHKE 24*0.33LIT");
}

#[test]
fn albanian_folding_and_case_give_identical_results() {
    let f = Fixture::load();
    let folded = f.index.search_products("pjeshke", 50);
    assert!(!folded.is_empty());
    assert_eq!(f.index.search_products("pjeshkë", 50), folded);
    assert_eq!(f.index.search_products("PJESHKE", 50), folded);
    assert_eq!(f.index.search_products("PJESHKË", 50), folded);
    assert!(
        f.product_names("pjeshke", 50)
            .contains(&"BRIZO PJESHKE 24*0.33LIT")
    );
}

#[test]
fn typos_and_transpositions_are_tolerated() {
    let f = Fixture::load();
    assert_eq!(
        f.product_names("belris", 10)[0],
        "BELARIS 6X0.7 L (KREM LIKER)"
    );
    assert_eq!(
        f.product_names("amaor korabi", 10)[0],
        "AMARO KORABI 6X0.7 L"
    );
}

#[test]
fn equal_matches_rank_by_order_count_then_name() {
    let mut f = Fixture::load();
    fn brizo(f: &Fixture) -> Vec<&str> {
        f.product_names("brizo", 4)
    }
    // All BRIZO products match "brizo" exactly: name byte order first.
    assert_eq!(
        brizo(&f),
        [
            "BRIZO  6*1.5LIT",
            "BRIZO 24*0.33LIT",
            "BRIZO LULE & BANANE 24*0.33LIT",
            "BRIZO LULE 24*0.33LIT",
        ]
    );
    let id_of = |f: &Fixture, name: &str| f.products.iter().find(|p| p.name == name).unwrap().id;
    let counts = [
        (id_of(&f, "BRIZO RRUSHI 24*0.33LIT"), 5),
        (id_of(&f, "BRIZO MOLLE 24*0.33LIT"), 2),
        (id_of(&f, "BRIZO ZERO 12*0.33ML"), 2),
    ];
    f.index.set_order_counts(counts.into());
    assert_eq!(
        brizo(&f),
        [
            "BRIZO RRUSHI 24*0.33LIT",
            "BRIZO MOLLE 24*0.33LIT",
            "BRIZO ZERO 12*0.33ML",
            "BRIZO  6*1.5LIT",
        ]
    );
    // Popularity never beats a better text match.
    assert_eq!(
        f.product_names("brizo pjeshke", 1),
        ["BRIZO PJESHKE 24*0.33LIT"]
    );
}

#[test]
fn partial_matches_fill_up_to_the_threshold() {
    let f = Fixture::load();
    // Nothing has both tokens, so the "brizo" matches (rightmost token
    // dropped) are listed: the 8 exact BRIZO products, then the two one-typo BRYZO products.
    let found = f.product_names("brizo zzzzzz", 50);
    assert_eq!(found.len(), 10);
    assert!(found[..8].iter().all(|n| n.starts_with("BRIZO ")));
    assert!(found[8..].iter().all(|n| n.contains("BRYZO")));
    // The left token is dropped the same way when only the right one matches.
    assert_eq!(f.product_names("zzzzzz brizo", 50), found);
    // "brizo pjeshke": one full match (below the threshold of 10), then the
    // "brizo"-only matches, then the "pjeshke"-only matches.
    let found = f.product_names("brizo pjeshke", 50);
    assert_eq!(found[0], "BRIZO PJESHKE 24*0.33LIT");
    assert!(found[1..10].iter().all(|n| !n.contains("PJESHKE")));
    assert_eq!(found.len(), 14);
    assert!(
        found[10..]
            .iter()
            .all(|n| n.contains("PJESHKE") && !n.contains("BRIZO"))
    );
    assert!(f.index.search_products("zzzzzz", 50).is_empty());
}

#[test]
fn product_codes_match_exact_or_prefix() {
    let f = Fixture::load();
    assert_eq!(f.product_names("L0009", 1), ["BRIZO 24*0.33LIT"]);
    assert_eq!(f.product_names("l0009", 6).len(), 6);
    assert_eq!(
        f.product_names("A0002", 1),
        ["LIKER MORENA 0.5LIT (LIQUER)"]
    );
    assert_eq!(f.product_names("L0009/3", 1), ["BRIZO LULE 24*0.33LIT"]);
}

#[test]
fn owners_match_nipt_name_and_address_within_kind() {
    let f = Fixture::load();
    assert_eq!(
        f.owner_names(OwnerKind::Client, "K47120593B", 5)[0],
        "3 YJE"
    );
    assert_eq!(
        f.owner_names(OwnerKind::Client, "k47120593b", 5)[0],
        "3 YJE"
    );
    assert_eq!(f.owner_names(OwnerKind::Client, "K471205", 5)[0], "3 YJE");
    assert!(
        !f.owner_names(OwnerKind::Supplier, "K47120593B", 5)
            .contains(&"3 YJE")
    );
    assert_eq!(
        f.owner_names(OwnerKind::Supplier, "A & V DISTRIBUTION", 1),
        ["A & V DISTRIBUTION"]
    );
    assert_eq!(
        f.owner_names(OwnerKind::Supplier, "M30512077R", 1),
        ["A & V DISTRIBUTION"]
    );
    let by_address = f.owner_names(OwnerKind::Client, "livadhet kamez", 50);
    assert!(by_address.contains(&"3 YJE"));
    assert!(
        f.index
            .search_owners(OwnerKind::Client, "zzzzzz", 50)
            .is_empty()
    );
}

#[test]
fn empty_owner_query_is_byte_ordered() {
    let f = Fixture::load();
    let all = f.owner_names(OwnerKind::Client, "", usize::MAX);
    assert_eq!(all.len(), 2_931);
    assert!(all.windows(2).all(|w| w[0].as_bytes() <= w[1].as_bytes()));
    assert_eq!(all[0], "*KLIENTE TE NDRYSHEM");
    assert_eq!(
        f.owner_names(OwnerKind::Supplier, "", usize::MAX).len(),
        186
    );
}

#[test]
#[ignore = "timing; run with --ignored --nocapture"]
fn timings() {
    let products = products();
    let owners = owners();
    let start = Instant::now();
    let index = SearchIndex::build(products, owners);
    let build = start.elapsed();
    println!(
        "build: {:?} for {} products + {} owners",
        build,
        index.product_count(),
        index.owner_count()
    );
    let queries = [
        "br pj",
        "pjeshkë",
        "belris",
        "amaor korabi",
        "caj i ftohte 0.33",
        "L0009",
    ];
    for q in queries {
        let start = Instant::now();
        let n = index.search_products(q, 50).len();
        println!("products {q:?}: {:?} ({n} hits)", start.elapsed());
    }
    for q in ["K47120593B", "livadhet kamez tirane", "distribution"] {
        let start = Instant::now();
        let n = index.search_owners(OwnerKind::Client, q, 50).len();
        println!("owners {q:?}: {:?} ({n} hits)", start.elapsed());
    }
}

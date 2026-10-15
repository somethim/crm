//! Order use cases (007 step 3, AC 2–5 and 8).

mod common;

use common::{buy_line, draft, draft_for, line, open, product, stock};
use crm_services::domain::{Money, OrderKind, OwnerInput, OwnerKind, Qty, StockError, time};
use crm_services::{Crm, Fault, OpenTab, OrderFilter, Page, ServiceError};

const SALE: OrderKind = OrderKind::Sale;
const PURCHASE: OrderKind = OrderKind::Purchase;

fn client(crm: &Crm, name: &str, description: &str) -> crm_services::domain::Owner {
    crm.create_owner(OwnerInput {
        kind: OwnerKind::Client,
        name: name.into(),
        description: description.into(),
        nipt: "K11111111A".into(),
        address: "Tiranë".into(),
        account_code: String::new(),
    })
    .unwrap()
}

fn all_orders(crm: &Crm) -> Vec<crm_services::OrderSummary> {
    crm.list_orders(OrderFilter {
        kind: None,
        from: jiff::civil::date(2000, 1, 1),
        to: jiff::civil::date(2100, 1, 1),
        text: None,
    })
    .unwrap()
}

// Ports of OrderControllerTest.php:31 and CreateOrderProductsTest.php:63.
#[test]
fn a_sale_decreases_stock_and_totals_its_lines() {
    let (_dir, crm) = open();
    let p1 = product(&crm, "P1", "Uje", 100, 500, 1_000);
    let p2 = product(&crm, "P2", "Leng", 50, 500, 2_000);
    let mut d = draft(
        SALE,
        "Supplier Alpha",
        vec![
            line(p1.product.id, 10, 1_000),
            line(p2.product.id, 5, 2_500),
        ],
    );
    d.owner_description = "Supplier Alpha Description".into();
    d.lines.push(Default::default()); // placeholder row
    let id = crm.save_order(d, None, None).unwrap();

    let order = crm.get_order(id).unwrap();
    assert_eq!(order.lines.len(), 2);
    // The given prices are kept (the grid fills a blank one with the SKU price).
    assert_eq!(order.lines[0].price, Money(1_000));
    assert_eq!(order.lines[1].price, Money(2_500));
    assert_eq!(order.lines[0].value, Money(10_000));
    assert_eq!(order.total, Money(10_000 + 12_500));
    assert_eq!(
        (order.lines[0].code.as_str(), order.lines[0].unit.as_str()),
        ("P1", "COPE")
    );
    assert_eq!(order.owner.name, "Supplier Alpha");
    assert_eq!(
        order.owner.description.as_deref(),
        Some("Supplier Alpha Description")
    );
    assert_eq!(order.issued_on, time::today());
    assert_eq!(stock(&crm, p1.product.id), 90);
    assert_eq!(stock(&crm, p2.product.id), 45);
    // A new owner was created, without a description (OrderController.php:65).
    let owners = crm.list_owners(OwnerKind::Client, "", Page::ALL).unwrap();
    assert_eq!(owners.len(), 1);
    assert_eq!(owners[0].description, None);
    assert_eq!(order.owner_id, Some(owners[0].id));
}

// Port of OrderControllerTest.php:218.
#[test]
fn a_sale_over_stock_fails_and_writes_nothing() {
    let (_dir, crm) = open();
    let p = product(&crm, "P1", "BRIZO", 10, 100, 200);
    let err = crm
        .save_order(
            draft(SALE, "Klient i ri", vec![line(p.product.id, 15, 0)]),
            None,
            None,
        )
        .unwrap_err();
    assert!(
        matches!(err, ServiceError::Stock(StockError::Insufficient { .. })),
        "{err:?}"
    );
    assert_eq!(
        err.user_message(),
        "Gjendje e pamjaftueshme për «BRIZO»: ka 10, kërkohen 15."
    );
    assert_eq!(stock(&crm, p.product.id), 10);
    assert!(all_orders(&crm).is_empty());
    assert!(
        crm.list_owners(OwnerKind::Client, "", Page::ALL)
            .unwrap()
            .is_empty()
    );
}

// Port of OrderControllerTest.php:237.
#[test]
fn an_edit_over_stock_counts_what_the_order_already_took() {
    let (_dir, crm) = open();
    let p = product(&crm, "P1", "BRIZO", 10, 100, 200);
    let id = crm
        .save_order(draft(SALE, "K", vec![line(p.product.id, 5, 0)]), None, None)
        .unwrap();
    assert_eq!(stock(&crm, p.product.id), 5);
    let err = crm
        .save_order(
            draft(SALE, "K", vec![line(p.product.id, 20, 0)]),
            Some(id),
            None,
        )
        .unwrap_err();
    assert_eq!(
        err.user_message(),
        "Gjendje e pamjaftueshme për «BRIZO»: ka 10, kërkohen 20."
    );
    assert_eq!(stock(&crm, p.product.id), 5);
    // Exactly the available amount works.
    crm.save_order(
        draft(SALE, "K", vec![line(p.product.id, 10, 0)]),
        Some(id),
        None,
    )
    .unwrap();
    assert_eq!(stock(&crm, p.product.id), 0);
}

#[test]
fn the_owner_resolves_by_id_then_folded_name_then_new() {
    let (_dir, crm) = open();
    let p = product(&crm, "P1", "BRIZO", 100, 100, 200);
    let alfa = client(&crm, "Bar Alfa", "Qendër");

    // Folded-name match within the kind; the owner's description fills in.
    let id = crm
        .save_order(
            draft(SALE, "  bar ALFA ", vec![line(p.product.id, 1, 0)]),
            None,
            None,
        )
        .unwrap();
    let order = crm.get_order(id).unwrap();
    assert_eq!(order.owner_id, Some(alfa.id));
    assert_eq!(order.owner.description.as_deref(), Some("Qendër"));
    assert_eq!(order.owner.nipt.as_deref(), Some("K11111111A"));
    assert_eq!(order.owner.address.as_deref(), Some("Tiranë"));

    // A supplier of the same name is not matched for a sale.
    crm.create_owner(OwnerInput {
        kind: OwnerKind::Supplier,
        name: "Furnitori".into(),
        description: String::new(),
        nipt: String::new(),
        address: String::new(),
        account_code: String::new(),
    })
    .unwrap();
    crm.save_order(
        draft(SALE, "Furnitori", vec![line(p.product.id, 1, 0)]),
        None,
        None,
    )
    .unwrap();
    assert_eq!(
        crm.list_owners(OwnerKind::Client, "furnitori", Page::ALL)
            .unwrap()
            .len(),
        1
    );

    // A selected id wins over the typed name; a typed description wins.
    let beta = client(&crm, "Bar Beta", "");
    let mut d = draft_for(SALE, beta.id, "Bar Beta", vec![line(p.product.id, 1, 0)]);
    d.owner_description = "Dyqani 2".into();
    let id = crm.save_order(d, None, None).unwrap();
    let order = crm.get_order(id).unwrap();
    assert_eq!(order.owner_id, Some(beta.id));
    assert_eq!(order.owner.description.as_deref(), Some("Dyqani 2"));

    // A deleted selected owner falls back to the name.
    crm.delete_owner(beta.id).unwrap();
    let id = crm
        .save_order(
            draft_for(SALE, beta.id, "Bar Beta", vec![line(p.product.id, 1, 0)]),
            None,
            None,
        )
        .unwrap();
    let order = crm.get_order(id).unwrap();
    // A new owner was created (SQLite may hand it the freed row id).
    let owner = crm.get_owner(order.owner_id.unwrap()).unwrap();
    assert_eq!(
        (owner.name.as_str(), owner.description.as_deref()),
        ("Bar Beta", None)
    );
}

// Port of OrderControllerTest.php:169 ("it updates an order", owner change).
#[test]
fn an_edit_can_change_the_owner() {
    let (_dir, crm) = open();
    let p = product(&crm, "P1", "BRIZO", 100, 100, 200);
    let id = crm
        .save_order(
            draft(SALE, "Bar Alfa", vec![line(p.product.id, 1, 0)]),
            None,
            None,
        )
        .unwrap();
    let beta = client(&crm, "Bar Beta", "");
    let mut d = draft_for(
        SALE,
        beta.id,
        "Supplier Beta",
        vec![line(p.product.id, 1, 0)],
    );
    d.owner_description = "Supplier Beta Description".into();
    crm.save_order(d, Some(id), None).unwrap();
    let order = crm.get_order(id).unwrap();
    assert_eq!(order.owner_id, Some(beta.id));
    assert_eq!(order.owner.name, "Supplier Beta");
    assert_eq!(
        order.owner.description.as_deref(),
        Some("Supplier Beta Description")
    );
    assert_eq!(stock(&crm, p.product.id), 99);
}

#[test]
fn re_saving_an_edit_is_net_zero_and_added_lines_move_stock() {
    let (_dir, crm) = open();
    let p1 = product(&crm, "P1", "Uje", 100, 100, 200);
    let p2 = product(&crm, "P2", "Leng", 20, 100, 200);
    let d = draft(SALE, "K", vec![line(p1.product.id, 5, 0)]);
    let id = crm.save_order(d.clone(), None, None).unwrap();
    let created = crm.get_order(id).unwrap().created_at;
    for _ in 0..3 {
        crm.save_order(d.clone(), Some(id), None).unwrap();
    }
    assert_eq!(stock(&crm, p1.product.id), 95);

    // Edit that adds a line.
    let mut d2 = d.clone();
    d2.lines.push(line(p2.product.id, 4, 0));
    crm.save_order(d2, Some(id), None).unwrap();
    assert_eq!(stock(&crm, p1.product.id), 95);
    assert_eq!(stock(&crm, p2.product.id), 16);
    let order = crm.get_order(id).unwrap();
    assert_eq!(order.lines.len(), 2);
    assert_eq!(order.created_at, created);
    assert!(order.updated_at >= created);

    // Port of UpdateOrderProductsTest.php:15: replacing a line restores the old one.
    crm.save_order(
        draft(SALE, "K", vec![line(p2.product.id, 10, 0)]),
        Some(id),
        None,
    )
    .unwrap();
    assert_eq!(stock(&crm, p1.product.id), 100);
    assert_eq!(stock(&crm, p2.product.id), 10);
    assert_eq!(crm.get_order(id).unwrap().lines.len(), 1);
}

#[test]
fn deleting_a_sale_restores_stock() {
    let (_dir, crm) = open();
    let p = product(&crm, "P1", "Uje", 50, 100, 200);
    let id = crm
        .save_order(
            draft(SALE, "K", vec![line(p.product.id, 20, 0)]),
            None,
            None,
        )
        .unwrap();
    assert_eq!(stock(&crm, p.product.id), 30);
    crm.delete_order(id).unwrap();
    assert_eq!(stock(&crm, p.product.id), 50);
    assert!(matches!(crm.get_order(id), Err(ServiceError::NotFound)));
    assert!(matches!(crm.delete_order(id), Err(ServiceError::NotFound)));
}

// Port of OrderControllerTest.php:70 and CreateOrderProducts.php:24 behaviour.
#[test]
fn a_purchase_reuses_a_product_by_folded_name_and_overwrites_prices() {
    let (_dir, crm) = open();
    let existing = product(&crm, "P1", "New Supplier Item", 100, 100, 130);
    let id = crm
        .save_order(
            draft(
                PURCHASE,
                "Supplier Beta",
                vec![buy_line(None, "new supplier ITEM", 20, 120, 150)],
            ),
            None,
            None,
        )
        .unwrap();
    let order = crm.get_order(id).unwrap();
    assert_eq!(order.lines[0].product_id, Some(existing.product.id));
    assert_eq!(order.lines[0].name, "New Supplier Item");
    // Line price = buy price; Q11 overwrites the SKU prices; stock += qty.
    assert_eq!(order.lines[0].price, Money(120));
    assert_eq!(order.total, Money(2_400));
    let row = crm.get_product(existing.product.id).unwrap();
    assert_eq!(
        (row.sku.quantity, row.sku.buy_price, row.sku.sell_price),
        (Qty(120), Money(120), Money(150))
    );
    assert_eq!(crm.list_products("", Page::ALL).unwrap().len(), 1);
    assert_eq!(
        crm.list_owners(OwnerKind::Supplier, "", Page::ALL)
            .unwrap()
            .len(),
        1
    );

    // A picked product without prices keeps the SKU prices, line price = buy.
    let mut plain = buy_line(Some(existing.product.id), "", 5, 0, 0);
    plain.buy_price = None;
    plain.sell_price = None;
    let id = crm
        .save_order(draft(PURCHASE, "Supplier Beta", vec![plain]), None, None)
        .unwrap();
    assert_eq!(crm.get_order(id).unwrap().lines[0].price, Money(120));
    assert_eq!(stock(&crm, existing.product.id), 125);
}

#[test]
fn a_purchase_of_a_new_name_creates_an_auto_coded_sku_once() {
    let (_dir, crm) = open();
    product(&crm, "A0007", "Old", 1, 1, 1);
    let id = crm
        .save_order(
            draft(
                PURCHASE,
                "Furnitori",
                vec![
                    buy_line(None, "Raki Skrapar", 12, 800, 1_000),
                    buy_line(None, "RAKI SKRAPAR", 3, 800, 1_000),
                    buy_line(None, "Verë e kuqe", 6, 500, 700),
                ],
            ),
            None,
            None,
        )
        .unwrap();
    let order = crm.get_order(id).unwrap();
    assert_eq!(order.lines[0].product_id, order.lines[1].product_id);
    let raki = crm.get_product(order.lines[0].product_id.unwrap()).unwrap();
    assert_eq!(raki.sku.code, "A0008");
    assert_eq!(raki.sku.unit, "SHISHE");
    assert_eq!(raki.sku.quantity, Qty(15), "not counted twice");
    assert_eq!(
        (raki.sku.buy_price, raki.sku.sell_price),
        (Money(800), Money(1_000))
    );
    let vere = crm.get_product(order.lines[2].product_id.unwrap()).unwrap();
    assert_eq!(
        (vere.sku.code.as_str(), vere.sku.quantity),
        ("A0009", Qty(6))
    );
    // The index learned the new products.
    assert_eq!(crm.search_products("raki", 10).unwrap(), [raki]);
}

#[test]
fn a_failed_purchase_leaves_no_new_products_behind() {
    let (_dir, crm) = open();
    let p = product(&crm, "P1", "BRIZO", 0, 100, 200);
    let id = crm
        .save_order(
            draft(
                PURCHASE,
                "F",
                vec![buy_line(Some(p.product.id), "", 10, 100, 200)],
            ),
            None,
            None,
        )
        .unwrap();
    // Sell 8; the purchase can now shrink to 2 at most.
    crm.save_order(draft(SALE, "K", vec![line(p.product.id, 8, 0)]), None, None)
        .unwrap();
    let err = crm
        .save_order(
            draft(
                PURCHASE,
                "F",
                vec![
                    buy_line(Some(p.product.id), "", 1, 100, 200),
                    buy_line(None, "Krejt i ri", 4, 10, 20),
                ],
            ),
            Some(id),
            None,
        )
        .unwrap_err();
    assert_eq!(
        err.user_message(),
        "Kjo porosi blerjeje nuk mund të ndryshohet: malli i saj është shitur tashmë \
         («BRIZO»: gjendja 2, duhet të hiqen 9)."
    );
    assert_eq!(stock(&crm, p.product.id), 2);
    assert_eq!(crm.list_products("", Page::ALL).unwrap().len(), 1);
    assert!(crm.search_products("krejt", 10).unwrap().is_empty());
}

// Q6.
#[test]
fn deleting_a_sold_purchase_is_blocked_and_changes_nothing() {
    let (_dir, crm) = open();
    let p = product(&crm, "P1", "BRIZO", 0, 100, 200);
    let purchase = crm
        .save_order(
            draft(
                PURCHASE,
                "F",
                vec![buy_line(Some(p.product.id), "", 10, 100, 200)],
            ),
            None,
            None,
        )
        .unwrap();
    crm.save_order(draft(SALE, "K", vec![line(p.product.id, 8, 0)]), None, None)
        .unwrap();
    let before = crm.get_order(purchase).unwrap();
    let err = crm.delete_order(purchase).unwrap_err();
    assert_eq!(
        err.user_message(),
        "Kjo porosi blerjeje nuk mund të fshihet: malli i saj është shitur tashmë \
         («BRIZO»: gjendja 2, duhet të hiqen 10)."
    );
    assert_eq!(crm.get_order(purchase).unwrap(), before);
    assert_eq!(stock(&crm, p.product.id), 2);
    // Editing up is always fine (net delta, never transiently negative).
    crm.save_order(
        draft(
            PURCHASE,
            "F",
            vec![buy_line(Some(p.product.id), "", 12, 100, 200)],
        ),
        Some(purchase),
        None,
    )
    .unwrap();
    assert_eq!(stock(&crm, p.product.id), 4);
}

#[test]
fn an_order_line_of_a_deleted_sku_is_skipped_on_delete() {
    let (_dir, crm) = open();
    let p = product(&crm, "P1", "BRIZO", 10, 100, 200);
    let id = crm
        .save_order(draft(SALE, "K", vec![line(p.product.id, 3, 0)]), None, None)
        .unwrap();
    assert!(crm.delete_product(p.product.id).unwrap().sku_deleted);
    let order = crm.get_order(id).unwrap();
    assert_eq!(
        (order.lines[0].product_id, order.lines[0].sku_id),
        (None, None)
    );
    assert_eq!(order.lines[0].name, "BRIZO", "snapshot kept");
    crm.delete_order(id).unwrap();
}

// Q2, replacing the old ProductPopularityTest.php:32 inflation.
#[test]
fn order_count_stays_one_however_often_an_order_is_re_saved() {
    let (_dir, crm) = open();
    let a = product(&crm, "P1", "AAA", 100, 100, 200);
    let b = product(&crm, "P2", "BBB", 100, 100, 200);
    let d = draft(SALE, "K", vec![line(a.product.id, 1, 0)]);
    let id = crm.save_order(d.clone(), None, None).unwrap();
    for _ in 0..3 {
        crm.save_order(d.clone(), Some(id), None).unwrap();
    }
    for _ in 0..2 {
        crm.save_order(draft(SALE, "K", vec![line(b.product.id, 1, 0)]), None, None)
            .unwrap();
    }
    // Empty query = popularity order: BBB (2 orders) before AAA (1 order).
    let ids: Vec<_> = crm
        .search_products("", 10)
        .unwrap()
        .into_iter()
        .map(|r| r.product.id)
        .collect();
    assert_eq!(ids, [b.product.id, a.product.id]);
    // The same after a full rebuild from the database (a commit through another
    // handle makes the poll rebuild the index).
    let other = crm_storage::Db::open(crm.paths(), &crm_services::DbKey::for_tests(1)).unwrap();
    other
        .write(|tx| crm_storage::repo::settings::set(tx, "probe", "1"))
        .unwrap();
    other.checkpoint_and_close().unwrap();
    assert!(crm.refresh_if_changed());
    let first = crm.search_products("", 10).unwrap()[0].product.id;
    assert_eq!(first, b.product.id);
}

#[test]
fn issued_on_is_stored_and_filters_the_list() {
    let (_dir, crm) = open();
    let p = product(&crm, "P1", "BRIZO", 100, 100, 200);
    let mut d = draft(SALE, "Bar Alfa", vec![line(p.product.id, 1, 0)]);
    d.issued_on = jiff::civil::date(2026, 1, 31);
    let old = crm.save_order(d, None, None).unwrap();
    let today = crm
        .save_order(
            draft(SALE, "Bar Beta", vec![line(p.product.id, 1, 0)]),
            None,
            None,
        )
        .unwrap();
    assert_eq!(
        crm.get_order(old).unwrap().issued_on,
        jiff::civil::date(2026, 1, 31)
    );

    let listed: Vec<_> = crm
        .list_orders(Crm::default_filter())
        .unwrap()
        .into_iter()
        .map(|o| o.id)
        .collect();
    assert_eq!(listed, [today]);
    let january = OrderFilter {
        kind: Some(SALE),
        from: jiff::civil::date(2026, 1, 1),
        to: jiff::civil::date(2026, 1, 31),
        text: None,
    };
    let listed: Vec<_> = crm
        .list_orders(january.clone())
        .unwrap()
        .into_iter()
        .map(|o| o.id)
        .collect();
    assert_eq!(listed, [old]);
    // Digits = the invoice number, whatever the dates; text filters the owner.
    let by_number = OrderFilter {
        text: Some(format!(" 00{} ", today.0)),
        ..january.clone()
    };
    assert_eq!(crm.list_orders(by_number).unwrap()[0].id, today);
    let by_name = OrderFilter {
        text: Some("alfa".into()),
        ..january
    };
    assert_eq!(crm.list_orders(by_name).unwrap().len(), 1);
    let purchases = OrderFilter {
        kind: Some(PURCHASE),
        ..Crm::default_filter()
    };
    assert!(crm.list_orders(purchases).unwrap().is_empty());
}

#[test]
fn save_order_clears_the_tab_draft() {
    let (_dir, crm) = open();
    let p = product(&crm, "P1", "BRIZO", 100, 100, 200);
    let d = draft(SALE, "K", vec![line(p.product.id, 2, 0)]);
    crm.save_tab(OpenTab {
        tab_key: "0123456789abcdef".into(),
        position: 0,
        kind: SALE,
        order_id: None,
        title: SALE.new_tab_title().into(),
        active: true,
        draft: Some(d.clone()),
    })
    .unwrap();
    let id = crm
        .save_order(d, None, Some("0123456789abcdef".into()))
        .unwrap();
    let tabs = crm.list_tabs().unwrap();
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0].draft, None);
    assert_eq!(tabs[0].order_id, Some(id));
    // Deleting the order closes its tab.
    crm.delete_order(id).unwrap();
    assert!(crm.list_tabs().unwrap().is_empty());
}

#[test]
fn invalid_drafts_and_kind_changes_are_refused() {
    let (_dir, crm) = open();
    let p = product(&crm, "P1", "BRIZO", 100, 100, 200);
    let err = crm
        .save_order(draft(SALE, "", vec![]), None, None)
        .unwrap_err();
    assert!(matches!(err, ServiceError::Validation(_)));
    assert_eq!(
        err.user_message(),
        "Vendosni emrin e klientit/furnitorit.\nShtoni të paktën një artikull në porosi."
    );
    let id = crm
        .save_order(draft(SALE, "K", vec![line(p.product.id, 1, 0)]), None, None)
        .unwrap();
    let err = crm
        .save_order(
            draft(
                PURCHASE,
                "K",
                vec![buy_line(Some(p.product.id), "", 1, 1, 1)],
            ),
            Some(id),
            None,
        )
        .unwrap_err();
    assert_eq!(
        err.user_message(),
        "Lloji i porosisë nuk mund të ndryshohet."
    );
    // A sale line whose product was deleted.
    let gone = product(&crm, "P9", "ZHDUKUR", 5, 1, 1);
    crm.delete_product(gone.product.id).unwrap();
    let mut l = line(gone.product.id, 1, 0);
    l.name = "ZHDUKUR".into();
    let err = crm
        .save_order(
            draft(SALE, "K", vec![line(p.product.id, 1, 0), l]),
            None,
            None,
        )
        .unwrap_err();
    assert_eq!(
        err.user_message(),
        "Rreshti 2: Artikulli «ZHDUKUR» nuk ekziston më."
    );
    assert_eq!(stock(&crm, p.product.id), 99);
}

/// The draft the grid builds from a saved purchase (`GridState::from_order`):
/// a line whose product is gone has no `product_id` and sell price 0.
fn redraft(crm: &Crm, id: crm_services::domain::OrderId) -> crm_services::domain::OrderDraft {
    let order = crm.get_order(id).unwrap();
    let lines = order
        .lines
        .iter()
        .map(|l| {
            let sell = l
                .product_id
                .map_or(0, |p| crm.get_product(p).unwrap().sku.sell_price.0);
            buy_line(l.product_id, &l.name, l.quantity.0, l.price.0, sell)
        })
        .collect();
    draft(OrderKind::Purchase, "F", lines)
}

fn product_count(crm: &Crm) -> usize {
    crm.list_products("", Page::default()).unwrap().len()
}

#[test]
fn resaving_a_purchase_of_a_deleted_product_keeps_its_line() {
    // F-017: X and Y share SKU L1; X is bought, then deleted.
    let (_dir, crm) = open();
    let x = product(&crm, "L1", "XNAME", 0, 100, 200);
    let y = product(&crm, "L1", "YNAME", 0, 100, 200);
    let id = crm
        .save_order(
            draft(
                OrderKind::Purchase,
                "F",
                vec![buy_line(Some(x.product.id), "XNAME", 10, 100, 200)],
            ),
            None,
            None,
        )
        .unwrap();
    crm.delete_product(x.product.id).unwrap();
    assert_eq!(stock(&crm, y.product.id), 10);
    let before = product_count(&crm);

    // Re-save untouched (as after changing only the date).
    crm.save_order(redraft(&crm, id), Some(id), None).unwrap();
    assert_eq!(product_count(&crm), before, "no new product");
    let names: Vec<_> = crm
        .list_products("", Page::default())
        .unwrap()
        .into_iter()
        .map(|r| r.product.name)
        .collect();
    assert_eq!(names, ["YNAME"]);
    let sibling = crm.get_product(y.product.id).unwrap();
    assert_eq!(sibling.sku.quantity, Qty(10), "stock unchanged");
    assert_eq!(
        sibling.sku.sell_price,
        Money(200),
        "sibling sell price kept"
    );
    let line = &crm.get_order(id).unwrap().lines[0];
    assert_eq!(line.product_id, None);
    assert_eq!(line.sku_id, Some(sibling.sku.id));
    assert_eq!((line.name.as_str(), line.code.as_str()), ("XNAME", "L1"));
    assert_eq!((line.quantity, line.price), (Qty(10), Money(100)));

    // A changed quantity moves only the difference on the kept SKU.
    let mut d = redraft(&crm, id);
    d.lines[0].quantity = Qty(4);
    crm.save_order(d, Some(id), None).unwrap();
    assert_eq!(stock(&crm, y.product.id), 4);
    assert_eq!(product_count(&crm), before);
}

#[test]
fn an_orphan_line_is_claimed_once_and_new_free_text_still_creates() {
    let (_dir, crm) = open();
    let x = product(&crm, "L1", "XNAME", 0, 100, 200);
    let y = product(&crm, "L1", "YNAME", 0, 100, 200);
    let id = crm
        .save_order(
            draft(
                OrderKind::Purchase,
                "F",
                vec![buy_line(Some(x.product.id), "XNAME", 10, 100, 200)],
            ),
            None,
            None,
        )
        .unwrap();
    crm.delete_product(x.product.id).unwrap();
    let before = product_count(&crm);
    let mut d = redraft(&crm, id);
    // A second "XNAME" line cannot claim the same old line: it is free text.
    d.lines.push(buy_line(None, "XNAME", 1, 50, 80));
    d.lines.push(buy_line(None, "ZNAME", 2, 30, 60));
    crm.save_order(d, Some(id), None).unwrap();
    assert_eq!(product_count(&crm), before + 2);
    let order = crm.get_order(id).unwrap();
    assert_eq!(order.lines[0].sku_id, Some(y.product.sku_id));
    assert_ne!(order.lines[1].sku_id, Some(y.product.sku_id));
    assert_eq!(stock(&crm, y.product.id), 10);
}

#[test]
fn an_orphan_line_whose_sku_is_gone_keeps_its_snapshot() {
    let (_dir, crm) = open();
    let x = product(&crm, "L1", "XNAME", 0, 100, 200);
    let id = crm
        .save_order(
            draft(
                OrderKind::Purchase,
                "F",
                vec![buy_line(Some(x.product.id), "XNAME", 10, 100, 200)],
            ),
            None,
            None,
        )
        .unwrap();
    crm.delete_product(x.product.id).unwrap(); // last product: the SKU goes too
    let before = product_count(&crm);
    crm.save_order(redraft(&crm, id), Some(id), None).unwrap();
    assert_eq!(product_count(&crm), before, "no phantom product");
    let line = &crm.get_order(id).unwrap().lines[0];
    assert_eq!((line.product_id, line.sku_id), (None, None));
    assert_eq!((line.name.as_str(), line.code.as_str()), ("XNAME", "L1"));
}

#[test]
fn a_committed_save_is_ok_even_when_the_index_update_fails() {
    // F-013: the commit happened; an `Err` would make the UI keep the draft and
    // a second Ruaj would insert the invoice (and move the stock) again.
    let (_dir, crm) = open();
    let p = product(&crm, "P1", "BRIZO", 10, 100, 200);
    crm.inject_fault(Fault::IndexRead);
    let id = crm
        .save_order(
            draft(SALE, "K", vec![line(p.product.id, 4, 200)]),
            None,
            None,
        )
        .expect("committed, so Ok");
    assert_eq!(crm.get_order(id).unwrap().total, Money(800));
    assert_eq!(stock(&crm, p.product.id), 6);
    // The next poll repairs the index although no foreign commit happened.
    assert!(crm.refresh_if_changed());
    assert!(!crm.refresh_if_changed());
    // A free-text purchase whose new product misses the index: found after the poll.
    crm.inject_fault(Fault::IndexRead);
    crm.save_order(
        draft(PURCHASE, "F", vec![buy_line(None, "QYQYQY", 1, 1, 2)]),
        None,
        None,
    )
    .unwrap();
    assert!(crm.search_products("qyqyqy", 5).unwrap().is_empty());
    assert!(crm.refresh_if_changed());
    assert_eq!(crm.search_products("qyqyqy", 5).unwrap().len(), 1);

    crm.inject_fault(Fault::IndexRead);
    crm.delete_order(id).expect("committed delete is Ok");
    assert_eq!(stock(&crm, p.product.id), 10);
    assert!(crm.refresh_if_changed());
}

#[test]
fn a_failing_index_rebuild_stays_dirty_until_it_succeeds() {
    let (_dir, crm) = open();
    let p = product(&crm, "P1", "BRIZO", 10, 100, 200);
    crm.inject_fault(Fault::IndexRead);
    crm.save_order(draft(SALE, "K", vec![line(p.product.id, 1, 0)]), None, None)
        .unwrap();
    crm.inject_fault(Fault::IndexRead); // the poll's rebuild fails too
    assert!(crm.refresh_if_changed());
    assert!(crm.refresh_if_changed(), "still dirty: rebuilt now");
    assert!(!crm.refresh_if_changed());
}

#[test]
fn a_sale_line_priced_zero_stays_free() {
    // F-012: free goods (bonus, samples) are invoiced at 0, not at the SKU price.
    let (_dir, crm) = open();
    let p = product(&crm, "P1", "BRIZO", 10, 100, 200);
    let id = crm
        .save_order(
            draft(
                SALE,
                "K",
                vec![line(p.product.id, 2, 0), line(p.product.id, 1, 200)],
            ),
            None,
            None,
        )
        .unwrap();
    let order = crm.get_order(id).unwrap();
    assert_eq!(order.lines[0].price, Money::ZERO);
    assert_eq!(order.lines[0].value, Money::ZERO);
    assert_eq!(order.total, Money(200));
    assert_eq!(
        stock(&crm, p.product.id),
        7,
        "free goods still leave the stock"
    );
}

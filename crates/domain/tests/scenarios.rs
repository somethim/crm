//! Scenarios ported from the old app's test suite (feature reference at 544086f),
//! run against the public `crm-domain` API.

use crm_domain::entities::{DraftLine, OrderDraft, OrderLine, OwnerInput, ProductInput};
use crm_domain::stock::{self, PurchaseAction, StockError};
use crm_domain::{Money, OrderKind, OwnerKind, ProductId, Qty, SkuId, format, sku, text, validate};
use jiff::civil::date;

fn line(sku_id: Option<i64>, name: &str, qty: i64) -> OrderLine {
    OrderLine {
        position: 0,
        product_id: Some(ProductId(1)),
        sku_id: sku_id.map(SkuId),
        name: name.into(),
        code: "L0009".into(),
        unit: "COPE".into(),
        quantity: Qty(qty),
        price: Money(6_500),
        value: Money(6_500 * qty),
    }
}

fn stock(pairs: &[(i64, i64)]) -> impl Fn(SkuId) -> Qty + '_ {
    move |id| {
        Qty(pairs
            .iter()
            .find(|(k, _)| *k == id.0)
            .map(|(_, q)| *q)
            .unwrap_or(0))
    }
}

const NO_LINES: &[OrderLine] = &[];

fn product_input() -> ProductInput {
    ProductInput {
        code: "T0001".into(),
        name: "Test Product".into(),
        unit: "SHISHE".into(),
        buy_price: Money(80_000),
        sell_price: Money(100_000),
        quantity: Qty(50),
    }
}

fn draft(kind: OrderKind, lines: Vec<DraftLine>) -> OrderDraft {
    OrderDraft {
        kind,
        owner_id: None,
        owner_name: "Supplier Gamma".into(),
        owner_description: String::new(),
        issued_on: date(2026, 10, 14),
        lines,
    }
}

// --- CreateProductTest.php ---------------------------------------------------

#[test]
fn create_product_name_missing() {
    // CreateProductTest.php:71
    let mut p = product_input();
    p.name = String::new();
    let errs = validate::product(&p).unwrap_err();
    assert_eq!(errs.messages_for("name"), ["Emri është i detyrueshëm"]);
}

#[test]
fn create_product_negative_price() {
    // CreateProductTest.php:101
    let mut p = product_input();
    p.sell_price = Money(-10_000);
    p.buy_price = Money(-1);
    let errs = validate::product(&p).unwrap_err();
    assert_eq!(
        errs.messages_for("sell_price"),
        ["Çmimi nuk mund të jetë negativ"]
    );
    assert_eq!(
        errs.messages_for("buy_price"),
        ["Çmimi nuk mund të jetë negativ"]
    );
}

#[test]
fn create_product_negative_quantity() {
    // CreateProductTest.php:117
    let mut p = product_input();
    p.quantity = Qty(-10);
    let errs = validate::product(&p).unwrap_err();
    assert_eq!(
        errs.messages_for("quantity"),
        ["Gjendja nuk mund të jetë negative"]
    );
}

#[test]
fn create_product_auto_code() {
    // CreateProductTest.php:51 (deterministic per A10)
    let code = sku::next_auto_code(["A0001", "L0009/3"]);
    assert_eq!(code, "A0002");
}

// --- CreateSupplierTest.php ---------------------------------------------------

#[test]
fn create_supplier_name_too_long() {
    // CreateSupplierTest.php:69
    let input = OwnerInput {
        kind: OwnerKind::Supplier,
        name: "a".repeat(51),
        description: String::new(),
        nipt: String::new(),
        address: String::new(),
        account_code: String::new(),
    };
    let errs = validate::owner(&input).unwrap_err();
    assert_eq!(
        errs.messages_for("name"),
        ["Emri nuk mund të ketë më shumë se 50 shkronja"]
    );
}

// --- CreateOrderProductTest.php / OrderControllerTest.php ---------------------

#[test]
fn order_line_quantity_below_one() {
    // CreateOrderProductTest.php:106
    let line = DraftLine {
        product_id: Some(ProductId(1)),
        name: "BRIZO".into(),
        quantity: Qty(0),
        price: Money(6_500),
        ..DraftLine::default()
    };
    let errs = validate::draft(&draft(OrderKind::Sale, vec![line])).unwrap_err();
    assert_eq!(
        errs.messages_for("quantity"),
        ["Sasia duhet të jetë të paktën 1"]
    );
    assert_eq!(errs.errors()[0].line, Some(0));
}

#[test]
fn order_requires_lines_and_owner() {
    // OrderControllerTest.php:197 (empty payload)
    let mut d = draft(
        OrderKind::Sale,
        vec![DraftLine::default(), DraftLine::default()],
    );
    d.owner_name = String::new();
    let errs = validate::draft(&d).unwrap_err();
    assert_eq!(
        errs.messages_for("lines"),
        ["Shtoni të paktën një artikull në porosi."]
    );
    assert_eq!(
        errs.messages_for("owner_name"),
        ["Vendosni emrin e klientit/furnitorit."]
    );
}

#[test]
fn supplier_line_shape() {
    // OrderControllerTest.php:201: a free-text purchase line needs both prices.
    let free = DraftLine {
        name: "Mixed Row".into(),
        quantity: Qty(10),
        buy_price: Some(Money(100)),
        ..DraftLine::default()
    };
    let errs = validate::draft(&draft(OrderKind::Purchase, vec![free.clone()])).unwrap_err();
    assert_eq!(
        errs.messages_for("price"),
        ["Vendosni çmimin e blerjes dhe të shitjes për «Mixed Row»."]
    );

    let complete = DraftLine {
        sell_price: Some(Money(150)),
        price: Money(100),
        ..free
    };
    assert_eq!(
        validate::draft(&draft(OrderKind::Purchase, vec![complete])),
        Ok(())
    );
}

#[test]
fn purchase_free_line_without_name() {
    let unnamed = DraftLine {
        quantity: Qty(2),
        buy_price: Some(Money(100)),
        sell_price: Some(Money(150)),
        ..DraftLine::default()
    };
    let errs = validate::draft(&draft(OrderKind::Purchase, vec![unnamed])).unwrap_err();
    assert_eq!(errs.messages_for("name"), ["Emri është i detyrueshëm"]);
}

#[test]
fn sale_line_needs_existing_product() {
    let typed = DraftLine {
        name: "Diçka".into(),
        quantity: Qty(1),
        ..DraftLine::default()
    };
    let errs = validate::draft(&draft(OrderKind::Sale, vec![typed])).unwrap_err();
    assert_eq!(
        errs.messages_for("product_id"),
        ["Zgjidhni një artikull ekzistues për porositë e shitjes."]
    );
}

// --- ReduceProductQuantityTest.php ------------------------------------------

#[test]
fn sale_over_stock_fails_with_the_exact_message() {
    // ReduceProductQuantityTest.php:37
    let s = stock(&[(1, 5)]);
    let err = stock::plan_change(
        OrderKind::Sale,
        NO_LINES,
        &[line(Some(1), "Test Product", 10)],
        &s,
    )
    .unwrap_err();
    assert_eq!(
        err,
        StockError::Insufficient {
            sku_id: SkuId(1),
            product_name: "Test Product".into(),
            available: Qty(5),
            required: Qty(10),
        }
    );
    assert_eq!(
        err.user_message(),
        "Gjendje e pamjaftueshme për «Test Product»: ka 5, kërkohen 10."
    );
}

#[test]
fn sale_may_take_stock_to_exactly_zero() {
    // ReduceProductQuantityTest.php:26
    let s = stock(&[(1, 10)]);
    assert_eq!(
        stock::plan_change(OrderKind::Sale, NO_LINES, &[line(Some(1), "A", 10)], &s),
        Ok(vec![(SkuId(1), Qty(-10))])
    );
}

// --- DeleteOrderTest.php ------------------------------------------------------

#[test]
fn deleting_a_sale_restores_stock() {
    // DeleteOrderTest.php:24
    let s = stock(&[(1, 5)]);
    assert_eq!(
        stock::plan_change(OrderKind::Sale, &[line(Some(1), "A", 5)], NO_LINES, &s),
        Ok(vec![(SkuId(1), Qty(5))])
    );
}

#[test]
fn deleting_an_order_aggregates_lines_per_sku() {
    // DeleteOrderTest.php:52, plus two lines on one SKU.
    let s = stock(&[(1, 7), (2, 13)]);
    let old = [
        line(Some(1), "A", 3),
        line(Some(2), "B", 7),
        line(Some(1), "A2", 4),
    ];
    assert_eq!(
        stock::plan_change(OrderKind::Sale, &old, NO_LINES, &s),
        Ok(vec![(SkuId(1), Qty(7)), (SkuId(2), Qty(7))])
    );
}

#[test]
fn lines_without_a_sku_are_ignored() {
    // DeleteOrder.php:26 `product_id &&` guard.
    let s = stock(&[]);
    let old = [line(None, "Fshirë", 50)];
    assert_eq!(
        stock::plan_change(OrderKind::Purchase, &old, NO_LINES, &s),
        Ok(vec![])
    );
}

// --- Q6 ---------------------------------------------------------------------

#[test]
fn deleting_a_sold_purchase_is_blocked() {
    // Bought 10, sold 6 since: stock 4, deleting would remove 10.
    let s = stock(&[(1, 4)]);
    let err = stock::plan_change(
        OrderKind::Purchase,
        &[line(Some(1), "Brizo", 10)],
        NO_LINES,
        &s,
    )
    .unwrap_err();
    assert!(matches!(
        err,
        StockError::PurchaseAlreadySold {
            action: PurchaseAction::Delete,
            ..
        }
    ));
    assert_eq!(
        err.user_message(),
        "Kjo porosi blerjeje nuk mund të fshihet: malli i saj është shitur tashmë \
         («Brizo»: gjendja 4, duhet të hiqen 10)."
    );
}

#[test]
fn editing_a_purchase_below_sold_is_blocked() {
    // Bought 10, sold 5 (stock 5); editing to 3 removes 7.
    let s = stock(&[(1, 5)]);
    let err = stock::plan_change(
        OrderKind::Purchase,
        &[line(Some(1), "Brizo", 10)],
        &[line(Some(1), "Brizo", 3)],
        &s,
    )
    .unwrap_err();
    assert_eq!(
        err.user_message(),
        "Kjo porosi blerjeje nuk mund të ndryshohet: malli i saj është shitur tashmë \
         («Brizo»: gjendja 5, duhet të hiqen 7)."
    );
}

#[test]
fn editing_a_purchase_down_to_exactly_the_unsold_rest_passes() {
    let s = stock(&[(1, 5)]);
    assert_eq!(
        stock::plan_change(
            OrderKind::Purchase,
            &[line(Some(1), "Brizo", 10)],
            &[line(Some(1), "Brizo", 5)],
            &s,
        ),
        Ok(vec![(SkuId(1), Qty(-5))])
    );
}

#[test]
fn editing_a_purchase_up_after_sales_passes() {
    // 10 → 12 with 5 sold: net +2, no transient failure.
    let s = stock(&[(1, 5)]);
    assert_eq!(
        stock::plan_change(
            OrderKind::Purchase,
            &[line(Some(1), "Brizo", 10)],
            &[line(Some(1), "Brizo", 12)],
            &s,
        ),
        Ok(vec![(SkuId(1), Qty(2))])
    );
}

#[test]
fn removing_a_purchase_line_by_edit_is_an_edit() {
    let s = stock(&[(1, 0), (2, 20)]);
    let old = [line(Some(1), "A", 4), line(Some(2), "B", 4)];
    let new = [line(Some(2), "B", 4)];
    let err = stock::plan_change(OrderKind::Purchase, &old, &new, &s).unwrap_err();
    assert!(matches!(
        err,
        StockError::PurchaseAlreadySold {
            action: PurchaseAction::Edit,
            sku_id: SkuId(1),
            available: Qty(0),
            required: Qty(4),
            ..
        }
    ));
}

// --- ImportTest.php -------------------------------------------------------------

#[test]
fn flavours_share_the_sku_pool() {
    // ImportTest.php:57: L0009 and L0009/1 share one stock pool of 10.
    assert_eq!(sku::base_code("L0009/1"), sku::base_code("L0009"));
    let s = stock(&[(9, 10)]);
    let new = [
        line(Some(9), "BRIZO 24*0.33LIT", 6),
        line(Some(9), "BRIZO PJESHKE 24*0.33LIT", 4),
    ];
    assert_eq!(
        stock::plan_change(OrderKind::Sale, NO_LINES, &new, &s),
        Ok(vec![(SkuId(9), Qty(-10))])
    );
    let more = [
        line(Some(9), "BRIZO 24*0.33LIT", 6),
        line(Some(9), "BRIZO PJESHKE 24*0.33LIT", 5),
    ];
    let err = stock::plan_change(OrderKind::Sale, NO_LINES, &more, &s).unwrap_err();
    assert_eq!(
        err.user_message(),
        "Gjendje e pamjaftueshme për «BRIZO 24*0.33LIT»: ka 10, kërkohen 11."
    );
}

#[test]
fn imported_prices_round_half_up() {
    // ImportTest.php:8: 12.345 → 12.35, 67.899 → 67.90.
    use crm_domain::ParseMode::Import;
    assert_eq!(format::parse_amount("12.345", Import), Ok(Money(1_235)));
    assert_eq!(format::parse_amount("67.899", Import), Ok(Money(6_790)));
}

// --- Acceptance criteria ------------------------------------------------------

#[test]
fn acceptance_examples() {
    assert_eq!(format::money(Money(123_456)), "1.234,56 Lekë");
    assert_eq!(format::money(Money(-5)), "-0,05 Lekë");
    assert_eq!(sku::base_code("L0009/3"), "L0009");
    assert_eq!(sku::base_code(" A0002 "), "A0002");
    assert_eq!(sku::base_code("A/B"), "A/B");
    assert_eq!(text::fold("Brizo Pjeshkë ÇAJ"), "brizo pjeshke caj");
}

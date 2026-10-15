//! AC 4 and step 4: every repository round-trips its entity.

use crm_domain::{
    Money, Order, OrderId, OrderKind, OrderLine, Owner, OwnerId, OwnerKind, OwnerSnapshot, Qty,
    Sku, SkuId,
};
use crm_storage::repo::orders::{self, OrderFilter};
use crm_storage::repo::skus::SkuPatch;
use crm_storage::repo::tabs::{self, OpenTab};
use crm_storage::repo::{owners, products, settings, skus};
use crm_storage::{DataPaths, Db, DbKey, StorageError, Tx};
use jiff::Timestamp;
use jiff::civil::{Date, date};

fn open() -> (tempfile::TempDir, Db) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&DataPaths::at(dir.path()), &DbKey::for_tests(1)).unwrap();
    (dir, db)
}

fn w<T: Send + 'static>(
    db: &Db,
    f: impl FnOnce(&Tx<'_>) -> Result<T, StorageError> + Send + 'static,
) -> T {
    db.write(f).unwrap()
}

fn sku(code: &str, quantity: i64) -> Sku {
    Sku {
        id: SkuId(0),
        code: code.into(),
        unit: "COPE".into(),
        buy_price: Money(6_200),
        sell_price: Money(6_500),
        quantity: Qty(quantity),
    }
}

fn owner(kind: OwnerKind, name: &str) -> Owner {
    Owner {
        id: OwnerId(0),
        kind,
        name: name.into(),
        description: Some("Tiranë".into()),
        nipt: Some("M02317046H".into()),
        address: None,
        account_code: None,
    }
}

fn ts(text: &str) -> Timestamp {
    text.parse().unwrap()
}

fn line(
    position: i64,
    product: Option<crm_domain::ProductId>,
    sku: Option<SkuId>,
    qty: i64,
) -> OrderLine {
    OrderLine {
        position,
        product_id: product,
        sku_id: sku,
        name: "BRIZO PJESHKË".into(),
        code: "L0009".into(),
        unit: "COPE".into(),
        quantity: Qty(qty),
        price: Money(6_500),
        value: Money(6_500 * qty),
    }
}

fn order(kind: OrderKind, issued_on: Date, lines: Vec<OrderLine>) -> Order {
    Order {
        id: OrderId(0),
        kind,
        owner_id: None,
        owner: OwnerSnapshot {
            name: "Klienti Alfa".into(),
            description: Some("Dyqani Qendër".into()),
            nipt: Some("K1".into()),
            address: Some("Rr. Pishave".into()),
        },
        issued_on,
        total: lines.iter().map(|l| l.value).sum(),
        created_at: ts("2026-10-14T08:00:00Z"),
        updated_at: ts("2026-10-14T09:30:00.5Z"),
        lines,
    }
}

#[test]
fn skus_round_trip() {
    let (_dir, db) = open();
    let id = w(&db, |tx| skus::insert(tx, &sku("L0009", 10)));
    let got = db.read(|c| skus::get(c, id)).unwrap();
    assert_eq!(
        got,
        Sku {
            id,
            ..sku("L0009", 10)
        }
    );
    assert_eq!(
        db.read(|c| skus::by_code(c, "L0009")).unwrap(),
        Some(got.clone())
    );
    assert_eq!(db.read(|c| skus::by_code(c, "nope")).unwrap(), None);
    assert_eq!(
        db.read(|c| skus::get(c, SkuId(999))),
        Err(StorageError::NotFound)
    );

    w(&db, |tx| skus::insert(tx, &sku("A0002", 0)));
    assert_eq!(db.read(skus::all_codes).unwrap(), ["A0002", "L0009"]);

    w(&db, move |tx| {
        skus::update_fields(
            tx,
            id,
            &SkuPatch {
                sell_price: Some(Money(7_000)),
                unit: Some("SHISHE".into()),
                ..SkuPatch::default()
            },
        )
    });
    let got = db.read(|c| skus::get(c, id)).unwrap();
    assert_eq!(
        (got.sell_price, got.unit.as_str(), got.buy_price),
        (Money(7_000), "SHISHE", Money(6_200))
    );

    w(&db, move |tx| skus::apply_delta(tx, id, Qty(-10)));
    assert_eq!(db.read(|c| skus::get(c, id)).unwrap().quantity, Qty(0));
    let below_zero = db.write(move |tx| skus::apply_delta(tx, id, Qty(-1)));
    assert!(
        matches!(below_zero, Err(StorageError::Constraint { .. })),
        "{below_zero:?}"
    );
    assert_eq!(
        db.write(|tx| skus::apply_delta(tx, SkuId(999), Qty(1))),
        Err(StorageError::NotFound)
    );

    let duplicate = db.write(|tx| skus::insert(tx, &sku("L0009", 1)));
    assert!(matches!(duplicate, Err(StorageError::Constraint { .. })));
}

#[test]
fn products_round_trip_and_orphan_sku_deletion() {
    let (_dir, db) = open();
    let (sku_id, p1, p2) = w(&db, |tx| {
        let sku_id = skus::insert(tx, &sku("L0009", 5))?;
        let p1 = products::insert(tx, sku_id, "BRIZO PJESHKË")?;
        let p2 = products::insert(tx, sku_id, "BRIZO MOLLË")?;
        Ok((sku_id, p1, p2))
    });
    let row = db.read(|c| products::get_row(c, p1)).unwrap();
    assert_eq!(row.product.name, "BRIZO PJESHKË");
    assert_eq!(row.product.sku_id, sku_id);
    assert_eq!(row.sku.code, "L0009");

    assert_eq!(
        db.read(|c| products::by_name_key(c, "brizo pjeshke"))
            .unwrap()
            .unwrap()
            .product
            .id,
        p1
    );
    assert_eq!(
        db.read(|c| products::by_name_key(c, "Brizo  Pjeshkë"))
            .unwrap()
            .unwrap()
            .product
            .id,
        p1
    );
    assert_eq!(
        db.read(move |c| products::by_sku_and_name(c, sku_id, "BRIZO MOLLË"))
            .unwrap()
            .unwrap()
            .id,
        p2
    );
    assert_eq!(
        db.read(move |c| products::by_sku_and_name(c, sku_id, "x"))
            .unwrap(),
        None
    );

    w(&db, move |tx| products::update_name(tx, p2, "BRIZO DARDHË"));
    assert_eq!(
        db.read(|c| products::by_name_key(c, "brizo dardhe"))
            .unwrap()
            .unwrap()
            .product
            .id,
        p2
    );

    // Deleting one flavour keeps the SKU; deleting the last one removes it (A11).
    let freed = w(&db, move |tx| {
        let s = products::delete(tx, p1)?;
        Ok((s, skus::delete_if_orphan(tx, s)?))
    });
    assert_eq!(freed, (sku_id, false));
    let freed = w(&db, move |tx| {
        let s = products::delete(tx, p2)?;
        Ok((s, skus::delete_if_orphan(tx, s)?))
    });
    assert_eq!(freed, (sku_id, true));
    assert_eq!(
        db.read(move |c| skus::get(c, sku_id)),
        Err(StorageError::NotFound)
    );
}

#[test]
fn product_list_filters_and_orders_by_code_then_name() {
    let (_dir, db) = open();
    w(&db, |tx| {
        let b = skus::insert(tx, &sku("L0002", 1))?;
        let a = skus::insert(tx, &sku("L0001", 1))?;
        products::insert(tx, b, "ÇAJ LIMON")?;
        products::insert(tx, a, "UJË")?;
        products::insert(tx, b, "ÇAJ BOROVINË")?;
        Ok(())
    });
    let names = |filter: &'static str, offset, limit| {
        db.read(move |c| products::list_rows(c, filter, offset, limit))
            .unwrap()
            .into_iter()
            .map(|r| r.product.name)
            .collect::<Vec<_>>()
    };
    assert_eq!(names("", 0, 100), ["UJË", "ÇAJ BOROVINË", "ÇAJ LIMON"]);
    assert_eq!(names("caj", 0, 100), ["ÇAJ BOROVINË", "ÇAJ LIMON"]);
    assert_eq!(names("Çaj limon", 0, 100), ["ÇAJ LIMON"]);
    assert_eq!(names("l0001", 0, 100), ["UJË"]);
    assert_eq!(names("", 1, 1), ["ÇAJ BOROVINË"]);
}

#[test]
fn order_count_is_derived_and_counts_an_order_once() {
    let (_dir, db) = open();
    let (sku_id, p1, p2) = w(&db, |tx| {
        let sku_id = skus::insert(tx, &sku("L0009", 50))?;
        Ok((
            sku_id,
            products::insert(tx, sku_id, "A")?,
            products::insert(tx, sku_id, "B")?,
        ))
    });
    w(&db, move |tx| {
        // The same product on two lines of one order.
        orders::insert(
            tx,
            &order(
                OrderKind::Sale,
                date(2026, 10, 14),
                vec![
                    line(0, Some(p1), Some(sku_id), 1),
                    line(1, Some(p1), Some(sku_id), 2),
                ],
            ),
        )?;
        orders::insert(
            tx,
            &order(
                OrderKind::Sale,
                date(2026, 10, 14),
                vec![line(0, Some(p1), Some(sku_id), 1)],
            ),
        )
    });
    let index = db.read(products::all_for_index).unwrap();
    let count = |id| index.iter().find(|r| r.id == id).unwrap().order_count;
    assert_eq!(count(p1), 2);
    assert_eq!(count(p2), 0);
    let row = index.iter().find(|r| r.id == p1).unwrap();
    assert_eq!(
        (
            row.code.as_str(),
            row.unit.as_str(),
            row.sell_price,
            row.quantity
        ),
        ("L0009", "COPE", Money(6_500), Qty(50))
    );
}

#[test]
fn owners_round_trip_and_byte_order() {
    let (_dir, db) = open();
    let ids = w(&db, |tx| {
        let mut ids = Vec::new();
        for name in ["A", "1A", "*KLIENTE", "b"] {
            ids.push(owners::insert(tx, &owner(OwnerKind::Client, name))?);
        }
        let mut supplier = owner(OwnerKind::Supplier, "Furnitori Ë");
        supplier.account_code = Some("401".into());
        ids.push(owners::insert(tx, &supplier)?);
        Ok(ids)
    });
    let list = |kind, filter: &'static str| {
        db.read(move |c| owners::list(c, kind, filter, 0, 100))
            .unwrap()
            .into_iter()
            .map(|o| o.name)
            .collect::<Vec<_>>()
    };
    assert_eq!(list(OwnerKind::Client, ""), ["*KLIENTE", "1A", "A", "b"]);
    assert_eq!(list(OwnerKind::Supplier, ""), ["Furnitori Ë"]);
    assert_eq!(list(OwnerKind::Supplier, "furnitori e"), ["Furnitori Ë"]);
    assert_eq!(list(OwnerKind::Client, "kliente"), ["*KLIENTE"]);

    let supplier = db
        .read(|c| owners::by_account(c, OwnerKind::Supplier, "401"))
        .unwrap()
        .unwrap();
    assert_eq!(supplier.id, ids[4]);
    assert_eq!(
        db.read(|c| owners::by_account(c, OwnerKind::Client, "401"))
            .unwrap(),
        None
    );
    assert_eq!(
        db.read(|c| owners::by_name_key(c, OwnerKind::Supplier, "FURNITORI E"))
            .unwrap()
            .unwrap()
            .id,
        ids[4]
    );
    assert_eq!(
        db.read(|c| owners::by_nipt(c, OwnerKind::Client, "M02317046H"))
            .unwrap()
            .len(),
        4
    );
    assert_eq!(db.read(owners::all_for_index).unwrap().len(), 5);

    let first = ids[0];
    let mut changed = db.read(move |c| owners::get(c, first)).unwrap();
    assert_eq!(
        changed,
        Owner {
            id: ids[0],
            ..owner(OwnerKind::Client, "A")
        }
    );
    changed.name = "Alfa".into();
    changed.address = Some("Durrës".into());
    let expected = changed.clone();
    w(&db, move |tx| owners::update(tx, &changed));
    assert_eq!(db.read(move |c| owners::get(c, first)).unwrap(), expected);

    let mut duplicate = owner(OwnerKind::Supplier, "Tjetër");
    duplicate.account_code = Some("401".into());
    assert!(matches!(
        db.write(move |tx| owners::insert(tx, &duplicate)),
        Err(StorageError::Constraint { .. })
    ));
}

#[test]
fn deleting_an_owner_keeps_its_orders() {
    let (_dir, db) = open();
    let (owner_id, order_id) = w(&db, |tx| {
        let owner_id = owners::insert(tx, &owner(OwnerKind::Supplier, "Furnitori"))?;
        let mut o = order(OrderKind::Purchase, date(2026, 10, 14), vec![]);
        o.owner_id = Some(owner_id);
        Ok((owner_id, orders::insert(tx, &o)?))
    });
    w(&db, move |tx| owners::delete(tx, owner_id));
    let o = db.read(move |c| orders::get(c, order_id)).unwrap();
    assert_eq!(o.owner_id, None);
    assert_eq!(o.owner.name, "Klienti Alfa");
    assert_eq!(
        db.write(move |tx| owners::delete(tx, owner_id)),
        Err(StorageError::NotFound)
    );
}

#[test]
fn orders_round_trip_replace_and_delete() {
    let (_dir, db) = open();
    let (sku_id, p1) = w(&db, |tx| {
        let s = skus::insert(tx, &sku("L0009", 50))?;
        Ok((s, products::insert(tx, s, "BRIZO PJESHKË")?))
    });
    let original = order(
        OrderKind::Sale,
        date(2026, 10, 13),
        vec![line(0, Some(p1), Some(sku_id), 2), line(1, None, None, 1)],
    );
    let to_insert = original.clone();
    let id = w(&db, move |tx| orders::insert(tx, &to_insert));
    let got = db.read(move |c| orders::get(c, id)).unwrap();
    assert_eq!(
        got,
        Order {
            id,
            ..original.clone()
        }
    );

    let mut edited = got.clone();
    edited.issued_on = date(2026, 10, 12);
    edited.lines = vec![line(0, Some(p1), Some(sku_id), 5)];
    edited.total = Money(32_500);
    edited.updated_at = ts("2026-10-15T10:00:00Z");
    edited.created_at = ts("2000-01-01T00:00:00Z"); // ignored by replace
    let to_replace = edited.clone();
    w(&db, move |tx| orders::replace(tx, &to_replace));
    let got = db.read(move |c| orders::get(c, id)).unwrap();
    assert_eq!(
        got,
        Order {
            created_at: original.created_at,
            ..edited
        }
    );

    w(&db, move |tx| orders::delete(tx, id));
    assert_eq!(
        db.read(move |c| orders::get(c, id)),
        Err(StorageError::NotFound)
    );
    // Numbers are never reused.
    let next = w(&db, |tx| {
        orders::insert(tx, &order(OrderKind::Sale, date(2026, 10, 13), vec![]))
    });
    assert!(next.0 > id.0);
}

#[test]
fn deleting_a_product_keeps_the_line_snapshot() {
    let (_dir, db) = open();
    let (sku_id, p1) = w(&db, |tx| {
        let s = skus::insert(tx, &sku("L0009", 50))?;
        let p = products::insert(tx, s, "BRIZO PJESHKË")?;
        products::insert(tx, s, "BRIZO MOLLË")?;
        Ok((s, p))
    });
    let id = w(&db, move |tx| {
        orders::insert(
            tx,
            &order(
                OrderKind::Sale,
                date(2026, 10, 14),
                vec![line(0, Some(p1), Some(sku_id), 2)],
            ),
        )
    });
    w(&db, move |tx| products::delete(tx, p1).map(|_| ()));
    let o = db.read(move |c| orders::get(c, id)).unwrap();
    assert_eq!(o.lines[0].product_id, None);
    assert_eq!(o.lines[0].sku_id, Some(sku_id));
    assert_eq!(
        (o.lines[0].name.as_str(), o.lines[0].code.as_str()),
        ("BRIZO PJESHKË", "L0009")
    );
}

#[test]
fn order_list_filters_by_issued_on_kind_and_text() {
    let (_dir, db) = open();
    let today = date(2026, 10, 14);
    let yesterday = date(2026, 10, 13);
    let ids = w(&db, move |tx| {
        // Issued yesterday, modified today.
        let old = orders::insert(tx, &order(OrderKind::Sale, yesterday, vec![]))?;
        let mut a = order(OrderKind::Sale, today, vec![]);
        a.owner.name = "Bar Kafe Ëndrra".into();
        a.owner.description = None;
        let a = orders::insert(tx, &a)?;
        let b = orders::insert(tx, &order(OrderKind::Purchase, today, vec![]))?;
        Ok([old, a, b])
    });
    let list = |kind, from, to, text: Option<&str>| {
        let filter = OrderFilter {
            kind,
            from,
            to,
            text: text.map(String::from),
        };
        db.read(move |c| orders::list(c, &filter))
            .unwrap()
            .into_iter()
            .map(|s| s.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(list(None, today, today, None), [ids[2], ids[1]]);
    assert_eq!(list(None, yesterday, today, None), [ids[2], ids[1], ids[0]]);
    assert_eq!(list(Some(OrderKind::Sale), today, today, None), [ids[1]]);
    assert_eq!(list(None, today, today, Some("endrra")), [ids[1]]);
    assert_eq!(list(None, today, today, Some("bar ËNDRRA")), [ids[1]]);
    assert_eq!(list(None, today, today, Some("qender")), [ids[2]]);
    // All digits: the order number, ignoring the dates.
    let number = ids[0].0.to_string();
    let n: &'static str = Box::leak(number.into_boxed_str());
    assert_eq!(list(None, today, today, Some(n)), [ids[0]]);
    assert_eq!(
        list(Some(OrderKind::Purchase), today, today, Some(n)),
        Vec::<OrderId>::new()
    );
    assert_eq!(
        list(None, today, today, Some("99999999999999999999")),
        Vec::<OrderId>::new()
    );

    let summary = db
        .read(move |c| {
            orders::list(
                c,
                &OrderFilter {
                    kind: None,
                    from: yesterday,
                    to: yesterday,
                    text: None,
                },
            )
        })
        .unwrap();
    assert_eq!(summary[0].owner_name, "Klienti Alfa");
    assert_eq!(
        summary[0].owner_description.as_deref(),
        Some("Dyqani Qendër")
    );
    assert_eq!(summary[0].issued_on, yesterday);
    assert_eq!(summary[0].updated_at, ts("2026-10-14T09:30:00.5Z"));
}

#[test]
fn report_lines_are_ordered_by_issued_on_id_position() {
    let (_dir, db) = open();
    let ids = w(&db, |tx| {
        let late = orders::insert(
            tx,
            &order(
                OrderKind::Sale,
                date(2026, 10, 14),
                vec![line(1, None, None, 1), line(0, None, None, 2)],
            ),
        )?;
        let early = orders::insert(
            tx,
            &order(
                OrderKind::Purchase,
                date(2026, 10, 1),
                vec![line(0, None, None, 3)],
            ),
        )?;
        let outside = orders::insert(
            tx,
            &order(
                OrderKind::Sale,
                date(2026, 9, 30),
                vec![line(0, None, None, 4)],
            ),
        )?;
        Ok([late, early, outside])
    });
    let rows = db
        .read(|c| orders::lines_for_report(c, date(2026, 10, 1), date(2026, 10, 31), None))
        .unwrap();
    let got: Vec<(OrderId, i64)> = rows.iter().map(|(h, l)| (h.id, l.position)).collect();
    assert_eq!(got, [(ids[1], 0), (ids[0], 0), (ids[0], 1)]);
    assert_eq!(rows[0].0.kind, OrderKind::Purchase);
    assert_eq!(rows[0].1.quantity, Qty(3));
    let sales = db
        .read(|c| {
            orders::lines_for_report(
                c,
                date(2026, 10, 1),
                date(2026, 10, 31),
                Some(OrderKind::Sale),
            )
        })
        .unwrap();
    assert_eq!(sales.len(), 2);
}

#[test]
fn tabs_round_trip_with_drafts() {
    let (_dir, db) = open();
    let order_id = w(&db, |tx| {
        orders::insert(tx, &order(OrderKind::Sale, date(2026, 10, 14), vec![]))
    });
    let tab = |key: &str, position, order_id, draft: Option<&str>| OpenTab {
        tab_key: key.into(),
        position,
        kind: OrderKind::Sale,
        order_id,
        title: "Porosi Shitje (e re)".into(),
        active: false,
        draft_json: draft.map(String::from),
        updated_at: ts("2026-10-14T10:00:00Z"),
    };
    let a = tab(
        "t-a",
        1,
        None,
        Some(r#"{"kind":0,"lines":[{"name":"ÇAJ"}]}"#),
    );
    let b = tab("t-b", 0, Some(order_id), None);
    let (a2, b2) = (a.clone(), b.clone());
    w(&db, move |tx| {
        tabs::upsert(tx, &a2)?;
        tabs::upsert(tx, &b2)?;
        tabs::set_active(tx, "t-a")
    });
    let listed = db.read(tabs::list).unwrap();
    assert_eq!(
        listed,
        [
            b.clone(),
            OpenTab {
                active: true,
                ..a.clone()
            }
        ]
    );

    let mut edited = a.clone();
    edited.draft_json = None;
    edited.title = "Porosia #7".into();
    let e2 = edited.clone();
    w(&db, move |tx| tabs::upsert(tx, &e2));
    assert_eq!(db.read(tabs::list).unwrap()[1], edited.clone());

    // Deleting the order closes its tab.
    w(&db, move |tx| orders::delete(tx, order_id));
    assert_eq!(db.read(tabs::list).unwrap().len(), 1);
    assert!(w(&db, |tx| tabs::remove(tx, "t-a")));
    assert!(!w(&db, |tx| tabs::remove(tx, "t-a")));
    // A tab of a deleted order cannot come back.
    let stale = db.write(move |tx| tabs::upsert(tx, &b));
    assert!(matches!(stale, Err(StorageError::Constraint { .. })));
    w(&db, move |tx| tabs::upsert(tx, &edited));
    w(&db, tabs::clear_all);
    assert!(db.read(tabs::list).unwrap().is_empty());
}

#[test]
fn settings_round_trip_and_defaults() {
    let (_dir, db) = open();
    assert_eq!(db.read(|c| settings::get(c, "x")).unwrap(), None);
    assert_eq!(db.read(settings::idle_lock_minutes).unwrap(), 0);
    assert_eq!(db.read(settings::receipt_width_mm).unwrap(), 72);
    assert_eq!(db.read(settings::receipt_font_pt).unwrap(), 9);
    assert_eq!(db.read(settings::last_backup_at).unwrap(), None);

    let at = ts("2026-10-14T10:00:00Z");
    w(&db, move |tx| {
        settings::set(tx, "x", "1")?;
        settings::set(tx, "x", "2")?;
        settings::set_idle_lock_minutes(tx, 15)?;
        settings::set_receipt_width_mm(tx, 58)?;
        settings::set_receipt_font_pt(tx, 10)?;
        settings::set_last_backup_at(tx, at)
    });
    assert_eq!(
        db.read(|c| settings::get(c, "x")).unwrap().as_deref(),
        Some("2")
    );
    assert_eq!(db.read(settings::idle_lock_minutes).unwrap(), 15);
    assert_eq!(db.read(settings::receipt_width_mm).unwrap(), 58);
    assert_eq!(db.read(settings::receipt_font_pt).unwrap(), 10);
    assert_eq!(db.read(settings::last_backup_at).unwrap(), Some(at));
}

#[test]
fn reads_inside_a_write_see_its_own_changes() {
    let (_dir, db) = open();
    let found = w(&db, |tx| {
        let id = skus::insert(tx, &sku("L0001", 1))?;
        Ok(skus::get(tx, id)?.code)
    });
    assert_eq!(found, "L0001");
}

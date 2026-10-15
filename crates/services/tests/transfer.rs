//! Import, export and reports (007 step 4, AC 6 and 9).

mod common;

use common::{draft, fixture, line, open, product};
use crm_services::domain::{Money, OrderKind, OwnerKind, Qty};
use crm_services::{Cell, Fault, ImportKind, Page, ReportKind, ReportParams, ServiceError};
use std::path::Path;

fn pre_import_backups(crm: &crm_services::Crm, kind: &str) -> usize {
    crm.list_backups()
        .unwrap()
        .iter()
        .filter(|b| b.name.reason.as_deref() == Some(&format!("pre-import-{kind}")))
        .count()
}

#[test]
fn artikuj_into_an_empty_db_then_again() {
    let (_dir, crm) = open();
    let report = crm
        .import(ImportKind::Products, &fixture("ARTIKUJ.csv"))
        .unwrap();
    assert_eq!(
        (
            report.created,
            report.updated,
            report.skipped,
            report.failed
        ),
        (776, 0, 0, 0),
        "{:?}",
        report.errors
    );
    assert_eq!(pre_import_backups(&crm, "products"), 1);

    let rows = crm.list_products("", Page::ALL).unwrap();
    assert_eq!(rows.len(), 776);
    let mut skus: Vec<_> = rows.iter().map(|r| r.sku.id).collect();
    skus.sort();
    skus.dedup();
    assert_eq!(skus.len(), 766, "10 flavour rows join their base SKU");

    // L0009 keeps its base row's Gjendje; its flavours (Gjendje 0) don't clobber it.
    let l0009: Vec<_> = rows.iter().filter(|r| r.sku.code == "L0009").collect();
    assert_eq!(l0009.len(), 6);
    let base_qty = base_row_quantity("L0009");
    assert!(base_qty > 0);
    assert!(l0009.iter().all(|r| r.sku.quantity == Qty(base_qty)));

    // The index was rebuilt.
    assert!(!crm.search_products("amaro korabi", 5).unwrap().is_empty());

    let again = crm
        .import(ImportKind::Products, &fixture("ARTIKUJ.csv"))
        .unwrap();
    assert_eq!(
        (again.created, again.updated, again.skipped, again.failed),
        (0, 0, 776, 0)
    );
    assert_eq!(pre_import_backups(&crm, "products"), 2);
}

/// The Gjendje of the ARTIKUJ row whose code is exactly `code`.
fn base_row_quantity(code: &str) -> i64 {
    let text = std::fs::read_to_string(fixture("ARTIKUJ.csv")).unwrap();
    let mut lines = text.lines();
    let header: Vec<_> = lines.next().unwrap().split(',').collect();
    let qty_col = header.iter().position(|h| *h == "Gjendje").unwrap();
    for l in lines {
        let cells: Vec<_> = l.split(',').collect();
        if cells[0] == code {
            return cells[qty_col].trim().parse().unwrap();
        }
    }
    panic!("no row {code}");
}

#[test]
fn a_changed_base_row_counts_as_updated() {
    let dir = tempfile::tempdir().unwrap();
    let (_db, crm) = open();
    let file = dir.path().join("a.csv");
    std::fs::write(
        &file,
        "Kodi,Përshkrimi,Njësi,Gjendje,CMIM BLERJE,CMIM SHITJE\n\
         X1,ALFA,COPE,5,1,2\nX1/1,ALFA MOLLE,COPE,0,1,2\n",
    )
    .unwrap();
    let r = crm.import(ImportKind::Products, &file).unwrap();
    assert_eq!((r.created, r.updated, r.skipped), (2, 0, 0));
    std::fs::write(
        &file,
        "Kodi,Përshkrimi,Njësi,Gjendje,CMIM BLERJE,CMIM SHITJE\n\
         X1/1,ALFA MOLLE,COPE,99,7,7\nX1,ALFA,COPE,9,1.5,2\nX2,,COPE,1,1,1\n",
    )
    .unwrap();
    let r = crm.import(ImportKind::Products, &file).unwrap();
    // The flavour row is unchanged (it never sets the SKU); the base row changed it.
    assert_eq!((r.created, r.updated, r.skipped, r.failed), (0, 1, 1, 1));
    assert_eq!(
        r.errors[0].user_message(),
        "Rreshti 4: Emri është i detyrueshëm"
    );
    let row = &crm.search_products("alfa molle", 1).unwrap()[0];
    assert_eq!(
        (row.sku.quantity, row.sku.buy_price, row.sku.sell_price),
        (Qty(9), Money(150), Money(200))
    );
}

#[test]
fn klientet_and_furnitoret_counts() {
    let (dir, crm) = open();
    let r = crm
        .import(ImportKind::Clients, &fixture("KLIENTET.csv"))
        .unwrap();
    assert_eq!(
        (r.created, r.updated, r.skipped, r.failed),
        (2_931, 0, 0, 0),
        "{:?}",
        r.errors
    );
    assert_eq!(pre_import_backups(&crm, "clients"), 1);
    let again = crm
        .import(ImportKind::Clients, &fixture("KLIENTET.csv"))
        .unwrap();
    assert_eq!((again.created, again.updated, again.skipped), (0, 0, 2_931));

    // One changed address → updated 1.
    let text = std::fs::read_to_string(fixture("KLIENTET.csv")).unwrap();
    let changed = text.replacen("LIVADHET, KAMEZ, TIRANE", "RRUGA E RE, TIRANE", 1);
    assert_ne!(changed, text);
    let file = dir.path().join("KLIENTET-changed.csv");
    std::fs::write(&file, changed).unwrap();
    let r = crm.import(ImportKind::Clients, &file).unwrap();
    assert_eq!((r.created, r.updated, r.skipped), (0, 1, 2_930));
    let three_yje = crm.search_owners(OwnerKind::Client, "3 YJE", 1).unwrap();
    assert_eq!(three_yje[0].address.as_deref(), Some("RRUGA E RE, TIRANE"));

    // Shared NIPTs stay separate owners (distinct Llogari).
    assert_eq!(
        crm.list_owners(OwnerKind::Client, "M02317046H", Page::ALL)
            .unwrap()
            .len(),
        2
    );

    let r = crm
        .import(ImportKind::Suppliers, &fixture("FURNITORET.csv"))
        .unwrap();
    assert_eq!((r.created, r.updated, r.skipped, r.failed), (186, 0, 0, 0));
    let r = crm
        .import(ImportKind::Suppliers, &fixture("FURNITORET.csv"))
        .unwrap();
    assert_eq!((r.created, r.skipped), (0, 186));
    assert_eq!(
        crm.list_owners(OwnerKind::Supplier, "", Page::ALL)
            .unwrap()
            .len(),
        186
    );
    assert_eq!(pre_import_backups(&crm, "suppliers"), 2);
}

#[test]
fn owner_rows_match_by_nipt_then_name_and_never_clear_fields() {
    let dir = tempfile::tempdir().unwrap();
    let (_db, crm) = open();
    let file = dir.path().join("f.csv");
    std::fs::write(&file, "Emër,Adresa,NIPT\nALFA,TIRANE,K1\nBETA,DURRES,\n").unwrap();
    assert_eq!(crm.import(ImportKind::Suppliers, &file).unwrap().created, 2);
    // Same NIPT under a new name → the same owner, renamed; BETA by name; an
    // empty address never clears the stored one.
    std::fs::write(&file, "Emër,Adresa,NIPT\nALFA SHPK,TIRANE,k1\nbeta,,\n").unwrap();
    let r = crm.import(ImportKind::Suppliers, &file).unwrap();
    assert_eq!((r.created, r.updated, r.skipped), (0, 2, 0));
    let all = crm.list_owners(OwnerKind::Supplier, "", Page::ALL).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].name, "ALFA SHPK");
    assert_eq!(all[1].address.as_deref(), Some("DURRES"));
}

#[test]
fn a_file_level_problem_fails_before_any_change() {
    let dir = tempfile::tempdir().unwrap();
    let (_db, crm) = open();
    let file = dir.path().join("x.csv");
    std::fs::write(&file, "Foo,Bar\n1,2\n").unwrap();
    let err = crm.import(ImportKind::Products, &file).unwrap_err();
    assert!(matches!(err, ServiceError::Io(_)), "{err:?}");
    assert!(
        err.user_message()
            .starts_with("Skedari nuk ka kolonat e nevojshme")
    );
    assert_eq!(pre_import_backups(&crm, "products"), 0);
    let missing = crm
        .import(ImportKind::Clients, Path::new("/nope/none.csv"))
        .unwrap_err();
    assert!(matches!(missing, ServiceError::Io(_)));
}

#[test]
fn export_round_trips_through_import() {
    let (dir, crm) = open();
    crm.import(ImportKind::Products, &fixture("ARTIKUJ.csv"))
        .unwrap();
    crm.import(ImportKind::Clients, &fixture("KLIENTET.csv"))
        .unwrap();
    let products = dir.path().join("artikuj.csv");
    assert_eq!(
        crm.export_csv(ImportKind::Products, &products).unwrap(),
        776
    );
    let clients = dir.path().join("klientet.csv");
    assert_eq!(
        crm.export_csv(ImportKind::Clients, &clients).unwrap(),
        2_931
    );
    let r = crm.import(ImportKind::Products, &products).unwrap();
    assert_eq!((r.created, r.updated, r.skipped, r.failed), (0, 0, 776, 0));
    let r = crm.import(ImportKind::Clients, &clients).unwrap();
    assert_eq!(
        (r.created, r.updated, r.skipped, r.failed),
        (0, 0, 2_931, 0)
    );
}

#[test]
fn reports_use_issued_on_and_label_fs_fb() {
    let (dir, crm) = open();
    let p = product(&crm, "P1", "BRIZO", 100, 100, 200);
    let mut sale = draft(OrderKind::Sale, "Bar Alfa", vec![line(p.product.id, 2, 0)]);
    sale.issued_on = jiff::civil::date(2026, 3, 10);
    let sale_id = crm.save_order(sale, None, None).unwrap();
    let mut purchase = draft(
        OrderKind::Purchase,
        "Furnitori",
        vec![common::buy_line(Some(p.product.id), "", 5, 100, 200)],
    );
    purchase.issued_on = jiff::civil::date(2026, 3, 11);
    let purchase_id = crm.save_order(purchase, None, None).unwrap();
    // Saved today, issued outside the range: excluded (Q3).
    let mut late = draft(OrderKind::Sale, "Bar Beta", vec![line(p.product.id, 1, 0)]);
    late.issued_on = jiff::civil::date(2026, 4, 1);
    crm.save_order(late, None, None).unwrap();

    let params = ReportParams {
        kind: ReportKind::Total,
        from: jiff::civil::date(2026, 3, 1),
        to: jiff::civil::date(2026, 3, 31),
        order_kind: None,
    };
    let preview = crm.report_preview(params).unwrap();
    assert_eq!((preview.total, preview.truncated), (2, false));
    let rows = &preview.table.rows;
    assert_eq!(rows.len(), 2);
    let lloji = preview
        .table
        .headers
        .iter()
        .position(|h| h == "Lloji")
        .expect("Lloji column");
    assert_eq!(rows[0][lloji], Cell::Text("Shitje".into()));
    assert_eq!(rows[1][lloji], Cell::Text("Blerje".into()));
    let flat = format!("{rows:?}");
    assert!(flat.contains(&format!("Int({})", sale_id.0)), "{flat}");
    assert!(flat.contains(&format!("Int({})", purchase_id.0)), "{flat}");

    let only_sales = ReportParams {
        order_kind: Some(OrderKind::Sale),
        ..params
    };
    assert_eq!(crm.report_preview(only_sales).unwrap().total, 1);
    let by_products = ReportParams {
        kind: ReportKind::ByProducts,
        ..params
    };
    assert!(
        !crm.report_preview(by_products)
            .unwrap()
            .table
            .rows
            .is_empty()
    );

    let path = dir
        .path()
        .join(crm_services::report_file_name(params.from, params.to));
    crm.export_report_xlsx(params, &path).unwrap();
    assert!(std::fs::metadata(&path).unwrap().len() > 0);
}

#[test]
fn the_report_preview_is_capped_at_500_rows() {
    let (_dir, crm) = open();
    let p = product(&crm, "P1", "BRIZO", 10_000, 100, 200);
    let lines = (0..260).map(|_| line(p.product.id, 1, 0)).collect();
    let mut d = draft(OrderKind::Sale, "K", lines);
    d.issued_on = jiff::civil::date(2026, 5, 5);
    crm.save_order(d.clone(), None, None).unwrap();
    crm.save_order(d, None, None).unwrap();
    let preview = crm
        .report_preview(ReportParams {
            kind: ReportKind::Total,
            from: jiff::civil::date(2026, 5, 5),
            to: jiff::civil::date(2026, 5, 5),
            order_kind: None,
        })
        .unwrap();
    assert_eq!(preview.table.rows.len(), 500);
    assert!(preview.truncated);
    assert!(preview.total > 500);
}

#[test]
fn a_committed_import_is_ok_when_the_index_rebuild_fails() {
    // F-013 variant: `import` reported the committed rows as a failure.
    let dir = tempfile::tempdir().unwrap();
    let (_db, crm) = open();
    let file = dir.path().join("a.csv");
    std::fs::write(
        &file,
        "Kodi,Përshkrimi,Njësi,Gjendje,CMIM BLERJE,CMIM SHITJE\nX1,QAQAQA,COPE,5,1,2\n",
    )
    .unwrap();
    crm.inject_fault(Fault::IndexRead);
    let r = crm
        .import(ImportKind::Products, &file)
        .expect("committed, so Ok");
    assert_eq!(r.created, 1);
    assert!(crm.search_products("qaqaqa", 5).unwrap().is_empty());
    assert!(crm.refresh_if_changed());
    assert_eq!(crm.search_products("qaqaqa", 5).unwrap().len(), 1);
}

#[test]
fn an_import_that_fails_mid_transaction_changes_nothing() {
    // F-002: every row is applied, then the transaction fails: all-or-nothing.
    let dir = tempfile::tempdir().unwrap();
    let (_db, crm) = open();
    product(&crm, "X1", "ALFA", 5, 1, 2);
    let file = dir.path().join("a.csv");
    std::fs::write(
        &file,
        "Kodi,Përshkrimi,Njësi,Gjendje,CMIM BLERJE,CMIM SHITJE\n\
         X1,ALFA,COPE,99,7,8\nX2,BETA,COPE,1,1,1\nX3,GAMA,COPE,1,1,1\n",
    )
    .unwrap();
    let before = crm.list_products("", Page::default()).unwrap();
    crm.inject_fault(Fault::ImportAfterApply);
    assert!(crm.import(ImportKind::Products, &file).is_err());
    assert_eq!(
        crm.list_products("", Page::default()).unwrap(),
        before,
        "rolled back"
    );
    assert!(crm.search_products("beta", 5).unwrap().is_empty());
    assert!(!crm.refresh_if_changed(), "nothing committed by the import");
    // The same file imports normally afterwards.
    let r = crm.import(ImportKind::Products, &file).unwrap();
    assert_eq!((r.created, r.updated), (2, 1));
    assert_eq!(crm.list_products("", Page::default()).unwrap().len(), 3);
}

#[test]
fn the_order_list_export_matches_the_filtered_list() {
    let (dir, crm) = open();
    let p = product(&crm, "P1", "BRIZO", 100, 100, 200);
    let day = jiff::civil::date(2026, 3, 10);
    let mut first = draft(
        OrderKind::Sale,
        "Bar Alfa",
        vec![line(p.product.id, 1, 123_456)],
    );
    first.issued_on = day;
    first.owner_description = "Tek ura".into();
    let first = crm.save_order(first, None, None).unwrap();
    let mut second = draft(
        OrderKind::Sale,
        "Bar Beta",
        vec![line(p.product.id, 2, 100)],
    );
    second.issued_on = day.tomorrow().unwrap();
    let second = crm.save_order(second, None, None).unwrap();
    let mut purchase = draft(
        OrderKind::Purchase,
        "Furnitori",
        vec![common::buy_line(Some(p.product.id), "", 5, 100, 200)],
    );
    purchase.issued_on = day;
    crm.save_order(purchase, None, None).unwrap();

    let filter = crm_services::OrderFilter {
        kind: Some(OrderKind::Sale),
        from: jiff::civil::date(2026, 3, 1),
        to: jiff::civil::date(2026, 3, 31),
        text: None,
    };
    let table = crm.order_list_table(filter.clone()).unwrap();
    assert_eq!(table.title, "LISTA E POROSIVE");
    assert_eq!(table.subtitle, "01.03.2026 - 31.03.2026 · Shitje");
    assert_eq!(
        table.headers,
        [
            "Nr.",
            "Lloji",
            "Klienti",
            "Përshkrimi",
            "Totali (LEK)",
            "Data",
            "Modifikuar"
        ]
    );
    // Two sales (newest first), then the Totali row.
    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.rows[0][0], Cell::Int(second.0));
    assert_eq!(table.rows[1][0], Cell::Int(first.0));
    assert_eq!(table.rows[1][1], Cell::Text("Shitje".into()));
    assert_eq!(table.rows[1][2], Cell::Text("Bar Alfa".into()));
    assert_eq!(table.rows[1][3], Cell::Text("Tek ura".into()));
    assert_eq!(table.rows[1][4], Cell::AmountText(Money(123_456)));
    assert_eq!(table.rows[1][5], Cell::Text("10.03.2026".into()));
    match &table.rows[1][6] {
        Cell::Text(t) => assert_eq!(t.len(), "dd.MM.yyyy HH:mm".len(), "{t}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(table.rows[2][0], Cell::Text("Totali".into()));
    assert_eq!(table.rows[2][4], Cell::AmountText(Money(123_656)));
    assert_eq!(
        crm_services::domain::format::amount(Money(123_456)),
        "1.234,56"
    );

    // A digits-only search is the order number, whatever the dates.
    let by_number = crm_services::OrderFilter {
        text: Some(first.0.to_string()),
        from: jiff::civil::date(2020, 1, 1),
        to: jiff::civil::date(2020, 1, 1),
        ..filter.clone()
    };
    let table = crm.order_list_table(by_number).unwrap();
    assert_eq!(table.rows.len(), 2);
    assert_eq!(table.rows[0][0], Cell::Int(first.0));
    assert!(
        table
            .subtitle
            .ends_with(&format!(" · Kërko: «{}»", first.0))
    );

    // Furnitori for Blerje.
    let purchases = crm_services::OrderFilter {
        kind: Some(OrderKind::Purchase),
        ..filter.clone()
    };
    let table = crm.order_list_table(purchases).unwrap();
    assert_eq!(table.headers[2], "Furnitori");
    assert_eq!(table.rows.len(), 2);

    let path = dir.path().join("porosite.xlsx");
    crm.export_order_list_xlsx(filter, &path).unwrap();
    assert!(std::fs::metadata(&path).unwrap().len() > 0);
}

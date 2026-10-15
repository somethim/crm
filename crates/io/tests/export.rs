//! CSV export round-trips (AC 7) and the XLSX report layout (AC 8, 9).

use calamine::{Data, Reader, Xlsx};
use crm_domain::{Money, OrderId, OrderKind, Owner, OwnerId, OwnerKind, ProductId, Qty};
use crm_io::report::{self, ReportKind, ReportLine};
use crm_io::{
    OwnerImportRow, ProductExportRow, parse_owners, parse_products, table, write_owners,
    write_products, write_xlsx,
};
use jiff::civil::date;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn products_round_trip() {
    let parsed = parse_products(&table::read(&fixture("ARTIKUJ.csv")).unwrap()).unwrap();
    let rows: Vec<ProductExportRow> = parsed
        .rows
        .iter()
        .map(|(_, r)| ProductExportRow {
            code: r.code.clone(),
            name: r.name.clone(),
            unit: r.unit.clone(),
            quantity: r.quantity,
            buy_price: r.buy_price,
            sell_price: r.sell_price,
        })
        .chain([ProductExportRow {
            code: "Z1".into(),
            name: "EMËR, ME \"THONJËZA\"".into(),
            unit: "KG".into(),
            quantity: Qty(1_234),
            buy_price: Money(5),
            sell_price: Money(123_456_789),
        }])
        .collect();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("artikujt.csv");
    write_products(&path, &rows).unwrap();
    assert!(!dir.path().join("artikujt.csv.tmp").exists());

    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(b"\xEF\xBB\xBF"));
    let text = std::str::from_utf8(&bytes[3..]).unwrap();
    let mut lines = text.split("\r\n");
    assert_eq!(
        lines.next(),
        Some("Kodi,Përshkrimi,Njësi,Gjendje,CMIM BLERJE,CMIM SHITJE")
    );
    assert_eq!(
        lines.next(),
        Some("A0002,LIKER MORENA 0.5LIT (LIQUER),SHISHE,6,1080,1200")
    );
    assert!(text.contains("\r\nCOK0024,FARA KUNGULLI (6*50CP),COPE,412,7.4,7.9\r\n"));
    assert!(text.contains("\r\nZ1,\"EMËR, ME \"\"THONJËZA\"\"\",KG,1234,0.05,1234567.89\r\n"));

    let again = parse_products(&table::read(&path).unwrap()).unwrap();
    assert_eq!(again.errors, []);
    let again: Vec<ProductExportRow> = again
        .rows
        .into_iter()
        .map(|(_, r)| ProductExportRow {
            code: r.code,
            name: r.name,
            unit: r.unit,
            quantity: r.quantity,
            buy_price: r.buy_price,
            sell_price: r.sell_price,
        })
        .collect();
    assert_eq!(again, rows);

    // Overwriting an existing export works (rename over the old file).
    write_products(&path, &rows[..1]).unwrap();
    assert_eq!(
        parse_products(&table::read(&path).unwrap())
            .unwrap()
            .rows
            .len(),
        1
    );
}

#[test]
fn formula_like_text_is_escaped_and_round_trips() {
    let product = |code: &str, name: &str, unit: &str| ProductExportRow {
        code: code.into(),
        name: name.into(),
        unit: unit.into(),
        quantity: Qty(3),
        buy_price: Money(1_250),
        sell_price: Money(850),
    };
    let rows = vec![
        product("F1", "=1+1", "COPE"),
        product("F2", "@SUM(A1)", "-KG"),
        product("F3", "-5 test", "+x"),
        product("F4", "+x", "COPE"),
        product("F5", "'=already quoted", "COPE"),
        product("F6", "'plain", "COPE"),
    ];
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("formula.csv");
    write_products(&path, &rows).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let text = std::str::from_utf8(&bytes[3..]).unwrap();
    let lines: Vec<&str> = text.split("\r\n").collect();
    assert_eq!(lines[1], "F1,'=1+1,COPE,3,12.5,8.5");
    assert_eq!(lines[2], "F2,'@SUM(A1),'-KG,3,12.5,8.5");
    assert_eq!(lines[3], "F3,'-5 test,'+x,3,12.5,8.5");
    assert_eq!(lines[4], "F4,'+x,COPE,3,12.5,8.5");
    assert_eq!(lines[5], "F5,''=already quoted,COPE,3,12.5,8.5");
    assert_eq!(lines[6], "F6,'plain,COPE,3,12.5,8.5");

    let again = parse_products(&table::read(&path).unwrap()).unwrap();
    assert_eq!(again.errors, []);
    let names: Vec<(String, String)> = again
        .rows
        .into_iter()
        .map(|(_, r)| (r.name, r.unit))
        .collect();
    let want: Vec<(String, String)> = rows
        .iter()
        .map(|r| (r.name.clone(), r.unit.clone()))
        .collect();
    assert_eq!(names, want);

    let owner = Owner {
        id: OwnerId(1),
        kind: OwnerKind::Supplier,
        name: "=HYPERLINK(\"http://x\",\"Kliko\")".into(),
        description: Some("@x".into()),
        nipt: Some("+355".into()),
        address: Some("-Rruga".into()),
        account_code: Some("=A1".into()),
    };
    let path = dir.path().join("furnitore.csv");
    write_owners(&path, OwnerKind::Supplier, std::slice::from_ref(&owner)).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let text = std::str::from_utf8(&bytes[3..]).unwrap();
    assert_eq!(
        text.split("\r\n").nth(1),
        Some("\"'=HYPERLINK(\"\"http://x\"\",\"\"Kliko\"\")\",'-Rruga,'+355,'@x,'=A1")
    );
    let again = parse_owners(&table::read(&path).unwrap()).unwrap();
    assert_eq!(again.errors, []);
    let again: Vec<OwnerImportRow> = again.rows.into_iter().map(|(_, r)| r).collect();
    assert_eq!(again, as_import(&[owner]));
}

fn owners_from(name: &str, kind: OwnerKind) -> Vec<Owner> {
    let parsed = parse_owners(&table::read(&fixture(name)).unwrap()).unwrap();
    (1..)
        .zip(parsed.rows)
        .map(|(id, (_, r))| Owner {
            id: OwnerId(id),
            kind,
            name: r.name,
            description: if id % 7 == 0 {
                Some(format!("Shënim {id}"))
            } else {
                r.description
            },
            nipt: r.nipt,
            address: r.address,
            account_code: r.account_code,
        })
        .collect()
}

fn as_import(owners: &[Owner]) -> Vec<OwnerImportRow> {
    owners
        .iter()
        .map(|o| OwnerImportRow {
            name: o.name.clone(),
            description: o.description.clone(),
            nipt: o.nipt.clone(),
            address: o.address.clone(),
            account_code: o.account_code.clone(),
        })
        .collect()
}

#[test]
fn owners_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    for (fixture_name, kind, header) in [
        (
            "KLIENTET.csv",
            OwnerKind::Client,
            "Llogari,Emër,Adresa,NIPT,Përshkrimi",
        ),
        (
            "FURNITORET.csv",
            OwnerKind::Supplier,
            "Emër,Adresa,NIPT,Përshkrimi,Llogari",
        ),
    ] {
        let owners = owners_from(fixture_name, kind);
        let path = dir.path().join(format!("{kind:?}.csv"));
        write_owners(&path, kind, &owners).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"\xEF\xBB\xBF"));
        let text = std::str::from_utf8(&bytes[3..]).unwrap();
        assert_eq!(text.split("\r\n").next(), Some(header));

        let again = parse_owners(&table::read(&path).unwrap()).unwrap();
        assert_eq!(again.errors, []);
        let again: Vec<OwnerImportRow> = again.rows.into_iter().map(|(_, r)| r).collect();
        assert_eq!(again, as_import(&owners), "{fixture_name}");
    }
}

fn report_lines() -> Vec<ReportLine> {
    let line = |order: i64,
                kind,
                day,
                owner: &str,
                product: Option<i64>,
                code: &str,
                name: &str,
                qty,
                price| {
        ReportLine {
            order_id: OrderId(order),
            order_kind: kind,
            issued_on: date(2026, 10, day),
            owner_name: owner.into(),
            product_id: product.map(ProductId),
            code: code.into(),
            name: name.into(),
            unit: "COPE".into(),
            quantity: Qty(qty),
            price: Money(price),
            value: Money(qty * price),
        }
    };
    vec![
        line(
            7,
            OrderKind::Sale,
            3,
            "Bar Alfa",
            Some(1),
            "L0009",
            "BRIZO",
            2,
            6_500,
        ),
        line(
            7,
            OrderKind::Sale,
            3,
            "Bar Alfa",
            None,
            "",
            "KAFE",
            1,
            123_456,
        ),
        line(
            9,
            OrderKind::Purchase,
            4,
            "Furnitori",
            Some(1),
            "L0009",
            "BRIZO",
            10,
            6_200,
        ),
    ]
}

fn cell_string(range: &calamine::Range<Data>, row: u32, col: u32) -> String {
    range
        .get_value((row, col))
        .map(ToString::to_string)
        .unwrap_or_default()
}

#[test]
fn total_report_xlsx() {
    let (from, to) = (date(2026, 10, 1), date(2026, 10, 31));
    let table = report::build(ReportKind::Total, from, to, &report_lines());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(report::file_name(from, to));
    assert!(path.ends_with("raporti-porosite-2026-10-01-2026-10-31.xlsx"));
    write_xlsx(&path, &table).unwrap();

    let mut workbook: Xlsx<_> = calamine::open_workbook(&path).unwrap();
    assert_eq!(workbook.sheet_names(), [report::SHEET_NAME]);
    let range = workbook.worksheet_range_at(0).unwrap().unwrap();
    assert_eq!(range.start(), Some((0, 0)));
    assert_eq!(cell_string(&range, 0, 0), "REGJISTRI ANALITIK I POROSIVE");
    assert_eq!(cell_string(&range, 1, 0), "01.10.2026 - 31.10.2026");
    assert!(cell_string(&range, 2, 0).is_empty());
    let headers: Vec<String> = (0..10).map(|c| cell_string(&range, 3, c)).collect();
    assert_eq!(
        headers,
        [
            "Lloji",
            "Data",
            "Numër",
            "Klienti/Furnitori",
            "Kodi",
            "Përshkrimi",
            "Njësia",
            "Sasia",
            "Çmimi",
            "Vlera"
        ]
    );
    let get = |row, col| range.get_value((row, col)).cloned().unwrap_or(Data::Empty);
    assert_eq!(get(4, 0), Data::String("Shitje".into()));
    assert_eq!(get(4, 1), Data::String("03.10.2026".into()));
    assert_eq!(get(4, 2), Data::Float(7.0));
    assert_eq!(get(4, 7), Data::Float(2.0));
    assert_eq!(get(4, 8), Data::Float(65.0));
    assert_eq!(get(4, 9), Data::Float(130.0));
    assert_eq!(get(5, 0), Data::Empty);
    assert_eq!(get(5, 3), Data::Empty);
    assert_eq!(get(5, 9), Data::Float(1_234.56));
    assert_eq!(get(6, 0), Data::String("Blerje".into()));
    assert_eq!(range.end(), Some((6, 9)));

    let merges = workbook
        .merge_cells_by_sheet_name(report::SHEET_NAME)
        .unwrap();
    let spans: Vec<((u32, u32), (u32, u32))> = merges.iter().map(|d| (d.start, d.end)).collect();
    assert_eq!(spans, [((0, 0), (0, 9)), ((1, 0), (1, 9))]);
}

#[test]
fn by_products_report_xlsx() {
    let (from, to) = (date(2026, 10, 1), date(2026, 10, 31));
    let table = report::build(ReportKind::ByProducts, from, to, &report_lines());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("r.xlsx");
    write_xlsx(&path, &table).unwrap();
    let mut workbook: Xlsx<_> = calamine::open_workbook(&path).unwrap();
    let range = workbook.worksheet_range_at(0).unwrap().unwrap();
    assert_eq!(cell_string(&range, 0, 0), "RAPORTI SIPAS ARTIKUJVE");
    assert_eq!(cell_string(&range, 3, 4), "Çmimi Mesatar");
    // BRIZO: (13000 + 62000) / 12 = 6250 qindarka.
    assert_eq!(cell_string(&range, 4, 1), "BRIZO");
    assert_eq!(range.get_value((4, 3)), Some(&Data::Float(12.0)));
    assert_eq!(range.get_value((4, 4)), Some(&Data::Float(62.5)));
    assert_eq!(range.get_value((4, 5)), Some(&Data::Float(750.0)));
    assert_eq!(cell_string(&range, 5, 1), "KAFE");
    assert_eq!(range.end(), Some((5, 5)));
    let merges = workbook
        .merge_cells_by_sheet_name(report::SHEET_NAME)
        .unwrap();
    assert_eq!(merges.len(), 2);
    assert_eq!(merges[0].end, (0, 5));
}

#[test]
fn amount_text_cells_are_strings() {
    let table = report::ReportTable {
        title: "LISTA E POROSIVE".into(),
        subtitle: "01.10.2026 - 31.10.2026 · Shitje".into(),
        headers: vec!["Nr.".into(), "Totali (LEK)".into()],
        rows: vec![
            vec![
                report::Cell::Int(7),
                report::Cell::AmountText(Money(123_456)),
            ],
            vec![
                report::Cell::Text("Totali".into()),
                report::Cell::AmountText(Money(-5)),
            ],
        ],
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lista.xlsx");
    write_xlsx(&path, &table).unwrap();
    let mut workbook: Xlsx<_> = calamine::open_workbook(&path).unwrap();
    let range = workbook.worksheet_range_at(0).unwrap().unwrap();
    let get = |row, col| range.get_value((row, col)).cloned().unwrap_or(Data::Empty);
    assert_eq!(get(4, 0), Data::Float(7.0));
    assert_eq!(get(4, 1), Data::String("1.234,56".into()));
    assert_eq!(get(5, 0), Data::String("Totali".into()));
    assert_eq!(get(5, 1), Data::String("-0,05".into()));
}

//! Import tests against the sample files (AC 3, 4, 6).

use crm_domain::{Money, Qty};
use crm_io::decode::DetectedEncoding;
use crm_io::table::{self, read_csv_bytes};
use crm_io::{IoError, ProductImportRow, parse_owners, parse_products};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn products_from(path: &Path) -> Vec<ProductImportRow> {
    let parsed = parse_products(&table::read(path).unwrap()).unwrap();
    assert_eq!(parsed.errors, [], "{}", path.display());
    parsed.rows.into_iter().map(|(_, row)| row).collect()
}

#[test]
fn artikuj() {
    let parsed = parse_products(&table::read(&fixture("ARTIKUJ.csv")).unwrap()).unwrap();
    assert_eq!(parsed.errors, []);
    assert_eq!(parsed.rows.len(), 776);
    assert_eq!(parsed.rows[0].0, 2);
    assert_eq!(parsed.rows[775].0, 777);

    let flavours: Vec<&str> = parsed
        .rows
        .iter()
        .filter(|(_, r)| !r.is_base_row)
        .map(|(_, r)| r.code.as_str())
        .collect();
    assert_eq!(
        flavours,
        [
            "L0009/1", "L0009/2", "L0009/3", "L0009/4", "L0009/5", "P0035/1", "P0035/2", "P0035/3",
            "P0035/4", "P0035/5"
        ]
    );
    let bases: HashSet<&str> = parsed
        .rows
        .iter()
        .map(|(_, r)| r.base_code.as_str())
        .collect();
    assert_eq!(bases.len(), 766);

    let by_code = |code: &str| &parsed.rows.iter().find(|(_, r)| r.code == code).unwrap().1;
    let first = by_code("A0002");
    assert_eq!(first.name, "LIKER MORENA 0.5LIT (LIQUER)");
    assert_eq!(first.unit, "SHISHE");
    assert_eq!(
        (first.quantity, first.buy_price, first.sell_price),
        (Qty(6), Money(108_000), Money(120_000))
    );
    let fara = by_code("COK0024");
    assert_eq!((fara.buy_price, fara.sell_price), (Money(740), Money(790)));
    let sugar = by_code("KRM0129");
    assert_eq!((sugar.unit.as_str(), sugar.quantity), ("KG", Qty(500)));
    let rolls = by_code("P0035");
    assert_eq!(rolls.buy_price, Money(4_738)); // 47.383 rounds to 47.38
    let flavour = by_code("L0009/3");
    assert_eq!(
        (flavour.base_code.as_str(), flavour.name.as_str()),
        ("L0009", "BRIZO LULE 24*0.33LIT")
    );
}

#[test]
fn klientet() {
    let mut raw = csv::Reader::from_path(fixture("KLIENTET.csv")).unwrap();
    let padded = raw
        .records()
        .filter(|r| {
            let nipt = &r.as_ref().unwrap()[3];
            nipt != nipt.trim()
        })
        .count();
    assert_eq!(padded, 129);

    let parsed = parse_owners(&table::read(&fixture("KLIENTET.csv")).unwrap()).unwrap();
    assert_eq!(parsed.errors, []);
    assert_eq!(parsed.rows.len(), 2_931);
    assert!(parsed.rows.iter().all(|(_, r)| r.account_code.is_some()));
    assert!(parsed.rows.iter().all(|(_, r)| {
        r.nipt
            .as_deref()
            .is_none_or(|n| n == n.trim() && !n.is_empty())
    }));
    let accounts: HashSet<&str> = parsed
        .rows
        .iter()
        .filter_map(|(_, r)| r.account_code.as_deref())
        .collect();
    assert_eq!(accounts.len(), 2_931);
    let yje = &parsed
        .rows
        .iter()
        .find(|(_, r)| r.account_code.as_deref() == Some("412732"))
        .unwrap()
        .1;
    assert_eq!(yje.name, "3 YJE");
    assert_eq!(yje.address.as_deref(), Some("LIVADHET, KAMEZ, TIRANE"));
    assert_eq!(yje.nipt.as_deref(), Some("K47120593B"));
    let first = &parsed.rows[0].1;
    assert_eq!(
        (
            first.name.as_str(),
            first.address.clone(),
            first.nipt.clone()
        ),
        ("*KLIENTE TE NDRYSHEM", None, None)
    );
}

#[test]
fn furnitoret() {
    let parsed = parse_owners(&table::read(&fixture("FURNITORET.csv")).unwrap()).unwrap();
    assert_eq!(parsed.errors, []);
    assert_eq!(parsed.rows.len(), 186);
    assert!(parsed.rows.iter().all(|(_, r)| r.account_code.is_none()));
    let first = &parsed.rows[0].1;
    assert_eq!(first.name, "A & D GRUP");
    assert_eq!(first.address.as_deref(), Some("Rr.Ullishtes"));
    assert_eq!(first.nipt.as_deref(), Some("L60318042D"));
}

#[test]
fn artikuj_in_windows_1252() {
    let utf8 = std::fs::read_to_string(fixture("ARTIKUJ.csv")).unwrap();
    let (bytes, _, had_errors) = encoding_rs::WINDOWS_1252.encode(&utf8);
    assert!(!had_errors);
    let (table, encoding) = read_csv_bytes(&bytes).unwrap();
    assert_eq!(encoding, DetectedEncoding::Windows1252);
    assert_eq!(table.headers[1], "pershkrimi");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ARTIKUJ-1252.csv");
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(products_from(&path), products_from(&fixture("ARTIKUJ.csv")));
}

#[test]
fn artikuj_semicolon_with_decimal_commas() {
    let utf8 = std::fs::read_to_string(fixture("ARTIKUJ.csv")).unwrap();
    let mut reader = csv::Reader::from_reader(utf8.as_bytes());
    let mut writer = csv::WriterBuilder::new()
        .delimiter(b';')
        .from_writer(Vec::new());
    writer.write_record(reader.headers().unwrap()).unwrap();
    for record in reader.records() {
        let record = record.unwrap();
        let fields: Vec<String> = record
            .iter()
            .enumerate()
            .map(|(i, f)| {
                if i >= 4 {
                    f.replace('.', ",")
                } else {
                    f.to_owned()
                }
            })
            .collect();
        writer.write_record(&fields).unwrap();
    }
    let bytes = writer.into_inner().unwrap();
    assert!(String::from_utf8_lossy(&bytes).starts_with("Kodi;Përshkrimi;Njësi;"));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ARTIKUJ-semicolon.csv");
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(products_from(&path), products_from(&fixture("ARTIKUJ.csv")));
}

#[test]
fn artikuj_in_utf16_and_with_bom() {
    let utf8 = std::fs::read_to_string(fixture("ARTIKUJ.csv")).unwrap();
    let want = products_from(&fixture("ARTIKUJ.csv"));
    let dir = tempfile::tempdir().unwrap();
    let mut le = vec![0xFF, 0xFE];
    let mut be = vec![0xFE, 0xFF];
    for unit in utf8.encode_utf16() {
        le.extend(unit.to_le_bytes());
        be.extend(unit.to_be_bytes());
    }
    let mut bom = b"\xEF\xBB\xBF".to_vec();
    bom.extend_from_slice(utf8.as_bytes());
    for (name, bytes, encoding) in [
        ("le.csv", le, DetectedEncoding::Utf16Le),
        ("be.txt", be, DetectedEncoding::Utf16Be),
        ("bom.csv", bom, DetectedEncoding::Utf8Bom),
    ] {
        assert_eq!(read_csv_bytes(&bytes).unwrap().1, encoding, "{name}");
        let path = dir.path().join(name);
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(products_from(&path), want, "{name}");
    }
}

#[test]
fn xlsx_import_uses_the_same_aliases() {
    use rust_xlsxwriter::Workbook;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("artikuj.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    for (col, h) in (0u16..).zip([
        "Kodi",
        "Përshkrimi",
        "Njësi",
        "Gjendje",
        "CMIM BLERJE",
        "CMIM SHITJE",
    ]) {
        sheet.write_string(0, col, h).unwrap();
    }
    sheet.write_string(1, 0, "L0009").unwrap();
    sheet.write_string(1, 1, "BRIZO 24*0.33LIT").unwrap();
    sheet.write_string(1, 2, "COPE").unwrap();
    sheet.write_number(1, 3, 4032.0).unwrap();
    sheet.write_number(1, 4, 62.0).unwrap();
    sheet.write_number(1, 5, 8.5).unwrap();
    // Row 3 is fully empty and skipped; row 4 is a flavour with a numeric-looking code.
    sheet.write_string(3, 0, "L0009/1").unwrap();
    sheet.write_string(3, 1, "BRIZO PJESHKË").unwrap();
    sheet.write_number(3, 5, 65.0).unwrap();
    // Row 5 lacks its code.
    sheet.write_string(4, 1, "PA KOD").unwrap();
    sheet.write_number(4, 5, 1.0).unwrap();
    sheet.write_number(4, 3, 2.5).unwrap();
    workbook.save(&path).unwrap();

    let parsed = parse_products(&table::read(&path).unwrap()).unwrap();
    assert_eq!(
        parsed.rows,
        vec![
            (
                2,
                ProductImportRow {
                    code: "L0009".into(),
                    base_code: "L0009".into(),
                    is_base_row: true,
                    name: "BRIZO 24*0.33LIT".into(),
                    unit: "COPE".into(),
                    buy_price: Money(6_200),
                    sell_price: Money(850),
                    quantity: Qty(4_032),
                }
            ),
            (
                4,
                ProductImportRow {
                    code: "L0009/1".into(),
                    base_code: "L0009".into(),
                    is_base_row: false,
                    name: "BRIZO PJESHKË".into(),
                    unit: "SHISHE".into(),
                    buy_price: Money::ZERO,
                    sell_price: Money(6_500),
                    quantity: Qty::ZERO,
                }
            ),
        ]
    );
    let errors: Vec<String> = parsed.errors.iter().map(|e| e.user_message()).collect();
    assert_eq!(
        errors,
        ["Rreshti 5: Kodi është i detyrueshëm; Gjendja duhet të jetë numër i plotë"]
    );
}

#[test]
fn xlsx_owner_import() {
    use rust_xlsxwriter::Workbook;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("klientet.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    for (col, h) in (0u16..).zip(["Llogari", "Emër", "Adresa", "NIPT"]) {
        sheet.write_string(0, col, h).unwrap();
    }
    sheet.write_number(1, 0, 412_732.0).unwrap();
    sheet.write_string(1, 1, "3 YJE").unwrap();
    sheet.write_string(1, 3, "k47120593b ").unwrap();
    workbook.save(&path).unwrap();
    let parsed = parse_owners(&table::read(&path).unwrap()).unwrap();
    assert_eq!(parsed.errors, []);
    let (row_no, row) = &parsed.rows[0];
    assert_eq!(*row_no, 2);
    assert_eq!(row.account_code.as_deref(), Some("412732"));
    assert_eq!(row.nipt.as_deref(), Some("K47120593B"));
    assert_eq!(row.address, None);
}

#[test]
fn unsupported_and_missing_columns() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.json");
    std::fs::write(&path, "{}").unwrap();
    assert!(matches!(
        table::read(&path),
        Err(IoError::UnsupportedFile(_))
    ));
    let err = parse_products(&table::read(&fixture("KLIENTET.csv")).unwrap()).unwrap_err();
    assert_eq!(
        err.user_message(),
        "Skedari nuk ka kolonat e nevojshme: Kodi, Përshkrimi, CMIM SHITJE"
    );
}

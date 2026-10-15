//! Product rows (old `Import.php` `transformProductRow`).

use super::{ImportParse, RowMessages, cell, import_messages};
use crate::error::IoError;
use crate::table::RawTable;
use crm_domain::{DEFAULT_UNIT, Money, ParseMode, ProductInput, Qty, format, sku, validate};

/// One validated product row of an import file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductImportRow {
    /// The code as written, e.g. `L0009/3`.
    pub code: String,
    /// The SKU code it belongs to, e.g. `L0009`.
    pub base_code: String,
    /// True when `code == base_code`. Only the base row is authoritative for the
    /// SKU stock and prices (old `Import.php:219`).
    pub is_base_row: bool,
    /// "Emri".
    pub name: String,
    /// "Njësia" (`SHISHE` when empty or absent).
    pub unit: String,
    /// "Çmimi i blerjes" (0 when empty or absent).
    pub buy_price: Money,
    /// "Çmimi i shitjes".
    pub sell_price: Money,
    /// "Gjendja" (0 when empty or absent).
    pub quantity: Qty,
}

const NAME: &[&str] = &["name", "product", "pershkrimi", "description", "item"];
const CODE: &[&str] = &["kodi", "code"];
const UNIT: &[&str] = &["njesi", "unit"];
const SELL: &[&str] = &["sell_price", "price", "cmimi", "cmim", "cmim_shitje"];
const BUY: &[&str] = &["buy_price", "cmim_blerje"];
const QTY: &[&str] = &["quantity", "sasia", "gjendje"];

/// Maps and validates product rows. Fails with [`IoError::MissingColumns`] when
/// the file has no code, name or sell-price column.
pub fn parse_products(table: &RawTable) -> Result<ImportParse<ProductImportRow>, IoError> {
    let code_col = table.column(CODE);
    // Old `isset($row['kodi'], $row['pershkrimi'])`: the ARTIKUJ layout.
    let name_col = match (table.column(&["kodi"]), table.column(&["pershkrimi"])) {
        (Some(_), Some(pershkrimi)) => Some(pershkrimi),
        _ => table.column(NAME),
    };
    let sell_col = table.column(SELL);
    let missing: Vec<&'static str> = [
        (code_col, "Kodi"),
        (name_col, "Përshkrimi"),
        (sell_col, "CMIM SHITJE"),
    ]
    .into_iter()
    .filter_map(|(col, label)| col.is_none().then_some(label))
    .collect();
    if !missing.is_empty() {
        return Err(IoError::MissingColumns(missing));
    }
    let unit_col = table.column(UNIT);
    let buy_col = table.column(BUY);
    let qty_col = table.column(QTY);

    let mut out = ImportParse::default();
    for (row_no, cells) in &table.rows {
        let mut msgs = RowMessages::default();
        let code = cell(cells, code_col).to_owned();
        if code.is_empty() {
            msgs.push(import_messages::CODE_REQUIRED);
        }
        let name = cell(cells, name_col).to_owned();
        let unit = match cell(cells, unit_col) {
            "" => DEFAULT_UNIT.to_owned(),
            unit => unit.to_owned(),
        };
        let buy_price = amount(cell(cells, buy_col), &mut msgs).unwrap_or(Money::ZERO);
        let sell_price = match cell(cells, sell_col) {
            "" => {
                msgs.push(import_messages::SELL_PRICE_REQUIRED);
                Money::ZERO
            }
            text => amount(text, &mut msgs).unwrap_or(Money::ZERO),
        };
        let quantity = quantity(cell(cells, qty_col), &mut msgs);

        let input = ProductInput {
            code,
            name,
            unit,
            buy_price,
            sell_price,
            quantity,
        };
        if let Err(errors) = validate::product(&input) {
            msgs.extend_validation(errors);
        }
        let base_code = sku::base_code(&input.code).to_owned();
        let row = ProductImportRow {
            is_base_row: input.code == base_code,
            base_code,
            code: input.code,
            name: input.name,
            unit: input.unit,
            buy_price: input.buy_price,
            sell_price: input.sell_price,
            quantity: input.quantity,
        };
        msgs.finish(*row_no, row, &mut out);
    }
    Ok(out)
}

/// An import amount (`8.5`, `1.234,56`); empty is 0.
fn amount(text: &str, msgs: &mut RowMessages) -> Option<Money> {
    if text.is_empty() {
        return Some(Money::ZERO);
    }
    format::parse_amount(text, ParseMode::Import)
        .map_err(|e| msgs.push(e.user_message()))
        .ok()
}

/// A whole quantity; `500`, `500.0` and `500,00` are 500, `2.5` is an error.
/// Empty is 0.
fn quantity(text: &str, msgs: &mut RowMessages) -> Qty {
    if text.is_empty() {
        return Qty::ZERO;
    }
    match format::parse_amount(text, ParseMode::Import) {
        Ok(m) if m.minor() % 100 == 0 => Qty(m.minor() / 100),
        Ok(_) => {
            msgs.push(import_messages::QUANTITY_NOT_WHOLE);
            Qty::ZERO
        }
        Err(_) => {
            msgs.push(crm_domain::messages::invalid_quantity(text));
            Qty::ZERO
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RowError;

    fn table(headers: &[&str], rows: &[&[&str]]) -> RawTable {
        RawTable::from_rows(
            headers.iter().copied(),
            rows.iter()
                .enumerate()
                .map(|(i, r)| (i + 2, r.iter().map(|c| (*c).to_owned()).collect())),
        )
    }

    fn one(headers: &[&str], row: &[&str]) -> ProductImportRow {
        let parsed = parse_products(&table(headers, &[row])).unwrap();
        assert_eq!(parsed.errors, [], "{headers:?}");
        parsed.rows.into_iter().next().unwrap().1
    }

    fn error(headers: &[&str], row: &[&str]) -> String {
        let parsed = parse_products(&table(headers, &[row])).unwrap();
        assert!(parsed.rows.is_empty());
        parsed.errors[0].user_message()
    }

    #[test]
    fn name_aliases() {
        for alias in ["name", "product", "pershkrimi", "description", "item"] {
            let row = one(&["code", alias, "price"], &["A1", "Brizo", "65"]);
            assert_eq!(row.name, "Brizo", "{alias}");
        }
        // `name` beats `description`; with `kodi`, `pershkrimi` beats `name`.
        let row = one(
            &["code", "description", "name", "price"],
            &["A1", "D", "N", "1"],
        );
        assert_eq!(row.name, "N");
        let row = one(
            &["Kodi", "name", "Përshkrimi", "price"],
            &["A1", "N", "P", "1"],
        );
        assert_eq!(row.name, "P");
        let row = one(
            &["code", "name", "Përshkrimi", "price"],
            &["A1", "N", "P", "1"],
        );
        assert_eq!(row.name, "N");
    }

    #[test]
    fn code_aliases() {
        for alias in ["Kodi", "code"] {
            assert_eq!(one(&[alias, "name", "price"], &["X1", "N", "1"]).code, "X1");
        }
    }

    #[test]
    fn unit_aliases_and_default() {
        for alias in ["Njësi", "njesi", "unit"] {
            let row = one(&["code", "name", alias, "price"], &["A", "N", "KG", "1"]);
            assert_eq!(row.unit, "KG", "{alias}");
        }
        assert_eq!(
            one(&["code", "name", "price"], &["A", "N", "1"]).unit,
            "SHISHE"
        );
        assert_eq!(
            one(&["code", "name", "unit", "price"], &["A", "N", "", "1"]).unit,
            "SHISHE"
        );
    }

    #[test]
    fn sell_price_aliases() {
        for alias in ["sell_price", "price", "cmimi", "Çmim", "CMIM SHITJE"] {
            let row = one(&["code", "name", alias], &["A", "N", "8.5"]);
            assert_eq!(row.sell_price, Money(850), "{alias}");
        }
    }

    #[test]
    fn buy_price_aliases_and_default() {
        for alias in ["buy_price", "CMIM BLERJE"] {
            let row = one(
                &["code", "name", "price", alias],
                &["A", "N", "1", "56.693"],
            );
            assert_eq!(row.buy_price, Money(5_669), "{alias}");
        }
        assert_eq!(
            one(&["code", "name", "price"], &["A", "N", "1"]).buy_price,
            Money::ZERO
        );
    }

    #[test]
    fn quantity_aliases_and_default() {
        for alias in ["quantity", "sasia", "Gjendje"] {
            let row = one(&["code", "name", "price", alias], &["A", "N", "1", "500"]);
            assert_eq!(row.quantity, Qty(500), "{alias}");
        }
        assert_eq!(
            one(&["code", "name", "price"], &["A", "N", "1"]).quantity,
            Qty::ZERO
        );
        let row = one(
            &["code", "name", "price", "sasia"],
            &["A", "N", "1", "12.0"],
        );
        assert_eq!(row.quantity, Qty(12));
    }

    #[test]
    fn base_rows() {
        let row = one(&["code", "name", "price"], &["L0009/3", "N", "1"]);
        assert_eq!((row.base_code.as_str(), row.is_base_row), ("L0009", false));
        let row = one(&["code", "name", "price"], &["L0009", "N", "1"]);
        assert_eq!((row.base_code.as_str(), row.is_base_row), ("L0009", true));
    }

    #[test]
    fn row_errors() {
        let h = &[
            "Kodi",
            "Përshkrimi",
            "CMIM SHITJE",
            "Gjendje",
            "CMIM BLERJE",
        ];
        assert_eq!(
            error(h, &["", "N", "1", "", ""]),
            "Rreshti 2: Kodi është i detyrueshëm"
        );
        assert_eq!(
            error(h, &["A", "", "1", "", ""]),
            "Rreshti 2: Emri është i detyrueshëm"
        );
        assert_eq!(
            error(h, &["A", "N", "", "", ""]),
            "Rreshti 2: Çmimi i shitjes është i detyrueshëm"
        );
        assert_eq!(
            error(h, &["A", "N", "1", "2.5", ""]),
            "Rreshti 2: Gjendja duhet të jetë numër i plotë"
        );
        assert_eq!(
            error(h, &["A", "N", "1", "-3", ""]),
            "Rreshti 2: Gjendja nuk mund të jetë negative"
        );
        assert_eq!(
            error(h, &["A", "N", "1", "abc", ""]),
            "Rreshti 2: «abc» nuk është një sasi e vlefshme."
        );
        assert_eq!(
            error(h, &["A", "N", "1", "", "x"]),
            "Rreshti 2: «x» nuk është një shumë e vlefshme."
        );
        assert_eq!(
            error(h, &["A", "N", "-1", "", ""]),
            "Rreshti 2: Çmimi nuk mund të jetë negativ"
        );
        assert_eq!(
            error(h, &["", "", "", "", "-1"]),
            "Rreshti 2: Kodi është i detyrueshëm; Çmimi i shitjes është i detyrueshëm; \
             Emri është i detyrueshëm; Çmimi nuk mund të jetë negativ"
        );
    }

    #[test]
    fn row_numbers_and_empty_rows() {
        let parsed = parse_products(&table(
            &["code", "name", "price"],
            &[&["A", "N", "1"], &["", "", ""], &["", "N", "1"]],
        ))
        .unwrap();
        assert_eq!(parsed.rows.len(), 1);
        assert_eq!(
            parsed.errors,
            [RowError {
                row: 4,
                message: "Kodi është i detyrueshëm".into()
            }]
        );
    }

    #[test]
    fn missing_columns() {
        let err = parse_products(&table(&["unit", "sasia"], &[])).unwrap_err();
        assert_eq!(
            err.user_message(),
            "Skedari nuk ka kolonat e nevojshme: Kodi, Përshkrimi, CMIM SHITJE"
        );
        let err = parse_products(&table(&["code", "name"], &[])).unwrap_err();
        assert!(matches!(err, IoError::MissingColumns(c) if c == ["CMIM SHITJE"]));
    }
}

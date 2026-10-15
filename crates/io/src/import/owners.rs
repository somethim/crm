//! Client and supplier rows (old `Import.php` `transformOrderOwnerRow`).

use super::{ImportParse, RowMessages, cell, columns, first_non_empty, non_empty};
use crate::error::IoError;
use crate::table::RawTable;
use crm_domain::{OwnerInput, OwnerKind, validate};

/// One validated client or supplier row of an import file. Empty fields are `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerImportRow {
    /// "Emri".
    pub name: String,
    /// "Përshkrimi".
    pub description: Option<String>,
    /// "NIPT", trimmed and upper-cased.
    pub nipt: Option<String>,
    /// "Adresa".
    pub address: Option<String>,
    /// "Llogari" (Q4).
    pub account_code: Option<String>,
}

const NAME: &[&str] = &["emer", "name", "emri"];
const DESCRIPTION: &[&str] = &["description", "pershkrimi"];
const NIPT: &[&str] = &["nipt"];
const ADDRESS: &[&str] = &["adresa", "address"];
const ACCOUNT: &[&str] = &["llogari", "account", "account_code"];

/// Maps and validates owner rows (the same for clients and suppliers). Fails with
/// [`IoError::MissingColumns`] when the file has no name column.
pub fn parse_owners(table: &RawTable) -> Result<ImportParse<OwnerImportRow>, IoError> {
    let Some(name_col) = table.column(NAME) else {
        return Err(IoError::MissingColumns(vec!["Emër"]));
    };
    let description = columns(table, DESCRIPTION);
    let nipt = columns(table, NIPT);
    let address = columns(table, ADDRESS);
    let account = columns(table, ACCOUNT);

    let mut out = ImportParse::default();
    for (row_no, cells) in &table.rows {
        let mut msgs = RowMessages::default();
        let row = OwnerImportRow {
            name: cell(cells, Some(name_col)).to_owned(),
            description: non_empty(first_non_empty(cells, &description)),
            nipt: non_empty(&first_non_empty(cells, &nipt).to_uppercase()),
            address: non_empty(first_non_empty(cells, &address)),
            account_code: non_empty(first_non_empty(cells, &account)),
        };
        let input = OwnerInput {
            // `validate::owner` does not look at the kind; the caller picks it.
            kind: OwnerKind::Client,
            name: row.name.clone(),
            description: row.description.clone().unwrap_or_default(),
            nipt: row.nipt.clone().unwrap_or_default(),
            address: row.address.clone().unwrap_or_default(),
            account_code: row.account_code.clone().unwrap_or_default(),
        };
        if let Err(errors) = validate::owner(&input) {
            msgs.extend_validation(errors);
        }
        msgs.finish(*row_no, row, &mut out);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(headers: &[&str], rows: &[&[&str]]) -> RawTable {
        RawTable::from_rows(
            headers.iter().copied(),
            rows.iter()
                .enumerate()
                .map(|(i, r)| (i + 2, r.iter().map(|c| (*c).to_owned()).collect())),
        )
    }

    fn one(headers: &[&str], row: &[&str]) -> OwnerImportRow {
        let parsed = parse_owners(&table(headers, &[row])).unwrap();
        assert_eq!(parsed.errors, [], "{headers:?}");
        parsed.rows.into_iter().next().unwrap().1
    }

    #[test]
    fn name_aliases() {
        for alias in ["Emër", "name", "Emri"] {
            assert_eq!(one(&[alias], &["Alfa"]).name, "Alfa", "{alias}");
        }
    }

    #[test]
    fn description_aliases() {
        for alias in ["description", "Përshkrimi"] {
            let row = one(&["name", alias], &["A", "Shënim"]);
            assert_eq!(row.description.as_deref(), Some("Shënim"), "{alias}");
        }
        let row = one(&["name", "description", "pershkrimi"], &["A", "", "P"]);
        assert_eq!(row.description.as_deref(), Some("P"));
    }

    #[test]
    fn nipt_is_trimmed_and_upper_cased() {
        let row = one(&["name", "NIPT"], &["A", " k47120593b "]);
        assert_eq!(row.nipt.as_deref(), Some("K47120593B"));
        assert_eq!(one(&["name", "NIPT"], &["A", " "]).nipt, None);
    }

    #[test]
    fn address_aliases() {
        for alias in ["Adresa", "address"] {
            let row = one(&["name", alias], &["A", "Tiranë"]);
            assert_eq!(row.address.as_deref(), Some("Tiranë"), "{alias}");
        }
    }

    #[test]
    fn account_aliases() {
        for alias in ["Llogari", "account", "account_code"] {
            let row = one(&["name", alias], &["A", "412732"]);
            assert_eq!(row.account_code.as_deref(), Some("412732"), "{alias}");
        }
        assert_eq!(one(&["name"], &["A"]).account_code, None);
    }

    #[test]
    fn row_errors_and_missing_columns() {
        let parsed = parse_owners(&table(
            &["Emër", "NIPT"],
            &[&["", "K1"], &["", ""], &[&"N".repeat(51), ""]],
        ))
        .unwrap();
        assert!(parsed.rows.is_empty());
        let messages: Vec<String> = parsed.errors.iter().map(|e| e.user_message()).collect();
        assert_eq!(
            messages,
            [
                "Rreshti 2: Emri është i detyrueshëm",
                "Rreshti 4: Emri nuk mund të ketë më shumë se 50 shkronja",
            ]
        );
        let err = parse_owners(&table(&["NIPT", "Adresa"], &[])).unwrap_err();
        assert_eq!(
            err.user_message(),
            "Skedari nuk ka kolonat e nevojshme: Emër"
        );
    }
}

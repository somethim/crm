//! CSV export in the import format (Q8), so a re-import is lossless.
//!
//! Files are UTF-8 with a BOM (Excel then reads the Albanian letters), `,`
//! separated with CRLF line ends, and written atomically (`<path>.tmp`, then rename).
//!
//! Text cells that a spreadsheet would run as a formula (first character `=`, `+`,
//! `-`, `@`, tab or CR, possibly after leading `'`) get one `'` in front
//! ([`csv_text`]); the import strips it again ([`unguard_text`]), so the round
//! trip stays lossless.

use crate::error::IoError;
use crm_domain::{Money, Owner, OwnerKind, Qty};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The product export header (the ARTIKUJ layout).
pub const PRODUCT_HEADERS: [&str; 6] = [
    "Kodi",
    "Përshkrimi",
    "Njësi",
    "Gjendje",
    "CMIM BLERJE",
    "CMIM SHITJE",
];
/// The client export header (the KLIENTET layout plus "Përshkrimi").
pub const CLIENT_HEADERS: [&str; 5] = ["Llogari", "Emër", "Adresa", "NIPT", "Përshkrimi"];
/// The supplier export header (the FURNITORET layout plus "Përshkrimi" and "Llogari").
pub const SUPPLIER_HEADERS: [&str; 5] = ["Emër", "Adresa", "NIPT", "Përshkrimi", "Llogari"];

const BOM: &[u8] = b"\xEF\xBB\xBF";

/// One exported product line: a product with its SKU's unit, stock and prices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductExportRow {
    /// "Kodi" (e.g. `L0009` or `L0009/3`).
    pub code: String,
    /// "Përshkrimi" (the product name).
    pub name: String,
    /// "Njësi".
    pub unit: String,
    /// "Gjendje".
    pub quantity: Qty,
    /// "CMIM BLERJE".
    pub buy_price: Money,
    /// "CMIM SHITJE".
    pub sell_price: Money,
}

/// Writes products as `Kodi,Përshkrimi,Njësi,Gjendje,CMIM BLERJE,CMIM SHITJE`.
pub fn write_products(path: &Path, rows: &[ProductExportRow]) -> Result<(), IoError> {
    write_atomic(path, &PRODUCT_HEADERS, |w| {
        for row in rows {
            w.write_record([
                csv_text(&row.code).as_ref(),
                &csv_text(&row.name),
                &csv_text(&row.unit),
                &row.quantity.get().to_string(),
                &csv_amount(row.buy_price),
                &csv_amount(row.sell_price),
            ])?;
        }
        Ok(())
    })
}

/// Writes clients (`Llogari,Emër,Adresa,NIPT,Përshkrimi`) or suppliers
/// (`Emër,Adresa,NIPT,Përshkrimi,Llogari`).
pub fn write_owners(path: &Path, kind: OwnerKind, owners: &[Owner]) -> Result<(), IoError> {
    let headers = match kind {
        OwnerKind::Client => &CLIENT_HEADERS,
        OwnerKind::Supplier => &SUPPLIER_HEADERS,
    };
    write_atomic(path, headers, |w| {
        for o in owners {
            let field =
                |v: &Option<String>| csv_text(v.as_deref().unwrap_or_default()).into_owned();
            let (account, name, address, nipt, description) = (
                field(&o.account_code),
                csv_text(&o.name).into_owned(),
                field(&o.address),
                field(&o.nipt),
                field(&o.description),
            );
            match kind {
                OwnerKind::Client => w.write_record([account, name, address, nipt, description])?,
                OwnerKind::Supplier => {
                    w.write_record([name, address, nipt, description, account])?;
                }
            }
        }
        Ok(())
    })
}

/// Characters that make a spreadsheet treat a cell as a formula.
const FORMULA_TRIGGERS: [char; 6] = ['=', '+', '-', '@', '\t', '\r'];

/// Whether `s`, after any leading `'`, starts with a formula trigger.
fn needs_guard(s: &str) -> bool {
    s.trim_start_matches('\'')
        .starts_with(FORMULA_TRIGGERS.as_slice())
}

/// A text cell safe to open in a spreadsheet: one `'` is prefixed when the value
/// (after any leading `'`) starts with `=`, `+`, `-`, `@`, tab or CR.
/// [`unguard_text`] is the inverse.
pub fn csv_text(s: &str) -> std::borrow::Cow<'_, str> {
    if needs_guard(s) {
        std::borrow::Cow::Owned(format!("'{s}"))
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

/// Removes the `'` that [`csv_text`] added: one leading `'` is dropped when the
/// rest still [needs the guard](csv_text). Other text is returned unchanged.
pub fn unguard_text(s: &str) -> &str {
    match s.strip_prefix('\'') {
        Some(rest) if needs_guard(s) => rest,
        _ => s,
    }
}

/// An amount as plain CSV text: `.` decimal, no grouping, no trailing zeros
/// (`850` → `8.5`, `113000` → `1130`, `5669` → `56.69`).
pub fn csv_amount(m: Money) -> String {
    let abs = m.minor().unsigned_abs();
    let sign = if m.is_negative() { "-" } else { "" };
    let (lek, qindarka) = (abs / 100, abs % 100);
    match qindarka {
        0 => format!("{sign}{lek}"),
        q if q % 10 == 0 => format!("{sign}{lek}.{}", q / 10),
        q => format!("{sign}{lek}.{q:02}"),
    }
}

/// `<path>.tmp` next to `path`.
pub(crate) fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".tmp");
    PathBuf::from(name)
}

/// Writes to `<path>.tmp` with `write`, then renames it over `path`.
pub(crate) fn finish_atomic(
    path: &Path,
    write: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), IoError> {
    let tmp = tmp_path(path);
    let result = write(&tmp).and_then(|()| std::fs::rename(&tmp, path).map_err(|e| e.to_string()));
    if let Err(reason) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(IoError::unwritable(path, reason));
    }
    Ok(())
}

fn write_atomic(
    path: &Path,
    headers: &[&str],
    rows: impl FnOnce(&mut csv::Writer<File>) -> Result<(), csv::Error>,
) -> Result<(), IoError> {
    finish_atomic(path, |tmp| {
        let run = || -> Result<(), Box<dyn std::error::Error>> {
            let mut file = File::create(tmp)?;
            file.write_all(BOM)?;
            let mut w = csv::WriterBuilder::new()
                .terminator(csv::Terminator::CRLF)
                .from_writer(file);
            w.write_record(headers)?;
            rows(&mut w)?;
            let file = w.into_inner().map_err(|e| e.into_error())?;
            file.sync_all()?;
            Ok(())
        };
        run().map_err(|e| e.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_amounts() {
        for (minor, want) in [
            (850, "8.5"),
            (113_000, "1130"),
            (5_669, "56.69"),
            (5_670, "56.7"),
            (5, "0.05"),
            (0, "0"),
            (-1_250, "-12.5"),
            (123_456_789, "1234567.89"),
        ] {
            assert_eq!(csv_amount(Money(minor)), want);
        }
    }

    #[test]
    fn formula_cells_are_guarded_and_unguarded() {
        for (value, exported) in [
            ("=1+1", "'=1+1"),
            ("@SUM(A1)", "'@SUM(A1)"),
            ("-5 test", "'-5 test"),
            ("+x", "'+x"),
            ("\tx", "'\tx"),
            ("\rx", "'\rx"),
            ("'=x", "''=x"),
            ("''-x", "'''-x"),
            ("'abc", "'abc"),
            ("'", "'"),
            ("abc=1", "abc=1"),
            ("L0009/3", "L0009/3"),
            ("", ""),
        ] {
            assert_eq!(csv_text(value), exported, "{value:?}");
            assert_eq!(unguard_text(exported), value, "{value:?}");
        }
    }

    #[test]
    fn tmp_path_appends() {
        assert_eq!(
            tmp_path(Path::new("/a/b.csv")),
            PathBuf::from("/a/b.csv.tmp")
        );
    }

    #[test]
    fn unwritable_target() {
        let err = write_products(Path::new("/nonexistent-dir/x.csv"), &[]).unwrap_err();
        assert!(matches!(err, IoError::Unwritable { .. }));
    }
}

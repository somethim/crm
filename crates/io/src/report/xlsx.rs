//! Writing a [`ReportTable`] to XLSX (old `ExportOrders.php` `buildSpreadsheet`).
//!
//! Title in A1 and subtitle in A2, each merged across the table and centred
//! (title bold 13); headers on row 4, bold on a `D9D9D9` fill; data from row 5;
//! amounts as numbers with `#,##0.00` (or, for [`Cell::AmountText`], as the
//! right-aligned text `1.234,56`); columns autofitted (money columns sized
//! to their formatted text).

use super::{Cell, ReportTable};
use crate::error::IoError;
use crate::export::finish_atomic;
use crm_domain::format;
use rust_xlsxwriter::{Color, Format, FormatAlign, FormatPattern, Workbook, XlsxError};
use std::path::Path;

/// The worksheet name.
pub const SHEET_NAME: &str = "Raporti";

const HEADER_ROW: u32 = 3;
const FIRST_DATA_ROW: u32 = 4;

/// Writes `table` to `path` (atomically, through `<path>.tmp`).
pub fn write_xlsx(path: &Path, table: &ReportTable) -> Result<(), IoError> {
    finish_atomic(path, |tmp| {
        let mut workbook = workbook(table).map_err(|e| e.to_string())?;
        workbook.save(tmp).map_err(|e| e.to_string())
    })
}

fn workbook(table: &ReportTable) -> Result<Workbook, XlsxError> {
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.set_name(SHEET_NAME)?;

    let title = Format::new()
        .set_bold()
        .set_font_size(13)
        .set_align(FormatAlign::Center);
    let subtitle = Format::new().set_align(FormatAlign::Center);
    let header = Format::new()
        .set_bold()
        .set_pattern(FormatPattern::Solid)
        .set_background_color(Color::RGB(0xD9D9D9));
    let money = Format::new().set_num_format("#,##0.00");
    let amount_text = Format::new().set_align(FormatAlign::Right);

    for (col, name) in (0u16..).zip(&table.headers) {
        sheet.write_string_with_format(HEADER_ROW, col, name, &header)?;
    }
    for (row, cells) in (FIRST_DATA_ROW..).zip(&table.rows) {
        for (col, cell) in (0u16..).zip(cells) {
            match cell {
                Cell::Text(s) if s.is_empty() => {}
                Cell::Text(s) => {
                    sheet.write_string(row, col, s)?;
                }
                Cell::Int(n) => {
                    sheet.write_number(row, col, *n as f64)?;
                }
                Cell::Money(m) => {
                    sheet.write_number_with_format(row, col, m.minor() as f64 / 100.0, &money)?;
                }
                Cell::AmountText(m) => {
                    sheet.write_string_with_format(row, col, format::amount(*m), &amount_text)?;
                }
            }
        }
    }
    // Autofit before the title rows exist: it would otherwise size column A to
    // the merged title. It measures numbers unformatted, so money columns are
    // then widened to their `#,##0.00` text (same length as `format::amount`).
    sheet.autofit();
    for (col, name) in (0u16..).zip(&table.headers) {
        let widest = table
            .rows
            .iter()
            .filter_map(|cells| match cells.get(usize::from(col)) {
                Some(Cell::Money(m) | Cell::AmountText(m)) => {
                    Some(format::amount(*m).chars().count())
                }
                _ => None,
            })
            .max();
        if let Some(widest) = widest {
            let chars = widest.max(name.chars().count()) + 2;
            sheet.set_column_width(col, u32::try_from(chars).unwrap_or(255).min(255))?;
        }
    }

    let last_col = u16::try_from(table.headers.len().saturating_sub(1)).unwrap_or(u16::MAX);
    for (row, text, format) in [(0, &table.title, &title), (1, &table.subtitle, &subtitle)] {
        if last_col > 0 {
            sheet.merge_range(row, 0, row, last_col, text, format)?;
        } else {
            sheet.write_string_with_format(row, 0, text, format)?;
        }
    }
    Ok(workbook)
}

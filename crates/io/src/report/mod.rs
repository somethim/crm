//! The two order reports as a [`ReportTable`], shared by the in-app preview and
//! the XLSX writer (old `ExportOrders.php`).

mod build;
mod xlsx;

pub use build::build;
pub use xlsx::{SHEET_NAME, write_xlsx};

use crm_domain::{Money, OrderId, OrderKind, ProductId, Qty};
use jiff::civil::Date;

/// How many rows the in-app preview shows (old `ImportExportController.php:88`).
pub const PREVIEW_LIMIT: usize = 500;

/// Which report to build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReportKind {
    /// "REGJISTRI ANALITIK I POROSIVE": every order line.
    Total,
    /// "RAPORTI SIPAS ARTIKUJVE": lines aggregated per product.
    ByProducts,
}

/// One report cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cell {
    /// Text (empty for the blank first-line-only cells).
    Text(String),
    /// A whole number (invoice number, quantity).
    Int(i64),
    /// An amount.
    Money(Money),
    /// An amount written as the right-aligned text `1.234,56`, so Excel shows it
    /// the same whatever the PC's regional separators (the order list export).
    AmountText(Money),
}

/// A built report: title, subtitle (the date range), headers and rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportTable {
    /// Row 1, e.g. "REGJISTRI ANALITIK I POROSIVE".
    pub title: String,
    /// Row 2, `dd.MM.yyyy - dd.MM.yyyy`.
    pub subtitle: String,
    /// Column headers.
    pub headers: Vec<String>,
    /// Data rows, each `headers.len()` cells.
    pub rows: Vec<Vec<Cell>>,
}

impl ReportTable {
    /// The first `limit` rows, the total row count, and whether rows were cut off.
    pub fn preview(&self, limit: usize) -> (&[Vec<Cell>], usize, bool) {
        let total = self.rows.len();
        (&self.rows[..total.min(limit)], total, total > limit)
    }
}

/// One saved order line with its order's details, in report order
/// (by `issued_on`, then order, then line position).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportLine {
    /// "Numër".
    pub order_id: OrderId,
    /// Shitje or Blerje.
    pub order_kind: OrderKind,
    /// "Data" (Q3).
    pub issued_on: Date,
    /// "Klienti/Furnitori" (the order's owner snapshot).
    pub owner_name: String,
    /// The product, unless it was deleted.
    pub product_id: Option<ProductId>,
    /// "Kodi" (snapshot).
    pub code: String,
    /// "Përshkrimi" (snapshot).
    pub name: String,
    /// "Njësia" (snapshot).
    pub unit: String,
    /// "Sasia".
    pub quantity: Qty,
    /// "Çmimi".
    pub price: Money,
    /// "Vlera".
    pub value: Money,
}

/// `raporti-porosite-YYYY-MM-DD-YYYY-MM-DD.xlsx`.
pub fn file_name(from: Date, to: Date) -> String {
    format!("raporti-porosite-{from}-{to}.xlsx")
}

/// `porosite-YYYY-MM-DD-YYYY-MM-DD.xlsx`: the order list export.
pub fn order_list_file_name(from: Date, to: Date) -> String {
    format!("porosite-{from}-{to}.xlsx")
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn preview_caps_rows() {
        let table = ReportTable {
            title: String::new(),
            subtitle: String::new(),
            headers: vec!["A".into()],
            rows: (0..501).map(|i| vec![Cell::Int(i)]).collect(),
        };
        let (rows, total, truncated) = table.preview(PREVIEW_LIMIT);
        assert_eq!((rows.len(), total, truncated), (500, 501, true));
        assert_eq!(rows[499], [Cell::Int(499)]);
        let (rows, total, truncated) = table.preview(600);
        assert_eq!((rows.len(), total, truncated), (501, 501, false));
        let (rows, total, truncated) = table.preview(501);
        assert_eq!((rows.len(), total, truncated), (501, 501, false));
    }

    #[test]
    fn file_names() {
        assert_eq!(
            file_name(date(2026, 1, 5), date(2026, 10, 14)),
            "raporti-porosite-2026-01-05-2026-10-14.xlsx"
        );
        assert_eq!(
            order_list_file_name(date(2026, 1, 5), date(2026, 10, 14)),
            "porosite-2026-01-05-2026-10-14.xlsx"
        );
    }
}

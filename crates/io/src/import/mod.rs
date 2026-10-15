//! Mapping a [`RawTable`] to validated import rows (old `Import.php` aliases).
//!
//! Aliases resolve once per table by column presence (as the old `??` chains on a
//! fully populated record did); a few owner fields pick the first non-empty value
//! (the old `! empty(…)` checks). Database decisions (create/update/skip, owner
//! dedupe, SKU grouping) are left to `crm-services`.

mod owners;
mod products;

pub use owners::{OwnerImportRow, parse_owners};
pub use products::{ProductImportRow, parse_products};

use crate::table::{RawTable, RowNo};
use crm_domain::messages;

/// Rows that parsed and validated, plus one error per rejected row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportParse<T> {
    /// Valid rows with their file row number, in file order.
    pub rows: Vec<(RowNo, T)>,
    /// Rejected rows, in file order.
    pub errors: Vec<RowError>,
}

impl<T> Default for ImportParse<T> {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            errors: Vec::new(),
        }
    }
}

/// Why one file row was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowError {
    /// The file row number (header = 1).
    pub row: RowNo,
    /// The Albanian message(s) for the row, without the row prefix.
    pub message: String,
}

impl RowError {
    /// `Rreshti 7: Kodi është i detyrueshëm`.
    pub fn user_message(&self) -> String {
        messages::line_prefixed(self.row, &self.message)
    }
}

impl std::fmt::Display for RowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.user_message())
    }
}

/// Import-only Albanian messages (the rest come from `crm_domain::messages`).
pub mod import_messages {
    /// An import row has no code.
    pub const CODE_REQUIRED: &str = "Kodi është i detyrueshëm";
    /// An import row has no sell price.
    pub const SELL_PRICE_REQUIRED: &str = "Çmimi i shitjes është i detyrueshëm";
    /// An import quantity has decimals.
    pub const QUANTITY_NOT_WHOLE: &str = "Gjendja duhet të jetë numër i plotë";
}

/// Collects the messages for one row; empty means the row is valid.
#[derive(Default)]
pub(crate) struct RowMessages(Vec<String>);

impl RowMessages {
    pub(crate) fn push(&mut self, message: impl Into<String>) {
        let message = message.into();
        if !self.0.contains(&message) {
            self.0.push(message);
        }
    }

    pub(crate) fn extend_validation(&mut self, errors: crm_domain::ValidationErrors) {
        for error in errors.0 {
            self.push(error.message);
        }
    }

    pub(crate) fn finish<T>(self, row: RowNo, value: T, out: &mut ImportParse<T>) {
        if self.0.is_empty() {
            out.rows.push((row, value));
        } else {
            out.errors.push(RowError {
                row,
                message: self.0.join("; "),
            });
        }
    }
}

/// The cell at `column`, or `""` when the column is absent.
pub(crate) fn cell(cells: &[String], column: Option<usize>) -> &str {
    column.and_then(|c| cells.get(c)).map_or("", String::as_str)
}

/// The first non-empty cell among `columns`.
pub(crate) fn first_non_empty<'a>(cells: &'a [String], columns: &[Option<usize>]) -> &'a str {
    columns
        .iter()
        .map(|c| cell(cells, *c))
        .find(|v| !v.is_empty())
        .unwrap_or("")
}

/// `None` for an empty string.
pub(crate) fn non_empty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_owned())
}

/// Column lookups for each alias in order.
pub(crate) fn columns(table: &RawTable, names: &[&str]) -> Vec<Option<usize>> {
    names.iter().map(|n| table.column(&[n])).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_error_message() {
        let e = RowError {
            row: 7,
            message: import_messages::CODE_REQUIRED.into(),
        };
        assert_eq!(e.user_message(), "Rreshti 7: Kodi është i detyrueshëm");
        assert_eq!(e.to_string(), e.user_message());
    }
}

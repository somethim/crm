//! CSV/XLSX import, CSV export and XLSX report writing.
//!
//! Boundary: depends on `crm-domain` only. It parses and writes files into and
//! from domain values; it never touches the database. Used by `crm-services`.
//!
//! Modules:
//! - [`decode`], [`table`]: bytes → text (BOM, UTF-8, UTF-16, Windows-1252),
//!   delimiter sniffing, CSV/Excel → [`RawTable`] with normalised headers;
//! - [`import`]: [`RawTable`] → validated [`ProductImportRow`]s / [`OwnerImportRow`]s
//!   with per-row [`RowError`]s (old header aliases);
//! - [`export`]: CSV export in the import layout (UTF-8 with BOM);
//! - [`report`]: the two order reports as a [`ReportTable`] and their XLSX file;
//! - [`error`]: [`IoError`] with Albanian messages.

#![warn(missing_docs)]

pub mod decode;
pub mod error;
pub mod export;
pub mod import;
pub mod report;
pub mod table;

pub use decode::DetectedEncoding;
pub use error::IoError;
pub use export::{ProductExportRow, write_owners, write_products};
pub use import::{
    ImportParse, OwnerImportRow, ProductImportRow, RowError, parse_owners, parse_products,
};
pub use report::{Cell, ReportKind, ReportLine, ReportTable, write_xlsx};
pub use table::{RawTable, RowNo};

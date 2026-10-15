//! Reading an import file (CSV or Excel) into a [`RawTable`] of trimmed strings.

use crate::decode::{self, DetectedEncoding};
use crate::error::IoError;
use calamine::{Data, Reader, Sheets, XlsxError};
use std::path::Path;

/// A file row number, 1-based, where the header is row 1.
pub type RowNo = usize;

/// An import file as text cells, before any field mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawTable {
    /// Header names, normalised with [`normalize_key`].
    pub headers: Vec<String>,
    /// Data rows with their file row number. Every row has exactly
    /// `headers.len()` trimmed cells; fully empty rows are left out.
    pub rows: Vec<(RowNo, Vec<String>)>,
}

impl RawTable {
    /// The index of the first column whose header is one of `names` (tried in order).
    pub fn column(&self, names: &[&str]) -> Option<usize> {
        names
            .iter()
            .find_map(|name| self.headers.iter().position(|h| h == name))
    }

    /// Builds a table from a header row and data rows, normalising headers,
    /// trimming and padding cells and dropping fully empty rows. The `'` that the
    /// CSV export puts before formula-like text is removed
    /// ([`unguard_text`](crate::export::unguard_text)).
    pub fn from_rows(
        headers: impl IntoIterator<Item = impl AsRef<str>>,
        rows: impl IntoIterator<Item = (RowNo, Vec<String>)>,
    ) -> Self {
        let headers: Vec<String> = headers
            .into_iter()
            .map(|h| normalize_key(h.as_ref()))
            .collect();
        let width = headers.len();
        let rows = rows
            .into_iter()
            .filter_map(|(row_no, cells)| {
                let mut cells: Vec<String> = cells
                    .into_iter()
                    .take(width)
                    .map(|c| crate::export::unguard_text(c.trim()).to_owned())
                    .collect();
                cells.resize(width, String::new());
                let empty = cells
                    .iter()
                    .zip(&headers)
                    .all(|(cell, header)| cell.is_empty() || header.is_empty());
                (!empty).then_some((row_no, cells))
            })
            .collect();
        Self { headers, rows }
    }
}

/// The old `normalizeKey`: ASCII-fold, lower-case, runs of non-alphanumerics
/// become `_`, then leading and trailing `_` are trimmed.
/// `"CMIM BLERJE"` → `cmim_blerje`, `"Përshkrimi"` → `pershkrimi`.
pub fn normalize_key(key: &str) -> String {
    let folded = crm_domain::text::fold(key);
    let mut out = String::with_capacity(folded.len());
    for c in folded.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_owned()
}

/// The largest delimited-text import file (50 MiB; real files are a few hundred KB).
pub const MAX_CSV_BYTES: u64 = 50 * 1024 * 1024;
/// The largest workbook import file (20 MiB, compressed).
pub const MAX_WORKBOOK_BYTES: u64 = 20 * 1024 * 1024;
/// The most worksheet rows a workbook import accepts.
pub const MAX_WORKBOOK_ROWS: usize = 200_000;
/// The most cells a workbook import builds: data rows times columns (F-020).
pub const MAX_WORKBOOK_CELLS: usize = 5_000_000;
/// The most cell text, in bytes, a workbook import keeps (F-020).
pub const MAX_WORKBOOK_TEXT_BYTES: usize = 64 * 1024 * 1024;

/// Refuses a file whose size is over `limit` bytes.
fn check_size(path: &Path, len: u64, limit: u64) -> Result<(), IoError> {
    if len > limit {
        return Err(IoError::TooLarge {
            path: path.to_owned(),
            reason: format!("{len} bytes > {limit}"),
        });
    }
    Ok(())
}

/// Reads an import file, choosing the parser by extension: `.csv`/`.txt` as
/// delimited text, `.xlsx`/`.xls` (and `.xlsm`, `.xlsb`, `.ods`) through calamine,
/// first worksheet. Files over [`MAX_CSV_BYTES`] / [`MAX_WORKBOOK_BYTES`] are
/// refused with [`IoError::TooLarge`] before anything is read.
pub fn read(path: &Path) -> Result<RawTable, IoError> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let limit = match ext.as_str() {
        "csv" | "txt" => MAX_CSV_BYTES,
        "xlsx" | "xls" | "xlsm" | "xlsb" | "ods" => MAX_WORKBOOK_BYTES,
        _ => return Err(IoError::UnsupportedFile(path.to_owned())),
    };
    let len = std::fs::metadata(path)
        .map_err(|e| IoError::unreadable(path, e))?
        .len();
    check_size(path, len, limit)?;
    match ext.as_str() {
        "csv" | "txt" => {
            let bytes = std::fs::read(path).map_err(|e| IoError::unreadable(path, e))?;
            read_csv_bytes(&bytes)
                .map(|(table, _)| table)
                .map_err(|reason| IoError::unreadable(path, reason))
        }
        "xlsx" | "xls" | "xlsm" | "xlsb" | "ods" => read_workbook(path),
        _ => Err(IoError::UnsupportedFile(path.to_owned())),
    }
}

/// Parses delimited text bytes (any supported encoding, `,` `;` or tab separated).
/// Row numbers are file line numbers. Returns the encoding that was detected.
pub fn read_csv_bytes(bytes: &[u8]) -> Result<(RawTable, DetectedEncoding), csv::Error> {
    let (text, encoding) = decode::to_utf8(bytes);
    let delimiter = sniff_delimiter(&text);
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .flexible(true)
        .trim(csv::Trim::All)
        .has_headers(true)
        .from_reader(text.as_bytes());
    let headers: Vec<String> = reader.headers()?.iter().map(str::to_owned).collect();
    let mut rows = Vec::new();
    // Line numbers are counted from byte offsets: the csv crate skips blank lines
    // without counting them.
    let (mut line, mut offset) = (1usize, 0usize);
    for record in reader.records() {
        let record = record?;
        let start = record
            .position()
            .and_then(|p| usize::try_from(p.byte()).ok())
            .unwrap_or(offset)
            .clamp(offset, text.len());
        // A record never starts on a blank line; the reported start may.
        let start = start
            + text.as_bytes()[start..]
                .iter()
                .take_while(|&&b| b == b'\n' || b == b'\r')
                .count();
        line += text.as_bytes()[offset..start]
            .iter()
            .filter(|&&b| b == b'\n')
            .count();
        offset = start;
        rows.push((line, record.iter().map(str::to_owned).collect()));
    }
    Ok((RawTable::from_rows(headers, rows), encoding))
}

/// Picks `,`, `;` or tab: whichever occurs most often (outside quotes) on the
/// header line. Ties and a header without any of them give `,`.
pub fn sniff_delimiter(text: &str) -> u8 {
    let header = text.lines().next().unwrap_or("");
    let mut counts = [(b',', 0usize), (b';', 0), (b'\t', 0)];
    let mut quoted = false;
    for b in header.bytes() {
        if b == b'"' {
            quoted = !quoted;
        } else if !quoted && let Some(slot) = counts.iter_mut().find(|(d, _)| *d == b) {
            slot.1 += 1;
        }
    }
    let mut best = counts[0];
    for candidate in &counts[1..] {
        if candidate.1 > best.1 {
            best = *candidate;
        }
    }
    best.0
}

/// Reads the first worksheet of an Excel (or ODS) workbook. Row numbers are sheet
/// rows; the header is the first row with a non-empty cell. A sheet spanning more
/// than [`MAX_WORKBOOK_ROWS`] rows, needing more than [`MAX_WORKBOOK_CELLS`] cells
/// (data rows times columns) or holding more than [`MAX_WORKBOOK_TEXT_BYTES`] of
/// text is refused with [`IoError::TooLarge`].
///
/// `.xlsx`/`.xlsm` and `.xlsb` sheets are streamed cell by cell under those caps,
/// so a small file declaring a huge sparse sheet is never built as a dense range
/// (F-020). calamine only offers whole ranges for `.xls` and `.ods`.
pub fn read_workbook(path: &Path) -> Result<RawTable, IoError> {
    let mut workbook =
        calamine::open_workbook_auto(path).map_err(|e| IoError::unreadable(path, e))?;
    let first_sheet = workbook.sheet_names().first().cloned();
    let mut cells = SheetCells::new(path);
    match &mut workbook {
        Sheets::Xlsx(xlsx) => {
            let Some(name) = first_sheet else {
                return Err(IoError::unreadable(path, "the workbook has no worksheet"));
            };
            let mut reader = match xlsx.worksheet_cells_reader(&name) {
                Ok(reader) => reader,
                // A chart sheet first: calamine reads it as an empty range.
                Err(XlsxError::NotAWorksheet(_)) => return cells.into_table(),
                Err(e) => return Err(IoError::unreadable(path, e)),
            };
            while let Some(cell) = reader
                .next_cell()
                .map_err(|e| IoError::unreadable(path, e))?
            {
                cells.push(cell.get_position(), Data::from(cell.get_value().clone()))?;
            }
        }
        Sheets::Xlsb(xlsb) => {
            let Some(name) = first_sheet else {
                return Err(IoError::unreadable(path, "the workbook has no worksheet"));
            };
            let mut reader = xlsb
                .worksheet_cells_reader(&name)
                .map_err(|e| IoError::unreadable(path, e))?;
            while let Some(cell) = reader
                .next_cell()
                .map_err(|e| IoError::unreadable(path, e))?
            {
                cells.push(cell.get_position(), Data::from(cell.get_value().clone()))?;
            }
        }
        sheets => {
            let range = sheets
                .worksheet_range_at(0)
                .ok_or_else(|| IoError::unreadable(path, "the workbook has no worksheet"))?
                .map_err(|e| IoError::unreadable(path, e))?;
            let (row0, col0) = range.start().unwrap_or_default();
            for (row, col, value) in range.used_cells() {
                let pos = (row0 + row as u32, col0 + col as u32);
                cells.push(pos, value.clone())?;
            }
        }
    }
    cells.into_table()
}

/// The non-empty cells of one sheet, collected under the workbook caps.
struct SheetCells<'p> {
    path: &'p Path,
    /// (row, column, text), 0-based sheet positions, in reading order.
    cells: Vec<(u32, u32, String)>,
    min_row: u32,
    max_row: u32,
    max_col: u32,
    text_bytes: usize,
}

impl<'p> SheetCells<'p> {
    fn new(path: &'p Path) -> Self {
        Self {
            path,
            cells: Vec::new(),
            min_row: u32::MAX,
            max_row: 0,
            max_col: 0,
            text_bytes: 0,
        }
    }

    fn too_large(&self, reason: String) -> IoError {
        IoError::TooLarge {
            path: self.path.to_owned(),
            reason,
        }
    }

    /// Adds one cell; empty cells are skipped (they do not widen the sheet).
    fn push(&mut self, (row, col): (u32, u32), value: Data) -> Result<(), IoError> {
        if matches!(value, Data::Empty) {
            return Ok(());
        }
        self.min_row = self.min_row.min(row);
        self.max_row = self.max_row.max(row);
        self.max_col = self.max_col.max(col);
        let height = (self.max_row - self.min_row) as usize + 1;
        if height > MAX_WORKBOOK_ROWS {
            return Err(self.too_large(format!("{height} rows > {MAX_WORKBOOK_ROWS}")));
        }
        if self.cells.len() == MAX_WORKBOOK_CELLS {
            return Err(self.too_large(format!("more than {MAX_WORKBOOK_CELLS} cells")));
        }
        let text = cell_text(&value);
        self.text_bytes += text.len();
        if self.text_bytes > MAX_WORKBOOK_TEXT_BYTES {
            return Err(self.too_large(format!(
                "more than {MAX_WORKBOOK_TEXT_BYTES} bytes of cell text"
            )));
        }
        self.cells.push((row, col, text));
        Ok(())
    }

    /// The header row (the first row with a cell) and the data rows below it, each
    /// as wide as the sheet's last used column, with cells at their sheet columns.
    fn into_table(mut self) -> Result<RawTable, IoError> {
        if self.cells.is_empty() {
            return Ok(RawTable::from_rows(Vec::<String>::new(), Vec::new()));
        }
        // Stable: of two cells at one position (a malformed sheet) the later wins.
        self.cells.sort_by_key(|&(row, col, _)| (row, col));
        let width = self.max_col as usize + 1;
        let mut data_rows = 0usize;
        let mut last = None;
        for &(row, _, _) in &self.cells {
            if last != Some(row) {
                last = Some(row);
                data_rows += usize::from(row != self.min_row);
            }
        }
        let needed = data_rows.saturating_mul(width);
        if needed > MAX_WORKBOOK_CELLS {
            return Err(self.too_large(format!(
                "{data_rows} rows x {width} columns > {MAX_WORKBOOK_CELLS} cells"
            )));
        }
        let min_row = self.min_row;
        let mut headers = Vec::new();
        let mut rows: Vec<(RowNo, Vec<String>)> = Vec::with_capacity(data_rows);
        let mut current: Option<(u32, Vec<String>)> = None;
        let mut finish = |row: u32, line: Vec<String>| {
            if row == min_row {
                headers = line;
            } else {
                rows.push((row as usize + 1, line));
            }
        };
        for (row, col, text) in self.cells {
            match &mut current {
                Some((r, line)) if *r == row => line[col as usize] = text,
                _ => {
                    if let Some((r, line)) = current.take() {
                        finish(r, line);
                    }
                    let mut line = vec![String::new(); width];
                    line[col as usize] = text;
                    current = Some((row, line));
                }
            }
        }
        if let Some((r, line)) = current {
            finish(r, line);
        }
        Ok(RawTable::from_rows(headers, rows))
    }
}

/// A cell as import text: integral floats lose the `.0` (`500`), others use the
/// shortest round-trip form (`8.5`).
fn cell_text(cell: &Data) -> String {
    match cell {
        Data::Empty | Data::Error(_) => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_key_matches_old() {
        for (input, want) in [
            ("CMIM BLERJE", "cmim_blerje"),
            ("Përshkrimi", "pershkrimi"),
            ("Emër", "emer"),
            ("Njësi", "njesi"),
            ("  Kodi ", "kodi"),
            ("\u{feff}Kodi", "kodi"),
            ("Sell-Price (ALL)", "sell_price_all"),
            ("Çmimi", "cmimi"),
            ("___", ""),
        ] {
            assert_eq!(normalize_key(input), want, "{input:?}");
        }
    }

    #[test]
    fn sniffs_delimiters() {
        assert_eq!(sniff_delimiter("Kodi,Emri,Njësi\n1;2;3"), b',');
        assert_eq!(sniff_delimiter("Kodi;Emri;Njësi\n1,5;2;3"), b';');
        assert_eq!(sniff_delimiter("Kodi\tEmri\tNjësi"), b'\t');
        assert_eq!(sniff_delimiter("\"A;B\",C"), b',');
        assert_eq!(sniff_delimiter("Kodi"), b',');
        assert_eq!(sniff_delimiter(""), b',');
    }

    #[test]
    fn csv_rows_are_trimmed_padded_and_numbered() {
        let text = "Kodi;Emri;Njësi\n A1 ; Brizo ;COPE\n;;\n\nB2;Ujë\n";
        let (table, encoding) = read_csv_bytes(text.as_bytes()).unwrap();
        assert_eq!(encoding, DetectedEncoding::Utf8);
        assert_eq!(table.headers, ["kodi", "emri", "njesi"]);
        assert_eq!(
            table.rows,
            vec![
                (2, vec!["A1".into(), "Brizo".into(), "COPE".into()]),
                (5, vec!["B2".into(), "Ujë".into(), String::new()]),
            ]
        );
        assert_eq!(table.column(&["name", "emri"]), Some(1));
        assert_eq!(table.column(&["unit"]), None);
    }

    #[test]
    fn size_limit_boundary() {
        let p = Path::new("x.csv");
        assert!(check_size(p, MAX_CSV_BYTES, MAX_CSV_BYTES).is_ok());
        assert!(check_size(p, 0, MAX_CSV_BYTES).is_ok());
        assert!(matches!(
            check_size(p, MAX_CSV_BYTES + 1, MAX_CSV_BYTES),
            Err(IoError::TooLarge { .. })
        ));
    }

    #[test]
    fn oversized_files_are_refused_before_reading() {
        let dir = tempfile::tempdir().unwrap();
        for (name, limit) in [
            ("big.csv", MAX_CSV_BYTES),
            ("big.TXT", MAX_CSV_BYTES),
            ("big.xlsx", MAX_WORKBOOK_BYTES),
            ("big.xls", MAX_WORKBOOK_BYTES),
            ("big.ods", MAX_WORKBOOK_BYTES),
        ] {
            let path = dir.path().join(name);
            // Sparse: no data is written, and none is read.
            std::fs::File::create(&path)
                .unwrap()
                .set_len(limit + 1)
                .unwrap();
            assert!(
                matches!(read(&path), Err(IoError::TooLarge { .. })),
                "{name}"
            );
        }
        // At the CSV cap a workbook is already refused.
        let path = dir.path().join("mid.xlsx");
        std::fs::File::create(&path)
            .unwrap()
            .set_len(MAX_WORKBOOK_BYTES + 1)
            .unwrap();
        const { assert!(MAX_WORKBOOK_BYTES + 1 < MAX_CSV_BYTES) };
        assert!(matches!(read(&path), Err(IoError::TooLarge { .. })));
    }

    #[test]
    fn workbook_row_cap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rows.xlsx");
        let mut book = rust_xlsxwriter::Workbook::new();
        let sheet = book.add_worksheet();
        sheet.write_string(0, 0, "Kodi").unwrap();
        // The used range spans header + MAX_WORKBOOK_ROWS data rows = one over the cap.
        sheet
            .write_string(u32::try_from(MAX_WORKBOOK_ROWS).unwrap(), 0, "Z")
            .unwrap();
        book.save(&path).unwrap();
        assert!(matches!(read(&path), Err(IoError::TooLarge { .. })));

        let mut book = rust_xlsxwriter::Workbook::new();
        let sheet = book.add_worksheet();
        sheet.write_string(0, 0, "Kodi").unwrap();
        sheet
            .write_string(u32::try_from(MAX_WORKBOOK_ROWS - 1).unwrap(), 0, "Z")
            .unwrap();
        book.save(&path).unwrap();
        let table = read(&path).unwrap();
        assert_eq!(table.rows, vec![(MAX_WORKBOOK_ROWS, vec!["Z".to_owned()])]);
    }

    /// F-020: a tiny workbook whose two cells span the whole sheet width must not
    /// be materialised as a dense range (200,000 x 16,384 cells).
    #[test]
    fn a_sparse_wide_workbook_is_read_without_a_dense_range() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wide.xlsx");
        let mut book = rust_xlsxwriter::Workbook::new();
        let sheet = book.add_worksheet();
        sheet.write_string(0, 0, "Kodi").unwrap();
        let last_row = u32::try_from(MAX_WORKBOOK_ROWS - 1).unwrap();
        sheet.write_string(last_row, 16_383, "x").unwrap();
        sheet.write_string(last_row, 0, "A1").unwrap();
        book.save(&path).unwrap();
        assert!(std::fs::metadata(&path).unwrap().len() < 64 * 1024);
        let table = read(&path).unwrap();
        assert_eq!(table.headers.len(), 16_384);
        assert_eq!(table.headers[0], "kodi");
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0].0, MAX_WORKBOOK_ROWS);
        assert_eq!(table.rows[0].1[0], "A1");
    }

    /// F-020: many rows times a wide header is refused by the cell cap instead of
    /// being built.
    #[test]
    fn workbook_cell_cap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cells.xlsx");
        let mut book = rust_xlsxwriter::Workbook::new();
        let sheet = book.add_worksheet();
        sheet.write_string(0, 0, "Kodi").unwrap();
        sheet.write_string(0, 16_383, "Emri").unwrap();
        // 16,384 columns x 400 rows = 6,553,600 cells > MAX_WORKBOOK_CELLS.
        let rows = MAX_WORKBOOK_CELLS / 16_384 + 1;
        for row in 1..=u32::try_from(rows).unwrap() {
            sheet.write_number(row, 0, f64::from(row)).unwrap();
        }
        book.save(&path).unwrap();
        assert!(matches!(read(&path), Err(IoError::TooLarge { .. })));

        // Just under the cap is read.
        let mut book = rust_xlsxwriter::Workbook::new();
        let sheet = book.add_worksheet();
        sheet.write_string(0, 0, "Kodi").unwrap();
        sheet.write_string(0, 16_383, "Emri").unwrap();
        let rows = MAX_WORKBOOK_CELLS / 16_384 - 1;
        for row in 1..=u32::try_from(rows).unwrap() {
            sheet.write_number(row, 0, f64::from(row)).unwrap();
        }
        book.save(&path).unwrap();
        assert_eq!(read(&path).unwrap().rows.len(), rows);
    }

    /// F-020: the total cell text is capped (shared strings repeated per cell).
    #[test]
    fn workbook_text_cap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("text.xlsx");
        let mut book = rust_xlsxwriter::Workbook::new();
        let sheet = book.add_worksheet();
        sheet.write_string(0, 0, "Kodi").unwrap();
        // One 32,767-char shared string (the xlsx cell limit), referenced by enough
        // cells to pass MAX_WORKBOOK_TEXT_BYTES.
        let long = "x".repeat(32_767);
        let rows = MAX_WORKBOOK_TEXT_BYTES / long.len() + 1;
        for row in 1..=u32::try_from(rows).unwrap() {
            sheet.write_string(row, 0, &long).unwrap();
        }
        book.save(&path).unwrap();
        assert!(std::fs::metadata(&path).unwrap().len() < 1024 * 1024);
        assert!(matches!(read(&path), Err(IoError::TooLarge { .. })));
    }

    /// The whole-range path (`.ods`, `.xls`) keeps sheet rows and columns.
    #[test]
    fn ods_cells_keep_their_sheet_positions() {
        let path = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/range.ods"
        ));
        let table = read(path).unwrap();
        assert_eq!(table.headers, ["", "kodi", "emri"]);
        assert_eq!(
            table.rows,
            vec![
                (2, vec![String::new(), "P1".into(), "BRAVO".into()]),
                (4, vec![String::new(), "P2".into(), String::new()]),
            ]
        );
    }

    #[test]
    fn rejects_unknown_extensions() {
        assert!(matches!(
            read(Path::new("x.pdf")),
            Err(IoError::UnsupportedFile(_))
        ));
        assert!(matches!(
            read(Path::new("/nonexistent/x.csv")),
            Err(IoError::Unreadable { .. })
        ));
    }
}

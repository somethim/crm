//! The Albanian texts the reports dialog builds in Rust.

/// "Deri më" before "Nga data" (same text as the invoice list filter, 010).
pub const RANGE_REVERSED: &str = "Data «Deri» nuk mund të jetë para datës «Nga»";
/// "Nga data" is not a date.
pub const FROM_INVALID: &str = "Shkruani një datë të vlefshme te «Nga data» (dd.MM.yyyy).";
/// "Deri më" is not a date.
pub const TO_INVALID: &str = "Shkruani një datë të vlefshme te «Deri më» (dd.MM.yyyy).";
/// More rows than the preview shows (old `order-report-export.tsx:259`).
pub const TRUNCATED: &str =
    "Parapamja tregon 500 rreshtat e parë. Skedari i ruajtur i përmban të gjithë.";
/// No invoice in the range.
pub const NO_ROWS: &str = "Nuk ka porosi për periudhën e zgjedhur.";
/// The save dialog title.
pub const SAVE_TITLE: &str = "Ruaj raportin si Excel";
/// The save dialog file type.
pub const XLSX_FILTER: &str = "Excel";

/// `{nga} - {deri} · {total} rreshta`.
pub fn subtitle(range: &str, total: usize) -> String {
    format!("{range} · {total} rreshta")
}

/// The status-bar flash after an export.
pub fn saved(path: &str) -> String {
    format!("Raporti u ruajt: {path}")
}

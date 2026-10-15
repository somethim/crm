//! The English denylist (001 AC 7, Q8): no old English user-visible string may
//! appear in any `.slint` file or in the Albanian message tables (every
//! `crates/ui/src/**/texts.rs` plus the domain, storage, services and io
//! tables). Every UI task must keep this green.

use std::path::{Path, PathBuf};

/// Substrings that must never appear.
const DENIED_SUBSTRINGS: &[&str] = &[
    "Searching",
    "No results",
    "Close all",
    "Close except",
    "Order #",
    "Pick a date",
    "\"Row ",
    "Loading",
];

/// Closed quoted literals: the regex `"(Add|Edit|Save|Delete|Print|Cancel|Error|
/// Warning|File|Help|Search)"` as exact strings, so "Printo" does not match "Print".
const DENIED_LITERALS: &[&str] = &[
    "Add", "Edit", "Save", "Delete", "Print", "Cancel", "Error", "Warning", "File", "Help",
    "Search",
];

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every file under `dir` (recursively, sorted) for which `keep` is true.
fn files_where(dir: &Path, keep: &dyn Fn(&Path) -> bool, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            files_where(&path, keep, out);
        } else if keep(&path) {
            out.push(path);
        }
    }
}

fn is_slint(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "slint")
}

fn is_texts_table(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "texts.rs")
}

/// The explicit Albanian message tables outside `crates/ui/src/**/texts.rs`.
const MESSAGE_TABLES: &[&str] = &[
    "../domain/src/messages.rs",
    "../storage/src/error.rs",
    "../services/src/messages.rs",
    "../io/src/error.rs",
    // `import_messages` (the per-row import errors).
    "../io/src/import/mod.rs",
    // Report titles and column headers.
    "../io/src/report/build.rs",
    // Text printed on the receipt (`UNNAMED_LINE`) and its template labels.
    "../reports/src/data.rs",
    "../reports/templates/receipt.typ",
    // The password gate and the lock screen (plan 016).
    "src/shell/gate.rs",
    "src/shell/password.rs",
];

/// Every scanned file: all `.slint` files, every `texts.rs` under `src`
/// (shell, catalogue, orders, reports, print, ...) and the message tables.
fn scanned_files() -> Vec<PathBuf> {
    let root = crate_dir();
    let mut files = Vec::new();
    files_where(&root.join("ui"), &is_slint, &mut files);
    files_where(&root.join("src"), &is_texts_table, &mut files);
    for table in MESSAGE_TABLES {
        files.push(root.join(table));
    }
    files
}

/// The violations in `text`, as `line: what`.
fn violations(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (n, line) in text.lines().enumerate() {
        for denied in DENIED_SUBSTRINGS {
            if line.contains(denied) {
                found.push(format!("{}: contains {denied}", n + 1));
            }
        }
        for word in DENIED_LITERALS {
            let literal = format!("\"{word}\"");
            if line.contains(&literal) {
                found.push(format!("{}: contains {literal}", n + 1));
            }
        }
    }
    found
}

#[test]
fn no_english_strings_in_ui_or_message_tables() {
    let files = scanned_files();
    assert!(
        files
            .iter()
            .filter(|f| f.extension().is_some_and(|e| e == "slint"))
            .count()
            >= 15,
        "the .slint scan found too few files: {files:?}"
    );
    // A rename or move must not silently shrink coverage (F-004).
    let tables: Vec<_> = files.iter().filter(|f| is_texts_table(f)).collect();
    assert!(
        tables.len() >= 5,
        "the texts.rs scan found too few tables: {tables:?}"
    );
    for slint in [
        "ui/shell/password-screen.slint",
        "ui/shell/unlock-window.slint",
        "ui/shell/change-password-dialog.slint",
    ] {
        assert!(
            files.contains(&crate_dir().join(slint)),
            "{slint} is not scanned"
        );
    }
    for feature in ["shell", "catalogue", "orders", "reports", "print"] {
        let expected = crate_dir().join("src").join(feature).join("texts.rs");
        assert!(
            tables.contains(&&expected),
            "{} is not scanned",
            expected.display()
        );
    }
    let mut report = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display()));
        for v in violations(&text) {
            report.push(format!("{}:{v}", file.display()));
        }
    }
    assert!(
        report.is_empty(),
        "English strings found:\n{}",
        report.join("\n")
    );
}

#[test]
fn the_matcher_catches_the_denied_forms_only() {
    assert_eq!(violations("text: \"Searching…\";").len(), 1);
    assert_eq!(violations("text: \"No results found\";").len(), 1);
    assert_eq!(violations("title: \"Order #12\";").len(), 1);
    assert_eq!(violations("format!(\"Row {n}\")").len(), 1);
    assert_eq!(violations("text: \"Print\";").len(), 1);
    assert_eq!(violations("text: \"Cancel\";").len(), 1);
    assert!(violations("text: \"Printo\";").is_empty());
    assert!(violations("text: \"Anulo\"; text: \"Ruaj\"; text: \"Kërko\";").is_empty());
    assert!(violations("text: \"Rreshti 3:\";").is_empty());
}

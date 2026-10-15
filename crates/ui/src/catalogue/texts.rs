//! Every Albanian user-facing string authored by the catalogue's Rust code (the
//! `.slint` files hold the rest).

use crm_services::ImportReport;

/// Title of the delete confirmation.
pub const DELETE_TITLE: &str = "Konfirmo fshirjen";

/// OK label of the delete confirmation.
pub const DELETE_OK: &str = "Fshi";

/// The delete question (old `list-page.tsx:464`).
pub fn delete_question(name: &str) -> String {
    format!("Ta fshij «{name}»? Ky veprim nuk mund të zhbëhet.")
}

/// Added to the delete question for the last product of a SKU (001 A11).
pub fn sku_deleted_too(code: &str) -> String {
    format!(" Kodi {code} do të fshihet gjithashtu.")
}

/// The status line of the product list.
pub fn products_count(n: usize) -> String {
    if n == 1 {
        "1 artikull".to_owned()
    } else {
        format!("{n} artikuj")
    }
}

/// The status line of the client list.
pub fn clients_count(n: usize) -> String {
    if n == 1 {
        "1 klient".to_owned()
    } else {
        format!("{n} klientë")
    }
}

/// The status line of the supplier list.
pub fn suppliers_count(n: usize) -> String {
    if n == 1 {
        "1 furnitor".to_owned()
    } else {
        format!("{n} furnitorë")
    }
}

/// Title of the product dialog in create mode.
pub const NEW_PRODUCT: &str = "Artikull i ri";

/// Title of the product dialog in edit mode.
pub fn product_caption(name: &str) -> String {
    format!("Artikulli: {name}")
}

/// The note shown when the SKU is shared with other products.
pub fn shared_sku_note(others: usize) -> String {
    format!("Kodi, njësia, çmimet dhe gjendja ndahen me {others} artikuj të tjerë.")
}

/// Title of the owner dialog in create mode.
pub const NEW_CLIENT: &str = "Klient i ri";

/// Title of the owner dialog in create mode.
pub const NEW_SUPPLIER: &str = "Furnitor i ri";

/// Title of the client dialog in edit mode.
pub fn client_caption(name: &str) -> String {
    format!("Klienti: {name}")
}

/// Title of the supplier dialog in edit mode.
pub fn supplier_caption(name: &str) -> String {
    format!("Furnitori: {name}")
}

/// File-type filter of the import picker.
pub const IMPORT_FILTER: &str = "Skedarë CSV/Excel";

/// Extensions of the import picker.
pub const IMPORT_EXTENSIONS: &[&str] = &["csv", "txt", "xlsx", "xls"];

/// Flash when the product editor is asked for a product deleted meanwhile
/// (e.g. the "/ N" stock link of an open invoice line).
pub const PRODUCT_DELETED: &str = "Ky artikull nuk ekziston më.";

/// Title of the import picker.
pub const PICK_IMPORT_FILE: &str = "Zgjidhni skedarin për importim";

/// The counts of the import result (old `list-page.tsx:214`), one per line.
pub fn import_counts(report: &ImportReport) -> String {
    format!(
        "U krijuan: {}\nU përditësuan: {}\nU anashkaluan: {}\nDështuan: {}",
        report.created, report.updated, report.skipped, report.failed
    )
}

/// The pre-import backup line of the import result.
pub fn import_backup(file: &str) -> String {
    format!("Para importimit u krijua një rezervim: {file}")
}

/// File-type filter of the export save dialog.
pub const EXPORT_FILTER: &str = "Skedar CSV";

/// Title of the export save dialog.
pub const SAVE_EXPORT: &str = "Ruani eksportimin";

/// Text of the export busy cover.
pub const EXPORTING: &str = "Duke eksportuar…";

/// The export succeeded.
pub fn export_saved(file: &str) -> String {
    format!("Eksportimi u ruajt: {file}")
}

//! Every Albanian user-facing string authored by the shell's Rust code (the
//! `.slint` files hold the rest). Scanned by the English denylist test.

/// Title of error message boxes.
pub const ERROR_TITLE: &str = "Gabim";

/// Title of information message boxes.
pub const INFO_TITLE: &str = "CRM";

/// The two new passwords differ.
pub const PASSWORD_MISMATCH: &str = "Fjalëkalimet nuk përputhen.";

/// The password was changed.
pub const PASSWORD_CHANGED: &str = "Fjalëkalimi u ndryshua.";

/// Settings were saved.
pub const SETTINGS_SAVED: &str = "Cilësimet u ruajtën.";

/// A settings field is not a whole number.
pub fn not_a_number(field: &str) -> String {
    format!("«{field}» duhet të jetë numër i plotë.")
}

/// A settings field outside its range.
pub fn field_out_of_range(field: &str, min: u32, max: u32) -> String {
    format!("«{field}» duhet të jetë nga {min} deri në {max}.")
}

/// Settings field names (for the messages above).
pub const FIELD_IDLE: &str = "Kyçje automatike";
/// Settings field name: receipt width.
pub const FIELD_RECEIPT_WIDTH: &str = "Gjerësia e letrës";
/// Settings field name: receipt font size.
pub const FIELD_RECEIPT_FONT: &str = "Madhësia e shkronjave";

/// "Rezervo tani" succeeded.
pub fn backup_done(file: &str) -> String {
    format!("Rezervimi u krye: {file}")
}

/// Title of the backup-folder picker.
pub const PICK_BACKUP_FOLDER: &str = "Zgjidhni dosjen për rezervimin";

/// The restore confirmation.
pub const RESTORE_CONFIRM: &str = "Të dhënat aktuale do të zëvendësohen me rezervimin e zgjedhur. Një kopje e të dhënave aktuale ruhet para rikthimit.";

/// Title of the restore confirmation.
pub const RESTORE_TITLE: &str = "Rikthe nga rezervimi";

/// OK label of the restore confirmation.
pub const RESTORE_OK: &str = "Rikthe";

/// The restore succeeded.
pub const RESTORE_DONE: &str = "Të dhënat u rikthyen nga rezervimi.";

/// Status bar: no backup yet.
pub const NO_BACKUP: &str = "Asnjë rezervim";

/// Status bar: the last backup.
pub fn last_backup(when: &str) -> String {
    format!("Rezervimi i fundit: {when}")
}

/// Backup kinds in the restore list.
pub fn backup_kind(reason: Option<&str>) -> String {
    match reason {
        None => "Ditor".to_owned(),
        Some("exit") => "Në mbyllje".to_owned(),
        Some("manual") => "Manual".to_owned(),
        Some("before-restore") => "Para rikthimit".to_owned(),
        Some(r) if r.starts_with("pre-import") => "Para importit".to_owned(),
        Some(r) if r.starts_with("pre-migrate") => "Para përditësimit".to_owned(),
        Some(_) => "Tjetër".to_owned(),
    }
}

/// The product editor of task 009 is not installed.
pub const PRODUCT_EDITOR_MISSING: &str = "Ndryshimi i artikullit nuk është i disponueshëm";

/// The database is corrupt and no backup copy exists.
pub const NO_BACKUP_TO_RESTORE: &str = "Nuk u gjet asnjë rezervim për rikthim.";

/// The restore at startup failed.
pub fn startup_restore_failed(reason: &str) -> String {
    format!("Rikthimi nga rezervimi dështoi: {reason}")
}

/// A background job failed unexpectedly (it panicked); the app keeps running.
pub const JOB_FAILED: &str = "Ndodhi një gabim i papritur. Veprimi nuk u krye.";

/// An unexpected internal error (panic hook).
pub const UNEXPECTED: &str =
    "Ndodhi një gabim i papritur. Programi do të mbyllet. Detajet janë në dosjen logs.";

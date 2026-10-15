//! Every Albanian user-facing string authored by `crm-services`.
//!
//! The other texts come from `crm_domain::messages`, `crm-storage` and `crm-io`.
//! The UI denylist test (task 008) should scan this file too.

use crm_domain::OwnerKind;

/// An `update_product` code already belongs to another SKU.
pub const CODE_TAKEN: &str = "Ky kod përdoret nga një artikull tjetër.";

/// A product name already used by another product of the same SKU (code).
pub const NAME_TAKEN: &str = "Ky emër përdoret nga një artikull tjetër me të njëjtin kod.";

/// An account code ("Llogari") already used by another client or supplier.
pub fn account_taken(kind: OwnerKind) -> &'static str {
    match kind {
        OwnerKind::Client => "Kjo llogari përdoret nga një klient tjetër.",
        OwnerKind::Supplier => "Kjo llogari përdoret nga një furnitor tjetër.",
    }
}

/// Too many failed attempts (task 008 text).
pub fn locked_for(secs: u64) -> String {
    format!("Provoni përsëri pas {secs} sekondash")
}

/// A wrong password (or recovery key or wipe code) at the gate or the lock screen.
pub const PASSWORD_WRONG: &str = "Fjalëkalim i gabuar";

/// A new password shorter than 6 characters.
pub const PASSWORD_TOO_SHORT: &str = "Fjalëkalimi duhet të ketë të paktën 6 karaktere.";

/// A new password shaped like a recovery key or a wipe code.
pub const PASSWORD_NOT_ALLOWED: &str = "Ky fjalëkalim nuk lejohet. Zgjidhni një tjetër.";

/// The two new-password fields differ.
pub const PASSWORD_MISMATCH: &str = "Fjalëkalimet nuk përputhen.";

/// No usable key file, here or in `backups/`.
pub const KEY_MISSING: &str = "Skedari i çelësit (data/crm.key) mungon ose është i dëmtuar dhe nuk u gjet asnjë kopje e tij në dosjen backups. Të dhënat nuk mund të hapen.";

/// Reading or writing the key file failed.
pub const KEY_FILE_IO: &str = "Skedari i çelësit nuk mund të lexohet ose të shkruhet. Kontrolloni USB-në dhe provoni përsëri.";

/// Randomness or the encryption library failed.
pub const CRYPTO_FAILED: &str = "Ndodhi një gabim gjatë enkriptimit. Provoni përsëri.";

/// An edit tries to turn a sale into a purchase or back.
pub const ORDER_KIND_CHANGE: &str = "Lloji i porosisë nuk mund të ndryshohet.";

/// A sale line references a product that no longer exists.
pub fn product_missing(name: &str) -> String {
    format!("Artikulli «{name}» nuk ekziston më.")
}

/// A number setting outside its range, e.g. "Vlera duhet të jetë nga 48 deri në 110.".
pub fn out_of_range(min: u32, max: u32) -> String {
    format!("Vlera duhet të jetë nga {min} deri në {max}.")
}

/// The database was closed (after `close`, or a failed restore).
pub const DB_CLOSED: &str = "Baza e të dhënave është mbyllur. Rinisni programin.";

/// Row 1 of the order list export.
pub const ORDER_LIST_TITLE: &str = "LISTA E POROSIVE";

/// Row 2 of the order list export: "01.10.2026 - 31.10.2026 · Shitje", plus
/// " · Kërko: «{text}»" when a search text is set.
pub fn order_list_subtitle(range: &str, kind: &str, text: Option<&str>) -> String {
    match text.map(str::trim).filter(|t| !t.is_empty()) {
        Some(text) => format!("{range} · {kind} · Kërko: «{text}»"),
        None => format!("{range} · {kind}"),
    }
}

/// The headers of the order list export; the third is "Klienti" or "Furnitori".
pub fn order_list_headers(owner: &str) -> [String; 7] {
    [
        "Nr.".into(),
        "Lloji".into(),
        owner.into(),
        "Përshkrimi".into(),
        "Totali (LEK)".into(),
        "Data".into(),
        "Modifikuar".into(),
    ]
}

/// The label of the order list export's final sum row.
pub const ORDER_LIST_TOTAL: &str = "Totali";

//! Every Albanian string the orders workspace builds in Rust. (The `.slint`
//! files hold the static labels.)

use crm_services::domain::{Money, OrderId, OrderKind, Qty, format};

/// "Nr. Porosie" of a new order.
pub const NO_NUMBER: &str = "—";
/// Saved tab titles are cut to this many characters.
pub const TITLE_MAX_CHARS: usize = 32;

/// Closing a tab with unsaved changes.
pub const CLOSE_DIRTY_TITLE: &str = "Mbyll skedën";
/// Closing a tab with unsaved changes.
pub const CLOSE_DIRTY_TEXT: &str = "Ndryshimet e paruajtura do të humbasin. Ta mbyll skedën?";
/// Closing several tabs, some with unsaved changes.
pub const CLOSE_DIRTY_MANY_TEXT: &str =
    "Disa skeda kanë ndryshime të paruajtura që do të humbasin. T'i mbyll skedat?";
/// The confirm button of [`CLOSE_DIRTY_TEXT`].
pub const CLOSE_LABEL: &str = "Mbyll";

/// Saving a saved invoice again.
pub const UPDATE_TITLE: &str = "Konfirmo përditësimin";
/// Saving a saved invoice again.
pub const UPDATE_TEXT: &str = "Kjo porosi është tashmë e ruajtur. Përditësimi do të mbishkruajë të dhënat aktuale dhe mund të ndikojë në versionin e printuar.";
/// Deleting an invoice.
pub const DELETE_TITLE: &str = "Konfirmo fshirjen";
/// Deleting an invoice.
pub const DELETE_TEXT: &str = "Ta fshij këtë porosi? Ky veprim nuk mund të zhbëhet.";
/// The confirm button of the update and delete questions.
pub const CONTINUE_LABEL: &str = "Vazhdo";
/// Printo on a saved invoice with unsaved edits (F-025).
pub const PRINT_UNSAVED_TITLE: &str = "Ndryshime të paruajtura";
/// Printo on a saved invoice with unsaved edits: the receipt is printed from
/// the saved invoice, so it is saved first (this also confirms the update).
pub const PRINT_UNSAVED_TEXT: &str = "Porosia ka ndryshime të paruajtura. Ruaj para printimit? Ruajtja do të mbishkruajë të dhënat aktuale të porosisë.";
/// The confirm button of [`PRINT_UNSAVED_TEXT`].
pub const SAVE_LABEL: &str = "Ruaj";

/// Flash after saving a new invoice.
pub const CREATED: &str = "Porosia u krijua.";
/// Flash after saving a saved invoice.
pub const UPDATED: &str = "Porosia u përditësua.";
/// Flash after deleting an invoice.
pub const DELETED: &str = "Porosia u fshi.";

/// The stock slot of a saved line whose product was deleted (F-017).
pub const DELETED_PRODUCT: &str = "artikull i fshirë";

/// Sasia is not a whole number.
pub const INVALID_QTY: &str = "Sasi e pavlefshme";
/// The date filter range is reversed.
pub const RANGE_REVERSED: &str = "Data «Deri» nuk mund të jetë para datës «Nga»";
/// The invoice date is not a valid date.
pub const INVALID_DATE: &str = "Data e porosisë nuk është e vlefshme.";

/// "Maksimumi është {N}" (sale quantity above the stock).
pub fn max_qty(max: Qty) -> String {
    format!("Maksimumi është {}", max.0)
}

/// "Totali: 1.234,56 Lekë".
pub fn total(text: &str) -> String {
    format!("Totali: {text}")
}

/// "/ N" beside the Artikulli cell.
pub fn stock(n: Qty) -> String {
    format!("/ {}", format::qty(n))
}

/// The title of a saved invoice's tab: "Shitje #1023 · Emri" (cut to 32
/// characters).
pub fn saved_title(kind: OrderKind, id: OrderId, owner: &str) -> String {
    let owner = owner.trim();
    let full = if owner.is_empty() {
        format!("{} #{}", kind.label(), id.0)
    } else {
        format!("{} #{} · {owner}", kind.label(), id.0)
    };
    truncate(&full, TITLE_MAX_CHARS)
}

/// The line under the owner field: "NIPT: … · Adresa: …", leaving out an
/// empty part; "" when both are empty.
pub fn owner_detail(nipt: &str, address: &str) -> String {
    let parts: Vec<String> = [("NIPT", nipt.trim()), ("Adresa", address.trim())]
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| format!("{k}: {v}"))
        .collect();
    parts.join(" · ")
}

/// Cuts `text` to `max` characters, ending with "…" when cut.
pub fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// The line under the order list: "{n} porosi · Totali: 1.234,56 LEK", or
/// "0 porosi" when the list is empty.
pub fn list_summary(count: usize, sum: Money) -> String {
    if count == 0 {
        "0 porosi".to_owned()
    } else {
        format!("{count} porosi · Totali: {}", format::money_lek(sum))
    }
}

/// An owner's place in the suggestion list, from Adresa: the last
/// comma-separated part is the city (Bilanc exports put it there), so
/// "LIVADHET, KAMEZ, TIRANE" gives "(TIRANE – LIVADHET, KAMEZ)", " TIRANE" gives
/// "(TIRANE)" and an empty address gives "".
pub fn owner_place(address: &str) -> String {
    let address = address.trim();
    if address.is_empty() {
        return String::new();
    }
    match address.rsplit_once(',') {
        Some((rest, city)) if !city.trim().is_empty() && !rest.trim().is_empty() => {
            format!("({} – {})", city.trim(), rest.trim())
        }
        Some((rest, city)) => {
            let only = if city.trim().is_empty() { rest } else { city };
            format!("({})", only.trim())
        }
        None => format!("({address})"),
    }
}

/// The dimmed note after an owner's name: [`owner_place`], then
/// " · {description}" when there is one.
pub fn owner_note(address: &str, description: &str) -> String {
    let place = owner_place(address);
    let description = description.trim();
    match (place.is_empty(), description.is_empty()) {
        (_, true) => place,
        (true, false) => description.to_owned(),
        (false, false) => format!("{place} · {description}"),
    }
}

/// The line above a suggestion list: "50/124 sugjerime".
pub fn suggestion_count(shown: usize, total: usize) -> String {
    format!("{shown}/{total} sugjerime")
}

/// The save dialog of the order list export.
pub const EXPORT_LIST_TITLE: &str = "Ruaj listën e porosive si Excel";
/// The save dialog file type.
pub const XLSX_FILTER: &str = "Excel";

/// Flash after the order list export.
pub fn list_saved(path: &str) -> String {
    format!("Lista u ruajt: {path}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles() {
        assert_eq!(
            saved_title(OrderKind::Sale, OrderId(1023), "Bar Alfa"),
            "Shitje #1023 · Bar Alfa"
        );
        assert_eq!(
            saved_title(OrderKind::Purchase, OrderId(7), " "),
            "Blerje #7"
        );
        let long = saved_title(OrderKind::Sale, OrderId(1), &"x".repeat(40));
        assert_eq!(long.chars().count(), TITLE_MAX_CHARS);
        assert!(long.ends_with('…'));
        assert_eq!(max_qty(Qty(5)), "Maksimumi është 5");
        assert_eq!(stock(Qty(1234)), "/ 1.234");
    }

    #[test]
    fn owner_details() {
        assert_eq!(
            owner_detail("M12345678A", "Rruga e Pishave"),
            "NIPT: M12345678A · Adresa: Rruga e Pishave"
        );
        assert_eq!(owner_detail("M12345678A", " "), "NIPT: M12345678A");
        assert_eq!(owner_detail("", "Tiranë"), "Adresa: Tiranë");
        assert_eq!(owner_detail("", ""), "");
    }

    #[test]
    fn owner_places_and_notes() {
        assert_eq!(
            owner_place("LIVADHET, KAMEZ, TIRANE"),
            "(TIRANE – LIVADHET, KAMEZ)"
        );
        assert_eq!(
            owner_place("Rruga e Shelgjeve, TIRANE"),
            "(TIRANE – Rruga e Shelgjeve)"
        );
        assert_eq!(owner_place(" TIRANE"), "(TIRANE)");
        assert_eq!(owner_place(""), "");
        assert_eq!(owner_place("  "), "");
        assert_eq!(owner_place("TIRANE,"), "(TIRANE)");
        assert_eq!(
            owner_note("Rr. Pishave, TIRANE", "Bar"),
            "(TIRANE – Rr. Pishave) · Bar"
        );
        assert_eq!(owner_note("", "Bar"), "Bar");
        assert_eq!(owner_note("TIRANE", " "), "(TIRANE)");
        assert_eq!(suggestion_count(50, 124), "50/124 sugjerime");
    }

    #[test]
    fn list_summaries() {
        assert_eq!(list_summary(0, Money(0)), "0 porosi");
        assert_eq!(
            list_summary(3, Money(123_456)),
            "3 porosi · Totali: 1.234,56 LEK"
        );
    }
}

//! Every Albanian user-facing string produced by `crm-domain`.
//!
//! Other modules never inline user-visible text; they use these constants and
//! functions. The UI denylist test (task 008) scans this file for English.

// ---------------------------------------------------------------------------
// Glossary labels
// ---------------------------------------------------------------------------

/// Order kind 0, label.
pub const SALE: &str = "Shitje";
/// Order kind 1, label.
pub const PURCHASE: &str = "Blerje";
/// Order kind 0, document title.
pub const SALE_DOCUMENT: &str = "Porosi Shitje";
/// Order kind 1, document title.
pub const PURCHASE_DOCUMENT: &str = "Porosi Blerje";
/// Order kind 0, title of a new, unsaved tab.
pub const SALE_NEW_TAB: &str = "Porosi Shitje (e re)";
/// Order kind 1, title of a new, unsaved tab.
pub const PURCHASE_NEW_TAB: &str = "Porosi Blerje (e re)";
/// Owner kind 0, label.
pub const CLIENT: &str = "Klient";
/// Owner kind 1, label.
pub const SUPPLIER: &str = "Furnitor";
/// Owner kind 0, plural.
pub const CLIENTS: &str = "Klientët";
/// Owner kind 1, plural.
pub const SUPPLIERS: &str = "Furnitorët";
/// Currency suffix used by `format::money`.
pub const CURRENCY: &str = "Lekë";
/// Currency code used by `format::money_lek` (the order list, as in Bilanc).
pub const CURRENCY_CODE: &str = "LEK";

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// A required name is empty.
pub const NAME_REQUIRED: &str = "Emri është i detyrueshëm";
/// A required unit is empty.
pub const UNIT_REQUIRED: &str = "Njësia është e detyrueshme";
/// A price is below zero.
pub const PRICE_NEGATIVE: &str = "Çmimi nuk mund të jetë negativ";
/// A stock quantity is below zero.
pub const STOCK_NEGATIVE: &str = "Gjendja nuk mund të jetë negative";
/// A line quantity is below one.
pub const QUANTITY_MIN: &str = "Sasia duhet të jetë të paktën 1";
/// An order has no lines.
pub const ORDER_NO_LINES: &str = "Shtoni të paktën një artikull në porosi.";
/// An order has no owner name.
pub const ORDER_OWNER_REQUIRED: &str = "Vendosni emrin e klientit/furnitorit.";
/// A sale line does not reference an existing product.
pub const SALE_LINE_NEEDS_PRODUCT: &str = "Zgjidhni një artikull ekzistues për porositë e shitjes.";

/// A text field is longer than allowed, e.g. "Emri nuk mund të ketë më shumë se 50 shkronja".
pub fn too_long(field_label: &str, max: usize) -> String {
    format!("{field_label} nuk mund të ketë më shumë se {max} shkronja")
}

/// A free-text purchase line lacks its buy or sell price.
pub fn purchase_line_needs_prices(name: &str) -> String {
    format!("Vendosni çmimin e blerjes dhe të shitjes për «{name}».")
}

/// Prefix for an error on order line `n` (1-based): "Rreshti n: …".
pub fn line_prefixed(n: usize, message: &str) -> String {
    format!("Rreshti {n}: {message}")
}

/// Tail when more errors exist than are shown: "…dhe n gabime të tjera.".
pub fn more_errors(n: usize) -> String {
    format!("…dhe {n} gabime të tjera.")
}

/// Field label "Emri".
pub const FIELD_NAME: &str = "Emri";
/// Field label "Kodi".
pub const FIELD_CODE: &str = "Kodi";
/// Field label "Njësia".
pub const FIELD_UNIT: &str = "Njësia";
/// Field label "Përshkrimi".
pub const FIELD_DESCRIPTION: &str = "Përshkrimi";
/// Field label "NIPT".
pub const FIELD_NIPT: &str = "NIPT";
/// Field label "Adresa".
pub const FIELD_ADDRESS: &str = "Adresa";

// ---------------------------------------------------------------------------
// Stock
// ---------------------------------------------------------------------------

/// Not enough stock for a sale.
pub fn stock_insufficient(name: &str, available: i64, required: i64) -> String {
    format!("Gjendje e pamjaftueshme për «{name}»: ka {available}, kërkohen {required}.")
}

/// Verb for deleting a purchase (Q6).
pub const VERB_DELETE: &str = "fshihet";
/// Verb for editing a purchase (Q6).
pub const VERB_EDIT: &str = "ndryshohet";

/// A purchase cannot be deleted or edited because its goods were sold (Q6).
pub fn purchase_already_sold(verb: &str, name: &str, stock: i64, remove: i64) -> String {
    format!(
        "Kjo porosi blerjeje nuk mund të {verb}: malli i saj është shitur tashmë \
         («{name}»: gjendja {stock}, duhet të hiqen {remove})."
    )
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// The input is not a valid amount.
pub fn invalid_amount(input: &str) -> String {
    format!("«{input}» nuk është një shumë e vlefshme.")
}

/// The amount is too large.
pub fn amount_out_of_range(input: &str) -> String {
    format!("Shuma «{input}» është shumë e madhe.")
}

/// The input is not a valid whole quantity.
pub fn invalid_quantity(input: &str) -> String {
    format!("«{input}» nuk është një sasi e vlefshme.")
}

/// The input is not a valid date.
pub fn invalid_date(input: &str) -> String {
    format!("«{input}» nuk është një datë e vlefshme (dd.MM.yyyy).")
}

/// A stored kind number is unknown.
pub fn unknown_kind(value: i64) -> String {
    format!("Lloj i panjohur: {value}.")
}

/// Month names, January first (the date calendar's title, "Tetor 2026").
pub const MONTHS: [&str; 12] = [
    "Janar", "Shkurt", "Mars", "Prill", "Maj", "Qershor", "Korrik", "Gusht", "Shtator", "Tetor",
    "Nëntor", "Dhjetor",
];

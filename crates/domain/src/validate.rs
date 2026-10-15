//! Input validation with Albanian messages (field limits: 001 A9).

use crate::entities::{DraftLine, OrderDraft, OwnerInput, ProductInput};
use crate::kind::OrderKind;
use crate::limits;
use crate::messages;
use crate::qty::Qty;
use std::fmt;

/// One invalid field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    /// The field key, e.g. `name`, `owner_name`, `lines` or `quantity`.
    pub field: &'static str,
    /// For an order line error: the index into [`OrderDraft::lines`].
    pub line: Option<usize>,
    /// The Albanian message.
    pub message: String,
}

/// Every invalid field of one input, in field order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidationErrors(pub Vec<FieldError>);

/// How many errors [`ValidationErrors::user_message`] lists before "…dhe n gabime të tjera.".
pub const SHOWN_ERRORS: usize = 5;

impl ValidationErrors {
    /// The errors.
    pub fn errors(&self) -> &[FieldError] {
        &self.0
    }

    /// True when there is no error.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The messages for `field` (any line).
    pub fn messages_for(&self, field: &str) -> Vec<&str> {
        self.0
            .iter()
            .filter(|e| e.field == field)
            .map(|e| e.message.as_str())
            .collect()
    }

    /// One message per line; line errors are prefixed "Rreshti n: " (1-based).
    /// After [`SHOWN_ERRORS`] lines, the rest are summarised as "…dhe n gabime të tjera.".
    pub fn user_message(&self) -> String {
        let mut lines: Vec<String> = self
            .0
            .iter()
            .take(SHOWN_ERRORS)
            .map(|e| match e.line {
                Some(i) => messages::line_prefixed(i + 1, &e.message),
                None => e.message.clone(),
            })
            .collect();
        if self.0.len() > SHOWN_ERRORS {
            lines.push(messages::more_errors(self.0.len() - SHOWN_ERRORS));
        }
        lines.join("\n")
    }

    fn push(&mut self, field: &'static str, message: impl Into<String>) {
        self.0.push(FieldError {
            field,
            line: None,
            message: message.into(),
        });
    }

    fn push_line(&mut self, line: usize, field: &'static str, message: impl Into<String>) {
        self.0.push(FieldError {
            field,
            line: Some(line),
            message: message.into(),
        });
    }

    fn into_result(self) -> Result<(), ValidationErrors> {
        if self.0.is_empty() { Ok(()) } else { Err(self) }
    }

    fn check_len(&mut self, field: &'static str, label: &str, value: &str, max: usize) {
        if value.trim().chars().count() > max {
            self.push(field, messages::too_long(label, max));
        }
    }
}

impl fmt::Display for ValidationErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.user_message())
    }
}

impl std::error::Error for ValidationErrors {}

/// Validates a client or supplier.
pub fn owner(input: &OwnerInput) -> Result<(), ValidationErrors> {
    let mut errs = ValidationErrors::default();
    if input.name.trim().is_empty() {
        errs.push("name", messages::NAME_REQUIRED);
    }
    errs.check_len(
        "name",
        messages::FIELD_NAME,
        &input.name,
        limits::OWNER_NAME,
    );
    errs.check_len(
        "description",
        messages::FIELD_DESCRIPTION,
        &input.description,
        limits::OWNER_DESCRIPTION,
    );
    errs.check_len(
        "nipt",
        messages::FIELD_NIPT,
        &input.nipt,
        limits::OWNER_NIPT,
    );
    errs.check_len(
        "address",
        messages::FIELD_ADDRESS,
        &input.address,
        limits::OWNER_ADDRESS,
    );
    errs.into_result()
}

/// Validates a product and its SKU fields.
pub fn product(input: &ProductInput) -> Result<(), ValidationErrors> {
    let mut errs = ValidationErrors::default();
    errs.check_len("code", messages::FIELD_CODE, &input.code, limits::SKU_CODE);
    if input.name.trim().is_empty() {
        errs.push("name", messages::NAME_REQUIRED);
    }
    errs.check_len(
        "name",
        messages::FIELD_NAME,
        &input.name,
        limits::PRODUCT_NAME,
    );
    if input.unit.trim().is_empty() {
        errs.push("unit", messages::UNIT_REQUIRED);
    }
    errs.check_len("unit", messages::FIELD_UNIT, &input.unit, limits::SKU_UNIT);
    if input.buy_price.is_negative() {
        errs.push("buy_price", messages::PRICE_NEGATIVE);
    }
    if input.sell_price.is_negative() {
        errs.push("sell_price", messages::PRICE_NEGATIVE);
    }
    if input.quantity.is_negative() {
        errs.push("quantity", messages::STOCK_NEGATIVE);
    }
    errs.into_result()
}

/// Validates an order draft before saving.
///
/// Placeholder rows ([`DraftLine::is_placeholder`]) are ignored. Line errors carry
/// the index into `draft.lines`.
pub fn draft(draft: &OrderDraft) -> Result<(), ValidationErrors> {
    let mut errs = ValidationErrors::default();
    if draft.owner_name.trim().is_empty() {
        errs.push("owner_name", messages::ORDER_OWNER_REQUIRED);
    }
    errs.check_len(
        "owner_name",
        messages::FIELD_NAME,
        &draft.owner_name,
        limits::OWNER_NAME,
    );
    errs.check_len(
        "owner_description",
        messages::FIELD_DESCRIPTION,
        &draft.owner_description,
        limits::OWNER_DESCRIPTION,
    );

    let mut any_line = false;
    for (i, line) in draft.lines.iter().enumerate() {
        if line.is_placeholder() {
            continue;
        }
        any_line = true;
        check_line(&mut errs, draft.kind, i, line);
    }
    if !any_line {
        errs.push("lines", messages::ORDER_NO_LINES);
    }
    errs.into_result()
}

fn check_line(errs: &mut ValidationErrors, kind: OrderKind, i: usize, line: &DraftLine) {
    let name = line.name.trim();
    if line.product_id.is_none() {
        match kind {
            OrderKind::Sale => errs.push_line(i, "product_id", messages::SALE_LINE_NEEDS_PRODUCT),
            OrderKind::Purchase if name.is_empty() => {
                errs.push_line(i, "name", messages::NAME_REQUIRED);
            }
            OrderKind::Purchase => {
                if name.chars().count() > limits::PRODUCT_NAME {
                    errs.push_line(
                        i,
                        "name",
                        messages::too_long(messages::FIELD_NAME, limits::PRODUCT_NAME),
                    );
                }
                if line.buy_price.is_none() || line.sell_price.is_none() {
                    errs.push_line(i, "price", messages::purchase_line_needs_prices(name));
                }
            }
        }
    }
    if line.quantity < Qty(1) {
        errs.push_line(i, "quantity", messages::QUANTITY_MIN);
    }
    if line.price.is_negative() {
        errs.push_line(i, "price", messages::PRICE_NEGATIVE);
    }
    if line.buy_price.is_some_and(|p| p.is_negative()) {
        errs.push_line(i, "buy_price", messages::PRICE_NEGATIVE);
    }
    if line.sell_price.is_some_and(|p| p.is_negative()) {
        errs.push_line(i, "sell_price", messages::PRICE_NEGATIVE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Money, OwnerKind, ProductId};
    use jiff::civil::date;

    fn product_input() -> ProductInput {
        ProductInput {
            code: "T0001".into(),
            name: "Test Product".into(),
            unit: "SHISHE".into(),
            buy_price: Money(80_000),
            sell_price: Money(100_000),
            quantity: Qty(50),
        }
    }

    fn owner_input() -> OwnerInput {
        OwnerInput {
            kind: OwnerKind::Supplier,
            name: "Supplier Gamma".into(),
            description: String::new(),
            nipt: "M02317046H".into(),
            address: String::new(),
            account_code: String::new(),
        }
    }

    fn sale(lines: Vec<DraftLine>) -> OrderDraft {
        OrderDraft {
            kind: OrderKind::Sale,
            owner_id: None,
            owner_name: "Klienti".into(),
            owner_description: String::new(),
            issued_on: date(2026, 10, 14),
            lines,
        }
    }

    fn picked(qty: i64) -> DraftLine {
        DraftLine {
            product_id: Some(ProductId(1)),
            name: "BRIZO".into(),
            quantity: Qty(qty),
            price: Money(6_500),
            ..DraftLine::default()
        }
    }

    #[test]
    fn valid_inputs_pass() {
        assert_eq!(product(&product_input()), Ok(()));
        assert_eq!(owner(&owner_input()), Ok(()));
        assert_eq!(draft(&sale(vec![picked(1), DraftLine::default()])), Ok(()));
    }

    #[test]
    fn product_limits() {
        let mut p = product_input();
        p.code = "K".repeat(51);
        p.unit = "U".repeat(21);
        p.name = "N".repeat(256);
        let errs = product(&p).unwrap_err();
        assert_eq!(
            errs.messages_for("code"),
            ["Kodi nuk mund të ketë më shumë se 50 shkronja"]
        );
        assert_eq!(
            errs.messages_for("unit"),
            ["Njësia nuk mund të ketë më shumë se 20 shkronja"]
        );
        assert_eq!(
            errs.messages_for("name"),
            ["Emri nuk mund të ketë më shumë se 255 shkronja"]
        );
        p = product_input();
        p.code = String::new();
        p.unit = " ".into();
        assert_eq!(
            product(&p).unwrap_err().messages_for("unit"),
            ["Njësia është e detyrueshme"]
        );
    }

    #[test]
    fn owner_limits_count_characters_not_bytes() {
        let mut o = owner_input();
        o.name = "ë".repeat(50);
        assert_eq!(owner(&o), Ok(()));
        o.nipt = "1".repeat(21);
        o.description = "d".repeat(256);
        o.address = "a".repeat(256);
        let errs = owner(&o).unwrap_err();
        assert_eq!(
            errs.messages_for("nipt"),
            ["NIPT nuk mund të ketë më shumë se 20 shkronja"]
        );
        assert_eq!(
            errs.messages_for("description"),
            ["Përshkrimi nuk mund të ketë më shumë se 255 shkronja"]
        );
        assert_eq!(
            errs.messages_for("address"),
            ["Adresa nuk mund të ketë më shumë se 255 shkronja"]
        );
    }

    #[test]
    fn draft_owner_name_required_and_limited() {
        let mut d = sale(vec![picked(1)]);
        d.owner_name = "  ".into();
        assert_eq!(
            draft(&d).unwrap_err().messages_for("owner_name"),
            ["Vendosni emrin e klientit/furnitorit."]
        );
        d.owner_name = "x".repeat(51);
        assert_eq!(
            draft(&d).unwrap_err().messages_for("owner_name"),
            ["Emri nuk mund të ketë më shumë se 50 shkronja"]
        );
    }

    #[test]
    fn draft_line_prices_not_negative() {
        let mut line = picked(1);
        line.price = Money(-1);
        let errs = draft(&sale(vec![line])).unwrap_err();
        assert_eq!(
            errs.messages_for("price"),
            ["Çmimi nuk mund të jetë negativ"]
        );
        assert_eq!(errs.errors()[0].line, Some(0));
    }

    #[test]
    fn user_message_prefixes_rows_and_truncates() {
        let lines = (0..7).map(|_| picked(0)).collect();
        let errs = draft(&sale(lines)).unwrap_err();
        let text = errs.user_message();
        let shown: Vec<&str> = text.lines().collect();
        assert_eq!(shown.len(), SHOWN_ERRORS + 1);
        assert_eq!(shown[0], "Rreshti 1: Sasia duhet të jetë të paktën 1");
        assert_eq!(shown[SHOWN_ERRORS], "…dhe 2 gabime të tjera.");
        assert_eq!(errs.to_string(), text);
    }
}

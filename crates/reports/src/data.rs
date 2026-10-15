//! The receipt content: pre-formatted strings (through `crm_domain::format`)
//! handed to the typst template as JSON.

use crm_domain::entities::Order;
use crm_domain::format;
use crm_domain::kind::OrderKind;

/// The name printed for a line without one.
pub const UNNAMED_LINE: &str = "Artikull pa emër";

/// Paper width and font size (the `receipt_width_mm` / `receipt_font_pt`
/// settings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiptSettings {
    /// Page width in millimetres (default 72).
    pub width_mm: u32,
    /// Body font size in points (default 9).
    pub font_pt: u32,
}

impl Default for ReceiptSettings {
    fn default() -> Self {
        ReceiptSettings {
            width_mm: 72,
            font_pt: 9,
        }
    }
}

/// One receipt line, every cell already formatted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptLine {
    /// 1-based "#".
    pub idx: String,
    /// "Artikull".
    pub name: String,
    /// "Sasia".
    pub qty: String,
    /// "Çmimi".
    pub price: String,
    /// "Vlera".
    pub value: String,
}

/// Everything the receipt prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptData {
    /// "Porosia Nr" (the order number).
    pub nr: String,
    /// Shitje or Blerje.
    pub kind: OrderKind,
    /// "Data" as `dd.MM.yyyy` (the issue date, Q5).
    pub issued_on: String,
    /// "Klient" or "Furnitor".
    pub owner_label: String,
    /// The owner's name.
    pub owner_name: String,
    /// "Përshkrimi" (printed only when not empty).
    pub description: String,
    /// The lines in display order.
    pub lines: Vec<ReceiptLine>,
    /// "Totali".
    pub total: String,
}

impl ReceiptData {
    /// The receipt of a saved order (amounts through `format::amount_compact`,
    /// 001 A8).
    pub fn from_order(order: &Order) -> ReceiptData {
        let lines = order
            .lines
            .iter()
            .enumerate()
            .map(|(i, l)| ReceiptLine {
                idx: (i + 1).to_string(),
                name: if l.name.trim().is_empty() {
                    UNNAMED_LINE.to_owned()
                } else {
                    l.name.clone()
                },
                qty: format::qty(l.quantity),
                price: format::amount_compact(l.price),
                value: format::amount_compact(l.value),
            })
            .collect();
        ReceiptData {
            nr: order.id.0.to_string(),
            kind: order.kind,
            issued_on: format::date(order.issued_on),
            owner_label: order.kind.owner_kind().label().to_owned(),
            owner_name: order.owner.name.clone(),
            description: order
                .owner
                .description
                .as_deref()
                .map(str::trim)
                .unwrap_or_default()
                .to_owned(),
            lines,
            total: format::amount_compact(order.total),
        }
    }

    /// The typst input: this receipt plus the page settings, as JSON.
    pub(crate) fn to_json(&self, settings: &ReceiptSettings) -> String {
        let lines: Vec<serde_json::Value> = self
            .lines
            .iter()
            .map(|l| {
                serde_json::json!({
                    "idx": l.idx,
                    "name": l.name,
                    "qty": l.qty,
                    "price": l.price,
                    "value": l.value,
                })
            })
            .collect();
        serde_json::json!({
            "width_mm": settings.width_mm,
            "font_pt": settings.font_pt,
            "nr": self.nr,
            "issued_on": self.issued_on,
            "owner_label": self.owner_label,
            "owner_name": self.owner_name,
            "description": self.description,
            "lines": lines,
            "total": self.total,
        })
        .to_string()
    }
}

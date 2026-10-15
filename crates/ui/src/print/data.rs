//! Maps a saved order and the receipt settings (crm-services) to the
//! crm-reports input.

use crm_reports::{ReceiptData, ReceiptSettings};
use crm_services::domain::OrderId;
use crm_services::{Crm, ServiceError};

/// The `receipt_width_mm` / `receipt_font_pt` settings as rendering settings.
pub fn settings(s: crm_services::ReceiptSettings) -> ReceiptSettings {
    ReceiptSettings {
        width_mm: s.width_mm,
        font_pt: s.font_pt,
    }
}

/// The receipt content of order `id` and the current settings.
pub fn load(crm: &Crm, id: OrderId) -> Result<(ReceiptData, ReceiptSettings), ServiceError> {
    let order = crm.get_order(id)?;
    let settings = settings(crm.receipt_settings()?);
    Ok((ReceiptData::from_order(&order), settings))
}

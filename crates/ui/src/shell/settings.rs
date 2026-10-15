//! Skedari → Cilësimet…: idle lock minutes and the receipt layout (used by 012).

use crate::shell::{modal, texts};
use crate::{SettingsDialog, UiContext};
use crm_services::{IDLE_LOCK_RANGE, RECEIPT_FONT_RANGE, RECEIPT_WIDTH_RANGE, ReceiptSettings};
use slint::{ComponentHandle, SharedString};
use std::ops::RangeInclusive;
use std::rc::Rc;

/// Parses one whole-number field within `range`.
pub fn parse_field(text: &str, name: &str, range: &RangeInclusive<u32>) -> Result<u32, String> {
    let value: u32 = text.trim().parse().map_err(|_| texts::not_a_number(name))?;
    if range.contains(&value) {
        Ok(value)
    } else {
        Err(texts::field_out_of_range(
            name,
            *range.start(),
            *range.end(),
        ))
    }
}

/// The validated dialog values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsValues {
    /// Idle lock minutes (0 = off).
    pub idle_minutes: u32,
    /// Receipt layout.
    pub receipt: ReceiptSettings,
}

/// Validates the three fields.
pub fn parse_values(idle: &str, width: &str, font: &str) -> Result<SettingsValues, String> {
    Ok(SettingsValues {
        idle_minutes: parse_field(idle, texts::FIELD_IDLE, &IDLE_LOCK_RANGE)?,
        receipt: ReceiptSettings {
            width_mm: parse_field(width, texts::FIELD_RECEIPT_WIDTH, &RECEIPT_WIDTH_RANGE)?,
            font_pt: parse_field(font, texts::FIELD_RECEIPT_FONT, &RECEIPT_FONT_RANGE)?,
        },
    })
}

/// Validates and stores the dialog's values; shows the error in the dialog.
fn save(ctx: &UiContext, dialog: &SettingsDialog) -> bool {
    let values = match parse_values(
        &dialog.get_idle_minutes(),
        &dialog.get_receipt_width(),
        &dialog.get_receipt_font(),
    ) {
        Ok(v) => v,
        Err(text) => {
            dialog.set_error(text.into());
            return false;
        }
    };
    let stored = ctx
        .crm
        .set_idle_lock_minutes(values.idle_minutes)
        .and_then(|()| ctx.crm.set_receipt_settings(values.receipt));
    match stored {
        Ok(()) => {
            ctx.idle_minutes.set(values.idle_minutes);
            dialog.set_error(SharedString::new());
            ctx.flash(texts::SETTINGS_SAVED);
            true
        }
        Err(e) => {
            dialog.set_error(e.user_message().into());
            false
        }
    }
}

/// Opens the dialog with the stored values (menu entry).
pub fn open(ctx: &Rc<UiContext>) {
    let _ = open_dialog(ctx);
}

/// Opens the dialog and returns it (tests drive it).
pub fn open_dialog(ctx: &Rc<UiContext>) -> Option<SettingsDialog> {
    let dialog = match SettingsDialog::new() {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("settings dialog: {e}");
            return None;
        }
    };
    let idle = ctx.crm.idle_lock_minutes().unwrap_or(0);
    let receipt = ctx.crm.receipt_settings().unwrap_or_default();
    dialog.set_idle_minutes(idle.to_string().into());
    dialog.set_receipt_width(receipt.width_mm.to_string().into());
    dialog.set_receipt_font(receipt.font_pt.to_string().into());
    dialog.set_data_folder(ctx.paths.data_dir().display().to_string().into());
    let modal = modal::open_over_app(ctx, &dialog)?;

    let (weak_ctx, weak_dialog, on_ok) = (Rc::downgrade(ctx), dialog.as_weak(), modal.clone());
    dialog.on_ok(move || {
        if let (Some(ctx), Some(dialog)) = (weak_ctx.upgrade(), weak_dialog.upgrade())
            && save(&ctx, &dialog)
        {
            on_ok.close();
        }
    });
    dialog.on_cancel(move || modal.close());
    Some(dialog)
}

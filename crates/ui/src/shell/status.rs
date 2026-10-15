//! The status bar values the shell owns: the last backup time. The flash slot
//! comes from [`UiContext::flash`].

use crate::shell::{format, texts};
use crate::{ShellBridge, UiContext};
use slint::ComponentHandle;

/// Fills the status bar once.
pub fn install(ctx: &UiContext) {
    update(ctx);
}

/// Refreshes the last-backup text.
pub fn update(ctx: &UiContext) {
    let Some(app) = ctx.app() else { return };
    let text = match ctx.crm.last_backup_at() {
        Ok(Some(ts)) => texts::last_backup(&format::timestamp(ts, &format::tirane())),
        Ok(None) => texts::NO_BACKUP.to_owned(),
        Err(e) => {
            tracing::warn!("last backup lookup failed: {e}");
            texts::NO_BACKUP.to_owned()
        }
    };
    app.global::<ShellBridge>().set_last_backup(text.into());
}

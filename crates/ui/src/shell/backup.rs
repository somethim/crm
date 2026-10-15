//! Skedari → Rezervo tani / Rezervo në këtë kompjuter… / Rikthe nga rezervimi….

use crate::shell::{format, modal, status, texts};
use crate::{OrdersBridge, RestoreDialog, UiContext};
use crm_services::domain::Qty;
use crm_services::{BackupEntry, BackupName};
use slint::{ComponentHandle, ModelRc, StandardListViewItem, VecModel};
use std::path::{Path, PathBuf};
use std::rc::Rc;

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// "Rezervo tani": a `-manual` copy in `backups/`.
pub fn backup_now(ctx: &Rc<UiContext>) {
    let weak = Rc::downgrade(ctx);
    ctx.run_bg(
        |crm| crm.backup_now(),
        move |result| {
            let Some(ctx) = weak.upgrade() else { return };
            match result {
                Ok(path) => {
                    tracing::info!("manual backup {}", path.display());
                    status::update(&ctx);
                    ctx.flash(texts::backup_done(&file_name(&path)));
                }
                Err(e) => ctx.show_error(&e),
            }
        },
    );
}

/// `Documents\CRM-Backups` in the user's profile (created when missing).
pub fn default_pc_folder() -> Option<PathBuf> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?;
    let dir = PathBuf::from(home).join("Documents").join("CRM-Backups");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!("cannot create {}: {e}", dir.display());
    }
    Some(dir)
}

/// "Rezervo në këtë kompjuter…": a copy into a folder the user picks.
pub fn backup_to_pc(ctx: &Rc<UiContext>) {
    let mut picker = rfd::FileDialog::new().set_title(texts::PICK_BACKUP_FOLDER);
    if let Some(dir) = default_pc_folder() {
        picker = picker.set_directory(dir);
    }
    let Some(dir) = picker.pick_folder() else {
        return;
    };
    let weak = Rc::downgrade(ctx);
    ctx.run_bg(
        move |crm| crm.backup_to_pc(&dir),
        move |result| {
            let Some(ctx) = weak.upgrade() else { return };
            match result {
                Ok(path) => {
                    tracing::info!("backup to PC {}", path.display());
                    status::update(&ctx);
                    ctx.flash(texts::backup_done(&path.display().to_string()));
                }
                Err(e) => ctx.show_error(&e),
            }
        },
    );
}

/// `dd.MM.yyyy HH:mm` of a backup name (its stamp is Tirana local time).
pub fn backup_when(name: &BackupName) -> String {
    let local = name.date.to_datetime(name.time);
    match local.to_zoned(format::tirane()) {
        Ok(zoned) => format::timestamp(zoned.timestamp(), &format::tirane()),
        Err(_) => format::date(name.date),
    }
}

/// The size column: whole kilobytes, `1.234 KB`.
pub fn backup_size(bytes: u64) -> String {
    let kb = i64::try_from(bytes.div_ceil(1024)).unwrap_or(i64::MAX);
    format!("{} KB", format::qty(Qty(kb)))
}

/// The restore list rows: Data, Lloji, Madhësia.
pub fn restore_rows(entries: &[BackupEntry]) -> Vec<Vec<String>> {
    entries
        .iter()
        .map(|e| {
            vec![
                backup_when(&e.name),
                texts::backup_kind(e.name.reason.as_deref()),
                backup_size(e.size),
            ]
        })
        .collect()
}

/// "Rikthe nga rezervimi…": pick a copy, confirm, restore, refresh everything.
pub fn open_restore(ctx: &Rc<UiContext>) {
    let entries = match ctx.crm.list_backups() {
        Ok(entries) => entries,
        Err(e) => return ctx.show_error(&e),
    };
    let dialog = match RestoreDialog::new() {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("restore dialog: {e}");
            return;
        }
    };
    let rows: Vec<ModelRc<StandardListViewItem>> = restore_rows(&entries)
        .into_iter()
        .map(|cells| {
            let items: Vec<StandardListViewItem> = cells
                .iter()
                .map(|c| StandardListViewItem::from(c.as_str()))
                .collect();
            ModelRc::new(VecModel::from(items))
        })
        .collect();
    dialog.set_rows(ModelRc::new(VecModel::from(rows)));
    if !entries.is_empty() {
        dialog.set_current_row(0);
    }
    let Some(modal) = modal::open_over_app(ctx, &dialog) else {
        return;
    };

    let weak = Rc::downgrade(ctx);
    let on_restore = modal.clone();
    dialog.on_restore(move |row| {
        let Some(ctx) = weak.upgrade() else { return };
        let Some(entry) = usize::try_from(row).ok().and_then(|i| entries.get(i)) else {
            return;
        };
        let path = entry.path.clone();
        let modal = on_restore.clone();
        let weak = Rc::downgrade(&ctx);
        ctx.confirm(
            texts::RESTORE_TITLE,
            texts::RESTORE_CONFIRM,
            texts::RESTORE_OK,
            move |ok| {
                let Some(ctx) = weak.upgrade() else { return };
                if !ok {
                    return;
                }
                modal.close();
                restore(&ctx, path);
            },
        );
    });
    dialog.on_cancel(move || modal.close());
}

/// Restores `path` in the background; afterwards every feature drops its
/// in-memory state of the replaced database (`notify_restored`), then refreshes.
pub fn restore(ctx: &Rc<UiContext>, path: PathBuf) {
    tracing::info!("restoring {}", path.display());
    // The drafts go into the data being replaced (and so into its safety copy);
    // no draft write is left pending for the restored database.
    if let Some(app) = ctx.app() {
        app.global::<OrdersBridge>().invoke_flush_drafts();
    }
    let weak = Rc::downgrade(ctx);
    ctx.run_bg(
        move |crm| crm.restore(&path),
        move |result| {
            let Some(ctx) = weak.upgrade() else { return };
            match result {
                Ok(safety) => {
                    if let Some(safety) = safety {
                        tracing::info!("data before restore saved as {}", safety.display());
                    }
                    ctx.notify_restored();
                    ctx.notify_data_changed();
                    status::update(&ctx);
                    ctx.flash(texts::RESTORE_DONE);
                }
                Err(e) => ctx.show_error(&e),
            }
        },
    );
}

//! Import and export. The import dialog picks a kind and a file, runs the
//! import on a worker thread and shows every count and rejected row; the export
//! asks for the file name with the native save dialog and reports in the
//! status bar. The native file dialogs have no parent window (the crate has no
//! raw-window-handle access); the caller's window is blocked meanwhile.

use crate::catalogue::{ListKind, dialogs, texts};
use crate::shell::format;
use crate::shell::modal::{Modal, ModalOwner};
use crate::shell::texts as shell_texts;
use crate::{ExportDialog, ImportDialog, UiContext};
use crm_services::{Crm, ImportKind, ImportReport, ServiceError};
use jiff::civil::Date;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

/// What the import dialog shows after a successful import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportOutcome {
    /// The counts and rejected rows.
    pub report: ImportReport,
    /// The file name of the pre-import backup copy.
    pub backup: Option<String>,
}

/// Runs an import (on the worker thread) and finds its backup copy.
pub fn run_import(crm: &Crm, kind: ImportKind, path: &Path) -> Result<ImportOutcome, ServiceError> {
    let report = crm.import(kind, path)?;
    let reason = format!("pre-import-{}", kind.slug());
    let backup = match crm.list_backups() {
        Ok(list) => list
            .into_iter()
            .find(|b| b.name.reason.as_deref() == Some(reason.as_str()))
            .and_then(|b| b.path.file_name().map(|n| n.to_string_lossy().into_owned())),
        Err(e) => {
            tracing::warn!("listing backups after the import: {e}");
            None
        }
    };
    Ok(ImportOutcome { report, backup })
}

/// Shows the result view of the import dialog.
pub fn show_import_result(dialog: &ImportDialog, outcome: &ImportOutcome) {
    dialog.set_counts_text(texts::import_counts(&outcome.report).into());
    let errors: Vec<SharedString> = outcome
        .report
        .errors
        .iter()
        .map(|e| e.user_message().into())
        .collect();
    dialog.set_errors(ModelRc::from(Rc::new(VecModel::from(errors))));
    dialog.set_backup_text(
        outcome
            .backup
            .as_deref()
            .map(texts::import_backup)
            .unwrap_or_default()
            .into(),
    );
    dialog.set_finished(true);
}

/// An open import dialog.
pub struct ImportFlow {
    ctx: Weak<UiContext>,
    dialog: slint::Weak<ImportDialog>,
    modal: Modal,
    show_error: Rc<dyn Fn(&str)>,
}

impl ImportFlow {
    /// The dialog, while it is open.
    pub fn dialog(&self) -> Option<ImportDialog> {
        self.dialog.upgrade()
    }

    /// The dialog's modal handle.
    pub fn modal(&self) -> Modal {
        self.modal.clone()
    }

    /// "Shfleto…": the native file picker.
    pub fn browse(&self) {
        let Some(d) = self.dialog() else { return };
        let picked = rfd::FileDialog::new()
            .set_title(texts::PICK_IMPORT_FILE)
            .add_filter(texts::IMPORT_FILTER, texts::IMPORT_EXTENSIONS)
            .pick_file();
        if let Some(path) = picked {
            d.set_file_path(path.display().to_string().into());
        }
    }

    /// "Importo": imports the chosen file in the background.
    pub fn start(self: &Rc<Self>) {
        let (Some(ctx), Some(d)) = (self.ctx.upgrade(), self.dialog()) else {
            return;
        };
        let Some(kind) = ListKind::from_index(d.get_kind()) else {
            return;
        };
        let path = PathBuf::from(d.get_file_path().as_str());
        // F-029: no import while another background job (a restore) runs.
        if path.as_os_str().is_empty() || d.get_busy() || ctx.is_busy() {
            return;
        }
        d.set_busy(true);
        let flow = self.clone();
        let ctx_weak = Rc::downgrade(&ctx);
        let import_kind = kind.import_kind();
        ctx.run_bg(
            move |crm| run_import(&crm, import_kind, &path),
            move |result| {
                if let Some(d) = flow.dialog() {
                    d.set_busy(false);
                }
                match result {
                    Ok(outcome) => {
                        if let Some(d) = flow.dialog() {
                            show_import_result(&d, &outcome);
                        }
                        // F-001: own writes are not reported by the poll;
                        // this also reloads the shown list of `kind`.
                        if let Some(ctx) = ctx_weak.upgrade() {
                            ctx.notify_data_changed();
                        }
                    }
                    Err(e) => {
                        tracing::warn!("import: {e}");
                        (flow.show_error)(&e.user_message());
                    }
                }
            },
        );
    }

    /// Sets the file as if picked, then imports it (tests and scripted use).
    pub fn start_with(self: &Rc<Self>, path: &Path) {
        if let Some(d) = self.dialog() {
            d.set_file_path(path.display().to_string().into());
        }
        self.start();
    }
}

/// Opens the import dialog over `owner` with `kind` preselected.
pub fn open_import<O: ModalOwner>(
    ctx: &Rc<UiContext>,
    owner: &O,
    kind: ListKind,
) -> Option<Rc<ImportFlow>> {
    let dialog = match ImportDialog::new() {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("import dialog: {e}");
            return None;
        }
    };
    dialog.set_kind(kind.index());
    let ctx_weak = Rc::downgrade(ctx);
    let owner_weak = owner.as_weak();
    let show_error: Rc<dyn Fn(&str)> = Rc::new(move |text: &str| {
        if let (Some(ctx), Some(owner)) = (ctx_weak.upgrade(), owner_weak.upgrade()) {
            dialogs::message_over(&ctx, &owner, shell_texts::ERROR_TITLE, text);
        }
    });
    let modal = dialogs::open_modal(ctx, owner, &dialog);
    let flow = Rc::new(ImportFlow {
        ctx: Rc::downgrade(ctx),
        dialog: dialog.as_weak(),
        modal: modal.clone(),
        show_error,
    });
    let f = Rc::downgrade(&flow);
    dialog.on_browse(move || {
        if let Some(f) = f.upgrade() {
            f.browse();
        }
    });
    let f = Rc::downgrade(&flow);
    dialog.on_start(move || {
        if let Some(f) = f.upgrade() {
            f.start();
        }
    });
    let f = Rc::downgrade(&flow);
    dialog.on_dismiss(move || {
        if let Some(f) = f.upgrade()
            && !f.dialog().is_some_and(|d| d.get_busy())
        {
            f.modal.close();
        }
    });
    // The open modal keeps the flow alive until the dialog closes.
    let keep = flow.clone();
    modal.on_closed(move || drop(keep));
    Some(flow)
}

/// `artikujt-YYYY-MM-DD.csv`, `klientet-…`, `furnitoret-…`.
pub fn default_export_name(kind: ListKind, day: Date) -> String {
    let prefix = match kind {
        ListKind::Products => "artikujt",
        ListKind::Clients => "klientet",
        ListKind::Suppliers => "furnitoret",
    };
    format!("{prefix}-{day}.csv")
}

/// "Eksporto…": asks for the file, then exports.
pub fn export<O: ModalOwner>(ctx: &Rc<UiContext>, owner: &O, kind: ListKind) {
    let picked = rfd::FileDialog::new()
        .set_title(texts::SAVE_EXPORT)
        .set_file_name(default_export_name(kind, format::today()))
        .add_filter(texts::EXPORT_FILTER, &["csv"])
        .save_file();
    if let Some(path) = picked {
        export_to(ctx, owner, kind, path);
    }
}

/// Writes the CSV in the background (the owner is covered meanwhile), then
/// flashes "Eksportimi u ruajt: {skedari}" or shows the error.
pub fn export_to<O: ModalOwner>(ctx: &Rc<UiContext>, owner: &O, kind: ListKind, path: PathBuf) {
    let cover = match ExportDialog::new() {
        Ok(d) => {
            d.set_text(texts::EXPORTING.into());
            Some(dialogs::open_modal(ctx, owner, &d))
        }
        Err(e) => {
            tracing::error!("export dialog: {e}");
            None
        }
    };
    let ctx_weak = Rc::downgrade(ctx);
    let owner_weak = owner.as_weak();
    let file = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let import_kind = kind.import_kind();
    ctx.run_bg(
        move |crm| crm.export_csv(import_kind, &path),
        move |result| {
            if let Some(cover) = cover {
                cover.close();
            }
            let Some(ctx) = ctx_weak.upgrade() else {
                return;
            };
            match result {
                Ok(rows) => {
                    tracing::info!("exported {rows} rows to {file}");
                    ctx.flash(texts::export_saved(&file));
                }
                Err(e) => {
                    tracing::warn!("export: {e}");
                    if let Some(owner) = owner_weak.upgrade() {
                        dialogs::message_over(
                            &ctx,
                            &owner,
                            shell_texts::ERROR_TITLE,
                            &e.user_message(),
                        );
                    }
                }
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_names_carry_the_kind_and_the_iso_date() {
        let day = Date::new(2026, 10, 14).unwrap();
        assert_eq!(
            default_export_name(ListKind::Products, day),
            "artikujt-2026-10-14.csv"
        );
        assert_eq!(
            default_export_name(ListKind::Clients, day),
            "klientet-2026-10-14.csv"
        );
        assert_eq!(
            default_export_name(ListKind::Suppliers, day),
            "furnitoret-2026-10-14.csv"
        );
    }
}

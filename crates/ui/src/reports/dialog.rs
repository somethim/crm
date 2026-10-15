//! The `ReportDialog` controller: reads the fields into [`ReportParams`], loads
//! the preview and writes the XLSX through [`UiContext::run_bg`], and fills the
//! dynamic table (one column per report header).

use super::texts;
use crate::shell::format;
use crate::{ReportDialog, UiContext};
use crm_services::domain::OrderKind;
use crm_services::{
    Cell, Crm, ReportKind, ReportParams, ReportPreview, ServiceError, report_file_name,
};
use jiff::civil::Date;
use slint::{ComponentHandle, ModelRc, SharedString, StandardListViewItem, TableColumn, VecModel};
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// The "Formati" list, in `format-index` order.
pub const FORMATS: [ReportKind; 2] = [ReportKind::Total, ReportKind::ByProducts];
/// The "Lloji" list, in `kind-index` order ("Të dyja", "Shitje", "Blerje").
pub const KINDS: [Option<OrderKind>; 3] = [None, Some(OrderKind::Sale), Some(OrderKind::Purchase)];

/// The default range: the first day of `today`'s month to `today`.
pub fn default_range(today: Date) -> (Date, Date) {
    (today.first_of_month(), today)
}

/// Creates the dialog with the default fields and handlers, without showing it.
pub fn create(ctx: &Rc<UiContext>) -> Option<ReportDialog> {
    let dialog = match ReportDialog::new() {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("report dialog: {e}");
            return None;
        }
    };
    crate::install_date_parse!(dialog);
    let (from, to) = default_range(format::today());
    dialog.set_from_text(format::date(from).into());
    dialog.set_to_text(format::date(to).into());

    let weak_ctx = Rc::downgrade(ctx);
    let weak = dialog.as_weak();
    dialog.on_preview(move || {
        if let (Some(ctx), Some(dialog)) = (weak_ctx.upgrade(), weak.upgrade()) {
            start_preview(&ctx, &dialog);
        }
    });
    let weak_ctx = Rc::downgrade(ctx);
    let weak = dialog.as_weak();
    dialog.on_save(move || {
        if let (Some(ctx), Some(dialog)) = (weak_ctx.upgrade(), weak.upgrade()) {
            save(&ctx, &dialog);
        }
    });
    Some(dialog)
}

/// "Raporte → Eksporto raport…": shows the dialog modal over the main window
/// (returned for tests).
pub fn open(ctx: &Rc<UiContext>) -> Option<ReportDialog> {
    let dialog = create(ctx)?;
    let modal = crate::shell::modal::open_over_app(ctx, &dialog)?;
    dialog.on_close_clicked(move || modal.close());
    Some(dialog)
}

/// The report parameters from the dialog fields, or the error text to show.
pub fn params_from(dialog: &ReportDialog) -> Result<ReportParams, String> {
    let from =
        format::parse_date(&dialog.get_from_text()).map_err(|_| texts::FROM_INVALID.to_owned())?;
    let to = format::parse_date(&dialog.get_to_text()).map_err(|_| texts::TO_INVALID.to_owned())?;
    if to < from {
        return Err(texts::RANGE_REVERSED.to_owned());
    }
    let pick = |index: i32| usize::try_from(index).unwrap_or(0);
    Ok(ReportParams {
        kind: FORMATS
            .get(pick(dialog.get_format_index()))
            .copied()
            .unwrap_or(ReportKind::Total),
        from,
        to,
        order_kind: KINDS
            .get(pick(dialog.get_kind_index()))
            .copied()
            .unwrap_or(None),
    })
}

/// The parameters, or `None` after showing the error in the dialog.
fn checked_params(dialog: &ReportDialog) -> Option<ReportParams> {
    match params_from(dialog) {
        Ok(params) => {
            dialog.set_error(SharedString::new());
            Some(params)
        }
        Err(text) => {
            dialog.set_error(text.into());
            None
        }
    }
}

/// One cell as shown in the preview: text as is (the blank first-line-only
/// cells stay blank), whole numbers as digits, amounts as `1.234,56`.
pub fn cell_text(cell: &Cell) -> SharedString {
    match cell {
        Cell::Text(s) => s.as_str().into(),
        Cell::Int(n) => n.to_string().into(),
        Cell::Money(m) | Cell::AmountText(m) => format::amount(*m).into(),
    }
}

/// A column's minimum width from its longest value (header included).
fn column_min_width(chars: usize) -> f32 {
    let chars = u16::try_from(chars.min(40)).unwrap_or(40);
    (f32::from(chars) * 7.0 + 20.0).max(60.0)
}

/// Shows a loaded preview: title, subtitle, note and the table.
pub fn show_preview(dialog: &ReportDialog, preview: &ReportPreview) {
    let table = &preview.table;
    let rows: Vec<Vec<SharedString>> = table
        .rows
        .iter()
        .map(|row| row.iter().map(cell_text).collect())
        .collect();
    let columns: Vec<TableColumn> = table
        .headers
        .iter()
        .enumerate()
        .map(|(i, header)| {
            let longest = rows
                .iter()
                .filter_map(|row| row.get(i))
                .map(|s| s.chars().count())
                .fold(header.chars().count(), usize::max);
            let mut column = TableColumn::default();
            column.title = header.as_str().into();
            column.min_width = column_min_width(longest);
            column.horizontal_stretch = 1.0;
            column
        })
        .collect();
    let items: Vec<ModelRc<StandardListViewItem>> = rows
        .into_iter()
        .map(|row| {
            let cells: Vec<StandardListViewItem> =
                row.into_iter().map(StandardListViewItem::from).collect();
            ModelRc::new(VecModel::from(cells))
        })
        .collect();
    dialog.set_columns(ModelRc::new(VecModel::from(columns)));
    dialog.set_rows(ModelRc::new(VecModel::from(items)));
    dialog.set_report_title(table.title.as_str().into());
    dialog.set_report_subtitle(texts::subtitle(&table.subtitle, preview.total).into());
    let note = if preview.total == 0 {
        texts::NO_ROWS
    } else if preview.truncated {
        texts::TRUNCATED
    } else {
        ""
    };
    dialog.set_note(note.into());
    dialog.set_has_preview(true);
    dialog.set_stale(false);
}

/// "Shiko të dhënat": loads the preview in the background.
fn start_preview(ctx: &Rc<UiContext>, dialog: &ReportDialog) {
    let Some(params) = checked_params(dialog) else {
        return;
    };
    dialog.set_busy(true);
    let weak_ctx = Rc::downgrade(ctx);
    let weak = dialog.as_weak();
    ctx.run_bg(
        move |crm| crm.report_preview(params),
        move |result| {
            let Some(dialog) = weak.upgrade() else { return };
            dialog.set_busy(false);
            match result {
                Ok(preview) => show_preview(&dialog, &preview),
                Err(e) => {
                    if let Some(ctx) = weak_ctx.upgrade() {
                        ctx.show_error(&e);
                    }
                }
            }
        },
    );
}

/// The save path with the `.xlsx` extension added when the user left it out.
pub fn with_xlsx_extension(mut path: PathBuf) -> PathBuf {
    if path.extension().is_none() {
        path.set_extension("xlsx");
    }
    path
}

/// Writes the full report to `path` (the background job of "Ruaj si Excel…").
pub fn write_report(crm: &Crm, params: ReportParams, path: &Path) -> Result<(), ServiceError> {
    crm.export_report_xlsx(params, path)
}

/// "Ruaj si Excel…": asks for the file, then writes it in the background.
fn save(ctx: &Rc<UiContext>, dialog: &ReportDialog) {
    let Some(params) = checked_params(dialog) else {
        return;
    };
    let picked = rfd::FileDialog::new()
        .set_title(texts::SAVE_TITLE)
        .set_file_name(report_file_name(params.from, params.to))
        .add_filter(texts::XLSX_FILTER, &["xlsx"])
        .save_file();
    let Some(path) = picked.map(with_xlsx_extension) else {
        return;
    };
    dialog.set_busy(true);
    let weak_ctx = Rc::downgrade(ctx);
    let weak = dialog.as_weak();
    ctx.run_bg(
        move |crm| write_report(&crm, params, &path).map(|()| path),
        move |result| {
            if let Some(dialog) = weak.upgrade() {
                dialog.set_busy(false);
            }
            let Some(ctx) = weak_ctx.upgrade() else {
                return;
            };
            match result {
                Ok(path) => {
                    tracing::info!("report saved to {}", path.display());
                    ctx.flash(texts::saved(&path.display().to_string()));
                }
                Err(e) => ctx.show_error(&e),
            }
        },
    );
}

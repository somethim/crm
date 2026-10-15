//! The order filters and list (left panel).

use super::texts;
use super::workspace::{DOUBLE_CLICK, FILTER_DEBOUNCE, Workspace, Ws, to_usize};
use crm_services::domain::{Money, OrderKind, format, time};
use crm_services::{OrderFilter, OrderSummary};
use slint::{ModelRc, SharedString, StandardListViewItem, VecModel};
use std::rc::Rc;
use std::time::Instant;

/// The filter the bridge shows, or the text of why it cannot apply.
fn read_filter(w: &Workspace) -> Option<Result<OrderFilter, &'static str>> {
    w.bridge(|b| {
        let kind = match b.get_filter_kind() {
            0 => Some(OrderKind::Sale),
            1 => Some(OrderKind::Purchase),
            _ => None,
        };
        let from = format::parse_date(&b.get_filter_from());
        let to = format::parse_date(&b.get_filter_to());
        let text = b.get_filter_text().trim().to_owned();
        match (from, to) {
            (Ok(from), Ok(to)) if to < from => Err(texts::RANGE_REVERSED),
            (Ok(from), Ok(to)) => Ok(OrderFilter {
                kind,
                from,
                to,
                text: (!text.is_empty()).then_some(text),
            }),
            _ => Err(""),
        }
    })
}

fn list_row(o: &OrderSummary) -> ModelRc<StandardListViewItem> {
    let cells: Vec<StandardListViewItem> = [
        o.id.0.to_string(),
        o.kind.label().to_owned(),
        o.owner_name.clone(),
        format::money_lek(o.total),
        format::date(o.issued_on),
    ]
    .into_iter()
    .map(|s| StandardListViewItem::from(SharedString::from(s)))
    .collect();
    ModelRc::from(Rc::new(VecModel::from(cells)))
}

/// Sorts `orders` by list column `column` (0 Nr., 1 Lloji, 2 Klienti,
/// 3 Totali, 4 Data). The sort is stable, so ties keep the default order.
pub fn sort_orders(orders: &mut [OrderSummary], column: usize, ascending: bool) {
    let by = |a: &OrderSummary, b: &OrderSummary| match column {
        0 => a.id.0.cmp(&b.id.0),
        1 => a
            .kind
            .label()
            .to_lowercase()
            .cmp(&b.kind.label().to_lowercase()),
        2 => a
            .owner_name
            .to_lowercase()
            .cmp(&b.owner_name.to_lowercase()),
        3 => a.total.0.cmp(&b.total.0),
        4 => a.issued_on.cmp(&b.issued_on),
        _ => std::cmp::Ordering::Equal,
    };
    if ascending {
        orders.sort_by(by);
    } else {
        orders.sort_by(|a, b| by(b, a));
    }
}

/// The sum of the listed totals (saturating on overflow).
fn listed_sum(orders: &[OrderSummary]) -> Money {
    orders.iter().fold(Money(0), |sum, o| {
        sum.checked_add(o.total).unwrap_or(if o.total.0 < 0 {
            Money(i64::MIN)
        } else {
            Money(i64::MAX)
        })
    })
}

impl Workspace {
    /// A header click: sorts the list by `column` and keeps that order over
    /// refreshes.
    pub fn sort_list(&mut self, column: usize, ascending: bool) {
        self.list_sort = Some((column, ascending));
        self.refresh_list();
    }

    /// Re-queries the list with the current filters. An invalid date keeps the
    /// list; a reversed range shows its message.
    pub fn refresh_list(&mut self) {
        let Some(filter) = read_filter(self) else {
            return;
        };
        let filter = match filter {
            Ok(filter) => filter,
            Err(msg) => {
                if !msg.is_empty() {
                    self.bridge(|b| b.set_filter_error(msg.into()));
                }
                return;
            }
        };
        self.bridge(|b| b.set_filter_error(SharedString::new()));
        let Some(ctx) = self.ctx() else { return };
        match ctx.crm.list_orders(filter) {
            Ok(mut orders) => {
                if let Some((column, ascending)) = self.list_sort {
                    sort_orders(&mut orders, column, ascending);
                }
                let summary = texts::list_summary(orders.len(), listed_sum(&orders));
                self.bridge(|b| b.set_list_summary(summary.into()));
                let selected = self
                    .bridge(|b| to_usize(b.get_list_current()))
                    .flatten()
                    .and_then(|i| self.list_ids.get(i).copied());
                self.list_ids = orders.iter().map(|o| o.id).collect();
                self.list_model
                    .set_vec(orders.iter().map(list_row).collect::<Vec<_>>());
                let current = selected
                    .and_then(|id| self.list_ids.iter().position(|&x| x == id))
                    .map_or(-1, super::workspace::to_i32);
                self.bridge(|b| b.set_list_current(current));
            }
            Err(e) => tracing::warn!("listing orders: {e}"),
        }
    }

    /// Resets the filters: Shitje, today – today, no text.
    pub fn reset_filters(&mut self) {
        let today = format::date(time::today());
        self.bridge(|b| {
            b.set_filter_kind(0);
            b.set_filter_text(SharedString::new());
            b.set_filter_from(today.clone().into());
            b.set_filter_to(today.into());
            b.set_filter_error(SharedString::new());
        });
        self.refresh_list();
    }
}

/// `filter-text-edited`: applies 200 ms after the last keystroke.
pub fn text_edited(ws: &Ws) {
    let weak = Rc::downgrade(ws);
    ws.borrow()
        .filter_timer
        .start(slint::TimerMode::SingleShot, FILTER_DEBOUNCE, move || {
            if let Some(ws) = weak.upgrade()
                && let Ok(mut w) = ws.try_borrow_mut()
            {
                w.refresh_list();
            }
        });
}

/// `open-order`: opens (or activates) the invoice of list row `row`.
pub fn open_row(ws: &Ws, row: usize) {
    let id = ws.borrow().list_ids.get(row).copied();
    if let Some(id) = id {
        ws.borrow_mut().open_order(id);
    }
}

thread_local! {
    /// Runs for `DOUBLE_CLICK` after a single list click. A `slint::Timer`
    /// runs on Slint's clock (the testing backend's mock time in headless
    /// tests), so the double-click window does not depend on wall-clock speed.
    static CLICK_WINDOW: slint::Timer = slint::Timer::default();
}

/// `list-clicked`: a second click on the same row within 500 ms opens it.
pub fn clicked(ws: &Ws, row: usize) {
    let in_window = CLICK_WINDOW.with(slint::Timer::running);
    let double = {
        let mut w = ws.borrow_mut();
        let double = in_window && w.last_click.is_some_and(|(r, _)| r == row);
        // The timestamp is informational only; the timer is the window.
        w.last_click = if double {
            None
        } else {
            Some((row, Instant::now()))
        };
        double
    };
    CLICK_WINDOW.with(|t| {
        if double {
            t.stop();
        } else {
            t.start(slint::TimerMode::SingleShot, DOUBLE_CLICK, || {});
        }
    });
    if double {
        open_row(ws, row);
    }
}

/// "Eksporto…": asks for the file, then writes the filtered list as XLSX in
/// the background. Nothing happens for an empty list or an invalid filter.
pub fn export(ws: &Ws) {
    let Some((from, to)) = exportable(ws).map(|f| (f.from, f.to)) else {
        return;
    };
    let picked = rfd::FileDialog::new()
        .set_title(texts::EXPORT_LIST_TITLE)
        .set_file_name(crm_services::order_list_file_name(from, to))
        .add_filter(texts::XLSX_FILTER, &["xlsx"])
        .save_file();
    if let Some(path) = picked {
        export_to(ws, crate::reports::dialog::with_xlsx_extension(path));
    }
}

/// The filter to export: the list's, when it is valid and lists something.
fn exportable(ws: &Ws) -> Option<OrderFilter> {
    let w = ws.borrow();
    if w.list_ids.is_empty() {
        return None;
    }
    read_filter(&w)?.ok()
}

/// Writes the filtered list to `path` in the background (busy overlay), then
/// flashes the path or shows the error.
pub fn export_to(ws: &Ws, path: std::path::PathBuf) {
    let Some(filter) = exportable(ws) else { return };
    let Some(ctx) = ws.borrow().ctx() else { return };
    let weak_ctx = Rc::downgrade(&ctx);
    ctx.run_bg(
        move |crm| crm.export_order_list_xlsx(filter, &path).map(|()| path),
        move |result| {
            let Some(ctx) = weak_ctx.upgrade() else {
                return;
            };
            match result {
                Ok(path) => {
                    tracing::info!("order list saved to {}", path.display());
                    ctx.flash(texts::list_saved(&path.display().to_string()));
                }
                Err(e) => ctx.show_error(&e),
            }
        },
    );
}

//! Headless tests of the reports dialog (i-slint-backend-testing, no event
//! loop): the fields become `ReportParams`, the preview fills the dynamic table,
//! a field change marks it stale, and the export writes the file.
//!
//! `run_bg` delivers its result through the event loop, which these tests do not
//! run; they call the same services and display functions the jobs use.

use super::{dialog, texts};
use crate::shell::format;
use crate::{ReportDialog, ReportsBridge, UiContext};
use crm_services::domain::{DraftLine, Money, OrderDraft, OrderKind, ProductId, ProductInput, Qty};
use crm_services::{Crm, DataPaths, ReportKind, ReportParams};
use i_slint_backend_testing::ElementHandle;
use jiff::civil::{Date, date};
use slint::{ComponentHandle, Model};
use std::rc::Rc;

struct Harness {
    app: crate::AppWindow,
    ctx: Rc<UiContext>,
    dir: tempfile::TempDir,
}

fn harness() -> Harness {
    i_slint_backend_testing::init_no_event_loop();
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    paths.ensure_dirs().unwrap();
    let crm = Crm::open(paths.clone(), &crm_services::DbKey::for_tests(1)).unwrap();
    let (app, ctx) = crate::build(
        crm,
        crm_services::Vault::with_params(paths.clone(), crm_services::KdfParams::TEST),
        paths,
    )
    .unwrap();
    app.show().unwrap();
    Harness { app, ctx, dir }
}

fn product(crm: &Crm, code: &str, name: &str, qty: i64) -> ProductId {
    crm.create_product(ProductInput {
        code: code.into(),
        name: name.into(),
        unit: "COPE".into(),
        buy_price: Money(100),
        sell_price: Money(250),
        quantity: Qty(qty),
    })
    .unwrap()
    .product
    .id
}

fn line(id: ProductId, qty: i64) -> DraftLine {
    DraftLine {
        product_id: Some(id),
        name: String::new(),
        quantity: Qty(qty),
        // The sell price of every product here (a sale saves the given price, F-012).
        price: Money(250),
        buy_price: None,
        sell_price: None,
    }
}

fn save(crm: &Crm, kind: OrderKind, owner: &str, on: Date, lines: Vec<DraftLine>) {
    let draft = OrderDraft {
        kind,
        owner_id: None,
        owner_name: owner.into(),
        owner_description: String::new(),
        issued_on: on,
        lines,
    };
    crm.save_order(draft, None, None).unwrap();
}

/// Two sales and one purchase: 02.03 sale (A×2, B×1), 03.03 purchase (B×5),
/// 05.03 sale (A×1).
fn seed(crm: &Crm) {
    let a = product(crm, "A1", "Ujë", 10);
    let b = product(crm, "B1", "Lëng", 10);
    save(
        crm,
        OrderKind::Sale,
        "Klienti Një",
        date(2026, 3, 2),
        vec![line(a, 2), line(b, 1)],
    );
    let mut purchase = line(b, 5);
    purchase.price = Money(100);
    purchase.buy_price = Some(Money(100));
    purchase.sell_price = Some(Money(250));
    save(
        crm,
        OrderKind::Purchase,
        "Furnitori",
        date(2026, 3, 3),
        vec![purchase],
    );
    save(
        crm,
        OrderKind::Sale,
        "Klienti Dy",
        date(2026, 3, 5),
        vec![line(a, 1)],
    );
}

/// A dialog (not shown) with the given fields.
fn dialog_with(
    h: &Harness,
    from: &str,
    to: &str,
    format_index: i32,
    kind_index: i32,
) -> ReportDialog {
    let d = dialog::create(&h.ctx).unwrap();
    d.set_from_text(from.into());
    d.set_to_text(to.into());
    d.set_format_index(format_index);
    d.set_kind_index(kind_index);
    d
}

/// Runs the preview the way "Shiko të dhënat" does, without the event loop.
fn preview(h: &Harness, d: &ReportDialog) {
    let params = dialog::params_from(d).unwrap();
    let preview = h.ctx.crm.report_preview(params).unwrap();
    dialog::show_preview(d, &preview);
}

fn column(d: &ReportDialog, index: usize) -> Vec<String> {
    let rows = d.get_rows();
    (0..rows.row_count())
        .map(|r| {
            rows.row_data(r)
                .unwrap()
                .row_data(index)
                .unwrap()
                .text
                .to_string()
        })
        .collect()
}

fn headers(d: &ReportDialog) -> Vec<String> {
    d.get_columns()
        .iter()
        .map(|c| c.title.to_string())
        .collect()
}

#[test]
fn the_bridge_opens_the_dialog_with_the_default_range() {
    let h = harness();
    let before = h.ctx.windows.borrow().len();
    h.app.global::<ReportsBridge>().invoke_open_dialog();
    let windows = h.ctx.windows.borrow();
    assert_eq!(windows.len(), before + 1);
    assert!(windows.last().unwrap().is_visible());
    drop(windows);
    assert!(h.app.global::<crate::ShellBridge>().get_input_blocked());

    let d = dialog::open(&h.ctx).unwrap();
    let today = format::today();
    assert_eq!(d.get_from_text(), format::date(today.first_of_month()));
    assert_eq!(d.get_to_text(), format::date(today));
    assert_eq!((d.get_format_index(), d.get_kind_index()), (0, 0));
    let params = dialog::params_from(&d).unwrap();
    assert_eq!(
        params,
        ReportParams {
            kind: ReportKind::Total,
            from: today.first_of_month(),
            to: today,
            order_kind: None,
        }
    );
    d.invoke_close_clicked();
    assert!(!d.window().is_visible());
}

#[test]
fn reversed_or_invalid_dates_are_refused() {
    let h = harness();
    let d = dialog_with(&h, "05.03.2026", "04.03.2026", 0, 0);
    assert_eq!(
        dialog::params_from(&d).unwrap_err(),
        "Data «Deri» nuk mund të jetë para datës «Nga»"
    );
    d.set_to_text("x".into());
    assert_eq!(dialog::params_from(&d).unwrap_err(), texts::TO_INVALID);
    d.set_from_text("".into());
    assert_eq!(dialog::params_from(&d).unwrap_err(), texts::FROM_INVALID);
    // The same day on both ends is a valid range.
    d.set_from_text("4.3.2026".into());
    d.set_to_text("2026-03-04".into());
    let params = dialog::params_from(&d).unwrap();
    assert_eq!(
        (params.from, params.to),
        (date(2026, 3, 4), date(2026, 3, 4))
    );
}

#[test]
fn total_report_both_kinds_lists_every_line() {
    let h = harness();
    seed(&h.ctx.crm);
    let d = dialog_with(&h, "01.03.2026", "31.03.2026", 0, 0);
    preview(&h, &d);
    assert_eq!(
        headers(&d),
        [
            "Lloji",
            "Data",
            "Numër",
            "Klienti/Furnitori",
            "Kodi",
            "Përshkrimi",
            "Njësia",
            "Sasia",
            "Çmimi",
            "Vlera"
        ]
    );
    assert_eq!(d.get_report_title(), "REGJISTRI ANALITIK I POROSIVE");
    assert_eq!(
        d.get_report_subtitle(),
        "01.03.2026 - 31.03.2026 · 4 rreshta"
    );
    assert_eq!(d.get_note(), "");
    // Lloji and Data only on an order's first line; Data = issued_on.
    assert_eq!(column(&d, 0), ["Shitje", "", "Blerje", "Shitje"]);
    assert_eq!(
        column(&d, 1),
        ["02.03.2026", "", "03.03.2026", "05.03.2026"]
    );
    assert_eq!(
        column(&d, 3),
        ["Klienti Një", "", "Furnitori", "Klienti Dy"]
    );
    assert_eq!(column(&d, 4), ["A1", "B1", "B1", "A1"]);
    assert_eq!(column(&d, 7), ["2", "1", "5", "1"]);
    // Sale lines take the sell price (2,50), the purchase its buy price (1,00).
    assert_eq!(column(&d, 8), ["2,50", "2,50", "1,00", "2,50"]);
    assert_eq!(column(&d, 9), ["5,00", "2,50", "5,00", "2,50"]);
}

#[test]
fn purchases_only_shows_fb() {
    let h = harness();
    seed(&h.ctx.crm);
    let d = dialog_with(&h, "01.03.2026", "31.03.2026", 0, 2);
    preview(&h, &d);
    assert_eq!(column(&d, 0), ["Blerje"]);
    assert_eq!(column(&d, 1), ["03.03.2026"]);
    let d = dialog_with(&h, "01.03.2026", "31.03.2026", 0, 1);
    preview(&h, &d);
    assert_eq!(column(&d, 0), ["Shitje", "", "Shitje"]);
}

#[test]
fn by_products_aggregates_rows() {
    let h = harness();
    seed(&h.ctx.crm);
    let d = dialog_with(&h, "01.03.2026", "31.03.2026", 1, 0);
    preview(&h, &d);
    assert_eq!(d.get_report_title(), "RAPORTI SIPAS ARTIKUJVE");
    assert_eq!(
        headers(&d),
        [
            "Kodi",
            "Përshkrimi",
            "Njësia",
            "Sasia Totale",
            "Çmimi Mesatar",
            "Vlera Totale"
        ]
    );
    assert_eq!(column(&d, 0), ["A1", "B1"]);
    assert_eq!(column(&d, 3), ["3", "6"]);
    assert_eq!(column(&d, 5), ["7,50", "7,50"]);
}

#[test]
fn a_narrower_range_drops_an_invoice() {
    let h = harness();
    seed(&h.ctx.crm);
    let d = dialog_with(&h, "03.03.2026", "05.03.2026", 0, 0);
    preview(&h, &d);
    assert_eq!(column(&d, 0), ["Blerje", "Shitje"]);
    assert_eq!(column(&d, 1), ["03.03.2026", "05.03.2026"]);
    assert_eq!(
        d.get_report_subtitle(),
        "03.03.2026 - 05.03.2026 · 2 rreshta"
    );
}

#[test]
fn an_empty_range_says_so() {
    let h = harness();
    seed(&h.ctx.crm);
    let d = dialog_with(&h, "01.04.2026", "30.04.2026", 0, 0);
    preview(&h, &d);
    assert_eq!(d.get_rows().row_count(), 0);
    assert_eq!(d.get_note(), texts::NO_ROWS);
    assert_eq!(d.get_columns().row_count(), 10);
}

#[test]
fn more_than_500_rows_are_truncated_with_a_note() {
    let h = harness();
    let crm = &h.ctx.crm;
    let id = product(crm, "C1", "Kafe", 1000);
    let lines: Vec<DraftLine> = (0..501).map(|_| line(id, 1)).collect();
    save(crm, OrderKind::Sale, "Klienti", date(2026, 3, 2), lines);
    let d = dialog_with(&h, "01.03.2026", "31.03.2026", 0, 0);
    preview(&h, &d);
    assert_eq!(d.get_rows().row_count(), 500);
    assert_eq!(
        d.get_report_subtitle(),
        "01.03.2026 - 31.03.2026 · 501 rreshta"
    );
    assert_eq!(
        d.get_note(),
        "Parapamja tregon 500 rreshtat e parë. Skedari i ruajtur i përmban të gjithë."
    );
}

#[test]
fn changing_a_field_marks_the_preview_stale() {
    let h = harness();
    seed(&h.ctx.crm);
    let d = dialog::open(&h.ctx).unwrap();
    d.set_from_text("01.03.2026".into());
    d.set_to_text("31.03.2026".into());
    // Before any preview, edits do not mark anything.
    let edit = ElementHandle::find_by_element_id(&d, "DateField::edit")
        .next()
        .unwrap();
    edit.set_accessible_value("02.03.2026");
    assert!(!d.get_stale());

    preview(&h, &d);
    assert!(d.get_has_preview() && !d.get_stale());
    edit.set_accessible_value("03.03.2026");
    assert!(d.get_stale(), "a date edit marks the preview stale");
    assert_eq!(d.get_rows().row_count(), 4, "no automatic reload");

    preview(&h, &d);
    assert!(!d.get_stale());
    let previous = ElementHandle::find_by_element_id(&d, "DateField::edit")
        .nth(1)
        .unwrap();
    previous.set_accessible_value("30.03.2026");
    assert!(d.get_stale(), "the second date field too");
    d.invoke_close_clicked();
}

#[test]
fn the_calendars_pick_dates_within_the_range_and_mark_the_preview_stale() {
    use crate::shell::calendar::testing as cal;
    let h = harness();
    seed(&h.ctx.crm);
    let d = dialog::open(&h.ctx).unwrap();
    d.set_from_text("01.03.2026".into());
    d.set_to_text("31.03.2026".into());
    preview(&h, &d);
    assert!(d.get_has_preview() && !d.get_stale());

    // Nga: on its month, nothing after Deri; "Sot" (after Deri) is greyed.
    let from = cal::field(&d, "ReportDialog::from");
    cal::click_button(&from);
    assert!(cal::is_open(&d));
    assert_eq!(cal::title(&d), "Mars 2026");
    assert_eq!(
        cal::day(&d, "01.04.2026").unwrap().accessible_enabled(),
        Some(false)
    );
    let sot = cal::today_button(&d);
    assert_eq!(sot.accessible_enabled(), Some(false));
    cal::click(&sot);
    assert!(cal::is_open(&d), "a greyed Sot does nothing");
    assert_eq!(d.get_from_text(), "01.03.2026");
    cal::click(&cal::day(&d, "02.03.2026").unwrap());
    assert!(!cal::is_open(&d));
    assert_eq!(d.get_from_text(), "02.03.2026");
    assert!(d.get_stale(), "a picked date marks the preview stale");

    // Deri: nothing before Nga; "Sot" picks today.
    preview(&h, &d);
    assert!(!d.get_stale());
    let to = cal::field(&d, "ReportDialog::to");
    cal::open(&to);
    assert_eq!(
        cal::day(&d, "01.03.2026").unwrap().accessible_enabled(),
        Some(false)
    );
    assert_eq!(
        cal::day(&d, "02.03.2026").unwrap().accessible_enabled(),
        Some(true)
    );
    cal::click(&cal::today_button(&d));
    assert!(!cal::is_open(&d));
    assert_eq!(d.get_to_text(), format::date(format::today()));
    assert!(d.get_stale(), "the second date field too");

    // A busy dialog's fields do not open.
    d.set_busy(true);
    cal::open(&from);
    assert!(!cal::is_open(&d));
    d.set_busy(false);
    d.invoke_close_clicked();
}

#[test]
fn export_writes_the_xlsx_file() {
    let h = harness();
    seed(&h.ctx.crm);
    let d = dialog_with(&h, "01.03.2026", "31.03.2026", 0, 0);
    let params = dialog::params_from(&d).unwrap();
    assert_eq!(
        crm_services::report_file_name(params.from, params.to),
        "raporti-porosite-2026-03-01-2026-03-31.xlsx"
    );
    let path = dialog::with_xlsx_extension(h.dir.path().join("raporti"));
    assert_eq!(path.extension().unwrap(), "xlsx");
    dialog::write_report(&h.ctx.crm, params, &path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.len() > 1000);
    assert_eq!(&bytes[..4], b"PK\x03\x04", "an XLSX (zip) file");
    assert_eq!(
        dialog::with_xlsx_extension(path.with_extension("XLSX")),
        path.with_extension("XLSX")
    );
}

#[test]
fn cells_are_formatted_by_the_domain_formatters() {
    use crm_services::Cell;
    assert_eq!(dialog::cell_text(&Cell::Text(String::new())), "");
    assert_eq!(dialog::cell_text(&Cell::Int(42)), "42");
    assert_eq!(dialog::cell_text(&Cell::Money(Money(123_456))), "1.234,56");
    assert_eq!(
        dialog::cell_text(&Cell::AmountText(Money(123_456))),
        "1.234,56"
    );
}

#[test]
fn the_preview_button_shows_a_field_error_in_the_dialog() {
    let h = harness();
    let d = dialog::open(&h.ctx).unwrap();
    d.set_from_text("05.03.2026".into());
    d.set_to_text("04.03.2026".into());
    ElementHandle::find_by_element_id(&d, "ReportDialog::preview-button")
        .next()
        .unwrap()
        .invoke_accessible_default_action();
    assert_eq!(d.get_error(), texts::RANGE_REVERSED);
    assert!(!d.get_busy(), "nothing was started");
    ElementHandle::find_by_element_id(&d, "ReportDialog::save-button")
        .next()
        .unwrap()
        .invoke_accessible_default_action();
    assert_eq!(d.get_error(), texts::RANGE_REVERSED);
    ElementHandle::find_by_element_id(&d, "ReportDialog::close-button")
        .next()
        .unwrap()
        .invoke_accessible_default_action();
    assert!(!d.window().is_visible());
    assert!(!h.app.global::<crate::ShellBridge>().get_input_blocked());
}

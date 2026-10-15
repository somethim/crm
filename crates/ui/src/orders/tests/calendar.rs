//! The calendar popup of the date fields: the list filters (Nga / Deri, bounded
//! by each other) and the document's Data (unbounded), by mouse and keyboard.

use super::{Harness, harness};
use crate::shell::calendar::testing as cal;
use crate::shell::format;
use crm_services::domain::{DraftLine, OrderDraft, OrderKind, Qty};
use jiff::civil::date;
use slint::platform::Key;
use slint::{ComponentHandle, Model};

fn sale(h: &Harness, owner: &str, issued_on: jiff::civil::Date) -> i64 {
    let p = h.product(&format!("ARTIKULL {owner}"), 10, 30);
    h.ctx
        .crm
        .save_order(
            OrderDraft {
                kind: OrderKind::Sale,
                owner_id: None,
                owner_name: owner.into(),
                owner_description: String::new(),
                issued_on,
                lines: vec![DraftLine {
                    product_id: Some(p.product.id),
                    name: p.product.name.clone(),
                    quantity: Qty(1),
                    ..DraftLine::default()
                }],
            },
            None,
            None,
        )
        .unwrap()
        .0
}

fn listed(h: &Harness) -> Vec<String> {
    let rows = h.bridge().get_list_rows();
    (0..rows.row_count())
        .map(|i| {
            rows.row_data(i)
                .unwrap()
                .row_data(0)
                .unwrap()
                .text
                .to_string()
        })
        .collect()
}

fn dirty(h: &Harness) -> bool {
    h.ws.borrow().current().is_some_and(|t| t.dirty)
}

#[test]
fn picking_a_day_in_nga_reloads_the_list() {
    let h = harness();
    let early = sale(&h, "Bar Mars", date(2026, 3, 5));
    let late = sale(&h, "Bar Fund", date(2026, 3, 20));
    h.bridge().set_filter_from("10.03.2026".into());
    h.bridge().set_filter_to("31.03.2026".into());
    h.bridge().invoke_filter_changed();
    assert_eq!(listed(&h), vec![late.to_string()]);

    let from = cal::field(&h.app, "Filters::from");
    assert!(!cal::is_open(&h.app));
    cal::click_button(&from);
    assert!(cal::is_open(&h.app), "the button opens the calendar");
    assert_eq!(cal::title(&h.app), "Mars 2026", "on the field's month");
    assert_eq!(cal::days(&h.app).len(), 42);
    // 1 March 2026 is a Sunday: the grid starts on Monday 23 February.
    assert_eq!(
        cal::days(&h.app)[0].accessible_label().unwrap(),
        "23.02.2026"
    );
    let selected = cal::day(&h.app, "10.03.2026").unwrap();
    assert_eq!(selected.accessible_item_selected(), Some(true));
    // Nga cannot pass Deri: the days after 31.03 are greyed.
    let after = cal::day(&h.app, "01.04.2026").unwrap();
    assert_eq!(after.accessible_enabled(), Some(false));
    cal::click(&after);
    assert!(cal::is_open(&h.app), "a greyed day does not pick");
    assert_eq!(h.bridge().get_filter_from(), "10.03.2026");

    cal::click(&cal::day(&h.app, "05.03.2026").unwrap());
    assert!(!cal::is_open(&h.app), "a click picks and closes");
    assert_eq!(h.bridge().get_filter_from(), "05.03.2026");
    assert_eq!(cal::text(&from), "05.03.2026");
    assert_eq!(
        listed(&h),
        vec![late.to_string(), early.to_string()],
        "the list reloads as when typing"
    );
}

#[test]
fn deri_greys_the_days_before_nga_and_moves_by_month() {
    let h = harness();
    let late = sale(&h, "Bar Prill", date(2026, 4, 2));
    h.bridge().set_filter_from("05.03.2026".into());
    h.bridge().set_filter_to("31.03.2026".into());
    h.bridge().invoke_filter_changed();
    assert!(listed(&h).is_empty());

    let to = cal::field(&h.app, "Filters::to");
    cal::open(&to);
    assert_eq!(cal::title(&h.app), "Mars 2026");
    assert_eq!(
        cal::day(&h.app, "04.03.2026").unwrap().accessible_enabled(),
        Some(false)
    );
    assert_eq!(
        cal::day(&h.app, "05.03.2026").unwrap().accessible_enabled(),
        Some(true),
        "the bound itself is allowed"
    );
    // The › button shows April; clicking a day there picks it.
    let next = cal::popup_part(&h.app, "DateField::next");
    cal::click(&next);
    assert_eq!(cal::title(&h.app), "Prill 2026");
    cal::click(&cal::day(&h.app, "02.04.2026").unwrap());
    assert_eq!(h.bridge().get_filter_to(), "02.04.2026");
    assert_eq!(listed(&h), vec![late.to_string()]);

    // Esc closes without changing anything; an outside click closes too.
    cal::open(&to);
    assert!(cal::is_open(&h.app));
    cal::key(h.app.window(), Key::Escape, false);
    assert!(!cal::is_open(&h.app));
    cal::open(&to);
    assert!(cal::is_open(&h.app));
    cal::click_at(h.app.window(), 1200.0, 5.0);
    assert!(!cal::is_open(&h.app), "a click outside closes");
    assert_eq!(h.bridge().get_filter_to(), "02.04.2026");
}

#[test]
fn the_document_date_by_keyboard_marks_the_order_unsaved() {
    let h = harness();
    h.new_document(0);
    assert!(!dirty(&h));
    let today = format::today();
    let field = cal::field(&h.app, "DocumentView::date");
    assert_eq!(cal::text(&field), format::date(today));
    // Owner → description → date, then F4.
    h.tap(Key::Tab);
    h.tap(Key::Tab);
    cal::key(h.app.window(), Key::F4, false);
    h.pump();
    assert!(cal::is_open(&h.app), "F4 opens the calendar");
    assert_eq!(cal::title(&h.app), format::month_year(today));
    let today_cell = cal::day(&h.app, &format::date(today)).unwrap();
    assert_eq!(today_cell.accessible_item_selected(), Some(true));
    // No bounds on the order date: every cell can be picked.
    assert!(
        cal::days(&h.app)
            .iter()
            .all(|d| d.accessible_enabled() == Some(true))
    );

    // PageDown / PageUp move by a month, Esc closes and keeps the date.
    cal::key(h.app.window(), Key::PageDown, false);
    let next_month = crate::shell::calendar::add_months(today, 1);
    assert_eq!(cal::title(&h.app), format::month_year(next_month));
    cal::key(h.app.window(), Key::PageUp, false);
    assert_eq!(cal::title(&h.app), format::month_year(today));
    cal::key(h.app.window(), Key::Escape, false);
    h.pump();
    assert!(!cal::is_open(&h.app));
    assert_eq!(h.bridge().get_doc_date(), format::date(today));
    assert!(!dirty(&h), "nothing picked");

    // Alt+Down reopens; Left, Up, Right, Down, Up moves to 8 days back; Enter picks.
    cal::key(h.app.window(), Key::DownArrow, true);
    h.pump();
    assert!(cal::is_open(&h.app), "Alt+Down opens the calendar");
    for k in [
        Key::LeftArrow,
        Key::UpArrow,
        Key::RightArrow,
        Key::DownArrow,
    ] {
        cal::key(h.app.window(), k, false);
    }
    cal::key(h.app.window(), Key::LeftArrow, false);
    cal::key(h.app.window(), Key::UpArrow, false);
    cal::key(h.app.window(), Key::Return, false);
    h.pump();
    let picked = today.checked_sub(jiff::Span::new().days(8)).unwrap();
    assert!(!cal::is_open(&h.app), "Enter picks and closes");
    assert_eq!(h.bridge().get_doc_date(), format::date(picked));
    assert_eq!(h.ws.borrow().current().unwrap().header.date, picked);
    assert!(dirty(&h), "a picked date marks the order unsaved");

    // The focus is back in the text box (which selects its text on focus):
    // typing replaces the date.
    h.type_text("x");
    assert_eq!(h.bridge().get_doc_date(), "x");
}

#[test]
fn sot_picks_today_in_the_document() {
    let h = harness();
    h.new_document(0);
    h.bridge().set_doc_date("01.02.2025".into());
    let field = cal::field(&h.app, "DocumentView::date");
    cal::open(&field);
    assert_eq!(cal::title(&h.app), "Shkurt 2025");
    let sot = cal::today_button(&h.app);
    assert_eq!(sot.accessible_label().unwrap(), "Sot");
    cal::click(&sot);
    assert!(!cal::is_open(&h.app));
    let today = format::today();
    assert_eq!(h.bridge().get_doc_date(), format::date(today));
    assert_eq!(h.ws.borrow().current().unwrap().header.date, today);
}

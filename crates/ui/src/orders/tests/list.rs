//! AC 1–2: the filters and the invoice list.

use super::harness;
use crate::shell::format;
use crm_services::domain::{DraftLine, Money, OrderDraft, OrderKind, Qty};
use slint::Model;

fn sale(h: &super::Harness, owner: &str, issued_on: jiff::civil::Date) -> i64 {
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

fn listed(h: &super::Harness) -> Vec<String> {
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

#[test]
fn the_today_filter_shows_one_and_a_digit_search_ignores_the_dates() {
    let h = harness();
    let today = format::today();
    let earlier = today.checked_sub(jiff::Span::new().days(10)).unwrap();
    let old = sale(&h, "Bar Vjeter", earlier);
    let new = sale(&h, "Bar Sot", today);
    // Start-up filter: Shitje, today – today.
    assert_eq!(h.bridge().get_filter_kind(), 0);
    assert_eq!(h.bridge().get_filter_from(), format::date(today));
    h.bridge().invoke_refresh();
    assert_eq!(listed(&h), vec![new.to_string()]);
    // A digits-only search is the invoice number, whatever the dates.
    h.bridge().set_filter_text(old.to_string().into());
    h.bridge().invoke_filter_text_edited();
    assert_eq!(listed(&h), vec![new.to_string()], "applied after 200 ms");
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(250));
    assert_eq!(listed(&h), vec![old.to_string()]);
    // Text search by name within the dates.
    h.bridge().set_filter_text("sot".into());
    h.bridge().invoke_filter_text_edited();
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(250));
    assert_eq!(listed(&h), vec![new.to_string()]);
    // Widen the range: newest issued_on first.
    h.bridge().set_filter_text("".into());
    h.bridge().set_filter_from(format::date(earlier).into());
    h.bridge().invoke_filter_changed();
    assert_eq!(listed(&h), vec![new.to_string(), old.to_string()]);
    // Exactly Nr., Lloji, Klienti, Totali (LEK), Data.
    let row = h.bridge().get_list_rows().row_data(1).unwrap();
    assert_eq!(row.row_count(), 5);
    assert_eq!(row.row_data(0).unwrap().text, old.to_string());
    assert_eq!(row.row_data(1).unwrap().text, "Shitje");
    assert_eq!(row.row_data(2).unwrap().text, "Bar Vjeter");
    assert!(row.row_data(3).unwrap().text.ends_with(" LEK"));
    assert_eq!(row.row_data(4).unwrap().text, format::date(earlier));
    // Blerje shows none; back to Shitje both.
    h.bridge().set_filter_kind(1);
    h.bridge().invoke_filter_changed();
    assert!(listed(&h).is_empty());
    h.bridge().set_filter_kind(0);
    h.bridge().invoke_filter_changed();
    assert_eq!(listed(&h).len(), 2);
    // Deri before Nga: the message, the list stays.
    h.bridge()
        .set_filter_to(format::date(earlier.yesterday().unwrap()).into());
    h.bridge().invoke_filter_changed();
    assert_eq!(
        h.bridge().get_filter_error(),
        "Data «Deri» nuk mund të jetë para datës «Nga»"
    );
    assert_eq!(listed(&h).len(), 2);
    // Pastro: Shitje, today – today.
    h.bridge().invoke_clear_filters();
    assert_eq!(h.bridge().get_filter_error(), "");
    assert_eq!(h.bridge().get_filter_kind(), 0);
    assert_eq!(h.bridge().get_filter_to(), format::date(today));
    assert_eq!(listed(&h), vec![new.to_string()]);
    // The list refreshes on data_changed.
    let third = sale(&h, "Bar Tjeter", today);
    h.ctx.notify_data_changed();
    assert_eq!(listed(&h), vec![third.to_string(), new.to_string()]);
}

#[test]
fn enter_or_a_double_click_opens_the_invoice() {
    let h = harness();
    let id = sale(&h, "Bar Sot", format::today());
    h.bridge().invoke_refresh();
    h.bridge().invoke_list_clicked(0);
    assert_eq!(h.bridge().get_open_tab_count(), 0, "one click only selects");
    h.bridge().invoke_list_clicked(0);
    assert_eq!(h.bridge().get_open_tab_count(), 1, "a double click opens");
    assert_eq!(h.bridge().get_doc_number(), id.to_string());
    h.bridge().invoke_open_order(0);
    assert_eq!(
        h.bridge().get_open_tab_count(),
        1,
        "already open: activated"
    );
}

#[test]
fn the_shitje_blerje_toggle_switches_the_list() {
    use i_slint_backend_testing::ElementHandle;
    let h = harness();
    let id = sale(&h, "Bar Sot", format::today());
    h.bridge().invoke_refresh();
    let button = |name: &str| {
        ElementHandle::find_by_element_id(&h.app, &format!("Filters::{name}"))
            .next()
            .unwrap_or_else(|| panic!("Filters::{name}"))
    };
    assert_eq!(button("sale").accessible_label().unwrap(), "Shitje");
    assert_eq!(button("purchase").accessible_label().unwrap(), "Blerje");
    assert_eq!(button("sale").accessible_checked(), Some(true));
    assert_eq!(button("purchase").accessible_checked(), Some(false));
    assert_eq!(listed(&h), vec![id.to_string()]);

    button("purchase").invoke_accessible_default_action();
    assert_eq!(h.bridge().get_filter_kind(), 1);
    assert_eq!(button("purchase").accessible_checked(), Some(true));
    assert_eq!(button("sale").accessible_checked(), Some(false));
    assert!(listed(&h).is_empty(), "the list follows at once");

    button("sale").invoke_accessible_default_action();
    assert_eq!(h.bridge().get_filter_kind(), 0);
    assert_eq!(listed(&h), vec![id.to_string()]);
    // There is no "both" choice and no ◀ ▶ date steppers any more.
    assert!(
        ElementHandle::find_by_element_id(&h.app, "Filters::kind")
            .next()
            .is_none()
    );
    for arrow in ["◀", "▶"] {
        assert!(
            ElementHandle::find_by_accessible_label(&h.app, arrow)
                .next()
                .is_none(),
            "{arrow}"
        );
    }
}

/// A sale for `owner` on `issued_on` with one line of `qty` × 300,00 Lekë.
fn sale_of(h: &super::Harness, owner: &str, issued_on: jiff::civil::Date, qty: i64) -> i64 {
    let p = h.product(&format!("ARTIKULL {owner}"), 100, 300);
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
                    quantity: Qty(qty),
                    price: Money(30_000),
                    ..DraftLine::default()
                }],
            },
            None,
            None,
        )
        .unwrap()
        .0
}

#[test]
fn the_summary_counts_and_sums_the_listed_orders() {
    let h = harness();
    h.bridge().invoke_refresh();
    assert_eq!(h.bridge().get_list_summary(), "0 porosi");
    let today = format::today();
    sale_of(&h, "Bar A", today, 1);
    sale_of(&h, "Bar B", today, 2);
    sale_of(&h, "Bar C", today, 3);
    h.bridge().invoke_refresh();
    assert_eq!(
        h.bridge().get_list_summary(),
        "3 porosi · Totali: 1.800,00 LEK"
    );
}

#[test]
fn a_header_click_sorts_and_the_sort_and_selection_survive_a_refresh() {
    let h = harness();
    let today = format::today();
    let earlier = today.checked_sub(jiff::Span::new().days(3)).unwrap();
    let a = sale_of(&h, "bar zeta", today, 2);
    let b = sale_of(&h, "Bar Alfa", earlier, 5);
    let c = sale_of(&h, "Bar Beta", today, 1);
    h.bridge().set_filter_from(format::date(earlier).into());
    h.bridge().invoke_filter_changed();
    // Default: issued_on DESC, id DESC.
    assert_eq!(listed(&h), [c, a, b].map(|i| i.to_string()));

    // Totali ascending, then descending.
    h.bridge().invoke_list_sort(3, true);
    assert_eq!(listed(&h), [c, a, b].map(|i| i.to_string()));
    h.bridge().invoke_list_sort(3, false);
    assert_eq!(listed(&h), [b, a, c].map(|i| i.to_string()));
    // Klienti, case-insensitive.
    h.bridge().invoke_list_sort(2, true);
    assert_eq!(listed(&h), [b, c, a].map(|i| i.to_string()));
    // Data ascending; the tie (a, c) keeps the default order (c before a).
    h.bridge().invoke_list_sort(4, true);
    assert_eq!(listed(&h), [b, c, a].map(|i| i.to_string()));
    // Nr. descending.
    h.bridge().invoke_list_sort(0, false);
    assert_eq!(listed(&h), [c, b, a].map(|i| i.to_string()));

    // The selection follows the order, not the row.
    h.bridge().set_list_current(2);
    h.bridge().invoke_list_sort(0, true);
    assert_eq!(listed(&h), [a, b, c].map(|i| i.to_string()));
    assert_eq!(h.bridge().get_list_current(), 0);

    // A refresh (F5, a save elsewhere) keeps the sort.
    let d = sale_of(&h, "Bar Delta", today, 1);
    h.ctx.notify_data_changed();
    assert_eq!(listed(&h), [a, b, c, d].map(|i| i.to_string()));
    assert_eq!(h.bridge().get_list_current(), 0);

    // Enter opens the order of the sorted row.
    h.bridge().invoke_open_order(1);
    assert_eq!(h.bridge().get_doc_number(), b.to_string());
}

#[test]
fn eksporto_writes_the_filtered_list_and_is_disabled_without_rows() {
    use i_slint_backend_testing::ElementHandle;
    let h = harness();
    h.bridge().invoke_refresh();
    let button = || {
        ElementHandle::find_by_element_id(&h.app, "Filters::export")
            .next()
            .expect("Filters::export")
    };
    assert_eq!(button().accessible_label().unwrap(), "Eksporto…");
    assert_eq!(button().accessible_enabled(), Some(false), "empty list");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("porosite.xlsx");
    super::super::list::export_to(&h.ws, path.clone());
    assert!(!h.ctx.is_busy(), "nothing to export");

    let today = format::today();
    sale_of(&h, "Bar A", today, 1);
    h.bridge().invoke_refresh();
    assert_eq!(button().accessible_enabled(), Some(true));
    super::super::list::export_to(&h.ws, path.clone());
    // The job runs on the worker thread; the file appears at once, whole
    // (written through `.tmp`).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !path.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(std::fs::metadata(&path).unwrap().len() > 0);

    // A reversed range disables it.
    h.bridge()
        .set_filter_to(format::date(today.yesterday().unwrap()).into());
    h.bridge().invoke_filter_changed();
    assert_eq!(button().accessible_enabled(), Some(false), "reversed range");
}

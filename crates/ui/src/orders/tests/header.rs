//! The document header (one column of label + content-sized fields), its Tab
//! order, the footer (only "Totali") and the 30 / 70 split of the window.

use super::{Harness, harness};
use crm_services::domain::OwnerKind;
use i_slint_backend_testing::ElementHandle;
use slint::ComponentHandle;
use slint::platform::Key;

fn element(h: &Harness, id: &str) -> Option<ElementHandle> {
    ElementHandle::find_by_element_id(&h.app, id).next()
}

fn width(h: &Harness, id: &str) -> f32 {
    element(h, id)
        .unwrap_or_else(|| panic!("{id} not found"))
        .size()
        .width
}

fn label(h: &Harness, id: &str) -> String {
    element(h, id)
        .and_then(|e| e.accessible_label())
        .unwrap_or_default()
        .to_string()
}

#[test]
fn the_header_is_one_column_of_content_sized_fields() {
    let h = harness();
    h.app
        .window()
        .set_size(slint::LogicalSize::new(1280.0, 800.0));
    h.owner(OwnerKind::Client, "Bar Alfa");
    h.new_document(0);
    assert_eq!(width(&h, "DocumentView::owner"), 420.0);
    assert_eq!(width(&h, "DocumentView::description"), 420.0);
    assert_eq!(width(&h, "DocumentView::date"), 140.0);
    // The fields start at the same x, under one another.
    let x = |id: &str| element(&h, id).unwrap().absolute_position().x;
    assert_eq!(x("DocumentView::owner"), x("DocumentView::description"));
    assert_eq!(x("DocumentView::owner"), x("DocumentView::date"));
    assert_eq!(x("DocumentView::owner"), x("DocumentView::number"));
    let y = |id: &str| element(&h, id).unwrap().absolute_position().y;
    assert!(y("DocumentView::owner") < y("DocumentView::description"));
    assert!(y("DocumentView::description") < y("DocumentView::date"));
    assert!(y("DocumentView::date") < y("DocumentView::number"));

    // Nr. Porosie is plain text; no NIPT / Adresa line without an owner.
    assert_eq!(label(&h, "DocumentView::number"), "—");
    assert!(element(&h, "DocumentView::detail").is_none());
    assert_eq!(h.bridge().get_owner_detail(), "");

    // Choosing the owner shows its NIPT and Adresa under the field.
    h.bridge().invoke_owner_focused("".into());
    h.bridge().invoke_owner_chosen(0);
    h.pump();
    assert_eq!(
        label(&h, "DocumentView::detail"),
        "NIPT: M12345678A · Adresa: Rruga e Pishave"
    );
    assert_eq!(x("DocumentView::owner"), x("DocumentView::detail"));
    assert!(y("DocumentView::owner") < y("DocumentView::detail"));
    assert!(y("DocumentView::detail") < y("DocumentView::description"));
    // Editing the name drops the owner and the line.
    h.bridge().invoke_owner_edited("Bar".into());
    h.pump();
    assert!(element(&h, "DocumentView::detail").is_none());
}

#[test]
fn the_footer_has_the_total_left_and_the_commands_right() {
    let h = harness();
    h.new_document(0);
    assert_eq!(label(&h, "DocumentView::total"), "Totali: 0,00 Lekë");
    let x = |id: &str| element(&h, id).unwrap().absolute_position().x;
    let y = |id: &str| element(&h, id).unwrap().absolute_position().y;
    assert_eq!(label(&h, "DocumentView::save"), "Ruaj");
    assert_eq!(label(&h, "DocumentView::print"), "Printo");
    assert_eq!(label(&h, "DocumentView::delete"), "Fshi");
    assert!(x("DocumentView::total") < x("DocumentView::save"));
    assert!(x("DocumentView::save") < x("DocumentView::print"));
    assert!(x("DocumentView::print") < x("DocumentView::delete"));
    // Under the grid, on the total's row.
    assert!(y("DocumentView::grid") < y("DocumentView::save"));
    for id in ["save", "print", "delete"] {
        assert!(
            element(&h, &format!("Toolbar::{id}")).is_none(),
            "Toolbar::{id}"
        );
    }
}

#[test]
fn tab_goes_from_the_owner_to_the_description_the_date_and_the_grid() {
    let h = harness();
    h.new_document(0);
    // A new document focuses the owner field.
    h.type_text("Bar");
    assert_eq!(h.bridge().get_owner_text(), "Bar");
    h.tap(Key::Tab);
    h.type_text("x");
    assert_eq!(h.bridge().get_doc_description(), "x");
    h.tap(Key::Tab);
    assert_eq!(h.focused(), (-1, -1), "the date field, not the grid");
    h.tap(Key::Tab);
    assert_eq!(h.focused(), (0, 0), "the first grid cell");
}

#[test]
fn the_list_takes_about_30_percent_and_never_cuts_its_columns() {
    let h = harness();
    h.new_document(0);
    // Wide: 30 / 70.
    h.app
        .window()
        .set_size(slint::LogicalSize::new(1890.0, 1000.0));
    h.pump();
    let pane = width(&h, "OrdersWorkspace::list-pane");
    assert!(
        (pane / 1890.0 - 0.3).abs() < 0.01,
        "list pane {pane}px of 1890"
    );
    // 1280: the minimum width keeps Nr., Lloji, Klienti, Totali and Data whole.
    h.app
        .window()
        .set_size(slint::LogicalSize::new(1280.0, 800.0));
    h.pump();
    let pane = width(&h, "OrdersWorkspace::list-pane");
    assert!(pane <= 0.4 * 1280.0, "list pane {pane}px of 1280");
    let table = width(&h, "OrderList::table");
    let columns = 60.0 + 56.0 + 100.0 + 128.0 + 92.0;
    assert!(table >= columns + 12.0, "table {table}px");
    // 1024 × 640, the minimum window: still whole.
    h.app
        .window()
        .set_size(slint::LogicalSize::new(1024.0, 640.0));
    h.pump();
    let table = width(&h, "OrderList::table");
    assert!(table >= columns + 12.0, "table {table}px at 1024");
}

#[test]
fn a_very_tall_window_grows_only_the_list_and_the_line_grid() {
    let h = harness();
    h.owner(OwnerKind::Client, "Bar Alfa");
    h.new_document(0);
    h.app
        .window()
        .set_size(slint::LogicalSize::new(1280.0, 2400.0));
    h.pump();
    let height = |id: &str| {
        element(&h, id)
            .unwrap_or_else(|| panic!("{id} not found"))
            .size()
            .height
    };
    // The toolbar, the filters, the header fields and the footer keep their height.
    for kind in ["LineEdit", "Button", "ToolButton"] {
        for e in ElementHandle::find_by_element_type_name(&h.app, kind) {
            let tall = e.size().height;
            assert!(tall <= 40.0, "{kind} {:?} is {tall}px tall", e.id());
        }
    }
    assert!(height("DocumentView::owner") <= 40.0);
    assert!(height("DocumentView::date") <= 40.0);
    // The order list and the line grid take the rest.
    assert!(height("OrderList::table") > 2000.0);
    assert!(height("DocumentView::grid") > 1800.0);
}

#[test]
fn the_owner_and_product_lists_show_a_match_count() {
    use crm_services::domain::OwnerInput;
    use slint::Model;
    let h = harness();
    for (name, address) in [
        ("ABAZ GJOLLARI", "Rr. e Bletëve, TIRANE"),
        ("ABA BAR", ""),
        ("BAR TJETER", "LIVADHET, KAMEZ, TIRANE"),
    ] {
        h.ctx
            .crm
            .create_owner(OwnerInput {
                kind: OwnerKind::Client,
                name: name.into(),
                description: String::new(),
                nipt: " L81302047Q ".into(),
                address: address.into(),
                account_code: String::new(),
            })
            .unwrap();
    }
    h.new_document(0);
    h.bridge().invoke_owner_edited("abaz".into());
    h.bridge().invoke_owner_focused("abaz".into());
    let items = h.bridge().get_owner_items();
    let first = items.row_data(0).unwrap();
    assert_eq!(first.label, "ABAZ GJOLLARI");
    assert_eq!(first.note, "(TIRANE – Rr. e Bletëve)");
    assert_eq!(first.detail, "L81302047Q");
    assert_eq!(
        h.bridge().get_owner_count_text(),
        format!("{}/{} sugjerime", items.row_count(), items.row_count())
    );
    h.bridge().invoke_owner_focused("".into());
    assert_eq!(h.bridge().get_owner_count_text(), "3/3 sugjerime");
    h.bridge().invoke_owner_focused("zzzzzz".into());
    assert_eq!(h.bridge().get_owner_items().row_count(), 0);
    assert_eq!(h.bridge().get_owner_count_text(), "");

    // The products: the same line, the rows unchanged.
    h.product("UJE TEPELENA", 5, 50);
    h.product("UJE GLINA", 7, 50);
    h.ws.borrow_mut().query_products(0, "", false);
    assert_eq!(h.bridge().get_product_count_text(), "2/2 sugjerime");
    let row = h.bridge().get_product_items().row_data(0).unwrap();
    assert_eq!(row.note, "");
    assert!(!row.detail.is_empty());
}

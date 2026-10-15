//! AC 13 / AC 15: the line grid's keyboard table, sent as real key events.

use super::harness;
use crm_services::domain::{Money, format};
use slint::Model;
use slint::platform::Key;

#[test]
fn tab_shift_tab_and_arrows_move_per_the_table_with_wrap_around() {
    let h = harness();
    h.product("BRIZO PJESHKE 24*0.33LIT", 10, 65);
    h.new_document(0);
    // A fresh sale: one empty entry + the placeholder, 3 editable columns.
    assert_eq!(h.rows(), 2);
    assert!(h.row(1).placeholder);

    h.focus(0, 1);
    assert_eq!(h.focused(), (0, 1));
    h.tap(Key::Tab);
    assert_eq!(h.focused(), (0, 2));
    h.tap(Key::Tab);
    assert_eq!(h.focused(), (1, 0), "row-major: next row's Artikulli");
    h.press(Key::Tab, true, false);
    assert_eq!(h.focused(), (0, 2), "Shift+Tab goes back");
    h.tap(Key::RightArrow);
    assert_eq!(h.focused(), (1, 0));
    h.tap(Key::LeftArrow);
    assert_eq!(h.focused(), (0, 2));
    // Wrap at the end and at the start.
    h.focus(1, 2);
    assert_eq!(h.focused(), (1, 2));
    h.tap(Key::Tab);
    assert_eq!(h.focused(), (0, 0), "wraps from the last cell to the first");
    // The Artikulli cell opened its suggestions: Esc closes them, then ↑/↓ move rows.
    assert!(h.bridge().get_dropdown_open());
    h.tap(Key::Escape);
    assert!(!h.bridge().get_dropdown_open());
    h.press(Key::Tab, true, false);
    assert_eq!(h.focused(), (1, 2), "wraps from the first cell to the last");
    h.tap(Key::DownArrow);
    assert_eq!(h.focused(), (0, 2), "↓ wraps to the first row");
    h.tap(Key::UpArrow);
    assert_eq!(h.focused(), (1, 2), "↑ wraps to the last row");
    h.tap(Key::LeftArrow);
    assert_eq!(h.focused(), (1, 1));
    // ↓ in a number cell moves; it never steps the value.
    h.focus(0, 1);
    h.type_text("3");
    h.tap(Key::DownArrow);
    assert_eq!(h.focused(), (1, 1));
    assert_eq!(h.cell_text(0, 1), "3");
    // Enter commits and stays.
    h.focus(0, 2);
    h.tap(Key::Return);
    assert_eq!(h.focused(), (0, 2));
}

#[test]
fn arrows_move_the_highlight_while_the_suggestions_are_open() {
    let h = harness();
    h.product("BRIZO PJESHKE 24*0.33LIT", 10, 65);
    h.product("BRIZO MOLLE 24*0.33LIT", 10, 65);
    h.product("UJE GLINA 0.5L", 10, 30);
    h.new_document(0);
    h.focus(0, 0);
    assert!(h.bridge().get_dropdown_open(), "suggestions show on focus");
    assert_eq!(h.bridge().get_product_items().row_count(), 3);
    assert_eq!(h.bridge().get_product_highlighted(), -1);
    h.tap(Key::DownArrow);
    assert_eq!(
        h.bridge().get_product_highlighted(),
        0,
        "↓ from none → first"
    );
    h.tap(Key::DownArrow);
    h.tap(Key::DownArrow);
    assert_eq!(h.bridge().get_product_highlighted(), 2);
    h.tap(Key::DownArrow);
    assert_eq!(h.bridge().get_product_highlighted(), 0, "wraps");
    h.tap(Key::UpArrow);
    assert_eq!(h.bridge().get_product_highlighted(), 2, "wraps back");
    assert_eq!(h.focused(), (0, 0), "the focus stays in the cell");
    h.bridge().set_product_highlighted(-1);
    h.tap(Key::UpArrow);
    assert_eq!(
        h.bridge().get_product_highlighted(),
        2,
        "↑ from none → last"
    );
}

#[test]
fn typing_and_enter_on_the_placeholder_appends_the_product_and_focuses_sasia() {
    let h = harness();
    h.product("BRIZO PJESHKE 24*0.33LIT", 10, 65);
    h.product("UJE GLINA 0.5L", 10, 30);
    h.new_document(0);
    h.focus(1, 0);
    h.type_text("br pj");
    assert!(h.bridge().get_dropdown_open());
    assert_eq!(
        h.bridge().get_product_highlighted(),
        0,
        "a sale highlights the first hit"
    );
    h.tap(Key::Return);
    assert_eq!(h.rows(), 3, "an entry was appended before the placeholder");
    let row = h.row(1);
    assert!(row.product.starts_with("BRIZO PJESHKE"), "{}", row.product);
    assert_eq!(row.qty, "");
    assert_eq!(row.price, "65,00");
    assert_eq!(row.stock, "/ 10");
    assert!(row.stock_link);
    assert!(h.row(2).placeholder);
    assert_eq!(h.focused(), (1, 1), "focus on that row's Sasia");
    assert!(!h.bridge().get_dropdown_open());
    // Sasia commits on Enter; Vlera and Totali follow.
    h.type_text("2");
    h.tap(Key::Return);
    assert_eq!(h.row(1).qty, "2");
    assert_eq!(h.row(1).value, "130,00");
    assert_eq!(h.bridge().get_doc_total(), "Totali: 130,00 Lekë");
    // The tab shows the unsaved mark.
    let tabs = h.bridge().get_tabs();
    assert!(tabs.row_data(0).unwrap().dirty);
}

#[test]
fn choosing_on_an_existing_row_replaces_it() {
    let h = harness();
    h.product("BRIZO PJESHKE 24*0.33LIT", 10, 65);
    h.product("UJE GLINA 0.5L", 10, 30);
    h.new_document(0);
    h.focus(0, 0);
    h.type_text("uje");
    h.tap(Key::Return);
    assert_eq!(h.rows(), 2, "row 0 was replaced, nothing appended");
    assert_eq!(h.row(0).product, "UJE GLINA 0.5L");
    assert_eq!(h.focused(), (0, 1));
}

#[test]
fn enter_in_the_placeholder_price_adds_a_row_and_focuses_its_artikulli() {
    let h = harness();
    h.new_document(1);
    // Purchase: 4 editable columns; both price cells of the placeholder add.
    assert_eq!(h.rows(), 2);
    h.focus(1, 3);
    h.tap(Key::Return);
    assert_eq!(h.rows(), 3);
    assert_eq!(h.focused(), (1, 0));
    h.focus(2, 2);
    h.tap(Key::Return);
    assert_eq!(h.rows(), 4);
    assert_eq!(h.focused(), (2, 0));
    // Enter in a price cell of another row only commits.
    h.focus(0, 2);
    h.tap(Key::Return);
    assert_eq!(h.rows(), 4);
}

#[test]
fn ctrl_delete_removes_a_row_but_not_the_placeholder() {
    let h = harness();
    h.product("UJE GLINA 0.5L", 10, 30);
    h.new_document(0);
    h.focus(0, 0);
    h.type_text("uje");
    h.tap(Key::Return);
    h.focus(1, 2);
    h.tap(Key::Return);
    assert_eq!(h.rows(), 3);
    h.focus(0, 1);
    h.press(Key::Delete, false, true);
    assert_eq!(h.rows(), 2);
    assert_eq!(h.row(0).product, "", "the empty row moved up");
    assert_eq!(h.focused(), (0, 1), "focus stays at the same place");
    // The placeholder cannot be removed; neither can the last entry go away.
    h.focus(1, 1);
    h.press(Key::Delete, false, true);
    assert_eq!(h.rows(), 2);
    h.focus(0, 1);
    h.press(Key::Delete, false, true);
    assert_eq!(h.rows(), 2, "one empty entry stays");
}

#[test]
fn sale_quantity_above_the_stock_flashes_and_resets_to_zero() {
    let h = harness();
    h.product("UJE GLINA 0.5L", 4, 30);
    h.new_document(0);
    h.focus(0, 0);
    h.type_text("uje");
    h.tap(Key::Return);
    assert_eq!(h.focused(), (0, 1));
    h.type_text("5");
    h.tap(Key::Tab);
    assert_eq!(h.flash(), "Maksimumi është 4");
    assert_eq!(h.row(0).qty, "");
    assert_eq!(h.cell_text(0, 1), "", "the field shows the reset value");
    // Non-numeric input.
    h.focus(0, 1);
    h.type_text("x");
    h.tap(Key::Return);
    assert_eq!(h.flash(), "Sasi e pavlefshme");
    assert_eq!(h.cell_text(0, 1), "");
    // Focus-out commits too.
    h.focus(0, 1);
    h.type_text("4");
    h.focus(0, 2);
    assert_eq!(h.row(0).qty, "4");
    // A typed 0 stays 0 (free goods); a blank price takes the SKU price back (F-012).
    h.focus(0, 2);
    h.type_text("0");
    h.tap(Key::Return);
    assert_eq!(h.row(0).price, "0,00");
    h.focus(0, 2);
    h.press(Key::Backspace, false, false);
    h.pump();
    h.tap(Key::Return);
    assert_eq!(h.row(0).price, format::amount(Money(3_000)));
}

#[test]
fn purchase_free_text_becomes_a_new_name_line_and_a_sale_reverts() {
    let h = harness();
    h.product("UJE GLINA 0.5L", 4, 30);
    h.new_document(1);
    h.focus(0, 0);
    h.type_text("LENG I RI");
    assert_eq!(
        h.bridge().get_product_highlighted(),
        -1,
        "a purchase does not pre-select"
    );
    h.tap(Key::Tab);
    assert_eq!(h.row(0).product, "LENG I RI");
    assert_eq!(h.row(0).stock, "/ 0");
    assert!(h.row(0).stock_warn);
    assert!(!h.row(0).stock_link);

    h.new_document(0);
    h.focus(0, 0);
    h.type_text("uje");
    h.tap(Key::Return);
    h.focus(0, 0);
    h.type_text("zzz");
    h.tap(Key::Tab);
    assert_eq!(h.row(0).product, "UJE GLINA 0.5L");
    assert_eq!(
        h.cell_text(0, 0),
        "UJE GLINA 0.5L",
        "the typed text reverted"
    );
}

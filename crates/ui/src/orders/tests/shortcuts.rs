//! The Bilanc shortcuts (task 018): Ctrl+R / Ctrl+K / Ctrl+F (new product,
//! client, supplier), Ctrl+O (Kërko porosi), Ctrl+F4 / Ctrl+Shift+F4 (close the
//! tab / every tab) and Alt+← / Alt+→ (previous / next tab), from the fields
//! that usually hold the focus.

use super::{Harness, harness, take_confirm};
use crate::catalogue::{self, dialogs, texts};
use crate::shell::calendar::testing as cal;
use slint::ComponentHandle;
use slint::platform::Key;

impl Harness {
    /// Presses and releases `key` with Alt held.
    fn alt(&self, key: Key) {
        self.send(Key::Alt, true);
        self.send(key, true);
        self.send(key, false);
        self.send(Key::Alt, false);
        self.pump();
    }

    fn active(&self) -> i32 {
        self.bridge().get_active_tab()
    }

    fn tab_count(&self) -> usize {
        self.ws.borrow().tabs.len()
    }
}

#[test]
fn ctrl_r_k_f_open_the_list_with_its_new_item_dialog() {
    let h = harness();
    h.press("r", false, true);
    let products = catalogue::products_list(&h.ctx).expect("Artikujt opened");
    assert!(products.window().window().is_visible());
    let dialog = dialogs::open_product_dialog().expect("the new product dialog");
    assert!(dialog.window().is_visible());
    assert_eq!(dialog.get_caption(), texts::NEW_PRODUCT);
    dialog.hide().unwrap();

    h.press("k", false, true);
    let clients = catalogue::clients_list(&h.ctx).expect("Klientët opened");
    assert!(clients.window().window().is_visible());
    let dialog = dialogs::open_owner_dialog().expect("the new client dialog");
    assert_eq!(dialog.get_caption(), "Klient i ri");
    assert_eq!(dialog.get_name(), "");
    dialog.hide().unwrap();

    h.press("f", false, true);
    let suppliers = catalogue::suppliers_list(&h.ctx).expect("Furnitorët opened");
    assert!(suppliers.window().window().is_visible());
    let dialog = dialogs::open_owner_dialog().expect("the new supplier dialog");
    assert_eq!(dialog.get_caption(), texts::NEW_SUPPLIER);
    dialog.hide().unwrap();
}

#[test]
fn ctrl_o_focuses_the_order_search_with_its_text_selected() {
    let h = harness();
    h.new_document(0);
    h.bridge().set_filter_text("abc".into());
    h.press("o", false, true);
    // The focus left the owner field: typing replaces the selected search text.
    h.type_text("x");
    assert_eq!(h.bridge().get_filter_text(), "x");
    assert_eq!(h.bridge().get_owner_text(), "");
}

#[test]
fn alt_arrows_switch_tabs_with_wrap_around() {
    let h = harness();
    // Fewer than two tabs: nothing happens.
    h.alt(Key::RightArrow);
    h.new_document(0);
    h.alt(Key::RightArrow);
    assert_eq!(h.active(), 0);

    h.new_document(1);
    h.new_document(0);
    assert_eq!(h.active(), 2);
    h.alt(Key::RightArrow);
    assert_eq!(h.active(), 0, "wraps to the first");
    h.alt(Key::LeftArrow);
    assert_eq!(h.active(), 2, "wraps to the last");
    h.alt(Key::LeftArrow);
    assert_eq!(h.active(), 1);

    // From a grid cell: the grid does not take Alt+→ as a cell move.
    h.focus(0, 0);
    let before = h.focused();
    h.alt(Key::RightArrow);
    assert_eq!(h.active(), 2);
    assert_ne!(h.focused(), (before.0, before.1 + 1));

    // From the document's date field.
    h.tap(Key::Tab);
    h.tap(Key::Tab);
    h.alt(Key::RightArrow);
    assert_eq!(h.active(), 0);
}

#[test]
fn ctrl_f4_closes_the_tab_from_a_grid_cell_and_a_date_field() {
    let h = harness();
    h.new_document(0);
    h.new_document(1);
    h.new_document(0);
    assert_eq!(h.tab_count(), 3);

    h.focus(0, 0);
    h.press(Key::F4, false, true);
    assert_eq!(h.tab_count(), 2, "Ctrl+F4 from a grid cell");

    // Owner → description → date: Ctrl+F4 closes, it does not open the calendar.
    h.tap(Key::Tab);
    h.tap(Key::Tab);
    h.press(Key::F4, false, true);
    assert!(!cal::is_open(&h.app));
    assert_eq!(h.tab_count(), 1, "Ctrl+F4 from a date field");

    // A plain F4 in the date field still opens the calendar (a new document
    // puts the focus in its owner field: owner → description → date).
    h.new_document(1);
    assert_eq!(h.tab_count(), 2);
    h.tap(Key::Tab);
    h.tap(Key::Tab);
    h.tap(Key::F4);
    assert!(cal::is_open(&h.app), "F4 opens the calendar");
    cal::key(h.app.window(), Key::Escape, false);
    h.pump();

    // Ctrl+W still closes.
    h.press("w", false, true);
    assert_eq!(h.tab_count(), 1);
}

#[test]
fn ctrl_shift_f4_closes_every_tab_and_asks_about_unsaved_ones() {
    let h = harness();
    h.new_document(0);
    h.new_document(1);
    assert_eq!(h.tab_count(), 2);
    // Make the second one unsaved.
    h.bridge().set_doc_description("x".into());
    h.bridge().invoke_description_edited("x".into());
    h.focus(0, 0);
    h.press(Key::F4, true, true);
    let confirm = take_confirm().expect("the unsaved-changes prompt");
    assert!(confirm.window().is_visible());
}

#[test]
fn the_shortcuts_do_nothing_while_locked() {
    let h = harness();
    h.new_document(0);
    h.new_document(1);
    h.ctx.lock();
    h.pump();
    h.alt(Key::LeftArrow);
    h.press(Key::F4, false, true);
    h.press(Key::F4, true, true);
    h.press("k", false, true);
    h.ctx.unlock();
    h.pump();
    assert_eq!(h.tab_count(), 2);
    assert_eq!(h.active(), 1);
    assert!(catalogue::clients_list(&h.ctx).is_none());
}

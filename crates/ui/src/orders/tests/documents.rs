//! AC 3–9 / AC 15: documents and tabs — save, edit, delete, close, restore.

use super::{Harness, harness, harness_in, take_confirm, take_message};
use crate::shell::format;
use crm_services::domain::{OrderId, OrderKind, OwnerKind};
use slint::platform::Key;
use slint::{ComponentHandle, Model};

/// A sale of `qty` × the product found by `query`, for owner `owner`.
fn fill_sale(h: &Harness, owner: &str, query: &str, qty: &str) {
    h.bridge().set_owner_text(owner.into());
    h.bridge().invoke_owner_edited(owner.into());
    h.focus(0, 0);
    h.type_text(query);
    h.tap(Key::Return);
    h.type_text(qty);
    h.tap(Key::Return);
}

fn tab_titles(h: &Harness) -> Vec<(String, bool)> {
    let tabs = h.bridge().get_tabs();
    (0..tabs.row_count())
        .map(|i| {
            let t = tabs.row_data(i).unwrap();
            (t.title.to_string(), t.dirty)
        })
        .collect()
}

#[test]
fn saving_a_new_sale_lists_it_with_todays_date_and_retitles_the_tab() {
    let h = harness();
    h.product("UJE GLINA 0.5L", 10, 30);
    let client = h.owner(OwnerKind::Client, "Bar Alfa");
    h.new_document(0);
    assert_eq!(tab_titles(&h), vec![("Porosi Shitje (e re)".into(), false)]);
    assert_eq!(h.bridge().get_doc_number(), "—");
    assert!(h.bridge().get_has_document());
    assert!(!h.bridge().get_current_is_saved());
    assert_eq!(h.bridge().get_open_tab_count(), 1);
    // The owner suggestions show the name, "(place) · description" dimmed and the
    // NIPT on the right; choosing fills NIPT/Adresa.
    h.bridge().invoke_owner_focused("".into());
    assert!(h.bridge().get_owner_open());
    let items = h.bridge().get_owner_items();
    assert_eq!(items.row_data(0).unwrap().label, "Bar Alfa");
    assert_eq!(
        items.row_data(0).unwrap().note,
        "(Rruga e Pishave) · Tiranë"
    );
    assert_eq!(items.row_data(0).unwrap().detail, "M12345678A");
    h.bridge().invoke_owner_chosen(0);
    assert_eq!(h.bridge().get_owner_text(), "Bar Alfa");
    assert_eq!(h.bridge().get_owner_nipt(), "M12345678A");
    assert_eq!(h.bridge().get_owner_address(), "Rruga e Pishave");
    assert_eq!(
        h.ws.borrow().current().unwrap().header.owner_id,
        Some(client.id)
    );
    // Editing the text after a selection clears the selected owner.
    h.bridge().invoke_owner_edited("Bar Alfa 2".into());
    assert_eq!(h.ws.borrow().current().unwrap().header.owner_id, None);
    assert_eq!(h.bridge().get_owner_nipt(), "");
    h.bridge().invoke_owner_chosen(0);
    h.focus(0, 0);
    h.type_text("uje");
    h.tap(Key::Return);
    h.type_text("3");
    // Ctrl+S goes to the menu (the shell); the bridge callback is what it calls.
    h.bridge().invoke_save_current();
    h.pump();
    assert!(
        take_confirm().is_none(),
        "a new invoice saves without asking"
    );
    assert!(take_message().is_none(), "no error");
    assert_eq!(h.flash(), "Porosia u krijua.");
    let ws = h.ws.borrow();
    let tab = ws.current().unwrap();
    let id = tab.order_id.expect("saved");
    assert_eq!(tab.title, format!("Shitje #{} · Bar Alfa", id.0));
    assert!(!tab.dirty);
    drop(ws);
    assert!(h.bridge().get_current_is_saved());
    assert_eq!(h.bridge().get_doc_number(), id.0.to_string());
    // The uncommitted quantity was committed before the save.
    let order = h.ctx.crm.get_order(id).unwrap();
    assert_eq!(order.lines.len(), 1);
    assert_eq!(order.lines[0].quantity.0, 3);
    // The list (Shitje, today) shows it.
    let rows = h.bridge().get_list_rows();
    assert_eq!(rows.row_count(), 1);
    let row = rows.row_data(0).unwrap();
    let cell = |i: usize| row.row_data(i).unwrap().text.to_string();
    assert_eq!(cell(0), id.0.to_string());
    assert_eq!(cell(1), "Shitje");
    assert_eq!(cell(2), "Bar Alfa");
    assert_eq!(cell(3), "90,00 LEK");
    assert_eq!(cell(4), format::date(format::today()));
    // Opening it from the list activates the existing tab.
    h.bridge().invoke_new_document(1);
    h.bridge().invoke_open_order(0);
    h.pump();
    assert_eq!(h.bridge().get_active_tab(), 0);
    assert_eq!(h.bridge().get_open_tab_count(), 2);
}

#[test]
fn editing_a_saved_invoice_asks_before_saving_and_errors_keep_the_draft() {
    let h = harness();
    h.product("UJE GLINA 0.5L", 10, 30);
    h.new_document(0);
    fill_sale(&h, "Klient i ri", "uje", "2");
    h.bridge().invoke_save_current();
    h.pump();
    let id = h.ws.borrow().current().unwrap().order_id.unwrap();
    // Edit: 2 → 4, then save → the confirm question.
    h.focus(0, 1);
    h.type_text("4");
    h.tap(Key::Return);
    assert!(tab_titles(&h)[0].1, "dirty");
    h.bridge().invoke_save_current();
    h.pump();
    let dialog = take_confirm().expect("Konfirmo përditësimin");
    assert_eq!(dialog.get_caption(), "Konfirmo përditësimin");
    assert!(
        dialog
            .get_text()
            .starts_with("Kjo porosi është tashmë e ruajtur.")
    );
    assert_eq!(dialog.get_ok_label(), "Vazhdo");
    dialog.invoke_answered(true);
    h.pump();
    assert_eq!(h.flash(), "Porosia u përditësua.");
    assert_eq!(h.ctx.crm.get_order(id).unwrap().lines[0].quantity.0, 4);
    // Anulo leaves the draft untouched.
    h.focus(0, 1);
    h.type_text("5");
    h.tap(Key::Return);
    h.bridge().invoke_save_current();
    h.pump();
    take_confirm().unwrap().invoke_answered(false);
    h.pump();
    assert_eq!(h.ctx.crm.get_order(id).unwrap().lines[0].quantity.0, 4);
    assert!(tab_titles(&h)[0].1, "still dirty");
    // A stock error from the service shows in a message; the draft stays.
    h.product("COCA COLA 1.5L", 1, 100);
    h.new_document(0);
    fill_sale(&h, "Klient", "coca", "1");
    // Someone else sells the last bottle meanwhile.
    let mut sale = h.ws.borrow().current().unwrap().draft();
    sale.owner_name = "Tjetër".into();
    h.ctx.crm.save_order(sale, None, None).unwrap();
    h.bridge().invoke_save_current();
    h.pump();
    let message = take_message().expect("stock error");
    assert!(
        message.get_text().starts_with("Gjendje e pamjaftueshme"),
        "{}",
        message.get_text()
    );
    assert!(h.ws.borrow().current().unwrap().order_id.is_none());
    assert!(tab_titles(&h)[1].1, "the draft is kept");
}

#[test]
fn deleting_a_purchase_whose_goods_were_sold_shows_the_q6_message() {
    let h = harness();
    let supplier = h.owner(OwnerKind::Supplier, "Furnitori Alfa");
    h.product("UJE GLINA 0.5L", 0, 30);
    // A purchase of 5, then a sale of 3 of them.
    h.new_document(1);
    h.bridge().invoke_owner_focused("".into());
    h.bridge().invoke_owner_chosen(0);
    assert_eq!(h.bridge().get_owner_text(), supplier.name.as_str());
    h.focus(0, 0);
    h.type_text("uje");
    h.tap(Key::DownArrow);
    h.tap(Key::Return);
    assert_eq!(h.focused(), (0, 1));
    h.type_text("5");
    h.tap(Key::Return);
    h.bridge().invoke_save_current();
    h.pump();
    assert_eq!(h.flash(), "Porosia u krijua.");
    let purchase = h.ws.borrow().current().unwrap().order_id.unwrap();
    h.new_document(0);
    fill_sale(&h, "Klient", "uje", "3");
    h.bridge().invoke_save_current();
    h.pump();
    assert_eq!(h.flash(), "Porosia u krijua.");
    // Back to the purchase: Fshi → confirm → Q6.
    h.bridge().invoke_select_tab(0);
    h.pump();
    assert!(h.bridge().get_current_is_saved());
    h.bridge().invoke_delete_current();
    h.pump();
    let dialog = take_confirm().expect("Konfirmo fshirjen");
    assert_eq!(dialog.get_caption(), "Konfirmo fshirjen");
    assert_eq!(
        dialog.get_text(),
        "Ta fshij këtë porosi? Ky veprim nuk mund të zhbëhet."
    );
    dialog.invoke_answered(true);
    h.pump();
    let message = take_message().expect("Q6");
    assert!(
        message
            .get_text()
            .starts_with("Kjo porosi blerjeje nuk mund të fshihet"),
        "{}",
        message.get_text()
    );
    assert!(h.ctx.crm.get_order(purchase).is_ok());
    assert_eq!(h.bridge().get_open_tab_count(), 2);
    // The sale can be deleted: its tab closes and the list refreshes.
    h.bridge().invoke_select_tab(1);
    h.bridge().invoke_delete_current();
    take_confirm().unwrap().invoke_answered(true);
    h.pump();
    assert_eq!(h.flash(), "Porosia u fshi.");
    assert_eq!(h.bridge().get_open_tab_count(), 1);
    assert_eq!(h.bridge().get_active_tab(), 0);
}

#[test]
fn closing_a_dirty_tab_asks_and_closing_activates_the_same_index() {
    let h = harness();
    h.new_document(0);
    h.new_document(1);
    h.new_document(0);
    assert_eq!(h.bridge().get_active_tab(), 2);
    // A clean tab closes at once; the active one becomes the one at the same index (clamped).
    h.bridge().invoke_select_tab(1);
    h.bridge().invoke_close_current();
    h.pump();
    assert!(take_confirm().is_none());
    assert_eq!(h.bridge().get_open_tab_count(), 2);
    assert_eq!(h.bridge().get_active_tab(), 1);
    // A dirty one asks "Mbyll / Anulo".
    h.bridge().set_doc_description("x".into());
    h.bridge().invoke_description_edited("x".into());
    h.bridge().invoke_close_tab(1);
    let dialog = take_confirm().expect("unsaved question");
    assert_eq!(
        dialog.get_text(),
        "Ndryshimet e paruajtura do të humbasin. Ta mbyll skedën?"
    );
    assert_eq!(dialog.get_ok_label(), "Mbyll");
    dialog.invoke_answered(false);
    assert_eq!(h.bridge().get_open_tab_count(), 2);
    h.bridge().invoke_close_tab(1);
    take_confirm().unwrap().invoke_answered(true);
    h.pump();
    assert_eq!(h.bridge().get_open_tab_count(), 1);
    assert_eq!(h.bridge().get_active_tab(), 0);
    assert_eq!(h.ctx.crm.list_tabs().unwrap().len(), 1);
    h.bridge().invoke_close_current();
    assert_eq!(h.bridge().get_open_tab_count(), 0);
    assert!(!h.bridge().get_has_document());
    assert_eq!(h.bridge().get_active_tab(), -1);
}

#[test]
fn close_others_and_close_all_clear_only_the_drafts_they_close() {
    let h = harness();
    h.new_document(0);
    h.bridge().invoke_description_edited("a".into());
    h.new_document(1);
    h.bridge().invoke_description_edited("b".into());
    h.new_document(0);
    h.bridge().invoke_flush_drafts();
    assert_eq!(h.ctx.crm.list_tabs().unwrap().len(), 3);
    h.bridge().invoke_close_other_tabs(1);
    take_confirm()
        .expect("dirty tabs ask")
        .invoke_answered(true);
    h.pump();
    let stored = h.ctx.crm.list_tabs().unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].kind, OrderKind::Purchase);
    assert_eq!(
        stored[0].draft.as_ref().unwrap().owner_description,
        "b",
        "the kept tab keeps its draft"
    );
    assert_eq!(h.bridge().get_active_tab(), 0);
    h.bridge().invoke_close_all_tabs();
    take_confirm().unwrap().invoke_answered(true);
    assert!(h.ctx.crm.list_tabs().unwrap().is_empty());
    assert_eq!(h.bridge().get_open_tab_count(), 0);
}

#[test]
fn drafts_and_tabs_survive_closing_and_reopening_the_app() {
    let h = harness();
    h.product("UJE GLINA 0.5L", 10, 30);
    // A saved sale with an unsaved edit, a new purchase draft, and a clean new sale.
    h.new_document(0);
    fill_sale(&h, "Bar Alfa", "uje", "2");
    h.bridge().invoke_save_current();
    h.pump();
    let id = h.ws.borrow().current().unwrap().order_id.unwrap();
    h.focus(0, 1);
    h.type_text("3");
    h.tap(Key::Return);
    h.new_document(1);
    h.focus(0, 0);
    h.type_text("Leng i ri");
    h.tap(Key::Tab);
    h.bridge().set_doc_description("porosi".into());
    h.bridge().invoke_description_edited("porosi".into());
    // The draft is written 500 ms after the last edit.
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(600));
    let stored = h.ctx.crm.list_tabs().unwrap();
    assert_eq!(stored.len(), 2);
    assert_eq!(
        stored[1].draft.as_ref().unwrap().owner_description,
        "porosi"
    );
    h.new_document(0);
    h.bridge().invoke_select_tab(1);
    h.pump();
    // Exit (flush-drafts, close) and start again on the same folder.
    let dir = h.close();
    let h = harness_in(dir);
    assert_eq!(
        tab_titles(&h),
        vec![
            (format!("Shitje #{} · Bar Alfa", id.0), true),
            ("Porosi Blerje (e re)".into(), true),
            ("Porosi Shitje (e re)".into(), false),
        ]
    );
    assert_eq!(h.bridge().get_active_tab(), 1, "the active tab is restored");
    assert_eq!(h.bridge().get_doc_description(), "porosi");
    assert_eq!(h.row(0).product, "Leng i ri");
    h.bridge().invoke_select_tab(0);
    h.pump();
    assert_eq!(h.row(0).qty, "3", "the unsaved edit of the saved sale");
    assert_eq!(h.bridge().get_doc_number(), id.0.to_string());
    // A tab whose invoice was deleted disappears.
    h.ctx.crm.delete_order(id).unwrap();
    let dir = h.close();
    let h = harness_in(dir);
    assert_eq!(h.bridge().get_open_tab_count(), 2);
}

#[test]
fn the_stock_link_opens_the_product_editor_and_refreshes_the_stock() {
    let h = harness();
    let p = h.product("UJE GLINA 0.5L", 4, 30);
    h.new_document(0);
    h.focus(0, 0);
    h.type_text("uje");
    h.tap(Key::Return);
    assert_eq!(h.row(0).stock, "/ 4");
    // A stand-in editor that changes the stock and reports the save.
    let opened = std::rc::Rc::new(std::cell::Cell::new(None));
    let seen = opened.clone();
    h.ctx.set_product_editor(Box::new(move |ctx, id, on_saved| {
        seen.set(Some(id));
        ctx.crm
            .update_product(
                id,
                crm_services::ProductPatch {
                    quantity: Some(crm_services::domain::Qty(9)),
                    ..Default::default()
                },
            )
            .unwrap();
        on_saved();
    }));
    h.bridge().invoke_stock_clicked(0);
    assert_eq!(opened.get(), Some(p.product.id));
    assert_eq!(h.row(0).stock, "/ 9");
    // The placeholder and empty rows have no link.
    assert_eq!(h.row(1).stock, "");
}

#[test]
fn fshi_in_the_product_editor_of_the_stock_link_deletes_the_product() {
    let h = harness();
    let p = h.product("UJE GLINA 0.5L", 4, 30);
    h.new_document(0);
    h.focus(0, 0);
    h.type_text("uje");
    h.tap(Key::Return);
    assert_eq!(h.row(0).stock, "/ 4");
    // The real editor (the catalogue's product dialog) over the main window.
    h.bridge().invoke_stock_clicked(0);
    let dialog = crate::catalogue::dialogs::open_product_dialog().expect("the editor opened");
    assert!(dialog.get_editing(), "Fshi is offered for a saved product");
    dialog.invoke_delete();
    let confirm = crate::catalogue::dialogs::open_confirm().expect("the delete confirmation");
    assert_eq!(confirm.get_caption(), "Konfirmo fshirjen");
    confirm.invoke_answered(true);
    h.pump();
    assert!(
        crate::catalogue::dialogs::open_product_dialog().is_none(),
        "Fshi closes"
    );
    assert!(!h.app.get_input_blocked());
    assert!(
        h.ctx.crm.get_product(p.product.id).is_err(),
        "the product is gone"
    );
    // The line stays, as after a delete in Artikujt.
    assert_eq!(h.row(0).product, "UJE GLINA 0.5L");
    // The link again: the product no longer exists (F-006).
    h.bridge().invoke_stock_clicked(0);
    assert!(crate::catalogue::dialogs::open_product_dialog().is_none());
    assert_eq!(h.flash(), "Ky artikull nuk ekziston më.");
}

#[test]
fn printo_opens_the_receipt_preview_of_the_saved_invoice() {
    let h = harness();
    h.product("UJE GLINA 0.5L", 10, 30);
    h.owner(OwnerKind::Client, "Bar Alfa");
    h.new_document(0);
    // A document that is not saved yet has nothing to print.
    h.bridge().invoke_print_current();
    assert!(crate::print::preview::last_preview().is_none());
    fill_sale(&h, "Bar Alfa", "uje", "3");
    h.bridge().invoke_save_current();
    assert!(h.bridge().get_current_is_saved());
    h.bridge().invoke_print_current();
    let preview = crate::print::preview::last_preview().expect("the preview opened");
    assert!(preview.is_open());
    assert!(preview.window().get_loading());
    assert_eq!(h.flash(), "Porosia u krijua.");
    preview.close();
    assert!(!preview.is_open());
}

/// The receipt quantities of preview `p`, rendered from the database as the
/// worker job would (the tests run no event loop).
fn preview_quantities(h: &Harness, p: &crate::print::Preview, id: OrderId) -> Vec<String> {
    p.apply(crate::print::preview::prepare(&h.ctx.crm, id, 96.0));
    let (data, _) = p.receipt().expect("the receipt rendered");
    data.lines.into_iter().map(|l| l.qty).collect()
}

#[test]
fn printo_with_unsaved_edits_saves_first_and_prints_what_is_on_screen() {
    let h = harness();
    h.product("UJE GLINA 0.5L", 10, 30);
    h.owner(OwnerKind::Client, "Bar Alfa");
    h.new_document(0);
    fill_sale(&h, "Bar Alfa", "uje", "3");
    h.bridge().invoke_save_current();
    h.pump();
    let id = h.ws.borrow().current().unwrap().order_id.unwrap();

    // A clean saved invoice: the preview opens at once, no question.
    h.bridge().invoke_print_current();
    assert!(take_confirm().is_none());
    let preview = crate::print::preview::last_preview().expect("the preview opened");
    assert_eq!(preview_quantities(&h, &preview, id), ["3"]);
    preview.close();
    drop(preview);
    assert!(crate::print::preview::last_preview().is_none());

    // Edited, not saved (3 → 4): Printo asks first; Anulo opens nothing.
    h.focus(0, 1);
    h.type_text("4");
    h.tap(Key::Return);
    assert!(tab_titles(&h)[0].1, "dirty");
    h.bridge().invoke_print_current();
    h.pump();
    let dialog = take_confirm().expect("Ruaj para printimit?");
    assert_eq!(dialog.get_caption(), "Ndryshime të paruajtura");
    assert!(
        dialog
            .get_text()
            .starts_with("Porosia ka ndryshime të paruajtura. Ruaj para printimit?")
    );
    assert_eq!(dialog.get_ok_label(), "Ruaj");
    dialog.invoke_answered(false);
    h.pump();
    assert!(crate::print::preview::last_preview().is_none());
    assert!(tab_titles(&h)[0].1, "still dirty");
    assert_eq!(h.ctx.crm.get_order(id).unwrap().lines[0].quantity.0, 3);

    // Ruaj: the invoice is saved (no second "Konfirmo përditësimin"), then the
    // preview shows the edited line.
    h.bridge().invoke_print_current();
    h.pump();
    take_confirm().expect("asked again").invoke_answered(true);
    h.pump();
    assert!(take_confirm().is_none(), "one question only");
    assert_eq!(h.ctx.crm.get_order(id).unwrap().lines[0].quantity.0, 4);
    assert!(!tab_titles(&h)[0].1, "saved");
    assert_eq!(h.flash(), "Porosia u përditësua.");
    let preview = crate::print::preview::last_preview().expect("the preview opened");
    assert!(preview.is_open());
    assert_eq!(preview_quantities(&h, &preview, id), ["4"]);
    preview.close();
}

#[test]
fn printo_opens_no_preview_when_the_save_before_it_fails() {
    let h = harness();
    h.product("UJE GLINA 0.5L", 10, 30);
    h.new_document(0);
    fill_sale(&h, "Bar Alfa", "uje", "3");
    h.bridge().invoke_save_current();
    h.pump();
    let id = h.ws.borrow().current().unwrap().order_id.unwrap();
    // More than the stock (10 bought, 3 sold): the save is refused.
    h.focus(0, 1);
    h.type_text("50");
    h.tap(Key::Return);
    let _ = take_message();
    assert!(tab_titles(&h)[0].1, "dirty");
    h.bridge().invoke_print_current();
    h.pump();
    take_confirm()
        .expect("Ruaj para printimit?")
        .invoke_answered(true);
    h.pump();
    assert!(take_message().is_some(), "the save error is shown");
    assert!(crate::print::preview::last_preview().is_none());
    assert!(tab_titles(&h)[0].1, "the draft stays");
    assert_eq!(h.ctx.crm.get_order(id).unwrap().lines[0].quantity.0, 3);

    // An invalid date: the date message, no save, no preview.
    h.focus(0, 1);
    h.type_text("2");
    h.tap(Key::Return);
    h.bridge().set_doc_date("32.13.2026".into());
    h.bridge().invoke_print_current();
    h.pump();
    take_confirm()
        .expect("Ruaj para printimit?")
        .invoke_answered(true);
    h.pump();
    let message = take_message().expect("the date message");
    assert_eq!(message.get_text(), "Data e porosisë nuk është e vlefshme.");
    assert!(crate::print::preview::last_preview().is_none());
    assert_eq!(h.ctx.crm.get_order(id).unwrap().lines[0].quantity.0, 3);

    // Committed but not reloadable (F-013): the error shows, no preview.
    h.bridge()
        .set_doc_date(crate::orders::document::today_text().into());
    crate::orders::document::FAIL_LOAD_SAVED.with(|f| f.set(true));
    h.bridge().invoke_print_current();
    h.pump();
    take_confirm()
        .expect("Ruaj para printimit?")
        .invoke_answered(true);
    h.pump();
    assert!(take_message().is_some(), "the reload error is shown");
    assert!(crate::print::preview::last_preview().is_none());
    assert_eq!(h.ctx.crm.get_order(id).unwrap().lines[0].quantity.0, 2);
}

#[test]
fn a_restore_replaces_the_open_tabs_with_the_restored_ones() {
    let h = harness();
    h.product("UJE GLINA 0.5L", 10, 30);
    // Invoice #1, open in a tab, is in the backup.
    h.new_document(0);
    fill_sale(&h, "Bar A", "uje", "1");
    h.bridge().invoke_save_current();
    h.pump();
    let first = h.ws.borrow().current().unwrap().order_id.unwrap();
    h.bridge().invoke_flush_drafts();
    let backup = h.ctx.crm.backup_now().unwrap();
    // Invoice #2 is saved after the backup; its tab stays open with a draft.
    h.new_document(0);
    fill_sale(&h, "Bar B", "uje", "2");
    h.bridge().invoke_save_current();
    h.pump();
    let second = h.ws.borrow().current().unwrap().order_id.unwrap();
    h.focus(0, 1);
    h.type_text("3");
    h.tap(Key::Return);
    assert!(tab_titles(&h)[1].1, "a stale draft");

    // What shell::backup::restore does once the copy is back.
    h.ctx.crm.restore(&backup).unwrap();
    h.ctx.notify_restored();
    h.ctx.notify_data_changed();
    h.pump();

    let ws = h.ws.borrow();
    let orders: Vec<_> = ws.tabs.iter().map(|t| t.order_id).collect();
    assert_eq!(orders, [Some(first)], "only the restored tab");
    drop(ws);
    // The restored database hands out #2 again; the stale tab cannot overwrite it.
    let mut sale = h.ws.borrow().current().unwrap().draft();
    sale.owner_name = "Bar C".into();
    assert_eq!(h.ctx.crm.save_order(sale, None, None).unwrap(), second);
    h.bridge().invoke_flush_drafts();
    slint::platform::update_timers_and_animations();
    let tabs = h.ctx.crm.list_tabs().unwrap();
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0].order_id, Some(first));
    assert!(tabs[0].draft.is_none());
    assert_eq!(h.ctx.crm.get_order(second).unwrap().owner.name, "Bar C");
}

#[test]
fn the_focus_returns_to_the_grid_after_a_save_and_after_overlays() {
    let h = harness();
    h.product("UJE GLINA 0.5L", 10, 30);
    h.new_document(0);
    fill_sale(&h, "Bar A", "uje", "2");
    // A new invoice: no dialog; the cell that had the focus keeps it.
    h.focus(0, 1);
    h.bridge().invoke_save_current();
    h.pump();
    assert_eq!(h.flash(), "Porosia u krijua.");
    assert_eq!(h.focused(), (0, 1));
    // A saved one: "Konfirmo përditësimin" takes the focus, Vazhdo gives it back.
    h.type_text("4");
    h.bridge().invoke_save_current();
    h.pump();
    assert_eq!(h.focused(), (-1, -1), "the dialog's blocker has the focus");
    take_confirm().unwrap().invoke_answered(true);
    h.pump();
    assert_eq!(h.flash(), "Porosia u përditësua.");
    assert_eq!(h.focused(), (0, 1));
    // Typing goes on in that cell.
    h.type_text("3");
    h.tap(Key::Return);
    assert_eq!(h.row(0).qty, "3");
    // The busy overlay and the lock screen hand the focus back too.
    h.focus(0, 2);
    let shell = h.app.global::<crate::ShellBridge>();
    shell.set_busy(true);
    h.pump();
    assert_eq!(h.focused(), (-1, -1));
    shell.set_busy(false);
    h.pump();
    assert_eq!(h.focused(), (0, 2));
    h.ctx.lock();
    h.pump();
    h.ctx.unlock();
    h.pump();
    assert_eq!(h.focused(), (0, 2));
    // After the user moved to a header field nothing is taken back.
    h.bridge().invoke_owner_focused("".into());
    shell.set_busy(true);
    h.pump();
    shell.set_busy(false);
    h.pump();
    assert_eq!(h.focused(), (-1, -1));
}

#[test]
fn saving_and_deleting_an_invoice_notify_every_view() {
    let h = harness();
    h.product("UJE GLINA 0.5L", 10, 30);
    let calls = std::rc::Rc::new(std::cell::Cell::new(0));
    let c = calls.clone();
    h.ctx.on_data_changed(move || c.set(c.get() + 1));
    h.new_document(0);
    fill_sale(&h, "Bar A", "uje", "2");
    h.bridge().invoke_save_current();
    h.pump();
    assert_eq!(calls.get(), 1, "after the save");
    assert_eq!(
        h.bridge().get_list_rows().row_count(),
        1,
        "the list refreshed"
    );
    h.bridge().invoke_delete_current();
    h.pump();
    take_confirm().unwrap().invoke_answered(true);
    h.pump();
    assert_eq!(h.flash(), "Porosia u fshi.");
    assert_eq!(calls.get(), 2, "after the delete");
    assert_eq!(h.bridge().get_list_rows().row_count(), 0);
}

#[test]
fn a_committed_save_that_cannot_be_reloaded_still_records_the_invoice() {
    let h = harness();
    h.product("UJE GLINA 0.5L", 10, 30);
    h.new_document(0);
    fill_sale(&h, "Bar A", "uje", "2");
    super::super::document::FAIL_LOAD_SAVED.with(|f| f.set(true));
    h.bridge().invoke_save_current();
    h.pump();
    assert!(take_message().is_some(), "the read error is shown");
    let id =
        h.ws.borrow()
            .current()
            .unwrap()
            .order_id
            .expect("the tab points at it");
    assert_eq!(
        tab_titles(&h),
        vec![(format!("Shitje #{} · Bar A", id.0), false)]
    );
    assert!(h.bridge().get_current_is_saved());
    // A second Ruaj replaces the invoice instead of inserting it again.
    h.bridge().invoke_save_current();
    h.pump();
    take_confirm()
        .expect("Konfirmo përditësimin")
        .invoke_answered(true);
    h.pump();
    assert_eq!(h.flash(), "Porosia u përditësua.");
    let product = h.ctx.crm.search_products("uje", 1).unwrap().remove(0);
    assert_eq!(product.sku.quantity.0, 8, "the stock moved once");
    assert_eq!(h.bridge().get_list_rows().row_count(), 1, "one invoice");
    // After a restart the tab shows the saved invoice.
    let dir = h.close();
    let h = harness_in(dir);
    assert_eq!(h.ws.borrow().current().unwrap().order_id, Some(id));
}

#[test]
fn a_deleted_products_line_is_read_only_but_its_quantity() {
    use crm_services::domain::{DraftLine, Money, OrderDraft, ProductInput, Qty};
    let h = harness();
    // X and Y share SKU L1; X is bought, then deleted.
    let input = |name: &str| ProductInput {
        code: "L1".into(),
        name: name.into(),
        unit: "COPE".into(),
        buy_price: Money(100),
        sell_price: Money(200),
        quantity: Qty(0),
    };
    let x = h.ctx.crm.create_product(input("XNAME")).unwrap();
    let y = h.ctx.crm.create_product(input("YNAME")).unwrap();
    let id = h
        .ctx
        .crm
        .save_order(
            OrderDraft {
                kind: OrderKind::Purchase,
                owner_id: None,
                owner_name: "F".into(),
                owner_description: String::new(),
                issued_on: crm_services::domain::time::today(),
                lines: vec![DraftLine {
                    product_id: Some(x.product.id),
                    name: "XNAME".into(),
                    quantity: Qty(10),
                    price: Money(100),
                    buy_price: Some(Money(100)),
                    sell_price: Some(Money(200)),
                }],
            },
            None,
            None,
        )
        .unwrap();
    h.ctx.crm.delete_product(x.product.id).unwrap();
    h.ws.borrow_mut().open_order(id);
    h.pump();
    let row = h.row(0);
    assert!(row.locked);
    assert_eq!(row.stock, "artikull i fshirë");
    assert!(row.stock_warn && !row.stock_link);
    // Artikulli and the prices do not change; no suggestions open.
    h.focus(0, 0);
    assert!(!h.bridge().get_dropdown_open());
    h.type_text("Z");
    h.tap(Key::Return);
    assert_eq!(h.row(0).product, "XNAME");
    h.focus(0, 2);
    h.type_text("5");
    h.tap(Key::Return);
    assert_eq!(h.row(0).price, "1,00");
    // Sasia does.
    h.focus(0, 1);
    h.type_text("12");
    h.tap(Key::Return);
    assert_eq!(h.row(0).qty, "12");
    h.bridge().invoke_save_current();
    h.pump();
    take_confirm().unwrap().invoke_answered(true);
    h.pump();
    assert!(take_message().is_none(), "saved");
    let names: Vec<_> = h
        .ctx
        .crm
        .list_products("", Default::default())
        .unwrap()
        .into_iter()
        .map(|r| r.product.name)
        .collect();
    assert_eq!(names, ["YNAME"], "no new product");
    let sibling = h.ctx.crm.get_product(y.product.id).unwrap();
    assert_eq!(sibling.sku.quantity, Qty(12), "only the difference moved");
    assert_eq!(sibling.sku.sell_price, Money(200));
    let line = &h.ctx.crm.get_order(id).unwrap().lines[0];
    assert_eq!((line.product_id, line.quantity), (None, Qty(12)));
    // Still a deleted product's line after the save.
    assert!(h.row(0).locked);
}

#[test]
fn restored_tabs_rebuild_a_legacy_stored_title_from_the_current_texts() {
    let h = harness();
    h.product("UJE GLINA 0.5L", 10, 30);
    h.new_document(0);
    fill_sale(&h, "Bar Alfa", "uje", "1");
    h.bridge().invoke_save_current();
    h.pump();
    let id = h.ws.borrow().current().unwrap().order_id.unwrap();
    let dir = h.close();
    // Titles stored before the rename to "Porosi", written with the app closed.
    let crm = crm_services::Crm::open(
        crm_services::DataPaths::at(dir.path()),
        &crm_services::DbKey::for_tests(1),
    )
    .unwrap();
    crm.clear_tabs().unwrap();
    let legacy = |key: &str, position, order_id, title: &str| crm_services::OpenTab {
        tab_key: key.into(),
        position,
        kind: OrderKind::Sale,
        order_id,
        title: title.into(),
        active: position == 0,
        draft: None,
    };
    crm.save_tab(legacy("00000000000000a1", 0, None, "Faturë Shitje (e re)"))
        .unwrap();
    crm.save_tab(legacy(
        "00000000000000a2",
        1,
        Some(id),
        &format!("Faturë Shitje #{} · Bar Alfa", id.0),
    ))
    .unwrap();
    crm.close().unwrap();
    let h = harness_in(dir);
    let expected = vec![
        ("Porosi Shitje (e re)".to_owned(), false),
        (format!("Shitje #{} · Bar Alfa", id.0), false),
    ];
    assert_eq!(tab_titles(&h), expected);
    // The corrected titles are written back.
    h.bridge().invoke_flush_drafts();
    let stored: Vec<String> = h
        .ctx
        .crm
        .list_tabs()
        .unwrap()
        .into_iter()
        .map(|t| t.title)
        .collect();
    assert_eq!(stored, [expected[0].0.clone(), expected[1].0.clone()]);
}

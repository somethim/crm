//! Headless tests of the catalogue (task 009): the Artikujt, Klientët and
//! Furnitorët windows (rows, search, sort, keys), the property dialogs
//! (create, edit, Ruaj / Fshi / Anulo, inline validation), the delete confirmation, the
//! data-change refresh and the product editor hook used by the order grid.

use crm_services::domain::{Money, OwnerInput, OwnerKind, ProductId, ProductInput, Qty};
use crm_services::{Crm, DataPaths};
use crm_ui::catalogue::{self, dialogs, texts};
use crm_ui::shell::modal::ModalOwner;
use crm_ui::{AppWindow, CatalogueBridge, CatalogueList, ShellBridge, UiContext};
use i_slint_backend_testing::ElementHandle;
use slint::platform::{Key, WindowEvent};
use slint::{ComponentHandle, Model, SharedString};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

struct Harness {
    app: AppWindow,
    ctx: Rc<UiContext>,
    _dir: tempfile::TempDir,
}

fn unlocked() -> Harness {
    i_slint_backend_testing::init_no_event_loop();
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    paths.ensure_dirs().unwrap();
    let crm = Crm::open(paths.clone(), &crm_services::DbKey::for_tests(1)).unwrap();
    let (app, ctx) = crm_ui::build(
        crm,
        crm_services::Vault::with_params(paths.clone(), crm_services::KdfParams::TEST),
        paths,
    )
    .unwrap();
    app.show().unwrap();
    ctx.unlock();
    Harness {
        app,
        ctx,
        _dir: dir,
    }
}

fn product(crm: &Crm, code: &str, name: &str, sell: i64, qty: i64) -> ProductId {
    crm.create_product(ProductInput {
        code: code.into(),
        name: name.into(),
        unit: "SHISHE".into(),
        buy_price: Money(sell / 2),
        sell_price: Money(sell),
        quantity: Qty(qty),
    })
    .unwrap()
    .product
    .id
}

fn owner(crm: &Crm, kind: OwnerKind, name: &str, nipt: &str) {
    crm.create_owner(OwnerInput {
        kind,
        name: name.into(),
        description: String::new(),
        nipt: nipt.into(),
        address: "Tiranë".into(),
        account_code: String::new(),
    })
    .unwrap();
}

/// Three products: A0001 Coca Cola, B0002 Fanta Portokall, C0003 Ujë Tepelena.
fn with_products() -> Harness {
    let h = unlocked();
    product(&h.ctx.crm, "C0003", "Ujë Tepelena", 5_000, 100);
    product(&h.ctx.crm, "A0001", "Coca Cola", 12_000, 24);
    product(&h.ctx.crm, "B0002", "Fanta Portokall", 9_000, 1_500);
    h
}

/// The cell texts of `row`.
fn cells(list: &CatalogueList<'_>, row: usize) -> Vec<String> {
    let rows = list.get_rows();
    let row = rows.row_data(row).unwrap();
    (0..row.row_count())
        .map(|i| row.row_data(i).unwrap().text.to_string())
        .collect()
}

fn column(list: &CatalogueList<'_>, col: usize) -> Vec<String> {
    let n = list.get_rows().row_count();
    (0..n).map(|r| cells(list, r)[col].clone()).collect()
}

fn key(window: &slint::Window, text: impl Into<SharedString>) {
    let text = text.into();
    window.dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    window.dispatch_event(WindowEvent::KeyReleased { text });
}

fn search(list: &CatalogueList<'_>, text: &str) {
    list.set_search_text(text.into());
    list.invoke_search_edited(text.into());
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(200));
}

fn select(list: &CatalogueList<'_>, col: usize, text: &str) {
    let row = column(list, col).iter().position(|c| c == text).unwrap();
    list.set_current_row(i32::try_from(row).unwrap());
}

#[test]
fn catalogue_products_window_lists_searches_adds_validates_and_deletes() {
    let h = with_products();
    h.app.global::<CatalogueBridge>().invoke_open_list(0);
    let ctrl = catalogue::products_list(&h.ctx).expect("the list opened");
    let window = ctrl.window();
    let list = window.global::<CatalogueList>();
    assert!(window.window().is_visible());

    // 3 rows by code, formatted through crm_domain::format.
    assert_eq!(ctrl.row_count(), 3);
    assert_eq!(column(&list, 0), ["A0001", "B0002", "C0003"]);
    assert_eq!(
        cells(&list, 1),
        [
            "B0002",
            "Fanta Portokall",
            "SHISHE",
            "45,00",
            "90,00",
            "1.500"
        ]
    );
    assert_eq!(list.get_status_text(), "3 artikuj");

    // Search (debounced 150 ms): one row; empty again: all.
    list.set_search_text("Fanta".into());
    list.invoke_search_edited("Fanta".into());
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
    assert_eq!(ctrl.row_count(), 3, "still debouncing");
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
    assert_eq!(ctrl.row_count(), 1);
    assert_eq!(cells(&list, 0)[1], "Fanta Portokall");
    assert_eq!(list.get_status_text(), "1 artikull");
    search(&list, "");
    assert_eq!(ctrl.row_count(), 3);

    // Shto → dialog → fields → OK → 4 rows, new row selected, window unblocked.
    list.invoke_add();
    let dialog = dialogs::open_product_dialog().expect("product dialog open");
    assert_eq!(dialog.get_caption(), texts::NEW_PRODUCT);
    assert_eq!(dialog.get_unit(), "SHISHE");
    assert!(
        list.get_input_blocked(),
        "the dialog is modal over the list"
    );
    dialog.set_name("Lëng Molle".into());
    dialog.set_buy_price("1.234,5".into());
    dialog.set_sell_price("2000".into());
    dialog.set_quantity("12".into());
    dialog.invoke_ok();
    assert!(dialogs::open_product_dialog().is_none(), "OK closes");
    assert!(!list.get_input_blocked());
    assert_eq!(ctrl.row_count(), 4);
    let selected = usize::try_from(list.get_current_row()).unwrap();
    assert_eq!(
        cells(&list, selected),
        [
            "A0002",
            "Lëng Molle",
            "SHISHE",
            "1.234,50",
            "2.000,00",
            "12"
        ]
    );

    // Invalid price → inline error under the field, nothing saved.
    list.invoke_add();
    let dialog = dialogs::open_product_dialog().unwrap();
    dialog.set_name("Gabim".into());
    dialog.set_buy_price("abc".into());
    dialog.invoke_ok();
    assert!(dialogs::open_product_dialog().is_some(), "stays open");
    assert!(!dialog.get_buy_price_error().is_empty());
    assert!(dialog.get_sell_price_error().is_empty());
    // A service validation error (blank name) lands under its field too.
    dialog.set_buy_price("1".into());
    dialog.set_name("  ".into());
    dialog.invoke_ok();
    assert!(dialog.get_buy_price_error().is_empty());
    assert!(!dialog.get_name_error().is_empty());
    dialog.invoke_cancel();
    assert!(dialogs::open_product_dialog().is_none());
    assert_eq!(
        h.ctx
            .crm
            .list_products("", crm_services::Page::ALL)
            .unwrap()
            .len(),
        4
    );
    assert_eq!(ctrl.row_count(), 4);

    // Delete with confirm → 3 rows. The last product of its code also drops the code.
    select(&list, 1, "Lëng Molle");
    list.invoke_remove();
    let confirm = dialogs::open_confirm().expect("delete confirmation");
    assert_eq!(confirm.get_caption(), "Konfirmo fshirjen");
    assert_eq!(
        confirm.get_text(),
        "Ta fshij «Lëng Molle»? Ky veprim nuk mund të zhbëhet. Kodi A0002 do të fshihet gjithashtu."
    );
    assert_eq!(confirm.get_ok_label(), "Fshi");
    confirm.invoke_answered(true);
    assert_eq!(ctrl.row_count(), 3);
    assert!(!list.get_input_blocked());

    // Anulo keeps the row.
    select(&list, 0, "A0001");
    list.invoke_remove();
    dialogs::open_confirm().unwrap().invoke_answered(false);
    assert_eq!(ctrl.row_count(), 3);
}

#[test]
fn catalogue_products_edit_ruaj_shared_code_note_and_sorting() {
    let h = with_products();
    // A second product on code A0001 (joins its SKU).
    product(&h.ctx.crm, "A0001", "Coca Cola Zero", 1, 1);
    catalogue::open_list(&h.ctx, catalogue::ListKind::Products);
    let ctrl = catalogue::products_list(&h.ctx).unwrap();
    let list = ctrl.window().global::<CatalogueList>();
    assert_eq!(ctrl.row_count(), 4);

    select(&list, 1, "Coca Cola");
    list.invoke_edit();
    let dialog = dialogs::open_product_dialog().unwrap();
    assert_eq!(dialog.get_caption(), "Artikulli: Coca Cola");
    assert_eq!(dialog.get_code(), "A0001");
    assert_eq!(dialog.get_sell_price(), "120,00");
    assert_eq!(
        dialog.get_note(),
        "Kodi, njësia, çmimet dhe gjendja ndahen me 1 artikuj të tjerë."
    );
    // Ruaj saves and closes; the shared SKU changes for both rows.
    dialog.set_sell_price("150".into());
    dialog.set_name("Coca Cola 0.5L".into());
    dialog.invoke_ok();
    assert!(dialogs::open_product_dialog().is_none(), "Ruaj closes");
    assert!(!list.get_input_blocked());
    let prices: Vec<String> = column(&list, 4);
    assert_eq!(prices.iter().filter(|p| *p == "150,00").count(), 2);
    assert_eq!(
        cells(&list, usize::try_from(list.get_current_row()).unwrap())[1],
        "Coca Cola 0.5L",
        "the edited row stays selected"
    );

    // Sort by Gjendja numerically (24 < 100 < 1.500), then descending.
    list.invoke_sort_ascending(5);
    assert_eq!(column(&list, 5), ["24", "24", "100", "1.500"]);
    list.invoke_sort_descending(5);
    assert_eq!(column(&list, 5), ["1.500", "100", "24", "24"]);
    // The sort survives a reload.
    list.invoke_refresh();
    assert_eq!(column(&list, 5), ["1.500", "100", "24", "24"]);

    // Deleting one of two products on a code keeps the code: no extra sentence.
    select(&list, 1, "Coca Cola Zero");
    list.invoke_remove();
    assert_eq!(
        dialogs::open_confirm().unwrap().get_text(),
        "Ta fshij «Coca Cola Zero»? Ky veprim nuk mund të zhbëhet."
    );
    dialogs::open_confirm().unwrap().invoke_answered(true);
    assert_eq!(ctrl.row_count(), 3);
}

#[test]
fn catalogue_keys_and_double_click_drive_the_list() {
    let h = with_products();
    catalogue::open_list(&h.ctx, catalogue::ListKind::Products);
    let ctrl = catalogue::products_list(&h.ctx).unwrap();
    let window = ctrl.window();
    let list = window.global::<CatalogueList>();

    // Ins = Shto (from the search box, where the focus starts).
    key(window.window(), Key::Insert);
    let dialog = dialogs::open_product_dialog().expect("Ins opens the dialog");
    // Esc in the dialog = Anulo.
    key(dialog.window(), Key::Escape);
    assert!(dialogs::open_product_dialog().is_none());

    // F2 = Ndrysho on the selected row.
    list.set_current_row(0);
    ctrl.focus_table();
    key(window.window(), Key::F2);
    let dialog = dialogs::open_product_dialog().expect("F2 edits");
    assert_eq!(dialog.get_name(), "Coca Cola");
    dialog.invoke_cancel();

    // Double click on a row = Ndrysho.
    list.invoke_row_pressed(2);
    assert!(
        dialogs::open_product_dialog().is_none(),
        "one press only selects"
    );
    list.invoke_row_pressed(2);
    assert_eq!(
        dialogs::open_product_dialog()
            .expect("double click edits")
            .get_name(),
        "Ujë Tepelena"
    );
    dialogs::open_product_dialog().unwrap().invoke_cancel();

    // F-007: the double-click window runs on Slint's (mock) clock, not the
    // wall clock. A slow machine (here a real 600 ms pause) still double-clicks.
    list.invoke_row_pressed(1);
    std::thread::sleep(Duration::from_millis(600));
    list.invoke_row_pressed(1);
    assert_eq!(
        dialogs::open_product_dialog()
            .expect("wall-clock time does not matter")
            .get_name(),
        "Fanta Portokall"
    );
    dialogs::open_product_dialog().unwrap().invoke_cancel();
    // 600 ms of Slint time between presses: two single presses.
    list.invoke_row_pressed(2);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(600));
    list.invoke_row_pressed(2);
    assert!(dialogs::open_product_dialog().is_none(), "too slow");
    // The second press opened a new window: one within 499 ms is a double.
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(499));
    list.invoke_row_pressed(2);
    assert!(dialogs::open_product_dialog().is_some(), "within 500 ms");
    dialogs::open_product_dialog().unwrap().invoke_cancel();
    // Presses on two different rows are not a double click.
    list.invoke_row_pressed(0);
    list.invoke_row_pressed(1);
    assert!(dialogs::open_product_dialog().is_none(), "different rows");
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(600));

    // Del = Fshi (asks first).
    ctrl.focus_table();
    key(window.window(), Key::Delete);
    assert!(dialogs::open_confirm().is_some());
    dialogs::open_confirm().unwrap().invoke_answered(false);

    // F5 = Rifresko picks up a product added elsewhere.
    product(&h.ctx.crm, "D0004", "Birrë Korça", 10_000, 6);
    key(window.window(), Key::F5);
    assert_eq!(ctrl.row_count(), 4);

    // Esc = Mbyll (hides); opening again reuses and shows the same window.
    key(window.window(), Key::Escape);
    assert!(!window.window().is_visible());
    h.app.global::<CatalogueBridge>().invoke_open_list(0);
    let again = catalogue::products_list(&h.ctx).unwrap();
    assert!(Rc::ptr_eq(&ctrl, &again));
    assert!(window.window().is_visible());
}

#[test]
fn catalogue_search_box_typing_filters() {
    let h = with_products();
    catalogue::open_list(&h.ctx, catalogue::ListKind::Products);
    let ctrl = catalogue::products_list(&h.ctx).unwrap();
    let search = ElementHandle::find_by_element_id(ctrl.window(), "CatalogueListWindow::search")
        .next()
        .expect("search box");
    search.set_accessible_value("Tepelena");
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(200));
    assert_eq!(ctrl.row_count(), 1);
}

#[test]
fn catalogue_clients_window_lists_searches_adds_validates_and_deletes() {
    let h = unlocked();
    owner(&h.ctx.crm, OwnerKind::Client, "Zana Market", "K11111111A");
    owner(&h.ctx.crm, OwnerKind::Client, "Alba Shpk", "K22222222B");
    owner(&h.ctx.crm, OwnerKind::Client, "Maxi Bar", "K33333333C");
    owner(
        &h.ctx.crm,
        OwnerKind::Supplier,
        "Furnitori Një",
        "L44444444D",
    );
    h.app.global::<CatalogueBridge>().invoke_open_list(1);
    let ctrl = catalogue::clients_list(&h.ctx).unwrap();
    let list = ctrl.window().global::<CatalogueList>();

    // By name in byte order; suppliers are not listed.
    assert_eq!(ctrl.row_count(), 3);
    assert_eq!(column(&list, 0), ["Alba Shpk", "Maxi Bar", "Zana Market"]);
    assert_eq!(
        cells(&list, 0),
        ["Alba Shpk", "", "K22222222B", "Tiranë", ""]
    );
    assert_eq!(list.get_status_text(), "3 klientë");

    search(&list, "Maxi");
    assert_eq!(ctrl.row_count(), 1);
    search(&list, "");

    // Shto → OK → 4 rows.
    list.invoke_add();
    let dialog = dialogs::open_owner_dialog().unwrap();
    assert_eq!(dialog.get_caption(), "Klient i ri");
    dialog.set_name("Bar Kafe Iliria".into());
    dialog.set_nipt("M55555555E".into());
    dialog.set_account("411-07".into());
    dialog.invoke_ok();
    assert!(dialogs::open_owner_dialog().is_none());
    assert_eq!(ctrl.row_count(), 4);
    let row = usize::try_from(list.get_current_row()).unwrap();
    assert_eq!(
        cells(&list, row),
        ["Bar Kafe Iliria", "", "M55555555E", "", "411-07"]
    );

    // A blank name and a too-long name → inline errors, nothing saved.
    list.invoke_add();
    let dialog = dialogs::open_owner_dialog().unwrap();
    dialog.invoke_ok();
    assert!(!dialog.get_name_error().is_empty());
    dialog.set_name("x".repeat(51).into());
    dialog.invoke_ok();
    assert!(!dialog.get_name_error().is_empty());
    dialog.invoke_cancel();
    assert_eq!(ctrl.row_count(), 4);

    // Edit: the title names the client; Ruaj saves and closes.
    select(&list, 0, "Maxi Bar");
    list.invoke_edit();
    let dialog = dialogs::open_owner_dialog().unwrap();
    assert_eq!(dialog.get_caption(), "Klienti: Maxi Bar");
    dialog.set_address(String::new().into());
    dialog.invoke_ok();
    assert!(dialogs::open_owner_dialog().is_none(), "Ruaj closes");
    let maxi = column(&list, 0)
        .iter()
        .position(|n| n == "Maxi Bar")
        .unwrap();
    assert_eq!(cells(&list, maxi)[3], "", "a blank field clears the value");

    // Delete with confirm → 3 rows.
    select(&list, 0, "Bar Kafe Iliria");
    list.invoke_remove();
    let confirm = dialogs::open_confirm().unwrap();
    assert_eq!(
        confirm.get_text(),
        "Ta fshij «Bar Kafe Iliria»? Ky veprim nuk mund të zhbëhet."
    );
    confirm.invoke_answered(true);
    assert_eq!(ctrl.row_count(), 3);
}

// F-005: a duplicate code + name or a used Llogari is an inline field error,
// not the generic "bie ndesh" message box.
#[test]
fn catalogue_duplicates_show_inline_field_errors() {
    let h = with_products();
    h.app.global::<CatalogueBridge>().invoke_open_list(0);
    let ctrl = catalogue::products_list(&h.ctx).unwrap();
    let list = ctrl.window().global::<CatalogueList>();
    list.invoke_add();
    let dialog = dialogs::open_product_dialog().unwrap();
    dialog.set_code("A0001".into());
    dialog.set_name("Coca Cola".into());
    dialog.invoke_ok();
    assert!(dialogs::open_product_dialog().is_some(), "stays open");
    assert_eq!(
        dialog.get_name_error(),
        "Ky emër përdoret nga një artikull tjetër me të njëjtin kod."
    );
    assert!(dialogs::open_message().is_none(), "no message box");
    dialog.invoke_cancel();
    assert_eq!(ctrl.row_count(), 3);

    owner(&h.ctx.crm, OwnerKind::Client, "Zana Market", "");
    h.ctx
        .crm
        .create_owner(OwnerInput {
            kind: OwnerKind::Client,
            name: "Alba Shpk".into(),
            description: String::new(),
            nipt: String::new(),
            address: String::new(),
            account_code: "411-01".into(),
        })
        .unwrap();
    h.app.global::<CatalogueBridge>().invoke_open_list(1);
    let ctrl = catalogue::clients_list(&h.ctx).unwrap();
    let list = ctrl.window().global::<CatalogueList>();
    select(&list, 0, "Zana Market");
    list.invoke_edit();
    let dialog = dialogs::open_owner_dialog().unwrap();
    dialog.set_account("411-01".into());
    dialog.invoke_ok();
    assert!(dialogs::open_owner_dialog().is_some(), "stays open");
    assert_eq!(
        dialog.get_account_error(),
        "Kjo llogari përdoret nga një klient tjetër."
    );
    assert!(dialogs::open_message().is_none(), "no message box");
    dialog.invoke_cancel();
}

#[test]
fn catalogue_suppliers_window_uses_its_own_kind() {
    let h = unlocked();
    owner(
        &h.ctx.crm,
        OwnerKind::Supplier,
        "Coca-Cola Albania",
        "J11111111A",
    );
    owner(&h.ctx.crm, OwnerKind::Client, "Klienti", "K11111111A");
    catalogue::open_list(&h.ctx, catalogue::ListKind::Suppliers);
    let ctrl = catalogue::suppliers_list(&h.ctx).unwrap();
    let list = ctrl.window().global::<CatalogueList>();
    assert_eq!(ctrl.row_count(), 1);
    assert_eq!(list.get_status_text(), "1 furnitor");
    list.invoke_add();
    let dialog = dialogs::open_owner_dialog().unwrap();
    assert_eq!(dialog.get_caption(), "Furnitor i ri");
    dialog.set_name("Birra Korça".into());
    dialog.invoke_ok();
    assert_eq!(ctrl.row_count(), 2);
    assert_eq!(
        h.ctx
            .crm
            .list_owners(OwnerKind::Client, "", crm_services::Page::ALL)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn catalogue_lists_refresh_on_data_changes_keeping_the_selection() {
    let h = with_products();
    catalogue::open_list(&h.ctx, catalogue::ListKind::Products);
    let ctrl = catalogue::products_list(&h.ctx).unwrap();
    let list = ctrl.window().global::<CatalogueList>();
    select(&list, 1, "Fanta Portokall");
    product(&h.ctx.crm, "0000", "Aaa i pari", 1, 1);
    h.ctx.notify_data_changed();
    assert_eq!(ctrl.row_count(), 4);
    let row = usize::try_from(list.get_current_row()).unwrap();
    assert_eq!(
        cells(&list, row)[1],
        "Fanta Portokall",
        "selection kept by id"
    );

    // A hidden list does not reload.
    list.invoke_close_window();
    product(&h.ctx.crm, "E0005", "Tjetër", 1, 1);
    h.ctx.notify_data_changed();
    assert_eq!(ctrl.row_count(), 4);
}

// F-001 wave 2: the 2 s poll no longer reports this process's own commits, so
// every catalogue write notifies the other windows itself (and a failed or
// cancelled one does not).
#[test]
fn catalogue_own_writes_notify_the_other_windows() {
    let h = with_products();
    let notified = Rc::new(std::cell::Cell::new(0));
    let n = notified.clone();
    h.ctx.on_data_changed(move || n.set(n.get() + 1));
    catalogue::open_list(&h.ctx, catalogue::ListKind::Products);
    let ctrl = catalogue::products_list(&h.ctx).unwrap();
    let list = ctrl.window().global::<CatalogueList>();

    // Shto → OK.
    list.invoke_add();
    let dialog = dialogs::open_product_dialog().unwrap();
    dialog.set_name("Lëng Molle".into());
    dialog.invoke_ok();
    assert_eq!(notified.get(), 1, "a created product notifies");
    assert_eq!(ctrl.row_count(), 4);

    // A validation error or Anulo writes nothing and does not notify.
    list.invoke_add();
    let dialog = dialogs::open_product_dialog().unwrap();
    dialog.set_name("  ".into());
    dialog.invoke_ok();
    dialog.invoke_cancel();
    assert_eq!(notified.get(), 1);

    // Ndrysho → Ruaj.
    select(&list, 1, "Coca Cola");
    list.invoke_edit();
    let dialog = dialogs::open_product_dialog().unwrap();
    dialog.set_quantity("7".into());
    dialog.invoke_ok();
    assert_eq!(notified.get(), 2, "an edited product notifies");

    // Fshi: Anulo does not notify, a confirmed delete does.
    select(&list, 1, "Lëng Molle");
    list.invoke_remove();
    dialogs::open_confirm().unwrap().invoke_answered(false);
    assert_eq!(notified.get(), 2);
    list.invoke_remove();
    dialogs::open_confirm().unwrap().invoke_answered(true);
    assert_eq!(notified.get(), 3, "a delete notifies");
    assert_eq!(ctrl.row_count(), 3);

    // An owner saved in Klientët.
    catalogue::open_list(&h.ctx, catalogue::ListKind::Clients);
    let clients = catalogue::clients_list(&h.ctx).unwrap();
    clients.window().global::<CatalogueList>().invoke_add();
    let dialog = dialogs::open_owner_dialog().unwrap();
    dialog.set_name("Bar Iliria".into());
    dialog.invoke_ok();
    assert_eq!(notified.get(), 4, "a created client notifies");

    // The product editor used by the invoice grid ("/ N" link).
    let id = h
        .ctx
        .crm
        .list_products("Fanta", crm_services::Page::ALL)
        .unwrap()[0]
        .product
        .id;
    h.ctx.open_product_editor(id, Box::new(|| {}));
    let dialog = dialogs::open_product_dialog().unwrap();
    dialog.set_quantity("9".into());
    dialog.invoke_ok();
    assert_eq!(notified.get(), 5, "the product editor notifies");
    let fanta = column(&list, 1)
        .iter()
        .position(|n| n == "Fanta Portokall")
        .unwrap();
    assert_eq!(
        cells(&list, fanta)[5],
        "9",
        "the shown Artikujt list reloaded"
    );
}

#[test]
fn catalogue_windows_are_hidden_by_the_lock() {
    let h = with_products();
    catalogue::open_list(&h.ctx, catalogue::ListKind::Clients);
    let ctrl = catalogue::clients_list(&h.ctx).unwrap();
    assert!(ctrl.window().window().is_visible());
    h.ctx.lock();
    assert!(!ctrl.window().window().is_visible());
    h.ctx.unlock();
    assert!(ctrl.window().window().is_visible());
}

#[test]
fn catalogue_product_editor_hook_opens_the_dialog_over_the_main_window() {
    let h = with_products();
    let id = h
        .ctx
        .crm
        .list_products("Coca", crm_services::Page::ALL)
        .unwrap()[0]
        .product
        .id;
    let saved = Rc::new(RefCell::new(0));
    let s = saved.clone();
    h.ctx
        .open_product_editor(id, Box::new(move || *s.borrow_mut() += 1));
    let dialog = dialogs::open_product_dialog().expect("editor open");
    assert_eq!(dialog.get_caption(), "Artikulli: Coca Cola");
    assert!(h.app.get_input_blocked(), "modal over the main window");
    dialog.set_quantity("30".into());
    dialog.invoke_ok();
    assert_eq!(*saved.borrow(), 1);
    assert!(!h.app.get_input_blocked());
    assert_eq!(h.ctx.crm.get_product(id).unwrap().sku.quantity, Qty(30));

    // A product deleted meanwhile: the status bar says it no longer exists
    // (F-006; "editor unavailable" is only for an unregistered editor).
    h.ctx.crm.delete_product(id).unwrap();
    h.ctx.open_product_editor(id, Box::new(|| {}));
    assert!(dialogs::open_product_dialog().is_none());
    assert_eq!(
        h.app.global::<ShellBridge>().get_flash_text(),
        "Ky artikull nuk ekziston më."
    );
    assert_eq!(texts::PRODUCT_DELETED, "Ky artikull nuk ekziston më.");
    assert_ne!(
        texts::PRODUCT_DELETED,
        crm_ui::shell::texts::PRODUCT_EDITOR_MISSING
    );
}

#[test]
fn catalogue_import_dialog_preselects_the_kind_and_blocks_its_owner() {
    let h = unlocked();
    h.app.global::<CatalogueBridge>().invoke_import(2);
    assert!(h.app.get_input_blocked());
    catalogue::open_list(&h.ctx, catalogue::ListKind::Products);
    let ctrl = catalogue::products_list(&h.ctx).unwrap();
    let flow =
        catalogue::transfer::open_import(&h.ctx, ctrl.window(), catalogue::ListKind::Clients)
            .unwrap();
    let dialog = flow.dialog().unwrap();
    assert_eq!(dialog.get_kind(), 1);
    assert!(ctrl.window().global::<CatalogueList>().get_input_blocked());
    assert_eq!(ctrl.window().native_title(), "Artikujt");
    dialog.invoke_dismiss();
    assert!(!ctrl.window().global::<CatalogueList>().get_input_blocked());
    assert!(!dialog.window().is_visible(), "Mbyll closes the dialog");
}

#[test]
fn catalogue_texts_have_no_english() {
    // The 008 denylist test scans shell/texts.rs; the same closed literals here.
    let text = include_str!("../src/catalogue/texts.rs");
    for word in [
        "Add",
        "Edit",
        "Save",
        "Delete",
        "Print",
        "Cancel",
        "Error",
        "Warning",
        "File",
        "Help",
        "Search",
        "Loading",
        "No results",
        "\"Row ",
    ] {
        assert!(
            !text.contains(&format!("\"{word}\"")) && !text.contains(&format!("\"{word} ")),
            "{word}"
        );
    }
}

// -- F-016: a restore closes the open dialogs without saving ----------------

#[test]
fn a_restore_closes_an_open_product_dialog_without_saving() {
    let h = with_products();
    h.app.global::<CatalogueBridge>().invoke_open_list(0);
    let ctrl = catalogue::products_list(&h.ctx).unwrap();
    let list = ctrl.window().global::<CatalogueList>();
    select(&list, 0, "A0001");
    list.invoke_edit();
    let dialog = dialogs::open_product_dialog().expect("edit dialog");
    dialog.set_name("Emër i vjetër".into());
    dialog.set_sell_price("1".into());

    h.ctx.notify_restored();

    assert!(
        dialogs::open_product_dialog().is_none(),
        "closed by the restore"
    );
    assert!(!list.get_input_blocked(), "the list is usable again");
    // A late OK from the stale dialog writes nothing.
    dialog.invoke_ok();
    let rows = h
        .ctx
        .crm
        .list_products("", crm_services::Page::ALL)
        .unwrap();
    let coca = rows.iter().find(|r| r.sku.code == "A0001").unwrap();
    assert_eq!(coca.product.name, "Coca Cola");
    assert_eq!(coca.sku.sell_price, Money(12_000));

    // A dialog opened after the restore works as before.
    select(&list, 0, "A0001");
    list.invoke_edit();
    let dialog = dialogs::open_product_dialog().expect("edit dialog again");
    assert_eq!(dialog.get_name(), "Coca Cola");
    dialog.invoke_cancel();
}

#[test]
fn a_restore_closes_open_owner_dialogs_without_saving() {
    let h = unlocked();
    owner(&h.ctx.crm, OwnerKind::Client, "Maxi Bar", "K11111111A");
    h.app.global::<CatalogueBridge>().invoke_open_list(1);
    let clients = catalogue::clients_list(&h.ctx).unwrap();
    let list = clients.window().global::<CatalogueList>();
    select(&list, 0, "Maxi Bar");
    list.invoke_edit();
    let dialog = dialogs::open_owner_dialog().expect("edit dialog");
    dialog.set_name("Bar i vjetër".into());
    // A create-mode dialog over another list closes too.
    h.app.global::<CatalogueBridge>().invoke_open_list(2);
    let suppliers = catalogue::suppliers_list(&h.ctx).unwrap();
    let slist = suppliers.window().global::<CatalogueList>();
    slist.invoke_add();
    let new_dialog = dialogs::open_owner_dialog().expect("create dialog");
    new_dialog.set_name("Furnitor i ri".into());

    h.ctx.notify_restored();

    assert!(dialogs::open_owner_dialog().is_none());
    assert!(!list.get_input_blocked());
    assert!(!slist.get_input_blocked());
    dialog.invoke_ok();
    new_dialog.invoke_ok();
    let clients = h
        .ctx
        .crm
        .list_owners(OwnerKind::Client, "", crm_services::Page::ALL)
        .unwrap();
    assert_eq!(
        clients.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(),
        ["Maxi Bar"]
    );
    assert!(
        h.ctx
            .crm
            .list_owners(OwnerKind::Supplier, "", crm_services::Page::ALL)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_restore_closes_an_open_delete_confirmation_without_deleting() {
    let h = with_products();
    h.app.global::<CatalogueBridge>().invoke_open_list(0);
    let ctrl = catalogue::products_list(&h.ctx).unwrap();
    let list = ctrl.window().global::<CatalogueList>();
    select(&list, 0, "B0002");
    list.invoke_remove();
    let confirm = dialogs::open_confirm().expect("delete confirmation");

    h.ctx.notify_restored();

    assert!(dialogs::open_confirm().is_none(), "closed by the restore");
    assert!(!list.get_input_blocked());
    confirm.invoke_answered(true);
    assert_eq!(
        h.ctx
            .crm
            .list_products("", crm_services::Page::ALL)
            .unwrap()
            .len(),
        3,
        "nothing deleted"
    );
}

/// The labels of the dialog buttons from left to right.
fn button_row(root: &impl i_slint_backend_testing::ElementRoot) -> Vec<String> {
    let mut buttons: Vec<(f32, String)> =
        ElementHandle::find_by_element_type_name(root, "DialogButton")
            .map(|b| {
                let label = b.accessible_label().unwrap_or_default().to_string();
                (b.absolute_position().x, label)
            })
            .collect();
    buttons.sort_by(|a, b| a.0.total_cmp(&b.0));
    buttons.into_iter().map(|(_, l)| l).collect()
}

#[test]
fn property_dialogs_offer_fshi_only_for_a_saved_record() {
    let h = with_products();
    owner(&h.ctx.crm, OwnerKind::Client, "Maxi Bar", "K11111111A");
    catalogue::open_list(&h.ctx, catalogue::ListKind::Products);
    let list = catalogue::products_list(&h.ctx).unwrap();
    let list = list.window().global::<CatalogueList>();

    list.invoke_add();
    let dialog = dialogs::open_product_dialog().unwrap();
    assert!(!dialog.get_editing());
    assert_eq!(
        button_row(&dialog),
        ["Ruaj", "Anulo"],
        "no Fshi for a new product"
    );
    dialog.invoke_cancel();

    select(&list, 1, "Coca Cola");
    list.invoke_edit();
    let dialog = dialogs::open_product_dialog().unwrap();
    assert!(dialog.get_editing());
    assert_eq!(button_row(&dialog), ["Ruaj", "Fshi", "Anulo"]);
    dialog.invoke_cancel();

    catalogue::open_list(&h.ctx, catalogue::ListKind::Clients);
    let clients = catalogue::clients_list(&h.ctx).unwrap();
    let clients = clients.window().global::<CatalogueList>();
    clients.invoke_add();
    let dialog = dialogs::open_owner_dialog().unwrap();
    assert_eq!(dialog.get_caption(), "Klient i ri");
    assert_eq!(
        button_row(&dialog),
        ["Ruaj", "Anulo"],
        "no Fshi for a new client"
    );
    dialog.invoke_cancel();

    select(&clients, 0, "Maxi Bar");
    clients.invoke_edit();
    let dialog = dialogs::open_owner_dialog().unwrap();
    assert_eq!(button_row(&dialog), ["Ruaj", "Fshi", "Anulo"]);
    dialog.invoke_cancel();
    assert!(dialogs::open_owner_dialog().is_none(), "Anulo closes");
}

#[test]
fn fshi_in_the_product_dialog_deletes_after_the_confirmation_and_closes() {
    let h = with_products();
    let notified = Rc::new(std::cell::Cell::new(0));
    let n = notified.clone();
    h.ctx.on_data_changed(move || n.set(n.get() + 1));
    catalogue::open_list(&h.ctx, catalogue::ListKind::Products);
    let ctrl = catalogue::products_list(&h.ctx).unwrap();
    let list = ctrl.window().global::<CatalogueList>();

    select(&list, 1, "Coca Cola");
    list.invoke_edit();
    let dialog = dialogs::open_product_dialog().unwrap();
    // Edited fields are not saved by Fshi.
    dialog.set_name("Coca Cola 2".into());
    dialog.invoke_delete();
    let confirm = dialogs::open_confirm().expect("the delete confirmation");
    assert_eq!(confirm.get_caption(), "Konfirmo fshirjen");
    assert_eq!(
        confirm.get_text(),
        "Ta fshij «Coca Cola»? Ky veprim nuk mund të zhbëhet. Kodi A0001 do të fshihet gjithashtu."
    );
    assert_eq!(confirm.get_ok_label(), "Fshi");
    assert!(
        dialog.get_input_blocked(),
        "the confirmation is modal over the dialog"
    );

    // Anulo in the confirmation keeps the dialog and the product.
    confirm.invoke_answered(false);
    assert!(
        dialogs::open_product_dialog().is_some(),
        "the dialog stays open"
    );
    assert!(!dialog.get_input_blocked());
    assert!(list.get_input_blocked(), "the dialog still covers the list");
    assert_eq!(ctrl.row_count(), 3);
    assert_eq!(notified.get(), 0);

    // Fshi again, confirmed: deleted, closed, every window told.
    dialog.invoke_delete();
    dialogs::open_confirm().unwrap().invoke_answered(true);
    assert!(
        dialogs::open_product_dialog().is_none(),
        "Fshi closes the dialog"
    );
    assert!(!list.get_input_blocked());
    assert_eq!(notified.get(), 1, "a delete notifies");
    assert_eq!(ctrl.row_count(), 2);
    assert_eq!(column(&list, 1), ["Fanta Portokall", "Ujë Tepelena"]);
}

#[test]
fn fshi_in_the_owner_dialog_deletes_and_a_failed_delete_keeps_it_open() {
    let h = unlocked();
    owner(
        &h.ctx.crm,
        OwnerKind::Supplier,
        "Coca-Cola HBC",
        "L11111111A",
    );
    owner(
        &h.ctx.crm,
        OwnerKind::Supplier,
        "Birra Tirana",
        "L22222222B",
    );
    catalogue::open_list(&h.ctx, catalogue::ListKind::Suppliers);
    let ctrl = catalogue::suppliers_list(&h.ctx).unwrap();
    let list = ctrl.window().global::<CatalogueList>();
    assert_eq!(ctrl.row_count(), 2);

    select(&list, 0, "Birra Tirana");
    list.invoke_edit();
    let dialog = dialogs::open_owner_dialog().unwrap();
    dialog.invoke_delete();
    let confirm = dialogs::open_confirm().unwrap();
    assert_eq!(
        confirm.get_text(),
        "Ta fshij «Birra Tirana»? Ky veprim nuk mund të zhbëhet."
    );
    confirm.invoke_answered(true);
    assert!(
        dialogs::open_owner_dialog().is_none(),
        "Fshi closes the dialog"
    );
    assert_eq!(ctrl.row_count(), 1);
    assert!(!list.get_input_blocked());

    // The row is deleted elsewhere while its confirmation is open: the error
    // shows over the dialog, which stays open.
    select(&list, 0, "Coca-Cola HBC");
    list.invoke_edit();
    let dialog = dialogs::open_owner_dialog().unwrap();
    dialog.invoke_delete();
    let id = h
        .ctx
        .crm
        .list_owners(OwnerKind::Supplier, "", crm_services::Page::ALL)
        .unwrap()[0]
        .id;
    h.ctx.crm.delete_owner(id).unwrap();
    dialogs::open_confirm().unwrap().invoke_answered(true);
    assert!(dialogs::open_message().is_some(), "the error is shown");
    assert!(
        dialogs::open_owner_dialog().is_some(),
        "the dialog stays open"
    );
    assert!(
        dialog.get_input_blocked(),
        "the message is modal over the dialog"
    );
    dialogs::open_message().unwrap().invoke_closed();
    assert!(!dialog.get_input_blocked());
    dialog.invoke_cancel();
    assert!(!list.get_input_blocked());
}

#[test]
fn fshi_in_the_product_editor_deletes_the_product_and_closes() {
    let h = with_products();
    let id = h
        .ctx
        .crm
        .list_products("Fanta", crm_services::Page::ALL)
        .unwrap()[0]
        .product
        .id;
    h.ctx.open_product_editor(id, Box::new(|| {}));
    let dialog = dialogs::open_product_dialog().unwrap();
    assert_eq!(button_row(&dialog), ["Ruaj", "Fshi", "Anulo"]);
    dialog.invoke_delete();
    dialogs::open_confirm().unwrap().invoke_answered(true);
    assert!(dialogs::open_product_dialog().is_none());
    assert!(!h.app.get_input_blocked());
    assert!(h.ctx.crm.get_product(id).is_err());
}

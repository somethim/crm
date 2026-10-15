//! F-029: while a background job runs (a restore, an import), the catalogue
//! windows are blocked and their open dialogs cannot write; after the job they
//! work as before. One test per file: the event-loop backend can be
//! initialised once per process.

use crm_services::domain::{Money, OwnerKind, ProductInput, Qty};
use crm_services::{Crm, DataPaths, Page};
use crm_ui::catalogue::{self, ListKind, dialogs};
use crm_ui::{CatalogueBridge, CatalogueList, ShellBridge};
use slint::ComponentHandle;
use std::path::PathBuf;
use std::sync::mpsc;

fn product(crm: &Crm, code: &str, name: &str) {
    crm.create_product(ProductInput {
        code: code.into(),
        name: name.into(),
        unit: "SHISHE".into(),
        buy_price: Money(500),
        sell_price: Money(1_000),
        quantity: Qty(10),
    })
    .unwrap();
}

fn names(crm: &Crm) -> Vec<String> {
    let mut names: Vec<String> = crm
        .list_products("", Page::ALL)
        .unwrap()
        .into_iter()
        .map(|r| r.product.name)
        .collect();
    names.sort();
    names
}

#[test]
fn catalogue_dialogs_cannot_write_while_a_background_job_runs() {
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    paths.ensure_dirs().unwrap();
    let crm = Crm::open(paths.clone(), &crm_services::DbKey::for_tests(1)).unwrap();
    product(&crm, "A0001", "Coca Cola");
    product(&crm, "B0002", "Fanta");
    let (app, ctx) = crm_ui::build(
        crm,
        crm_services::Vault::with_params(paths.clone(), crm_services::KdfParams::TEST),
        paths,
    )
    .unwrap();
    app.show().unwrap();
    ctx.unlock();

    app.global::<CatalogueBridge>().invoke_open_list(0);
    let ctrl = catalogue::products_list(&ctx).unwrap();
    let list = ctrl.window().global::<CatalogueList>();
    // An edit dialog and a delete confirmation are open when the job starts
    // (Artikujt and Klientët each have one).
    ctrl.add();
    let dialog = dialogs::open_product_dialog().expect("create dialog");
    dialog.set_code("C0003".into());
    dialog.set_name("Ujë".into());
    app.global::<CatalogueBridge>().invoke_open_list(1);
    let clients = catalogue::clients_list(&ctx).unwrap();
    let clist = clients.window().global::<CatalogueList>();
    clients.add();
    let client = dialogs::open_owner_dialog().expect("client dialog");
    client.set_name("Maxi Bar".into());

    // The job (standing in for a restore) waits until released.
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let weak = app.as_weak();
    ctx.run_bg(
        move |_crm| release_rx.recv().unwrap(),
        move |()| {
            assert!(!weak.upgrade().unwrap().global::<ShellBridge>().get_busy());
            slint::quit_event_loop().unwrap();
        },
    );
    assert!(ctx.is_busy());
    assert!(list.get_busy(), "Artikujt is blocked");
    assert!(clist.get_busy(), "Klientët is blocked");

    // OK in the open dialog writes nothing and leaves it open.
    dialog.invoke_ok();
    assert_eq!(names(&ctx.crm), ["Coca Cola", "Fanta"]);
    assert!(dialogs::open_product_dialog().is_some(), "still open");
    client.invoke_ok();
    let no_clients = |crm: &Crm| {
        crm.list_owners(OwnerKind::Client, "", Page::ALL)
            .unwrap()
            .is_empty()
    };
    assert!(no_clients(&ctx.crm), "no client saved");
    client.invoke_cancel();
    // An import cannot start either.
    let flow = catalogue::transfer::open_import(&ctx, clients.window(), ListKind::Clients)
        .expect("import dialog");
    flow.start_with(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../io/tests/fixtures/KLIENTET.csv"),
    );
    assert!(!flow.dialog().unwrap().get_busy(), "not started");
    assert!(no_clients(&ctx.crm));
    flow.modal().close();

    release_tx.send(()).unwrap();
    slint::run_event_loop_until_quit().unwrap();

    assert!(!ctx.is_busy());
    assert!(!list.get_busy());
    assert!(!clist.get_busy());
    assert!(!clist.get_input_blocked());
    assert!(list.get_input_blocked(), "the dialog still blocks its list");
    // The same OK saves now.
    dialog.invoke_ok();
    assert_eq!(names(&ctx.crm), ["Coca Cola", "Fanta", "Ujë"]);
    assert!(dialogs::open_product_dialog().is_none());
    assert!(!list.get_input_blocked());

    // A delete confirmed while a job runs deletes nothing.
    let current = names(&ctx.crm);
    list.set_current_row(0);
    ctrl.remove();
    let confirm = dialogs::open_confirm().expect("delete confirmation");
    let (release_tx, release_rx) = mpsc::channel::<()>();
    ctx.run_bg(
        move |_crm| release_rx.recv().unwrap(),
        |()| slint::quit_event_loop().unwrap(),
    );
    confirm.invoke_answered(true);
    assert_eq!(names(&ctx.crm), current, "nothing deleted");
    release_tx.send(()).unwrap();
    slint::run_event_loop_until_quit().unwrap();

    // After the job a delete works.
    list.set_current_row(0);
    ctrl.remove();
    dialogs::open_confirm()
        .expect("delete confirmation")
        .invoke_answered(true);
    assert_eq!(names(&ctx.crm).len(), 2, "deleted");
}

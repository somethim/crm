//! Headless tests of the shell (i-slint-backend-testing): the lock screen
//! (password, recovery key, wipe code), the password change, menu and shortcut
//! routing to the feature bridges, toolbar states, status bar, modal dialogs,
//! settings and the lock hiding windows.

use crm_services::{Crm, DataPaths, DbKey, KdfParams, StartState, Unlock, Vault, messages};
use crm_ui::shell::{modal, password, settings, texts, timers, worker};
use crm_ui::{
    AboutDialog, AppWindow, CatalogueBridge, ChangePasswordDialog, ClientsWindow, Commands,
    ConfirmDialog, DateParse, ExportDialog, FatalWindow, ImportDialog, MessageDialog, OrdersBridge,
    OwnerDialog, PrintPreview, ProductDialog, ProductsWindow, ReportDialog, ReportsBridge,
    RestoreDialog, SettingsDialog, ShellBridge, SuppliersWindow, Theme, UiContext, UnlockWindow,
};
use i_slint_backend_testing::ElementHandle;
use slint::platform::{Key, WindowEvent};
use slint::{ComponentHandle, SharedString};
use std::cell::RefCell;
use std::rc::Rc;

const PASSWORD: &str = "fjalëkalim";

struct Harness {
    app: AppWindow,
    ctx: Rc<UiContext>,
    key: DbKey,
    vault: Vault,
    recovery: String,
    wipe: String,
    dir: tempfile::TempDir,
}

/// The main window over a fresh encrypted database, as the gate leaves it:
/// `data/crm.key` committed (password [`PASSWORD`]) and unlocked.
fn harness() -> Harness {
    i_slint_backend_testing::init_no_event_loop();
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    paths.ensure_dirs().unwrap();
    let vault = Vault::with_params(paths.clone(), KdfParams::TEST);
    let pending = vault.setup(PASSWORD).unwrap();
    let recovery = pending.recovery_display().to_string();
    let wipe = pending.wipe_display().to_string();
    let key = pending.commit().unwrap();
    let crm = Crm::open(paths.clone(), &key).unwrap();
    let (app, ctx) = crm_ui::build(crm, vault.clone(), paths).unwrap();
    app.show().unwrap();
    Harness {
        app,
        ctx,
        key,
        vault,
        recovery,
        wipe,
        dir,
    }
}

/// The main window starts unlocked: the gate ran before it.
fn unlocked() -> Harness {
    let h = harness();
    assert!(!h.app.get_locked());
    h
}

fn element(root: &impl i_slint_backend_testing::ElementRoot, id: &str) -> ElementHandle {
    ElementHandle::find_by_element_id(root, id)
        .next()
        .unwrap_or_else(|| panic!("element {id} not found"))
}

fn exists(root: &impl i_slint_backend_testing::ElementRoot, id: &str) -> bool {
    ElementHandle::find_by_element_id(root, id).next().is_some()
}

fn type_into(root: &impl i_slint_backend_testing::ElementRoot, id: &str, text: &str) {
    element(root, id).set_accessible_value(text);
}

fn click(root: &impl i_slint_backend_testing::ElementRoot, id: &str) {
    element(root, id).invoke_accessible_default_action();
}

fn key(app: &AppWindow, text: impl Into<SharedString>, pressed: bool) {
    let text = text.into();
    app.window().dispatch_event(if pressed {
        WindowEvent::KeyPressed { text }
    } else {
        WindowEvent::KeyReleased { text }
    });
}

/// Ctrl (+ Shift) + `letter`.
fn shortcut(app: &AppWindow, shift: bool, letter: &str) {
    key(app, Key::Control, true);
    if shift {
        key(app, Key::Shift, true);
    }
    key(app, letter, true);
    key(app, letter, false);
    if shift {
        key(app, Key::Shift, false);
    }
    key(app, Key::Control, false);
}

/// Alt + `k`.
fn alt(app: &AppWindow, k: Key) {
    key(app, Key::Alt, true);
    key(app, k, true);
    key(app, k, false);
    key(app, Key::Alt, false);
}

/// Records the calls of every stub bridge callback as strings.
fn record_bridges(app: &AppWindow) -> Rc<RefCell<Vec<String>>> {
    let log = Rc::new(RefCell::new(Vec::new()));
    let push = |log: &Rc<RefCell<Vec<String>>>| {
        let log = log.clone();
        move |s: String| log.borrow_mut().push(s)
    };
    let orders = app.global::<OrdersBridge>();
    let p = push(&log);
    orders.on_new_document(move |k| p(format!("new-document {k}")));
    let p = push(&log);
    orders.on_save_current(move || p("save".into()));
    let p = push(&log);
    orders.on_print_current(move || p("print".into()));
    let p = push(&log);
    orders.on_delete_current(move || p("delete".into()));
    let p = push(&log);
    orders.on_close_current(move || p("close".into()));
    let p = push(&log);
    orders.on_refresh(move || p("refresh".into()));
    let p = push(&log);
    orders.on_flush_drafts(move || p("flush-drafts".into()));
    let p = push(&log);
    orders.on_focus_search(move || p("focus-search".into()));
    let p = push(&log);
    orders.on_close_all_tabs(move || p("close-all".into()));
    let p = push(&log);
    orders.on_select_tab(move |i| p(format!("select-tab {i}")));
    let catalogue = app.global::<CatalogueBridge>();
    let p = push(&log);
    catalogue.on_open_list(move |k| p(format!("open-list {k}")));
    let p = push(&log);
    catalogue.on_import(move |k| p(format!("import {k}")));
    let p = push(&log);
    catalogue.on_export(move |k| p(format!("export {k}")));
    let p = push(&log);
    catalogue.on_create(move |k| p(format!("create {k}")));
    let p = push(&log);
    app.global::<ReportsBridge>()
        .on_open_dialog(move || p("report".into()));
    log
}

#[test]
fn the_main_window_starts_unlocked() {
    let h = harness();
    assert!(!h.app.get_locked());
    assert!(!exists(&h.app, "AppWindow::lock"));
}

/// Types `entry` into the lock screen and presses "Hap" (jobs run inline).
fn lock_entry(h: &Harness, entry: &str) {
    type_into(&h.app, "PasswordScreen::first", entry);
    click(&h.app, "PasswordScreen::ok");
}

#[test]
fn lock_shows_the_overlay_and_a_wrong_password_shows_the_error() {
    let h = unlocked();
    worker::set_inline_for_tests(true);
    let shell = h.app.global::<ShellBridge>();

    shortcut(&h.app, false, "l");
    assert!(h.app.get_locked());
    assert!(exists(&h.app, "AppWindow::lock"));
    assert_eq!(shell.get_lock_mode(), "unlock");

    lock_entry(&h, "gabim-gabim");
    assert!(h.app.get_locked());
    assert_eq!(shell.get_lock_error(), messages::PASSWORD_WRONG);
    assert!(!shell.get_lock_busy());
    assert_eq!(
        element(&h.app, "PasswordScreen::error")
            .accessible_label()
            .as_deref(),
        Some("Fjalëkalim i gabuar")
    );

    lock_entry(&h, PASSWORD);
    assert!(!h.app.get_locked());
    assert_eq!(shell.get_lock_error(), "");
}

#[test]
fn five_wrong_passwords_start_the_lockout() {
    let h = unlocked();
    worker::set_inline_for_tests(true);
    h.ctx.lock();
    let shell = h.app.global::<ShellBridge>();
    for _ in 0..4 {
        shell.invoke_lock_submit_entry("gabim-gabim".into());
        assert_eq!(shell.get_lock_error(), "Fjalëkalim i gabuar");
    }
    shell.invoke_lock_submit_entry("gabim-gabim".into());
    assert_eq!(shell.get_lock_error(), "Provoni përsëri pas 30 sekondash");
    // Even the right password waits during the lockout.
    shell.invoke_lock_submit_entry(PASSWORD.into());
    assert!(h.app.get_locked());
    assert!(shell.get_lock_error().starts_with("Provoni përsëri pas "));
}

#[test]
fn the_recovery_key_at_the_lock_screen_needs_a_new_password() {
    let h = unlocked();
    worker::set_inline_for_tests(true);
    h.ctx.lock();
    let shell = h.app.global::<ShellBridge>();
    lock_entry(&h, &h.recovery.to_lowercase());
    assert!(h.app.get_locked());
    assert_eq!(shell.get_lock_mode(), "new-password");

    type_into(&h.app, "PasswordScreen::first", "i-ri-fjalëkalim");
    type_into(&h.app, "PasswordScreen::second", "i-ri-fjalëkalin");
    click(&h.app, "PasswordScreen::ok");
    assert_eq!(shell.get_lock_error(), texts::PASSWORD_MISMATCH);
    type_into(&h.app, "PasswordScreen::first", "12345");
    type_into(&h.app, "PasswordScreen::second", "12345");
    click(&h.app, "PasswordScreen::ok");
    assert_eq!(shell.get_lock_error(), messages::PASSWORD_TOO_SHORT);

    type_into(&h.app, "PasswordScreen::first", "i-ri-fjalëkalim");
    type_into(&h.app, "PasswordScreen::second", "i-ri-fjalëkalim");
    click(&h.app, "PasswordScreen::ok");
    assert!(!h.app.get_locked(), "{}", shell.get_lock_error());
    assert_eq!(shell.get_flash_text(), texts::PASSWORD_CHANGED);
    assert!(matches!(h.vault.submit(PASSWORD).unwrap(), Unlock::Wrong));
    assert!(matches!(
        h.vault.submit("i-ri-fjalëkalim").unwrap(),
        Unlock::Unlocked(_)
    ));
}

/// Every file under `root` with its size.
fn files(root: &std::path::Path) -> Vec<(std::path::PathBuf, u64)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                stack.push(entry.path());
            } else {
                out.push((entry.path(), entry.metadata().unwrap().len()));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn a_mistyped_wipe_code_at_the_lock_screen_changes_nothing() {
    let h = unlocked();
    worker::set_inline_for_tests(true);
    h.ctx.crm.backup_now().unwrap();
    h.ctx.lock();
    let before = files(h.dir.path());
    let mut typo: Vec<char> = h.wipe.chars().collect();
    typo[0] = if typo[0] == '7' { '8' } else { '7' };
    lock_entry(&h, &typo.into_iter().collect::<String>());
    assert_eq!(
        h.app.global::<ShellBridge>().get_lock_error(),
        "Fjalëkalim i gabuar"
    );
    assert_eq!(files(h.dir.path()), before);
    assert!(!h.ctx.crm.is_closed());
}

#[test]
fn the_wipe_code_at_the_lock_screen_wipes_without_writing_drafts() {
    let h = unlocked();
    worker::set_inline_for_tests(true);
    let log = record_bridges(&h.app);
    h.ctx.crm.backup_now().unwrap();
    std::fs::write(h.ctx.paths.logs.join("crm.log"), b"line").unwrap();
    let list = ProductsWindow::new().unwrap();
    h.ctx.register_window(Box::new(list.as_weak()));
    list.show().unwrap();
    h.ctx.lock();
    assert!(!list.window().is_visible());

    lock_entry(&h, &h.wipe);
    assert_eq!(h.ctx.outcome(), crm_ui::RunOutcome::Wiped);
    assert!(!log.borrow().iter().any(|c| c == "flush-drafts"), "{log:?}");
    assert!(!h.app.window().is_visible());
    assert!(!list.window().is_visible());
    assert!(h.ctx.crm.is_closed());
    let left: Vec<_> = files(h.dir.path());
    assert!(left.is_empty(), "{left:?}");
    for dir in ["data", "backups", "logs"] {
        assert!(h.dir.path().join(dir).is_dir(), "{dir}");
    }
    assert_eq!(h.vault.probe().unwrap(), StartState::FirstRun);
}

#[test]
fn menu_commands_and_shortcuts_reach_the_stub_bridges() {
    let h = unlocked();
    let log = record_bridges(&h.app);
    let orders = h.app.global::<OrdersBridge>();
    orders.set_has_document(true);
    orders.set_current_is_saved(true);

    let commands = h.app.global::<Commands>();
    commands.invoke_new_sale();
    commands.invoke_new_purchase();
    commands.invoke_save();
    commands.invoke_print();
    commands.invoke_delete();
    commands.invoke_close_tab();
    commands.invoke_refresh();
    for kind in 0..3 {
        commands.invoke_open_list(kind);
        commands.invoke_import(kind);
        commands.invoke_export(kind);
    }
    commands.invoke_report();
    assert_eq!(
        *log.borrow(),
        [
            "new-document 0",
            "new-document 1",
            "save",
            "print",
            "delete",
            "close",
            "refresh",
            "open-list 0",
            "import 0",
            "export 0",
            "open-list 1",
            "import 1",
            "export 1",
            "open-list 2",
            "import 2",
            "export 2",
            "report",
        ]
    );

    log.borrow_mut().clear();
    shortcut(&h.app, false, "n");
    shortcut(&h.app, true, "N");
    shortcut(&h.app, false, "s");
    shortcut(&h.app, false, "p");
    shortcut(&h.app, false, "w");
    key(&h.app, Key::F5, true);
    key(&h.app, Key::F5, false);
    assert_eq!(
        *log.borrow(),
        [
            "new-document 0",
            "new-document 1",
            "save",
            "print",
            "close",
            "refresh"
        ]
    );

    // The Bilanc shortcuts (task 018).
    log.borrow_mut().clear();
    shortcut(&h.app, false, "r");
    shortcut(&h.app, false, "k");
    shortcut(&h.app, false, "f");
    shortcut(&h.app, false, "o");
    // Ctrl+F4 is caught by a FocusScope around the window's content, so it needs
    // the focus inside (an open order always has it); the orders tests cover it.
    // No tabs yet: Ctrl+Shift+F4 and Alt+arrows do nothing.
    shortcut(&h.app, true, &SharedString::from(Key::F4));
    alt(&h.app, Key::RightArrow);
    let titles = [
        ("Porosi Shitje (e re)", false),
        ("Porosi Blerje (e re)", false),
        ("Porosi Shitje (e re)", false),
    ];
    let tabs: Vec<crm_ui::TabItem> = titles
        .iter()
        .map(|(t, d)| crm_ui::TabItem {
            title: (*t).into(),
            dirty: *d,
        })
        .collect();
    orders.set_tabs(std::rc::Rc::new(slint::VecModel::from(tabs)).into());
    orders.set_active_tab(2);
    alt(&h.app, Key::RightArrow);
    alt(&h.app, Key::LeftArrow);
    shortcut(&h.app, true, &SharedString::from(Key::F4));
    assert_eq!(
        *log.borrow(),
        [
            "create 0",
            "create 1",
            "create 2",
            "focus-search",
            "select-tab 0",
            "select-tab 1",
            "close-all"
        ]
    );

    // Toolbar buttons: only the order commands; the lists and the report are in the menu.
    log.borrow_mut().clear();
    click(&h.app, "Toolbar::new-sale");
    click(&h.app, "Toolbar::new-purchase");
    assert_eq!(*log.borrow(), ["new-document 0", "new-document 1"]);
    let label = |id: &str| element(&h.app, id).accessible_label().unwrap_or_default();
    assert_eq!(label("Toolbar::new-sale"), "+ Porosi Shitje");
    assert_eq!(label("Toolbar::new-purchase"), "+ Porosi Blerje");
    for gone in [
        "Toolbar::products",
        "Toolbar::clients",
        "Toolbar::suppliers",
        "Toolbar::report",
    ] {
        assert!(!exists(&h.app, gone), "{gone} is menu-only");
    }
    // Ruaj, Printo and Fshi moved to the bottom of the open order.
    for gone in ["Toolbar::save", "Toolbar::print", "Toolbar::delete"] {
        assert!(!exists(&h.app, gone), "{gone} is in the order footer");
    }
}

#[test]
fn the_empty_workspace_offers_the_two_new_order_buttons() {
    let h = unlocked();
    let log = record_bridges(&h.app);
    let label = |id: &str| element(&h.app, id).accessible_label().unwrap_or_default();
    assert_eq!(label("OrdersWorkspace::empty-new-sale"), "+ Porosi Shitje");
    assert_eq!(
        label("OrdersWorkspace::empty-new-purchase"),
        "+ Porosi Blerje"
    );
    click(&h.app, "OrdersWorkspace::empty-new-sale");
    click(&h.app, "OrdersWorkspace::empty-new-purchase");
    assert_eq!(*log.borrow(), ["new-document 0", "new-document 1"]);

    // They follow the same guards as the menu commands.
    log.borrow_mut().clear();
    h.app.set_input_blocked(true);
    click(&h.app, "OrdersWorkspace::empty-new-sale");
    assert!(log.borrow().is_empty());
    h.app.set_input_blocked(false);

    // An open document replaces them.
    h.app.global::<OrdersBridge>().set_has_document(true);
    assert!(!exists(&h.app, "OrdersWorkspace::empty-new-sale"));
    assert!(!exists(&h.app, "OrdersWorkspace::empty-new-purchase"));
}

#[test]
fn commands_are_ignored_while_locked_or_blocked() {
    let h = unlocked();
    let log = record_bridges(&h.app);
    h.ctx.lock();
    shortcut(&h.app, false, "n");
    shortcut(&h.app, false, "k");
    shortcut(&h.app, false, "o");
    h.app.global::<Commands>().invoke_open_list(0);
    h.app.global::<Commands>().invoke_new_product();
    h.app.global::<Commands>().invoke_find_order();
    assert!(log.borrow().is_empty());

    h.ctx.unlock();
    h.app.set_input_blocked(true);
    shortcut(&h.app, false, "n");
    h.app.global::<Commands>().invoke_report();
    assert!(log.borrow().is_empty());

    h.app.set_input_blocked(false);
    shortcut(&h.app, false, "n");
    assert_eq!(*log.borrow(), ["new-document 0"]);
}

#[test]
fn save_print_delete_follow_the_orders_bridge() {
    let h = unlocked();
    let log = record_bridges(&h.app);
    let orders = h.app.global::<OrdersBridge>();
    let enabled = |id: &str| element(&h.app, id).accessible_enabled();

    // No open order: no footer buttons, and the shortcut does nothing.
    for id in [
        "DocumentView::save",
        "DocumentView::print",
        "DocumentView::delete",
    ] {
        assert!(!exists(&h.app, id), "{id} without a document");
    }
    shortcut(&h.app, false, "s");
    assert!(log.borrow().is_empty(), "save without a document");

    orders.set_has_document(true);
    // The first walk instantiates the `if` element.
    let _ = exists(&h.app, "DocumentView::save");
    assert_eq!(enabled("DocumentView::save"), Some(true));
    assert_eq!(enabled("DocumentView::print"), Some(false));
    assert_eq!(enabled("DocumentView::delete"), Some(false));
    let label = |id: &str| element(&h.app, id).accessible_label().unwrap_or_default();
    assert_eq!(label("DocumentView::save"), "Ruaj");
    assert_eq!(label("DocumentView::print"), "Printo");
    assert_eq!(label("DocumentView::delete"), "Fshi");
    shortcut(&h.app, false, "p");
    click(&h.app, "DocumentView::print");
    click(&h.app, "DocumentView::delete");
    assert!(
        log.borrow().is_empty(),
        "print or delete of an unsaved document"
    );
    click(&h.app, "DocumentView::save");
    assert_eq!(*log.borrow(), ["save"]);

    log.borrow_mut().clear();
    orders.set_current_is_saved(true);
    assert_eq!(enabled("DocumentView::print"), Some(true));
    assert_eq!(enabled("DocumentView::delete"), Some(true));
    click(&h.app, "DocumentView::print");
    click(&h.app, "DocumentView::delete");
    assert_eq!(*log.borrow(), ["print", "delete"]);

    // The footer follows the same guards as the menu commands.
    log.borrow_mut().clear();
    h.app.set_input_blocked(true);
    click(&h.app, "DocumentView::save");
    assert!(log.borrow().is_empty());
}

#[test]
fn status_bar_shows_only_the_last_backup_and_flash() {
    let h = unlocked();
    let shell = h.app.global::<ShellBridge>();
    let label = |id: &str| element(&h.app, id).accessible_label().unwrap_or_default();

    // No data path, journal mode or tab count.
    assert!(!exists(&h.app, "StatusBar::data-path"));
    assert!(!exists(&h.app, "StatusBar::tabs"));
    // Crm::open took the daily backup.
    assert!(
        label("StatusBar::last-backup").starts_with("Rezervimi i fundit: "),
        "{}",
        label("StatusBar::last-backup")
    );

    shell.invoke_flash("Porosia u krijua.".into());
    assert_eq!(label("StatusBar::flash"), "Porosia u krijua.");
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(5_100));
    assert_eq!(shell.get_flash_text(), "");
}

#[test]
fn confirm_dialog_blocks_the_main_window_until_answered() {
    let h = unlocked();
    let answer = Rc::new(RefCell::new(None));
    let a = answer.clone();
    let dialog = modal::confirm(&h.ctx, "Konfirmo", "Vazhdo?", "Fshi", move |ok| {
        *a.borrow_mut() = Some(ok)
    })
    .unwrap();
    assert!(h.app.get_input_blocked());
    assert!(dialog.window().is_visible());
    assert_eq!(
        element(&dialog, "ConfirmDialog::ok")
            .accessible_label()
            .as_deref(),
        Some("Fshi")
    );
    click(&dialog, "ConfirmDialog::ok");
    assert_eq!(*answer.borrow(), Some(true));
    assert!(!h.app.get_input_blocked());
    assert!(!dialog.window().is_visible());

    let a = answer.clone();
    let dialog = modal::confirm(&h.ctx, "Konfirmo", "Vazhdo?", "OK", move |ok| {
        *a.borrow_mut() = Some(ok)
    })
    .unwrap();
    click(&dialog, "ConfirmDialog::cancel");
    assert_eq!(*answer.borrow(), Some(false));
    assert!(!h.app.get_input_blocked());
}

#[test]
fn settings_dialog_validates_and_persists() {
    let h = unlocked();
    let dialog = settings::open_dialog(&h.ctx).unwrap();
    assert_eq!(dialog.get_idle_minutes(), "0");
    assert_eq!(dialog.get_receipt_width(), "72");
    assert_eq!(dialog.get_receipt_font(), "9");

    dialog.set_receipt_width("30".into());
    click(&dialog, "SettingsDialog::ok-button");
    assert!(
        dialog.get_error().contains("48 deri në 110"),
        "{}",
        dialog.get_error()
    );
    assert!(dialog.window().is_visible());

    dialog.set_idle_minutes("5".into());
    dialog.set_receipt_width("80".into());
    dialog.set_receipt_font("10".into());
    click(&dialog, "SettingsDialog::ok-button");
    assert_eq!(dialog.get_error(), "");
    assert!(!dialog.window().is_visible());
    assert!(!h.app.get_input_blocked());
    assert_eq!(h.ctx.crm.idle_lock_minutes().unwrap(), 5);
    let receipt = h.ctx.crm.receipt_settings().unwrap();
    assert_eq!((receipt.width_mm, receipt.font_pt), (80, 10));
}

/// Whether `brush` is a light colour (relative luminance above 0.5).
fn is_light(brush: slint::Brush) -> bool {
    let c = brush.color();
    let channel = |v: u8| f32::from(v) / 255.0;
    let luminance =
        0.2126 * channel(c.red()) + 0.7152 * channel(c.green()) + 0.0722 * channel(c.blue());
    luminance > 0.5
}

#[test]
fn the_settings_dialog_has_no_pamja() {
    let h = unlocked();
    let dialog = settings::open_dialog(&h.ctx).unwrap();
    assert!(
        ElementHandle::find_by_accessible_label(&dialog, "Pamja")
            .next()
            .is_none()
    );
    assert!(
        ElementHandle::find_by_accessible_label(&dialog, "Dosja e të dhënave")
            .next()
            .is_some(),
        "the label search works"
    );
}

#[test]
fn a_stored_dark_appearance_is_ignored() {
    i_slint_backend_testing::init_no_event_loop();
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    paths.ensure_dirs().unwrap();
    let key = DbKey::for_tests(1);
    let db = crm_storage::Db::open(&paths, &key).unwrap();
    db.write(|tx| crm_storage::repo::settings::set(tx, "appearance", "dark"))
        .unwrap();
    drop(db);
    let crm = Crm::open(paths.clone(), &key).unwrap();
    let vault = Vault::with_params(paths.clone(), KdfParams::TEST);
    let (app, ctx) = crm_ui::build(crm, vault, paths).unwrap();
    assert!(is_light(app.global::<Theme>().get_chrome_background()));
    let list = ProductsWindow::new().unwrap();
    ctx.register_window(Box::new(list.as_weak()));
    assert!(is_light(list.global::<Theme>().get_chrome_background()));
}

#[test]
fn every_window_type_is_light() {
    i_slint_backend_testing::init_no_event_loop();
    macro_rules! check {
        ($($ty:ident),* $(,)?) => {
            $(
                let w = $ty::new().unwrap();
                assert!(is_light(w.global::<Theme>().get_chrome_background()), stringify!($ty));
            )*
        };
    }
    check!(
        AppWindow,
        ProductsWindow,
        ClientsWindow,
        SuppliersWindow,
        ProductDialog,
        OwnerDialog,
        ImportDialog,
        ExportDialog,
        ReportDialog,
        PrintPreview,
        MessageDialog,
        ConfirmDialog,
        RestoreDialog,
        SettingsDialog,
        AboutDialog,
        ChangePasswordDialog,
        FatalWindow,
        UnlockWindow,
    );
}

#[test]
fn change_password_needs_the_current_password() {
    let h = unlocked();
    worker::set_inline_for_tests(true);
    let dialog = password::open_change_password_dialog(&h.ctx).unwrap();
    dialog.invoke_submit("gabim-gabim".into(), "tjetër-1".into(), "tjetër-1".into());
    assert_eq!(dialog.get_error(), "Fjalëkalim i gabuar");
    dialog.invoke_submit(PASSWORD.into(), "tjetër-1".into(), "tjetër-2".into());
    assert_eq!(dialog.get_error(), texts::PASSWORD_MISMATCH);
    dialog.invoke_submit(PASSWORD.into(), "12345".into(), "12345".into());
    assert_eq!(dialog.get_error(), messages::PASSWORD_TOO_SHORT);
    // The recovery key is no current password here.
    dialog.invoke_submit(
        h.recovery.clone().into(),
        "tjetër-1".into(),
        "tjetër-1".into(),
    );
    assert_eq!(dialog.get_error(), "Fjalëkalim i gabuar");
    dialog.invoke_submit(PASSWORD.into(), "tjetër-1".into(), "tjetër-1".into());
    assert!(!dialog.window().is_visible(), "{}", dialog.get_error());
    assert_eq!(
        h.app.global::<ShellBridge>().get_flash_text(),
        texts::PASSWORD_CHANGED
    );
    assert!(matches!(h.vault.submit(PASSWORD).unwrap(), Unlock::Wrong));
    assert!(matches!(
        h.vault.submit("tjetër-1").unwrap(),
        Unlock::Unlocked(_)
    ));
}

#[test]
fn lock_hides_secondary_windows_and_unlock_restores_them() {
    let h = unlocked();
    let list = ProductsWindow::new().unwrap();
    list.show().unwrap();
    let hidden = ProductsWindow::new().unwrap();
    h.ctx.register_window(Box::new(list.as_weak()));
    h.ctx.register_window(Box::new(hidden.as_weak()));

    h.ctx.lock();
    assert!(!list.window().is_visible());
    h.ctx.unlock();
    assert!(list.window().is_visible());
    assert!(
        !hidden.window().is_visible(),
        "a window hidden before the lock stays hidden"
    );
}

#[test]
fn exit_flushes_the_drafts() {
    let h = unlocked();
    let log = record_bridges(&h.app);
    h.app.global::<Commands>().invoke_exit();
    assert_eq!(*log.borrow(), ["flush-drafts"]);
    assert!(!h.app.window().is_visible());
}

#[test]
fn data_changes_reach_the_subscribers() {
    let h = unlocked();
    let calls = Rc::new(RefCell::new(0));
    let c = calls.clone();
    h.ctx.on_data_changed(move || *c.borrow_mut() += 1);
    timers::poll(&h.ctx);
    let before = *calls.borrow();
    // A commit through another handle (another process or tool); this
    // facade's own commits do not count (F-001).
    h.ctx.crm.set_idle_lock_minutes(2).unwrap();
    timers::poll(&h.ctx);
    assert_eq!(
        *calls.borrow(),
        before,
        "own commits are not external changes"
    );
    let other = Crm::open(h.ctx.crm.paths().clone(), &h.key).unwrap();
    other.set_idle_lock_minutes(3).unwrap();
    timers::poll(&h.ctx);
    assert_eq!(*calls.borrow(), before + 1);
    timers::poll(&h.ctx);
    assert_eq!(*calls.borrow(), before + 1);
}

#[test]
fn product_editor_hook_flashes_for_a_deleted_product() {
    let h = unlocked();
    // The harness installs the catalogue editor, so a missing product hits
    // its "deleted" branch, not the shell's no-editor fallback.
    h.ctx
        .open_product_editor(crm_services::domain::ProductId(1), Box::new(|| {}));
    assert_eq!(
        h.app.global::<ShellBridge>().get_flash_text(),
        crm_ui::catalogue::texts::PRODUCT_DELETED
    );
    let opened = Rc::new(RefCell::new(None));
    let o = opened.clone();
    h.ctx
        .set_product_editor(Box::new(move |_, id, _| *o.borrow_mut() = Some(id)));
    h.ctx
        .open_product_editor(crm_services::domain::ProductId(7), Box::new(|| {}));
    assert_eq!(*opened.borrow(), Some(crm_services::domain::ProductId(7)));
}

#[test]
fn date_parse_global_formats_and_steps() {
    let h = harness();
    let parse = h.app.global::<DateParse>();
    let d = parse.invoke_parse("1.2.2026".into());
    assert!(d.ok);
    assert_eq!(
        (d.iso.as_str(), d.display.as_str()),
        ("2026-02-01", "01.02.2026")
    );
    let d = parse.invoke_shift("28.02.2026".into(), 1);
    assert_eq!(d.display, "01.03.2026");
    let d = parse.invoke_shift("01.01.2026".into(), -1);
    assert_eq!(d.display, "31.12.2025");
    assert!(!parse.invoke_parse("32.01.2026".into()).ok);
}

#[test]
fn settings_values_parse_ranges() {
    let v = settings::parse_values(" 15 ", "72", "9").unwrap();
    assert_eq!(v.idle_minutes, 15);
    assert!(settings::parse_values("x", "72", "9").is_err());
    assert!(settings::parse_values("121", "72", "9").is_err());
    assert!(settings::parse_values("0", "72", "15").is_err());
    assert!(password::check_pair("fjalëkalim", "fjalëkalim").is_ok());
    assert_eq!(
        password::check_pair("fjalëkalim", "fjalëkalin"),
        Err(texts::PASSWORD_MISMATCH.to_owned())
    );
    assert!(password::check_pair("12345", "12345").is_err());
}

#[test]
fn the_line_grid_scroll_bar_is_always_shown_and_scrolls() {
    use slint::platform::PointerEventButton;
    use slint::{LogicalPosition, ModelRc, VecModel};
    let h = unlocked();
    let orders = h.app.global::<OrdersBridge>();
    h.app
        .window()
        .set_size(slint::LogicalSize::new(1280.0, 800.0));
    orders.set_has_document(true);
    let _ = exists(&h.app, "LineGrid::bar");

    // Few rows: the track shows, the thumb does not.
    let bar = element(&h.app, "LineGrid::bar");
    assert_eq!(bar.size().width, 14.0);
    assert!(bar.size().height > 100.0);
    let thumb = || element(&h.app, "VScrollBar::thumb");
    assert_eq!(thumb().size().height, 0.0);

    let rows: Vec<crm_ui::GridRow> = (1..=60)
        .map(|n| crm_ui::GridRow {
            number: n.to_string().into(),
            ..Default::default()
        })
        .collect();
    orders.set_grid_rows(ModelRc::new(VecModel::from(rows)));
    let _ = exists(&h.app, "LineGrid::bar");
    let top = thumb().absolute_position().y;
    let height = thumb().size().height;
    assert!(height >= 24.0 && height < bar.size().height, "{height}");
    assert_eq!(top, bar.absolute_position().y + 2.0, "starts at the top");

    // A click on the track under the thumb moves one page down.
    let pos = bar.absolute_position();
    let at = LogicalPosition::new(pos.x + 7.0, pos.y + bar.size().height - 10.0);
    let window = h.app.window();
    window.dispatch_event(WindowEvent::PointerMoved { position: at });
    window.dispatch_event(WindowEvent::PointerPressed {
        position: at,
        button: PointerEventButton::Left,
    });
    window.dispatch_event(WindowEvent::PointerReleased {
        position: at,
        button: PointerEventButton::Left,
    });
    let paged = thumb().absolute_position().y;
    assert!(paged > top, "{paged} > {top}");

    // The wheel over the bar scrolls back up.
    window.dispatch_event(WindowEvent::PointerScrolled {
        position: at,
        delta_x: 0.0,
        delta_y: 10_000.0,
    });
    assert_eq!(thumb().absolute_position().y, top);

    // Dragging the thumb moves it with the pointer.
    let grab = LogicalPosition::new(pos.x + 7.0, top + height / 2.0);
    let to = LogicalPosition::new(grab.x, grab.y + 50.0);
    window.dispatch_event(WindowEvent::PointerMoved { position: grab });
    window.dispatch_event(WindowEvent::PointerPressed {
        position: grab,
        button: PointerEventButton::Left,
    });
    window.dispatch_event(WindowEvent::PointerMoved { position: to });
    window.dispatch_event(WindowEvent::PointerReleased {
        position: to,
        button: PointerEventButton::Left,
    });
    let dragged = thumb().absolute_position().y - top;
    assert!((dragged - 50.0).abs() < 1.0, "{dragged}");
}

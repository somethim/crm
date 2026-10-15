//! Window sizes (headless): a tiling window manager may hand a window any size, so every
//! dialog keeps its fields and buttons at their own height in a much larger window, and the
//! list windows and the print preview give the extra height to their table or page only.

use crm_ui::{
    AboutDialog, ChangePasswordDialog, ClientsWindow, ConfirmDialog, FatalWindow, ImportDialog,
    MessageDialog, OwnerDialog, PrintPreview, ProductDialog, ProductsWindow, ReportDialog,
    RestoreDialog, SettingsDialog, SuppliersWindow,
};
use i_slint_backend_testing::{ElementHandle, ElementRoot};
use slint::{ComponentHandle, LogicalSize};

/// The tallest a one-line field or a button may be.
const MAX_ROW: f32 = 40.0;
/// The shortest a one-line text field may be (a squashed one is about 20px).
const MIN_ROW: f32 = 28.0;

fn heights(root: &impl ElementRoot, type_name: &str) -> Vec<f32> {
    ElementHandle::find_by_element_type_name(root, type_name)
        .map(|e| e.size().height)
        .collect()
}

/// Shows `component` at 1600 x 1000 and checks every LineEdit and DialogButton in it:
/// no row stretches, and no text field is squashed below `MIN_ROW`.
fn assert_rows_hold<C: ComponentHandle + ElementRoot>(name: &str, component: &C) -> usize {
    component.show().unwrap();
    component
        .window()
        .set_size(LogicalSize::new(1600.0, 1000.0));
    let edits = heights(component, "LineEdit");
    let buttons = heights(component, "DialogButton");
    assert!(!buttons.is_empty(), "{name}: no DialogButton found");
    for h in &edits {
        assert!(
            (MIN_ROW..=MAX_ROW).contains(h),
            "{name}: a text field is {h}px tall"
        );
    }
    for h in &buttons {
        assert!(*h > 0.0 && *h <= MAX_ROW, "{name}: a button is {h}px tall");
    }
    component.hide().unwrap();
    edits.len()
}

#[test]
fn dialog_fields_and_buttons_do_not_stretch_in_a_huge_window() {
    i_slint_backend_testing::init_no_event_loop();

    let owner = OwnerDialog::new().unwrap();
    assert_eq!(assert_rows_hold("OwnerDialog", &owner), 5);
    // A validation error adds a line; the fields still keep their height.
    owner.set_name_error("Emri është i detyrueshëm.".into());
    assert_eq!(assert_rows_hold("OwnerDialog (gabim)", &owner), 5);

    let product = ProductDialog::new().unwrap();
    product.set_note("Kodi, njësia, çmimet dhe gjendja ndahen me 2 artikuj të tjerë.".into());
    assert_eq!(assert_rows_hold("ProductDialog", &product), 6);

    assert_eq!(
        assert_rows_hold(
            "ChangePasswordDialog",
            &ChangePasswordDialog::new().unwrap()
        ),
        3
    );
    assert_eq!(
        assert_rows_hold("SettingsDialog", &SettingsDialog::new().unwrap()),
        4
    );
    let import = ImportDialog::new().unwrap();
    assert_eq!(assert_rows_hold("ImportDialog", &import), 1);
    import.set_finished(true);
    assert_rows_hold("ImportDialog (rezultati)", &import);

    let confirm = ConfirmDialog::new().unwrap();
    confirm.set_text("Të fshihet artikulli?".into());
    assert_rows_hold("ConfirmDialog", &confirm);
    assert_rows_hold("MessageDialog", &MessageDialog::new().unwrap());
    assert_rows_hold("AboutDialog", &AboutDialog::new().unwrap());
    assert_rows_hold("RestoreDialog", &RestoreDialog::new().unwrap());
    assert_rows_hold("ReportDialog", &ReportDialog::new().unwrap());
    let fatal = FatalWindow::new().unwrap();
    fatal.set_message("Baza e të dhënave nuk u hap.".into());
    fatal.set_offer_restore(true);
    assert_rows_hold("FatalWindow", &fatal);
}

#[test]
fn dialogs_open_at_their_fixed_width() {
    i_slint_backend_testing::init_no_event_loop();
    // The testing backend opens a window at its preferred size, which the dialogs fix to
    // their min = max width (the height follows the content).
    fn width(c: &impl ComponentHandle) -> u32 {
        c.show().unwrap();
        let w = c.window().size().width;
        c.hide().unwrap();
        w
    }
    assert_eq!(width(&OwnerDialog::new().unwrap()), 480);
    assert_eq!(width(&ProductDialog::new().unwrap()), 480);
    assert_eq!(width(&AboutDialog::new().unwrap()), 480);
    assert_eq!(width(&FatalWindow::new().unwrap()), 480);
    assert_eq!(width(&ConfirmDialog::new().unwrap()), 400);
    assert_eq!(width(&MessageDialog::new().unwrap()), 400);
    assert_eq!(width(&ChangePasswordDialog::new().unwrap()), 400);
    assert_eq!(width(&SettingsDialog::new().unwrap()), 560);
    assert_eq!(width(&ImportDialog::new().unwrap()), 560);
    assert_eq!(width(&RestoreDialog::new().unwrap()), 560);
    assert_eq!(width(&ReportDialog::new().unwrap()), 960);
}

/// A resizable list window: the toolbar, the search row and the status line keep their
/// height in a very tall window, and the table takes the rest.
fn assert_only_the_table_grows<C: ComponentHandle + ElementRoot>(name: &str, window: &C) {
    window.show().unwrap();
    window.window().set_size(LogicalSize::new(1600.0, 1600.0));
    for h in heights(window, "LineEdit")
        .iter()
        .chain(&heights(window, "ToolButton"))
    {
        assert!(*h > 0.0 && *h <= MAX_ROW, "{name}: a row is {h}px tall");
    }
    let table = heights(window, "StandardTableView");
    assert_eq!(table.len(), 1, "{name}: one table");
    assert!(
        table[0] > 1400.0,
        "{name}: the table is only {}px",
        table[0]
    );
    window.hide().unwrap();
}

#[test]
fn list_windows_give_the_extra_height_to_the_table() {
    i_slint_backend_testing::init_no_event_loop();
    assert_only_the_table_grows("ProductsWindow", &ProductsWindow::new().unwrap());
    assert_only_the_table_grows("ClientsWindow", &ClientsWindow::new().unwrap());
    assert_only_the_table_grows("SuppliersWindow", &SuppliersWindow::new().unwrap());
}

#[test]
fn the_print_preview_buttons_keep_their_height() {
    i_slint_backend_testing::init_no_event_loop();
    let preview = PrintPreview::new().unwrap();
    preview.show().unwrap();
    preview.window().set_size(LogicalSize::new(1600.0, 1600.0));
    let buttons = heights(&preview, "Button");
    assert!(!buttons.is_empty());
    for h in buttons {
        assert!(
            h > 0.0 && h <= MAX_ROW,
            "PrintPreview: a button is {h}px tall"
        );
    }
    preview.hide().unwrap();
}

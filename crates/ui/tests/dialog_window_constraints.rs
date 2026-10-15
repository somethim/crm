//! The size constraints each window hands the windowing system (the winit backend turns
//! min == max into a non-resizable window with a fixed min/max inner size, which Hyprland
//! floats and Windows shows as a normal dialog). A small platform records them; one test
//! only, as a process can set its Slint platform once.

use crm_ui::{
    AboutDialog, AppWindow, ChangePasswordDialog, ConfirmDialog, ExportDialog, FatalWindow,
    ImportDialog, MessageDialog, OwnerDialog, PrintPreview, ProductDialog, ProductsWindow,
    ReportDialog, RestoreDialog, SettingsDialog,
};
use slint::ComponentHandle;
use slint::platform::software_renderer::SoftwareRenderer;
use slint::platform::{
    LayoutConstraints, Platform, PlatformError, Renderer, WindowAdapter, WindowProperties,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

struct RecordingWindow {
    window: slint::Window,
    renderer: SoftwareRenderer,
    constraints: Cell<Option<LayoutConstraints>>,
}

impl WindowAdapter for RecordingWindow {
    fn window(&self) -> &slint::Window {
        &self.window
    }

    fn size(&self) -> slint::PhysicalSize {
        slint::PhysicalSize::new(800, 600)
    }

    fn renderer(&self) -> &dyn Renderer {
        &self.renderer
    }

    fn update_window_properties(&self, properties: WindowProperties<'_>) {
        self.constraints.set(Some(properties.layout_constraints()));
    }
}

thread_local! {
    static WINDOWS: RefCell<Vec<Rc<RecordingWindow>>> = const { RefCell::new(Vec::new()) };
}

struct RecordingPlatform;

impl Platform for RecordingPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        let adapter = Rc::new_cyclic(|weak: &std::rc::Weak<RecordingWindow>| RecordingWindow {
            window: slint::Window::new(weak.clone()),
            renderer: SoftwareRenderer::new(),
            constraints: Cell::new(None),
        });
        WINDOWS.with(|w| w.borrow_mut().push(adapter.clone()));
        Ok(adapter)
    }
}

/// Shows the component and returns the (min, max) size its window asked for.
fn constraints(c: &impl ComponentHandle) -> (slint::LogicalSize, Option<slint::LogicalSize>) {
    c.show().unwrap();
    let adapter = WINDOWS.with(|w| w.borrow().last().cloned().unwrap());
    let constraints = adapter.constraints.get().expect("no window properties");
    c.hide().unwrap();
    (constraints.min.expect("no minimum size"), constraints.max)
}

#[track_caller]
fn assert_fixed(name: &str, c: &impl ComponentHandle, width: f32) -> f32 {
    let (min, max) = constraints(c);
    let max = max.unwrap_or_else(|| panic!("{name}: no maximum size (resizable)"));
    assert_eq!(min, max, "{name}: min {min:?} != max {max:?}");
    assert_eq!(min.width, width, "{name}: width");
    assert!(min.height > 0.0, "{name}: zero height");
    min.height
}

#[test]
fn dialogs_are_fixed_size_and_list_windows_resizable() {
    slint::platform::set_platform(Box::new(RecordingPlatform)).unwrap();

    let owner = assert_fixed("OwnerDialog", &OwnerDialog::new().unwrap(), 480.0);
    // A validation error makes the fixed height taller instead of squeezing the rows.
    let with_error = OwnerDialog::new().unwrap();
    with_error.set_name_error("Emri është i detyrueshëm.".into());
    assert!(assert_fixed("OwnerDialog (gabim)", &with_error, 480.0) > owner);

    assert_fixed("ProductDialog", &ProductDialog::new().unwrap(), 480.0);
    assert_fixed("AboutDialog", &AboutDialog::new().unwrap(), 480.0);
    assert_fixed("FatalWindow", &FatalWindow::new().unwrap(), 480.0);
    assert_fixed("ConfirmDialog", &ConfirmDialog::new().unwrap(), 400.0);
    assert_fixed("MessageDialog", &MessageDialog::new().unwrap(), 400.0);
    assert_fixed(
        "ChangePasswordDialog",
        &ChangePasswordDialog::new().unwrap(),
        400.0,
    );
    assert_fixed("SettingsDialog", &SettingsDialog::new().unwrap(), 560.0);
    assert_eq!(
        assert_fixed("RestoreDialog", &RestoreDialog::new().unwrap(), 560.0),
        400.0
    );
    assert_eq!(
        assert_fixed("ReportDialog", &ReportDialog::new().unwrap(), 960.0),
        600.0
    );
    assert_eq!(
        assert_fixed("ExportDialog", &ExportDialog::new().unwrap(), 320.0),
        120.0
    );
    let import = ImportDialog::new().unwrap();
    assert_eq!(assert_fixed("ImportDialog", &import, 560.0), 200.0);
    let import = ImportDialog::new().unwrap();
    import.set_finished(true);
    assert_eq!(
        assert_fixed("ImportDialog (rezultati)", &import, 560.0),
        280.0
    );

    // The main window, the list windows and the print preview stay resizable.
    for (name, (min, max)) in [
        ("AppWindow", constraints(&AppWindow::new().unwrap())),
        (
            "ProductsWindow",
            constraints(&ProductsWindow::new().unwrap()),
        ),
        ("PrintPreview", constraints(&PrintPreview::new().unwrap())),
    ] {
        if let Some(max) = max {
            assert!(
                min.width < max.width && min.height < max.height,
                "{name}: min {min:?} max {max:?}"
            );
        }
    }
}

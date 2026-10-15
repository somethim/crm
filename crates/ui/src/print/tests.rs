//! Headless tests of the receipt preview (i-slint-backend-testing, no event
//! loop). `run_bg` delivers its result through the event loop, which these
//! tests do not run; they call the same job and display functions directly.

use super::preview::{self, PHOTO_DPI, Prepared, open_preview, preview_dpi, save_png};
use super::texts;
use crate::UiContext;
use crm_services::domain::{
    DraftLine, Money, OrderDraft, OrderId, OrderKind, ProductInput, Qty, time,
};
use crm_services::{Crm, DataPaths, ReceiptSettings};
use slint::ComponentHandle;
use std::rc::Rc;

struct Harness {
    _app: crate::AppWindow,
    ctx: Rc<UiContext>,
    dir: tempfile::TempDir,
}

fn harness() -> Harness {
    i_slint_backend_testing::init_no_event_loop();
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    paths.ensure_dirs().unwrap();
    let crm = Crm::open(paths.clone(), &crm_services::DbKey::for_tests(1)).unwrap();
    let (app, ctx) = crate::build(
        crm,
        crm_services::Vault::with_params(paths.clone(), crm_services::KdfParams::TEST),
        paths,
    )
    .unwrap();
    app.show().unwrap();
    Harness {
        _app: app,
        ctx,
        dir,
    }
}

fn order(crm: &Crm, kind: OrderKind, owner: &str) -> OrderId {
    let product = crm
        .create_product(ProductInput {
            code: "A1".into(),
            name: "Lëng çaji".into(),
            unit: "COPE".into(),
            buy_price: Money(100),
            sell_price: Money(250),
            quantity: Qty(50),
        })
        .unwrap()
        .product
        .id;
    let draft = OrderDraft {
        kind,
        owner_id: None,
        owner_name: owner.into(),
        owner_description: "Rruga e Durrësit".into(),
        issued_on: time::today(),
        lines: vec![DraftLine {
            product_id: Some(product),
            name: "Lëng çaji".into(),
            quantity: Qty(3),
            price: Money(250),
            buy_price: None,
            sell_price: None,
        }],
    };
    crm.save_order(draft, None, None).unwrap()
}

fn px(mm: u32, dpi: f32) -> u32 {
    (f64::from(mm) / 25.4 * f64::from(dpi)).round() as u32
}

fn shown(h: &Harness, id: OrderId) -> Rc<preview::Preview> {
    let p = open_preview(&h.ctx, id).expect("the preview opens");
    assert!(p.is_open());
    assert!(p.window().get_loading());
    let dpi = preview_dpi(1.0);
    p.apply(preview::prepare(&h.ctx.crm, id, dpi));
    p
}

#[test]
fn the_preview_shows_the_receipt_at_twice_the_screen_resolution() {
    let h = harness();
    let id = order(&h.ctx.crm, OrderKind::Sale, "Bar Alfa");
    let p = shown(&h, id);
    let w = p.window();
    assert!(!w.get_loading());
    let size = w.get_receipt().size();
    assert_eq!(preview_dpi(1.0), 192.0);
    // Capped at a 200 % screen (F-022): the raster does not grow with the screen.
    assert_eq!(preview_dpi(2.0), preview::PREVIEW_MAX_DPI);
    assert_eq!(preview_dpi(3.0), preview::PREVIEW_MAX_DPI);
    assert_eq!(size.width, px(72, 192.0));
    assert!(size.height > 100);
    // At 100 % the image is 72 mm wide on screen.
    assert!((w.get_paper_width() - 72.0 * 96.0 / 25.4).abs() < 0.01);
    let (data, _) = p.receipt().unwrap();
    assert_eq!(data.owner_label, "Klient");
    assert_eq!(data.owner_name, "Bar Alfa");
    assert_eq!(data.total, "7,50");
    // Printing is Windows-only.
    assert_eq!(w.get_can_print(), cfg!(windows));
    assert_eq!(w.get_print_hint(), texts::PRINT_ONLY_ON_WINDOWS);
}

#[test]
fn a_purchase_prints_the_supplier() {
    let h = harness();
    let id = order(&h.ctx.crm, OrderKind::Purchase, "Furnitori Beta");
    let p = shown(&h, id);
    let (data, _) = p.receipt().unwrap();
    assert_eq!(data.owner_label, "Furnitor");
    assert_eq!(data.owner_name, "Furnitori Beta");
}

#[test]
fn zoom_stays_between_50_and_300_percent() {
    let h = harness();
    let id = order(&h.ctx.crm, OrderKind::Sale, "Bar Alfa");
    let p = shown(&h, id);
    let w = p.window();
    assert_eq!(w.get_zoom_percent(), 100);
    for _ in 0..20 {
        w.invoke_zoom_by(1);
    }
    assert_eq!(w.get_zoom_percent(), 300);
    for _ in 0..20 {
        w.invoke_zoom_by(-1);
    }
    assert_eq!(w.get_zoom_percent(), 50);
}

#[test]
fn the_settings_change_the_next_preview() {
    let h = harness();
    let id = order(&h.ctx.crm, OrderKind::Sale, "Bar Alfa");
    h.ctx
        .crm
        .set_receipt_settings(ReceiptSettings {
            width_mm: 58,
            font_pt: 10,
        })
        .unwrap();
    let p = shown(&h, id);
    assert_eq!(p.window().get_receipt().size().width, px(58, 192.0));
    let (_, settings) = p.receipt().unwrap();
    assert_eq!((settings.width_mm, settings.font_pt), (58, 10));
}

#[test]
fn save_as_photo_writes_a_300_dpi_png() {
    let h = harness();
    let id = order(&h.ctx.crm, OrderKind::Sale, "Bar Alfa");
    let p = shown(&h, id);
    let (data, settings) = p.receipt().unwrap();
    let path = preview::with_png_extension(h.dir.path().join(format!("porosia-{}", data.nr)));
    assert!(path.to_string_lossy().ends_with(".png"));
    save_png(&data, &settings, &path).unwrap();
    let png = std::fs::read(&path).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let width = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
    assert_eq!(width, px(72, PHOTO_DPI));
    // The handler's completion reports the file in the window.
    p.saved(&path, Ok(()));
    let status = p.window().get_status_text().to_string();
    assert!(status.starts_with("Fotoja u ruajt: "), "{status}");
    assert!(status.ends_with(".png"));
}

#[test]
fn a_missing_order_shows_the_reason_and_a_message_over_the_preview() {
    let h = harness();
    let p = open_preview(&h.ctx, OrderId(999)).unwrap();
    p.apply(preview::prepare(&h.ctx.crm, OrderId(999), preview_dpi(1.0)));
    let w = p.window();
    assert!(!w.get_loading());
    assert!(w.get_status_text().starts_with("Parapamja nuk u krijua: "));
    // The message box blocks the preview.
    assert!(w.get_input_blocked());
}

#[test]
fn closing_unblocks_the_main_window() {
    let h = harness();
    let id = order(&h.ctx.crm, OrderKind::Sale, "Bar Alfa");
    let p = shown(&h, id);
    let app = h.ctx.app().unwrap();
    assert!(app.get_input_blocked());
    p.window().invoke_close_clicked();
    assert!(!p.is_open());
    assert!(!app.get_input_blocked());
}

#[test]
fn png_extension_is_added_once() {
    use std::path::PathBuf;
    assert_eq!(
        preview::with_png_extension(PathBuf::from("a/porosia-1")),
        PathBuf::from("a/porosia-1.png")
    );
    assert_eq!(
        preview::with_png_extension(PathBuf::from("a/porosia-1.PNG")),
        PathBuf::from("a/porosia-1.PNG")
    );
}

#[test]
fn texts_fill_their_placeholders() {
    assert_eq!(
        texts::fill(texts::PRINT_FAILED, "arsyeja", "x"),
        "Printimi dështoi: x"
    );
    let _: Option<Prepared> = None;
}

#[test]
fn ctrl_plus_zooms_and_escape_closes() {
    use slint::platform::{Key, WindowEvent};
    let h = harness();
    let id = order(&h.ctx.crm, OrderKind::Sale, "Bar Alfa");
    let p = shown(&h, id);
    let win = p.window().window();
    let press = |text: slint::SharedString| {
        win.dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        win.dispatch_event(WindowEvent::KeyReleased { text });
    };
    win.dispatch_event(WindowEvent::KeyPressed {
        text: Key::Control.into(),
    });
    press("=".into());
    win.dispatch_event(WindowEvent::KeyReleased {
        text: Key::Control.into(),
    });
    assert_eq!(p.window().get_zoom_percent(), 125);
    press(Key::Escape.into());
    assert!(!p.is_open());
}

/// No user-visible reason may carry library or OS text (F-024).
fn assert_albanian_only(text: &str) {
    for raw in [
        "os error", "error", "failed", "HRESULT", "denied", "No such", "(0x",
    ] {
        assert!(!text.contains(raw), "{text:?} contains {raw:?}");
    }
}

#[test]
fn a_photo_into_a_missing_folder_reports_an_albanian_reason() {
    let h = harness();
    let id = order(&h.ctx.crm, OrderKind::Sale, "Bar Alfa");
    let p = shown(&h, id);
    let (data, settings) = p.receipt().unwrap();
    let path = h.dir.path().join("nuk-ekziston").join("porosia.png");
    let reason = save_png(&data, &settings, &path).unwrap_err();
    assert_eq!(reason, texts::REASON_FOLDER_NOT_FOUND);
    let shown = texts::fill(texts::SAVE_FAILED, "arsyeja", &reason);
    assert_eq!(shown, "Ruajtja e fotos dështoi: dosja nuk u gjet");
    assert_albanian_only(&shown);
    // The handler shows it in a message over the preview.
    p.saved(&path, Err(reason));
    assert!(p.window().get_input_blocked());
    // A folder with the file's name.
    let folder = h.dir.path().join("dosje.png");
    std::fs::create_dir(&folder).unwrap();
    let reason = save_png(&data, &settings, &folder).unwrap_err();
    assert!(
        [texts::REASON_IS_FOLDER, texts::REASON_NO_WRITE_PERMISSION].contains(&reason.as_str()),
        "{reason}"
    );
}

#[cfg(unix)]
#[test]
fn a_photo_into_a_read_only_folder_reports_an_albanian_reason() {
    use std::os::unix::fs::PermissionsExt;
    let h = harness();
    let id = order(&h.ctx.crm, OrderKind::Sale, "Bar Alfa");
    let p = shown(&h, id);
    let (data, settings) = p.receipt().unwrap();
    let locked = h.dir.path().join("vetem-lexim");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
    let path = locked.join("porosia.png");
    let result = save_png(&data, &settings, &path);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    if std::fs::metadata(&path).is_ok() {
        // Running as root: permissions are not enforced; nothing to check.
        return;
    }
    let reason = result.unwrap_err();
    assert_eq!(reason, texts::REASON_NO_WRITE_PERMISSION);
    assert_albanian_only(&reason);
}

#[test]
fn io_errors_map_to_albanian_reasons() {
    use std::io::{Error, ErrorKind};
    for (kind, expected) in [
        (
            ErrorKind::PermissionDenied,
            texts::REASON_NO_WRITE_PERMISSION,
        ),
        (
            ErrorKind::ReadOnlyFilesystem,
            texts::REASON_NO_WRITE_PERMISSION,
        ),
        (ErrorKind::StorageFull, texts::REASON_DISK_FULL),
        (ErrorKind::NotFound, texts::REASON_FOLDER_NOT_FOUND),
        (ErrorKind::IsADirectory, texts::REASON_IS_FOLDER),
        (ErrorKind::Other, texts::REASON_NOT_WRITTEN),
        (ErrorKind::Interrupted, texts::REASON_NOT_WRITTEN),
    ] {
        assert_eq!(texts::io_reason(&Error::from(kind)), expected, "{kind:?}");
    }
}

#[test]
fn render_and_print_errors_map_to_albanian_reasons() {
    use crm_reports::{PrintError, PrintStep, RenderError};
    assert_eq!(
        texts::render_reason(&RenderError::TooTall(2100)),
        "porosia është shumë e gjatë për printim (2100 mm)"
    );
    for e in [
        RenderError::Template("unknown variable: data".to_owned()),
        RenderError::Png("Encoding error: out of memory".to_owned()),
    ] {
        let reason = texts::render_reason(&e);
        assert_eq!(reason, texts::REASON_NOT_PREPARED);
        assert_albanian_only(&reason);
    }
    let too_large = RenderError::TooLarge {
        width: u32::MAX,
        height: 10,
    };
    assert_eq!(texts::render_reason(&too_large), texts::REASON_TOO_LARGE);

    let steps = [
        PrintStep::Dialog,
        PrintStep::StartDoc,
        PrintStep::StartPage,
        PrintStep::SendImage,
        PrintStep::EndPage,
        PrintStep::EndDoc,
    ];
    let mut errors = vec![
        PrintError::NoWindow,
        PrintError::NoPrinter,
        PrintError::NoResolution,
        PrintError::Render(too_large),
    ];
    for step in steps {
        for code in [
            None,
            Some(0),
            Some(5),
            Some(8),
            Some(63),
            Some(112),
            Some(1722),
            Some(1804),
        ] {
            errors.push(PrintError::Os { step, code });
        }
    }
    for e in &errors {
        let shown = texts::fill(texts::PRINT_FAILED, "arsyeja", &texts::print_reason(e));
        assert_albanian_only(&shown);
        assert!(shown.len() > "Printimi dështoi: ".len(), "{e:?}");
    }
    let reason = |step, code| texts::print_reason(&PrintError::Os { step, code });
    assert_eq!(
        reason(PrintStep::StartDoc, Some(1804)),
        texts::REASON_START_DOC
    );
    assert_eq!(reason(PrintStep::EndPage, None), texts::REASON_PAGE);
    assert_eq!(
        reason(PrintStep::SendImage, Some(5)),
        texts::REASON_PRINTER_DENIED
    );
    assert_eq!(
        reason(PrintStep::EndDoc, Some(63)),
        texts::REASON_JOB_CANCELLED
    );
    assert_eq!(
        reason(PrintStep::StartDoc, Some(1722)),
        texts::REASON_SPOOLER
    );
    assert_eq!(
        reason(PrintStep::Dialog, Some(-2147467259)),
        texts::REASON_DIALOG
    );
    // The log keeps the technical text.
    let logged = PrintError::Os {
        step: PrintStep::StartDoc,
        code: Some(5),
    }
    .to_string();
    assert!(
        logged.contains("StartDoc") && logged.contains("os error 5"),
        "{logged}"
    );
}

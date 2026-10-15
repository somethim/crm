//! The "Parapamja e printimit" window: renders the receipt on a worker thread,
//! shows it, saves it as PNG and (on Windows) prints it.

use super::{data, texts};
use crate::shell::modal::{self, Modal, ModalOwner};
use crate::{MessageDialog, PrintPreview, UiContext};
use crm_reports::{Raster, ReceiptData, ReceiptSettings};
use crm_services::Crm;
use crm_services::domain::OrderId;
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

/// The preview renders at this multiple of the screen resolution, so zooming
/// up to 200 % stays sharp.
pub const PREVIEW_SCALE: f32 = 2.0;
/// "Ruaj si foto…" renders at this resolution (a crisp photo to send).
pub const PHOTO_DPI: f32 = 300.0;

/// The highest preview resolution (twice a 200 % screen). A higher scale
/// factor gets a slightly softer zoom instead of a raster that grows with the
/// screen (F-022); `crm_reports::MAX_PIXELS` is sized for this.
pub const PREVIEW_MAX_DPI: f32 = 384.0;

/// The preview resolution for a window scale factor (96 DPI is 100 %).
pub fn preview_dpi(scale_factor: f32) -> f32 {
    (96.0 * PREVIEW_SCALE * scale_factor.max(1.0)).min(PREVIEW_MAX_DPI)
}

/// A rendered receipt and what produced it.
pub struct Prepared {
    /// The receipt content.
    pub data: ReceiptData,
    /// The width and font size.
    pub settings: ReceiptSettings,
    /// The rendered page.
    pub raster: Raster,
}

/// Loads order `id` and renders it at `dpi` (the worker job). The error is
/// the Albanian reason.
pub fn prepare(crm: &Crm, id: OrderId, dpi: f32) -> Result<Prepared, String> {
    let (data, settings) = data::load(crm, id).map_err(|e| {
        tracing::warn!("receipt {}: {e}", id.0);
        e.user_message()
    })?;
    let raster = crm_reports::render(&data, &settings, dpi)
        .map_err(|e| {
            tracing::warn!("receipt {}: {e}", id.0);
            texts::render_reason(&e)
        })?
        .into_iter()
        .next()
        .ok_or_else(|| texts::EMPTY_PAGE.to_owned())?;
    Ok(Prepared {
        data,
        settings,
        raster,
    })
}

/// Renders the receipt at [`PHOTO_DPI`] and writes it to `path`. The error
/// is the Albanian reason; the raw typst / OS text is logged here (F-024).
pub fn save_png(data: &ReceiptData, settings: &ReceiptSettings, path: &Path) -> Result<(), String> {
    let png = crm_reports::render_png(data, settings, PHOTO_DPI).map_err(|e| {
        tracing::warn!("receipt {} as PNG: {e}", data.nr);
        texts::render_reason(&e)
    })?;
    std::fs::write(path, png).map_err(|e| {
        tracing::warn!("writing {}: {e}", path.display());
        texts::io_reason(&e).to_owned()
    })
}

/// `path` with a `.png` extension.
pub fn with_png_extension(path: PathBuf) -> PathBuf {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("png"))
    {
        path
    } else {
        let mut s = path.into_os_string();
        s.push(".png");
        PathBuf::from(s)
    }
}

/// The Slint image of a premultiplied RGBA raster.
pub fn to_image(raster: &Raster) -> Image {
    let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        &raster.data,
        raster.width,
        raster.height,
    );
    Image::from_rgba8_premultiplied(buffer)
}

impl ModalOwner for PrintPreview {
    fn set_input_blocked(&self, blocked: bool) {
        self.set_input_blocked(blocked);
    }

    fn native_title(&self) -> String {
        texts::PREVIEW_TITLE.to_owned()
    }
}

/// An open preview window.
pub struct Preview {
    ctx: Weak<UiContext>,
    window: PrintPreview,
    modal: RefCell<Option<Modal>>,
    /// The receipt shown (set once rendered).
    receipt: RefCell<Option<(ReceiptData, ReceiptSettings)>>,
}

#[cfg(test)]
thread_local! {
    /// The last opened preview (headless tests).
    static LAST: RefCell<Option<Weak<Preview>>> = const { RefCell::new(None) };
}

/// The most recently opened preview on this thread, while it is open.
#[cfg(test)]
pub fn last_preview() -> Option<Rc<Preview>> {
    LAST.with(|l| l.borrow().as_ref().and_then(Weak::upgrade))
}

/// Opens the preview of saved order `id` modal over the main window and
/// renders it in the background.
pub fn open_preview(ctx: &Rc<UiContext>, id: OrderId) -> Option<Rc<Preview>> {
    let window = match PrintPreview::new() {
        Ok(w) => w,
        Err(e) => {
            tracing::error!("print preview: {e}");
            return None;
        }
    };
    window.set_can_print(cfg!(windows));
    window.set_print_hint(texts::PRINT_ONLY_ON_WINDOWS.into());
    window.set_loading(true);
    let preview = Rc::new(Preview {
        ctx: Rc::downgrade(ctx),
        window: window.clone_strong(),
        modal: RefCell::new(None),
        receipt: RefCell::new(None),
    });
    wire(&preview);
    let modal = modal::open_over_app(ctx, &window)?;
    // The modal keeps the preview alive until it closes.
    let keep = preview.clone();
    modal.on_closed(move || drop(keep));
    *preview.modal.borrow_mut() = Some(modal);
    #[cfg(test)]
    LAST.with(|l| *l.borrow_mut() = Some(Rc::downgrade(&preview)));

    let dpi = preview_dpi(window.window().scale_factor());
    let weak = Rc::downgrade(&preview);
    ctx.run_bg(
        move |crm| prepare(&crm, id, dpi),
        move |result| {
            if let Some(p) = weak.upgrade() {
                p.apply(result);
            }
        },
    );
    Some(preview)
}

fn wire(preview: &Rc<Preview>) {
    let w = &preview.window;
    let weak = Rc::downgrade(preview);
    w.on_close_clicked(move || {
        if let Some(p) = weak.upgrade() {
            p.close();
        }
    });
    let weak = Rc::downgrade(preview);
    w.on_save_image(move || {
        if let Some(p) = weak.upgrade() {
            p.ask_and_save();
        }
    });
    let weak = Rc::downgrade(preview);
    w.on_print(move || {
        if let Some(p) = weak.upgrade() {
            p.print();
        }
    });
}

impl Preview {
    /// The window (tests).
    pub fn window(&self) -> &PrintPreview {
        &self.window
    }

    /// Whether the window is still open.
    pub fn is_open(&self) -> bool {
        self.modal.borrow().as_ref().is_some_and(Modal::is_open)
    }

    /// Closes the window.
    pub fn close(&self) {
        let modal = self.modal.borrow().clone();
        if let Some(m) = modal {
            m.close();
        }
    }

    /// Shows the rendered receipt, or the reason it failed.
    pub fn apply(&self, result: Result<Prepared, String>) {
        self.window.set_loading(false);
        match result {
            Ok(p) => {
                self.window.set_receipt(to_image(&p.raster));
                self.window
                    .set_paper_width(p.settings.width_mm as f32 * 96.0 / 25.4);
                *self.receipt.borrow_mut() = Some((p.data, p.settings));
            }
            Err(reason) => {
                let text = texts::fill(texts::PREVIEW_FAILED, "arsyeja", &reason);
                self.window.set_status_text(text.clone().into());
                self.message(&text);
            }
        }
    }

    /// The receipt shown, once rendered.
    pub fn receipt(&self) -> Option<(ReceiptData, ReceiptSettings)> {
        self.receipt.borrow().clone()
    }

    /// "Ruaj si foto…": asks for the file, then writes it in the background.
    fn ask_and_save(self: &Rc<Self>) {
        let Some((data, _)) = self.receipt() else {
            return;
        };
        let picked = rfd::FileDialog::new()
            .set_title(texts::SAVE_TITLE)
            .set_file_name(format!("porosia-{}.png", data.nr))
            .add_filter(texts::PNG_FILTER, &["png"])
            .save_file();
        if let Some(path) = picked {
            self.save_to(with_png_extension(path));
        }
    }

    /// Writes the PNG to `path` in the background and reports the result.
    pub fn save_to(self: &Rc<Self>, path: PathBuf) {
        let Some((data, settings)) = self.receipt() else {
            return;
        };
        let Some(ctx) = self.ctx.upgrade() else {
            return;
        };
        let weak = Rc::downgrade(self);
        let target = path.clone();
        ctx.run_bg(
            move |_| save_png(&data, &settings, &target),
            move |result| {
                if let Some(p) = weak.upgrade() {
                    p.saved(&path, result);
                }
            },
        );
    }

    /// Reports a finished save.
    pub fn saved(&self, path: &Path, result: Result<(), String>) {
        match result {
            Ok(()) => {
                let shown = path.display().to_string();
                self.window
                    .set_status_text(texts::fill(texts::SAVED, "skedari", &shown).into());
            }
            Err(reason) => {
                tracing::warn!("saving {}: {reason}", path.display());
                self.message(&texts::fill(texts::SAVE_FAILED, "arsyeja", &reason));
            }
        }
    }

    /// "Printo…" (Windows): the print dialog, then the receipt rendered at the
    /// printer's resolution.
    #[cfg(windows)]
    fn print(&self) {
        let Some((data, settings)) = self.receipt() else {
            return;
        };
        let owner = crate::shell::win::find_window(texts::PREVIEW_TITLE)
            .or_else(|| crate::shell::win::find_window("CRM"));
        let doc_name = texts::fill(texts::DOC_NAME, "nr", &data.nr);
        let outcome = crm_reports::print_receipt(
            owner,
            |dpi| crm_reports::render(&data, &settings, dpi),
            &doc_name,
        );
        match outcome {
            Ok(crm_reports::PrintOutcome::Printed) => {
                self.window.set_status_text(texts::PRINTED.into());
            }
            Ok(crm_reports::PrintOutcome::Cancelled) => {}
            Err(e) => {
                tracing::warn!("printing receipt {}: {e}", data.nr);
                let reason = texts::print_reason(&e);
                self.message(&texts::fill(texts::PRINT_FAILED, "arsyeja", &reason));
            }
        }
    }

    /// Printing is Windows-only (the button is disabled elsewhere).
    #[cfg(not(windows))]
    fn print(&self) {}

    /// A message box modal over the preview.
    fn message(&self, text: &str) {
        let dialog = match MessageDialog::new() {
            Ok(d) => d,
            Err(e) => {
                tracing::error!("message dialog: {e}");
                return;
            }
        };
        dialog.set_caption(crate::shell::texts::ERROR_TITLE.into());
        dialog.set_text(text.into());
        if let Some(ctx) = self.ctx.upgrade() {
            ctx.register_window(Box::new(dialog.as_weak()));
        }
        let m = modal::open(&self.window, &dialog);
        dialog.on_closed(move || m.close());
    }
}

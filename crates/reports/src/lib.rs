//! Receipt rendering (preview, PNG) and printing.
//!
//! Boundary: depends on `crm-domain` only (no services, no Slint). Used by
//! `crm-ui`, which maps a saved order and the receipt settings to
//! [`ReceiptData`] and [`ReceiptSettings`].
//!
//! - [`data`]: the pre-formatted receipt content;
//! - [`receipt`]: the typst template compiled with embedded Noto Sans fonts and
//!   rasterised at any DPI ([`render`], [`render_png`]);
//! - [`print_layout`]: the page bands and pixel conversion of the printing;
//! - `print_windows` (Windows only): the GDI printing (`print_receipt`).

pub mod data;
pub mod print_layout;
pub mod receipt;

// The only FFI module of this crate; every unsafe block has a `// SAFETY:` comment.
#[cfg(windows)]
#[allow(unsafe_code)]
mod print_windows;

#[cfg(test)]
mod tests;

pub use data::{ReceiptData, ReceiptLine, ReceiptSettings};
#[cfg(windows)]
pub use print_windows::print_receipt;
pub use receipt::{MAX_HEIGHT_MM, MAX_PIXELS, Raster, render, render_png};

/// A rendering failure (the template did not compile, the receipt is too tall
/// or too large, or the PNG could not be encoded). The text is for the log;
/// the UI shows its own Albanian sentence for each variant (F-024).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenderError {
    /// The typst template failed to compile.
    #[error("{0}")]
    Template(String),
    /// The content is taller than [`MAX_HEIGHT_MM`].
    #[error("porosia është shumë e gjatë për printim ({0} mm)")]
    TooTall(u32),
    /// The page is larger than [`receipt::MAX_PIXELS`] at the requested
    /// resolution (an out-of-range width setting or DPI).
    #[error("porosia është shumë e madhe për t'u vizatuar ({width}×{height} px)")]
    TooLarge {
        /// Width in pixels (saturated).
        width: u32,
        /// Height in pixels (saturated).
        height: u32,
    },
    /// The PNG encoder failed.
    #[error("{0}")]
    Png(String),
}

/// How a print request ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrintOutcome {
    /// The receipt was sent to the printer.
    Printed,
    /// The user closed the print dialog, or the driver's "Save print output
    /// as" prompt, without printing.
    Cancelled,
}

/// The GDI printing step whose call failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrintStep {
    /// `PrintDlgExW`.
    Dialog,
    /// `StartDocW`.
    StartDoc,
    /// `StartPage`.
    StartPage,
    /// `StretchDIBits`.
    SendImage,
    /// `EndPage`.
    EndPage,
    /// `EndDoc`.
    EndDoc,
}

/// A printing failure. It carries the failed step and the OS code, never a
/// formatted OS message: the UI maps it to an Albanian sentence, and the
/// `Display` text (technical, with the OS message) is for the log only (F-024).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PrintError {
    /// The program window that owns the print dialog was not found.
    #[error("the main window was not found")]
    NoWindow,
    /// The dialog returned no printer device context.
    #[error("the printer device context was not created")]
    NoPrinter,
    /// The printer reported no resolution or printable area.
    #[error("the printer reported no resolution or printable area")]
    NoResolution,
    /// A Win32 call failed: the step and the `GetLastError` code (for
    /// [`PrintStep::Dialog`], the HRESULT).
    #[error("{step:?} failed ({})", os_text(.step, .code))]
    Os {
        /// The failed step.
        step: PrintStep,
        /// The Win32 error code or HRESULT, when there is one.
        code: Option<i32>,
    },
    /// The receipt could not be rendered.
    #[error("rendering: {0}")]
    Render(RenderError),
}

/// The OS text of a failure, for the log.
fn os_text(step: &PrintStep, code: &Option<i32>) -> String {
    match (*step, *code) {
        (_, None) => "no error code".to_owned(),
        (PrintStep::Dialog, Some(hr)) => format!("HRESULT {:#010x}", hr as u32),
        (_, Some(c)) => std::io::Error::from_raw_os_error(c).to_string(),
    }
}

impl From<RenderError> for PrintError {
    fn from(e: RenderError) -> Self {
        PrintError::Render(e)
    }
}

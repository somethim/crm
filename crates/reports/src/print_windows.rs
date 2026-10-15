//! Windows GDI printing: `PrintDlgExW` (printer, copies) → the printer's DPI
//! and printable area (`GetDeviceCaps`) → the receipt rendered at that DPI,
//! capped at [`PRINT_MAX_DPI`](crate::print_layout::PRINT_MAX_DPI) (scaled up
//! to the device by `StretchDIBits`) →
//! `StartDocW`, one `StartPage`/`StretchDIBits`/`EndPage` per band, `EndDoc`.
//! Works with any installed printer driver (the thermal model is not known
//! yet); the image is left-aligned in the printable area.

use crate::print_layout::{StartDoc, bands, render_dpi, start_doc_outcome, to_bgra_on_white};
use crate::{PrintError, PrintOutcome, PrintStep, Raster, RenderError};
use windows::Win32::Foundation::{GlobalFree, HGLOBAL, HWND};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteDC, GetDeviceCaps, HALFTONE, HDC,
    HORZRES, LOGPIXELSX, LOGPIXELSY, SRCCOPY, SetStretchBltMode, StretchDIBits, VERTRES,
};
use windows::Win32::Storage::Xps::{AbortDoc, DOCINFOW, EndDoc, EndPage, StartDocW, StartPage};
use windows::Win32::UI::Controls::Dialogs::{
    PD_NOCURRENTPAGE, PD_NOPAGENUMS, PD_NOSELECTION, PD_RESULT_PRINT, PD_RETURNDC,
    PD_USEDEVMODECOPIESANDCOLLATE, PRINTDLGEXW, PrintDlgExW, START_PAGE_GENERAL,
};
use windows::core::HSTRING;

/// The last Win32 failure of `step`, with its `GetLastError` code.
fn last_error(step: PrintStep) -> PrintError {
    PrintError::Os {
        step,
        code: std::io::Error::last_os_error().raw_os_error(),
    }
}

/// Frees the dialog's device settings and deletes the printer DC on drop.
struct DialogResult {
    dc: HDC,
    dev_mode: HGLOBAL,
    dev_names: HGLOBAL,
}

impl Drop for DialogResult {
    fn drop(&mut self) {
        // SAFETY: the handles came from PrintDlgExW and are released exactly
        // once, here; null handles are skipped.
        unsafe {
            if !self.dc.is_invalid() {
                let _ = DeleteDC(self.dc);
            }
            if !self.dev_mode.is_invalid() {
                let _ = GlobalFree(Some(self.dev_mode));
            }
            if !self.dev_names.is_invalid() {
                let _ = GlobalFree(Some(self.dev_names));
            }
        }
    }
}

/// Asks for the printer and the copies, then prints the receipt that
/// `render_at(dpi)` produces at the printer's resolution. `owner` is the
/// window the dialog belongs to (required by `PrintDlgExW`). Cancelling the
/// dialog, or the driver's own "Save print output as" prompt, is
/// [`PrintOutcome::Cancelled`], not an error.
pub fn print_receipt(
    owner: Option<HWND>,
    render_at: impl Fn(f32) -> Result<Vec<Raster>, RenderError>,
    doc_name: &str,
) -> Result<PrintOutcome, PrintError> {
    let owner = owner.ok_or(PrintError::NoWindow)?;
    let mut pd = PRINTDLGEXW {
        lStructSize: std::mem::size_of::<PRINTDLGEXW>() as u32,
        hwndOwner: owner,
        Flags: PD_RETURNDC
            | PD_NOPAGENUMS
            | PD_NOSELECTION
            | PD_NOCURRENTPAGE
            | PD_USEDEVMODECOPIESANDCOLLATE,
        nCopies: 1,
        nStartPage: START_PAGE_GENERAL,
        ..Default::default()
    };
    // SAFETY: `pd` is a fully initialised PRINTDLGEXW with its size set, no
    // page ranges, no callback and no property pages; it outlives the call.
    let shown = unsafe { PrintDlgExW(&mut pd) };
    let result = DialogResult {
        dc: pd.hDC,
        dev_mode: pd.hDevMode,
        dev_names: pd.hDevNames,
    };
    shown.map_err(|e| PrintError::Os {
        step: PrintStep::Dialog,
        code: Some(e.code().0),
    })?;
    if pd.dwResultAction != PD_RESULT_PRINT {
        return Ok(PrintOutcome::Cancelled);
    }
    if result.dc.is_invalid() {
        return Err(PrintError::NoPrinter);
    }
    let dc = result.dc;

    // SAFETY: `dc` is the valid printer DC returned by the dialog.
    let (dpi_x, dpi_y, printable_w, printable_h) = unsafe {
        (
            GetDeviceCaps(Some(dc), LOGPIXELSX),
            GetDeviceCaps(Some(dc), LOGPIXELSY),
            GetDeviceCaps(Some(dc), HORZRES),
            GetDeviceCaps(Some(dc), VERTRES),
        )
    };
    if dpi_x <= 0 || dpi_y <= 0 || printable_w <= 0 || printable_h <= 0 {
        return Err(PrintError::NoResolution);
    }
    // At most PRINT_MAX_DPI: the raster's size no longer follows the driver.
    let src_dpi = render_dpi(dpi_x);
    let mut pages = render_at(src_dpi as f32)?;
    // Converted once, in place (not per copy, no second full-size buffer).
    for page in &mut pages {
        to_bgra_on_white(&mut page.data);
    }
    let copies = pd.nCopies.max(1);

    let name = HSTRING::from(doc_name);
    let info = DOCINFOW {
        cbSize: std::mem::size_of::<DOCINFOW>() as i32,
        lpszDocName: windows::core::PCWSTR(name.as_ptr()),
        ..Default::default()
    };
    // SAFETY: `dc` is valid; `info` and the `name` it points to outlive the call.
    let job = unsafe { StartDocW(dc, &info) };
    // Read at once: a file-based driver's "Save as" prompt cancelled by the
    // user fails StartDoc with ERROR_CANCELLED, which is not a failure (F-023).
    let code = std::io::Error::last_os_error().raw_os_error();
    match start_doc_outcome(job, code) {
        StartDoc::Started => {}
        StartDoc::Cancelled => return Ok(PrintOutcome::Cancelled),
        StartDoc::Failed(code) => {
            return Err(PrintError::Os {
                step: PrintStep::StartDoc,
                code,
            });
        }
    }
    let printed = (0..copies).try_for_each(|_| {
        pages.iter().try_for_each(|page| {
            print_raster(dc, page, src_dpi, dpi_x, dpi_y, printable_w, printable_h)
        })
    });
    match printed {
        Ok(()) => {
            // SAFETY: a document was started on `dc` above.
            if unsafe { EndDoc(dc) } <= 0 {
                return Err(last_error(PrintStep::EndDoc));
            }
            Ok(PrintOutcome::Printed)
        }
        Err(e) => {
            // SAFETY: a document was started on `dc` above; abort discards it.
            unsafe {
                AbortDoc(dc);
            }
            Err(e)
        }
    }
}

/// Prints one rendered page (already BGRA, rendered at `src_dpi`) as one or
/// more device pages.
fn print_raster(
    dc: HDC,
    raster: &Raster,
    src_dpi: i32,
    dpi_x: i32,
    dpi_y: i32,
    printable_w: i32,
    printable_h: i32,
) -> Result<(), PrintError> {
    let bgra = &raster.data;
    let stride = raster.width as usize * 4;
    for band in bands(
        raster.width,
        raster.height,
        src_dpi,
        dpi_x,
        dpi_y,
        printable_w,
        printable_h,
    ) {
        let start = band.src_y as usize * stride;
        let end = start + band.src_rows as usize * stride;
        let rows = &bgra[start..end];
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: raster.width as i32,
                // Negative: a top-down DIB (first row first).
                biHeight: -(band.src_rows as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: `dc` has an open document; StartPage takes no pointers.
        if unsafe { StartPage(dc) } <= 0 {
            return Err(last_error(PrintStep::StartPage));
        }
        // SAFETY: `rows` holds exactly `width × src_rows` 32-bit pixels as
        // described by `info`; both outlive the call.
        let copied = unsafe {
            SetStretchBltMode(dc, HALFTONE);
            StretchDIBits(
                dc,
                0,
                0,
                band.dest_w,
                band.dest_h,
                0,
                0,
                raster.width as i32,
                band.src_rows as i32,
                Some(rows.as_ptr().cast()),
                &info,
                DIB_RGB_COLORS,
                SRCCOPY,
            )
        };
        if copied == 0 {
            return Err(last_error(PrintStep::SendImage));
        }
        // SAFETY: a page was started on `dc` above.
        if unsafe { EndPage(dc) } <= 0 {
            return Err(last_error(PrintStep::EndPage));
        }
    }
    Ok(())
}

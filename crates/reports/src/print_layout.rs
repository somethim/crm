//! The printing geometry and pixel conversion behind the Windows GDI printing,
//! kept platform-independent so it is tested on every OS.

/// One printed page of a receipt: a horizontal slice of the rendered image
/// and its size on the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Band {
    /// First source row.
    pub src_y: u32,
    /// Source rows in this band.
    pub src_rows: u32,
    /// Width on the device, in device pixels.
    pub dest_w: i32,
    /// Height on the device, in device pixels.
    pub dest_h: i32,
}

/// The highest resolution a receipt is rendered at for printing. Receipt
/// text needs no more (thermal heads are ~203 DPI); a finer device gets the
/// image scaled up by `StretchDIBits`, so peak memory no longer grows with
/// the driver's DPI (F-022).
pub const PRINT_MAX_DPI: i32 = 300;

/// The resolution to render at for a device reporting `device_dpi`.
pub fn render_dpi(device_dpi: i32) -> i32 {
    device_dpi.clamp(1, PRINT_MAX_DPI)
}

/// Splits a `width × height` image rendered at `src_dpi` into pages of the
/// printable area (`printable_w × printable_h` device pixels of a
/// `dpi_x × dpi_y` device). The image is scaled to the device resolution,
/// then clamped to the printable width (scaled down, keeping the aspect
/// ratio); a device whose vertical resolution differs is compensated.
/// Roll-paper drivers usually report a very tall page, so a receipt is one
/// band; a fixed form (or A4 on "Microsoft Print to PDF") gets several.
pub fn bands(
    width: u32,
    height: u32,
    src_dpi: i32,
    dpi_x: i32,
    dpi_y: i32,
    printable_w: i32,
    printable_h: i32,
) -> Vec<Band> {
    if width == 0
        || height == 0
        || src_dpi <= 0
        || dpi_x <= 0
        || dpi_y <= 0
        || printable_w <= 0
        || printable_h <= 0
    {
        return Vec::new();
    }
    let to_device = f64::from(dpi_x) / f64::from(src_dpi);
    let device_w = f64::from(width) * to_device;
    let scale_x = to_device * (f64::from(printable_w) / device_w).min(1.0);
    let dest_w = ((f64::from(width) * scale_x).round().max(1.0) as i32).min(printable_w);
    let scale_y = scale_x * f64::from(dpi_y) / f64::from(dpi_x);
    let rows_per_page = ((f64::from(printable_h) / scale_y).floor() as u32).max(1);
    let mut out = Vec::new();
    let mut y = 0;
    while y < height {
        let rows = rows_per_page.min(height - y);
        let dest_h = ((f64::from(rows) * scale_y).round() as i32).clamp(1, printable_h);
        out.push(Band {
            src_y: y,
            src_rows: rows,
            dest_w,
            dest_h,
        });
        y += rows;
    }
    out
}

/// Premultiplied RGBA to the BGRA a 32-bpp DIB expects, composited over
/// white (the page is opaque, so this is a channel swap in practice), in
/// place: no second full-size buffer (F-022).
pub fn to_bgra_on_white(pixels: &mut [u8]) {
    for p in pixels.chunks_exact_mut(4) {
        let white = 255 - p[3];
        let (r, g, b) = (p[0], p[1], p[2]);
        p[0] = b.saturating_add(white);
        p[1] = g.saturating_add(white);
        p[2] = r.saturating_add(white);
        p[3] = 255;
    }
}

/// `ERROR_CANCELLED`: the user dismissed a prompt the driver showed (the
/// "Save print output as" box of Print to PDF / XPS / OneNote).
pub const ERROR_CANCELLED: i32 = 1223;

/// How `StartDocW` ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartDoc {
    /// A print job was started.
    Started,
    /// The user cancelled the driver's file prompt: no print, no error (F-023).
    Cancelled,
    /// A real failure, with the Win32 error code when there is one.
    Failed(Option<i32>),
}

/// Classifies `StartDocW`'s return value `ret` (a job id, ≤ 0 on failure) and
/// the `GetLastError` code read right after it.
pub fn start_doc_outcome(ret: i32, last_error: Option<i32>) -> StartDoc {
    match (ret, last_error) {
        (r, _) if r > 0 => StartDoc::Started,
        (_, Some(ERROR_CANCELLED)) => StartDoc::Cancelled,
        (_, code) => StartDoc::Failed(code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_receipt_on_a_roll_is_one_band_at_full_size() {
        let b = bands(575, 1200, 203, 203, 203, 576, 100_000);
        assert_eq!(
            b,
            vec![Band {
                src_y: 0,
                src_rows: 1200,
                dest_w: 575,
                dest_h: 1200
            }]
        );
    }

    #[test]
    fn the_width_is_clamped_to_the_printable_width() {
        // 80 mm rendered, 72 mm printable at 203 DPI.
        let b = bands(639, 1000, 203, 203, 203, 576, 100_000);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].dest_w, 576);
        assert_eq!(b[0].dest_h, (1000.0_f64 * 576.0 / 639.0).round() as i32);
    }

    #[test]
    fn a_short_form_splits_into_pages_that_cover_every_row() {
        let b = bands(575, 2500, 203, 203, 203, 576, 1000);
        assert_eq!(b.len(), 3);
        assert_eq!(b.iter().map(|x| x.src_rows).sum::<u32>(), 2500);
        assert_eq!(b[1].src_y, 1000);
        assert!(b.iter().all(|x| x.dest_h <= 1000));
    }

    #[test]
    fn a_different_vertical_resolution_is_compensated() {
        let b = bands(600, 600, 300, 300, 600, 10_000, 100_000);
        assert_eq!((b[0].dest_w, b[0].dest_h), (600, 1200));
    }

    #[test]
    fn degenerate_inputs_print_nothing() {
        assert!(bands(0, 10, 203, 203, 203, 576, 1000).is_empty());
        assert!(bands(10, 10, 203, 0, 203, 576, 1000).is_empty());
        assert!(bands(10, 10, 0, 203, 203, 576, 1000).is_empty());
    }

    #[test]
    fn pixels_become_opaque_bgra() {
        let mut pixels = vec![10, 20, 30, 255, 0, 0, 0, 0, 100, 50, 0, 128];
        to_bgra_on_white(&mut pixels);
        assert_eq!(
            pixels,
            vec![30, 20, 10, 255, 255, 255, 255, 255, 127, 177, 227, 255]
        );
    }

    #[test]
    fn the_render_resolution_is_capped() {
        assert_eq!(render_dpi(203), 203);
        assert_eq!(render_dpi(300), 300);
        assert_eq!(render_dpi(600), PRINT_MAX_DPI);
        assert_eq!(render_dpi(1200), PRINT_MAX_DPI);
        assert_eq!(render_dpi(0), 1);
    }

    #[test]
    fn a_lower_resolution_render_is_scaled_up_to_the_device() {
        // 72 mm rendered at 300 DPI (850 px) on a 600 DPI device with an A4
        // printable area (4960 × 7016): twice the size, pages of 3508 rows.
        let b = bands(850, 10_000, 300, 600, 600, 4960, 7016);
        assert_eq!(b.len(), 3);
        assert_eq!((b[0].dest_w, b[0].dest_h), (1700, 7016));
        assert_eq!(b[0].src_rows, 3508);
        assert_eq!(b.iter().map(|x| x.src_rows).sum::<u32>(), 10_000);
        assert_eq!(b[2].dest_h, (10_000 - 2 * 3508) * 2);
        // Scaled up past the printable width: clamped to it, keeping the ratio.
        let b = bands(850, 1000, 300, 1200, 1200, 2000, 1_000_000);
        assert_eq!(b[0].dest_w, 2000);
        assert_eq!(b[0].dest_h, (1000.0_f64 * 2000.0 / 850.0).round() as i32);
        // A non-square device keeps its vertical compensation.
        let b = bands(850, 1000, 300, 600, 1200, 10_000, 1_000_000);
        assert_eq!((b[0].dest_w, b[0].dest_h), (1700, 4000));
    }

    #[test]
    fn cancelling_the_drivers_file_prompt_is_not_a_failure() {
        assert_eq!(start_doc_outcome(7, None), StartDoc::Started);
        // A job id is a success whatever the stale last error says.
        assert_eq!(
            start_doc_outcome(1, Some(ERROR_CANCELLED)),
            StartDoc::Started
        );
        assert_eq!(start_doc_outcome(0, Some(1223)), StartDoc::Cancelled);
        assert_eq!(start_doc_outcome(-1, Some(1223)), StartDoc::Cancelled);
        assert_eq!(start_doc_outcome(0, Some(5)), StartDoc::Failed(Some(5)));
        assert_eq!(start_doc_outcome(-1, None), StartDoc::Failed(None));
        assert_eq!(start_doc_outcome(0, Some(0)), StartDoc::Failed(Some(0)));
    }
}

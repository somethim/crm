//! The receipt: `templates/receipt.typ` compiled with the embedded Noto Sans
//! fonts (no system fonts, so the output is the same on every machine) and
//! rasterised with `typst-render`.

use crate::RenderError;
use crate::data::{ReceiptData, ReceiptSettings};
use std::sync::LazyLock;
use typst::diag::Warned;
use typst::foundations::{Dict, IntoValue};
use typst::layout::Abs;
use typst::utils::Scalar;
use typst_as_lib::{TypstAsLibError, TypstEngine, TypstTemplateMainFile};
use typst_render::RenderOptions;

/// The tallest receipt rendered (a guard; roll paper needs no page split).
pub const MAX_HEIGHT_MM: u32 = 2000;

/// The largest page rasterised, in pixels (2^26, a 256 MiB RGBA buffer). The
/// biggest valid receipt (110 mm × [`MAX_HEIGHT_MM`]) at the highest DPI the
/// app asks for (384, the preview on a 200 % screen) is about 50 million
/// pixels; anything above the cap (a corrupt width setting, an absurd DPI) is
/// a [`RenderError::TooLarge`] instead of a multi-GB allocation or a panic in
/// `typst-render` (F-021).
pub const MAX_PIXELS: u64 = 1 << 26;

static TEMPLATE: &str = include_str!("../templates/receipt.typ");
static FONT_REGULAR: &[u8] = include_bytes!("../assets/fonts/NotoSans-Regular.ttf");
static FONT_BOLD: &[u8] = include_bytes!("../assets/fonts/NotoSans-Bold.ttf");

static ENGINE: LazyLock<TypstEngine<TypstTemplateMainFile>> = LazyLock::new(|| {
    TypstEngine::builder()
        .main_file(("receipt.typ", TEMPLATE))
        .fonts([FONT_REGULAR, FONT_BOLD])
        .build()
});

/// A rendered page: premultiplied RGBA8, row-major, top row first. The page
/// is filled white, so every pixel is opaque.
#[derive(Clone, PartialEq, Eq)]
pub struct Raster {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes.
    pub data: Vec<u8>,
}

impl std::fmt::Debug for Raster {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Raster({}x{})", self.width, self.height)
    }
}

/// Pins `doc` to the document type `render_merged` accepts
/// (`typst_layout::PagedDocument`, which this crate cannot name without a
/// direct `typst-layout` dependency).
fn pin<D, R>(
    doc: D,
    _render: fn(&D, &RenderOptions, Abs, Option<typst::visualize::Color>) -> R,
) -> D {
    doc
}

fn template_error(e: TypstAsLibError) -> RenderError {
    let text = match e {
        TypstAsLibError::TypstSource(diags) => diags
            .iter()
            .map(|d| d.message.to_string())
            .collect::<Vec<_>>()
            .join("; "),
        other => other.to_string(),
    };
    RenderError::Template(text)
}

/// Compiles the receipt and checks its height: `Ok((document, warnings))`.
/// A macro, so the document's inferred type stays concrete at each use.
macro_rules! compile {
    ($data:expr, $settings:expr) => {{
        let mut inputs = Dict::new();
        inputs.insert("data".into(), $data.to_json($settings).into_value());
        let Warned { output, warnings } = ENGINE.compile_with_input(inputs);
        match output.map_err(template_error) {
            Err(e) => Err(e),
            Ok(doc) => {
                let doc = pin(doc, typst_render::render_merged);
                let tallest = doc
                    .pages()
                    .iter()
                    .map(|p| p.frame.height().to_mm())
                    .fold(0.0_f64, f64::max);
                if tallest > f64::from(MAX_HEIGHT_MM) {
                    Err(RenderError::TooTall(tallest.round() as u32))
                } else {
                    let warnings: Vec<String> =
                        warnings.iter().map(|w| w.message.to_string()).collect();
                    Ok((doc, warnings))
                }
            }
        }
    }};
}

/// The page frames and the typst warnings (tests inspect the glyphs).
#[cfg(test)]
pub(crate) fn frames(
    data: &ReceiptData,
    settings: &ReceiptSettings,
) -> Result<(Vec<typst::layout::Frame>, Vec<String>), RenderError> {
    let (doc, warnings) = compile!(data, settings)?;
    Ok((
        doc.pages().iter().map(|p| p.frame.clone()).collect(),
        warnings,
    ))
}

/// The pixel size `typst-render` gives a `width_pt × height_pt` page at `dpi`
/// (its rounding, in `f64` so nothing overflows).
pub(crate) fn pixel_size(width_pt: f64, height_pt: f64, dpi: f32) -> (f64, f64) {
    let per_pt = f64::from(dpi.max(1.0)) / 72.0;
    (
        (per_pt * width_pt).round().max(1.0),
        (per_pt * height_pt).round().max(1.0),
    )
}

/// Refuses a page above [`MAX_PIXELS`] at `dpi`, before anything is allocated.
pub(crate) fn check_pixels(width_pt: f64, height_pt: f64, dpi: f32) -> Result<(), RenderError> {
    let (w, h) = pixel_size(width_pt, height_pt, dpi);
    let total = w * h;
    if total.is_nan() || total > MAX_PIXELS as f64 {
        let clamp = |v: f64| {
            if v >= f64::from(u32::MAX) {
                u32::MAX
            } else {
                v as u32
            }
        };
        return Err(RenderError::TooLarge {
            width: clamp(w),
            height: clamp(h),
        });
    }
    Ok(())
}

fn check_frame(frame: &typst::layout::Frame, dpi: f32) -> Result<(), RenderError> {
    let size = frame.size();
    check_pixels(size.x.to_pt(), size.y.to_pt(), dpi)
}

fn options(dpi: f32) -> RenderOptions {
    RenderOptions {
        pixel_per_pt: Scalar::new(f64::from(dpi.max(1.0)) / 72.0),
        render_bleed: false,
    }
}

/// Renders the receipt at `dpi` (one page: the height follows the content).
pub fn render(
    data: &ReceiptData,
    settings: &ReceiptSettings,
    dpi: f32,
) -> Result<Vec<Raster>, RenderError> {
    let (doc, _) = compile!(data, settings)?;
    for page in doc.pages() {
        check_frame(&page.frame, dpi)?;
    }
    let opts = options(dpi);
    Ok(doc
        .pages()
        .iter()
        .map(|page| {
            let pixmap = typst_render::render(page, &opts);
            Raster {
                width: pixmap.width(),
                height: pixmap.height(),
                data: pixmap.take(),
            }
        })
        .collect())
}

/// Renders the receipt at `dpi` and encodes the first page as PNG.
pub fn render_png(
    data: &ReceiptData,
    settings: &ReceiptSettings,
    dpi: f32,
) -> Result<Vec<u8>, RenderError> {
    let (doc, _) = compile!(data, settings)?;
    let page = doc
        .pages()
        .first()
        .ok_or_else(|| RenderError::Template("dokumenti nuk ka faqe".to_owned()))?;
    check_frame(&page.frame, dpi)?;
    typst_render::render(page, &options(dpi))
        .encode_png()
        .map_err(|e| RenderError::Png(e.to_string()))
}

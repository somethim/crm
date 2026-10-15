//! Render tests (AC 2): dimensions, growth with the line count, PNG encoding,
//! Albanian glyphs, and the share of non-white pixels. Snapshot-free.

use crate::data::{ReceiptData, ReceiptLine, ReceiptSettings, UNNAMED_LINE};
use crate::receipt::{self, Raster, render, render_png};
use crm_domain::entities::{Order, OrderLine, OwnerSnapshot};
use crm_domain::{Money, OrderId, OrderKind, Qty, format, time};
use typst::layout::{Frame, FrameItem};

const DPI: f32 = 203.0;

fn line(i: usize, name: &str) -> OrderLine {
    let qty = (i as i64 % 7) + 1;
    let price = 850 + (i as i64) * 125;
    OrderLine {
        position: i as i64,
        product_id: None,
        sku_id: None,
        name: name.to_owned(),
        code: format!("A{i:04}"),
        unit: "copë".to_owned(),
        quantity: Qty(qty),
        price: Money(price),
        value: Money(qty * price),
    }
}

fn order(kind: OrderKind, names: &[&str]) -> Order {
    let lines: Vec<OrderLine> = names.iter().enumerate().map(|(i, n)| line(i, n)).collect();
    Order {
        id: OrderId(1042),
        kind,
        owner_id: None,
        owner: OwnerSnapshot {
            name: "Bar Çajupi Ëndrra".to_owned(),
            description: Some("Rruga e Pishave".to_owned()),
            nipt: None,
            address: None,
        },
        issued_on: format::parse_date("13.10.2026").unwrap(),
        total: Money(lines.iter().map(|l| l.value.0).sum()),
        created_at: time::now(),
        updated_at: time::now(),
        lines,
    }
}

fn sample(n: usize) -> ReceiptData {
    let names: Vec<String> = (0..n)
        .map(|i| match i % 3 {
            0 => format!("BRIZO PJESHKE 1L me çaj {i}"),
            1 => format!("Ujë mineral Glina 0.5L me një emër shumë të gjatë që mbështillet {i}"),
            _ => format!("ÇOKOLLATË ËMBËL {i}"),
        })
        .collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    ReceiptData::from_order(&order(OrderKind::Sale, &refs))
}

/// Share of pixels that are not (almost) white.
fn ink_ratio(r: &Raster) -> f64 {
    let dark = r
        .data
        .chunks_exact(4)
        .filter(|p| u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) < 600)
        .count();
    dark as f64 / f64::from(r.width * r.height)
}

fn single(data: &ReceiptData, settings: &ReceiptSettings, dpi: f32) -> Raster {
    let mut pages = render(data, settings, dpi).unwrap();
    assert_eq!(pages.len(), 1, "a receipt is one page");
    pages.remove(0)
}

#[test]
fn width_follows_the_setting_and_height_the_lines() {
    let settings = ReceiptSettings::default();
    let small = single(&sample(3), &settings, DPI);
    let large = single(&sample(60), &settings, DPI);
    let expected = (72.0_f64 / 25.4 * 203.0).round() as u32;
    assert_eq!(expected, 575);
    assert_eq!(small.width, expected);
    assert_eq!(large.width, expected);
    assert_eq!(small.data.len(), (small.width * small.height * 4) as usize);
    assert!(small.height > 200, "3 lines: {}", small.height);
    assert!(
        large.height > small.height * 5,
        "{} vs {}",
        large.height,
        small.height
    );
    // Roughly 72 mm × (height) at 203 DPI: a 60-line receipt stays well below the guard.
    assert!(f64::from(large.height) / 203.0 * 25.4 < f64::from(receipt::MAX_HEIGHT_MM));

    let wide = single(
        &sample(3),
        &ReceiptSettings {
            width_mm: 80,
            font_pt: 9,
        },
        DPI,
    );
    assert_eq!(wide.width, (80.0_f64 / 25.4 * 203.0).round() as u32);
}

#[test]
fn the_page_is_white_with_some_ink() {
    let r = single(&sample(3), &ReceiptSettings::default(), DPI);
    assert!(r.data.chunks_exact(4).all(|p| p[3] == 255), "opaque page");
    assert_eq!(&r.data[..4], &[255, 255, 255, 255], "white corner");
    let ink = ink_ratio(&r);
    assert!((0.01..0.35).contains(&ink), "ink ratio {ink}");
    let big = single(&sample(60), &ReceiptSettings::default(), DPI);
    let ink = ink_ratio(&big);
    assert!((0.01..0.35).contains(&ink), "ink ratio {ink}");
}

#[test]
fn a_larger_font_makes_a_taller_receipt() {
    let data = sample(10);
    let normal = single(&data, &ReceiptSettings::default(), DPI);
    let larger = single(
        &data,
        &ReceiptSettings {
            width_mm: 72,
            font_pt: 12,
        },
        DPI,
    );
    assert!(larger.height > normal.height);
}

#[test]
fn png_encodes() {
    let png = render_png(&sample(3), &ReceiptSettings::default(), 300.0).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    // IHDR width (big endian) at 300 DPI: round(72 / 25.4 * 300) = 850.
    let width = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
    assert_eq!(width, 850);
    if let Ok(dir) = std::env::var("CRM_RECEIPT_SAMPLE_DIR") {
        std::fs::write(std::path::Path::new(&dir).join("receipt-sample.png"), &png).unwrap();
        let long = render_png(&sample(60), &ReceiptSettings::default(), 300.0).unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join("receipt-sample-60.png"),
            &long,
        )
        .unwrap();
    }
}

fn collect_glyphs(frame: &Frame, out: &mut Vec<(String, u16)>) {
    for (_, item) in frame.items() {
        match item {
            FrameItem::Group(g) => collect_glyphs(&g.frame, out),
            FrameItem::Text(t) => {
                for g in &t.glyphs {
                    let text = t.text[g.range()].to_string();
                    out.push((text, g.id));
                }
            }
            _ => {}
        }
    }
}

#[test]
fn albanian_letters_have_glyphs_and_no_warnings() {
    let data = sample(6);
    let (frames, warnings) = receipt::frames(&data, &ReceiptSettings::default()).unwrap();
    assert!(warnings.is_empty(), "typst warnings: {warnings:?}");
    let mut glyphs = Vec::new();
    for f in &frames {
        collect_glyphs(f, &mut glyphs);
    }
    let text: String = glyphs.iter().map(|(t, _)| t.as_str()).collect();
    for c in ['ë', 'ç', 'Ë', 'Ç'] {
        assert!(text.contains(c), "{c} is printed");
    }
    let missing: Vec<&String> = glyphs
        .iter()
        .filter(|(_, id)| *id == 0)
        .map(|(t, _)| t)
        .collect();
    assert!(missing.is_empty(), "missing glyphs for {missing:?}");
    for word in [
        "Porosia Nr: 1042",
        "Data: 13.10.2026",
        "Klient",
        "Përshkrimi",
        "Çmimi",
        "Totali",
    ] {
        let compact: String = word.chars().filter(|c| !c.is_whitespace()).collect();
        let flat: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(flat.contains(&compact), "{word} in {text}");
    }
}

#[test]
fn from_order_formats_every_field() {
    let mut o = order(OrderKind::Purchase, &["", "Lëng portokalli"]);
    o.lines[1].price = Money(850);
    o.lines[1].quantity = Qty(1200);
    o.lines[1].value = Money(1_020_000);
    o.total = Money(o.lines[0].value.0 + 1_020_000);
    o.owner.description = Some("   ".to_owned());
    let d = ReceiptData::from_order(&o);
    assert_eq!(d.nr, "1042");
    assert_eq!(d.kind, OrderKind::Purchase);
    assert_eq!(d.issued_on, "13.10.2026");
    assert_eq!(d.owner_label, "Furnitor");
    assert_eq!(d.owner_name, "Bar Çajupi Ëndrra");
    assert_eq!(d.description, "");
    assert_eq!(d.lines[0].name, UNNAMED_LINE);
    assert_eq!(
        d.lines[1],
        ReceiptLine {
            idx: "2".to_owned(),
            name: "Lëng portokalli".to_owned(),
            qty: "1.200".to_owned(),
            price: "8,50".to_owned(),
            value: "10.200".to_owned(),
        }
    );
    let sale = ReceiptData::from_order(&order(OrderKind::Sale, &["x"]));
    assert_eq!(sale.owner_label, "Klient");
    assert_eq!(sale.description, "Rruga e Pishave");
}

#[test]
fn an_empty_description_is_not_printed() {
    let mut data = sample(2);
    data.description.clear();
    let (frames, _) = receipt::frames(&data, &ReceiptSettings::default()).unwrap();
    let mut glyphs = Vec::new();
    for f in &frames {
        collect_glyphs(f, &mut glyphs);
    }
    let text: String = glyphs.iter().map(|(t, _)| t.as_str()).collect();
    assert!(!text.contains("Përshkrimi"));
}

#[test]
fn markup_in_names_is_printed_as_text() {
    let mut data = sample(1);
    data.owner_name = "#panic(\"x\") *bold* $x$".to_owned();
    let pages = render(&data, &ReceiptSettings::default(), 96.0).unwrap();
    assert_eq!(pages.len(), 1);
}

#[test]
fn the_height_guard_rejects_huge_receipts() {
    let data = sample(1500);
    let err = render(&data, &ReceiptSettings::default(), 72.0).unwrap_err();
    assert!(
        matches!(err, crate::RenderError::TooTall(mm) if mm > 2000),
        "{err:?}"
    );
}

/// Points in `mm` millimetres.
fn pt(mm: f64) -> f64 {
    mm / 25.4 * 72.0
}

#[test]
fn the_pixel_guard_admits_the_largest_valid_receipt_at_the_preview_dpi() {
    // 110 mm × MAX_HEIGHT_MM at 384 DPI (the preview on a 200 % screen) and
    // at 300 DPI (print and photo) fit under MAX_PIXELS.
    let tallest = pt(f64::from(receipt::MAX_HEIGHT_MM));
    for dpi in [96.0, 203.0, 300.0, 384.0] {
        assert_eq!(
            receipt::check_pixels(pt(110.0), tallest, dpi),
            Ok(()),
            "{dpi}"
        );
    }
    let (w, h) = receipt::pixel_size(pt(110.0), tallest, 384.0);
    assert!(w * h < crate::MAX_PIXELS as f64);
}

#[test]
fn the_pixel_guard_rejects_absurd_sizes_without_overflowing() {
    for (w, h, dpi) in [
        (pt(f64::from(u32::MAX)), pt(100.0), 203.0),
        (pt(110.0), pt(2000.0), 1_000_000.0),
        (pt(110.0), pt(2000.0), f32::MAX),
        (f64::MAX, f64::MAX, 203.0),
    ] {
        let err = receipt::check_pixels(w, h, dpi).unwrap_err();
        assert!(
            matches!(err, crate::RenderError::TooLarge { .. }),
            "{err:?}"
        );
    }
}

#[test]
fn a_corrupt_width_setting_is_an_error_not_a_panic() {
    let data = sample(3);
    for width_mm in [u32::MAX, 4_000_000_000, 100_000] {
        let settings = ReceiptSettings {
            width_mm,
            font_pt: 9,
        };
        let err = render(&data, &settings, 192.0).unwrap_err();
        assert!(
            matches!(err, crate::RenderError::TooLarge { .. }),
            "{err:?}"
        );
        let err = render_png(&data, &settings, 300.0).unwrap_err();
        assert!(
            matches!(err, crate::RenderError::TooLarge { .. }),
            "{err:?}"
        );
    }
    // An absurd resolution is refused too, not a multi-GB buffer (F-022).
    for dpi in [20_000.0, 1_000_000.0, f32::MAX] {
        let err = render(&data, &ReceiptSettings::default(), dpi).unwrap_err();
        assert!(
            matches!(err, crate::RenderError::TooLarge { .. }),
            "{err:?}"
        );
    }
    // An in-range width still renders.
    let ok = single(&data, &ReceiptSettings::default(), 192.0);
    assert!(ok.width > 0);
}

#[test]
fn a_word_wider_than_its_column_wraps_inside_the_column() {
    let settings = ReceiptSettings::default();
    let mut short = sample(1);
    short.lines[0].name = "UJE".to_owned();
    let mut long = sample(1);
    long.lines[0].name = format!("{}-ÇOKOLLATË_PA_HAPËSIRA", "X".repeat(40));
    long.owner_name = "Y".repeat(80);
    let (frames, warnings) = receipt::frames(&long, &settings).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let mut glyphs = Vec::new();
    for f in &frames {
        collect_glyphs(f, &mut glyphs);
    }
    assert!(glyphs.iter().all(|(_, id)| *id != 0), "no missing glyph");
    let flat: String = glyphs
        .iter()
        .map(|(t, _)| t.as_str())
        .collect::<String>()
        .replace(['\u{200B}', '\n', ' '], "");
    assert!(flat.contains("ÇOKOLLATË_PA_HAPËSIRA"), "{flat}");
    // The name takes several lines instead of running over Sasia/Çmimi/Vlera, and
    // the page keeps its width.
    let a = single(&short, &settings, DPI);
    let b = single(&long, &settings, DPI);
    assert_eq!(b.width, a.width);
    assert!(b.height > a.height + 100, "{} vs {}", b.height, a.height);
    if let Ok(dir) = std::env::var("CRM_RECEIPT_SAMPLE_DIR") {
        let png = render_png(&long, &settings, 300.0).unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join("receipt-long-word.png"),
            png,
        )
        .unwrap();
    }
}

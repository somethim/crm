//! Building the report rows from order lines.

use super::{Cell, ReportKind, ReportLine, ReportTable};
use crm_domain::{Money, OrderId, ProductId, format};
use jiff::civil::Date;
use std::collections::HashMap;

const TOTAL_TITLE: &str = "REGJISTRI ANALITIK I POROSIVE";
const TOTAL_HEADERS: [&str; 10] = [
    "Lloji",
    "Data",
    "Numër",
    "Klienti/Furnitori",
    "Kodi",
    "Përshkrimi",
    "Njësia",
    "Sasia",
    "Çmimi",
    "Vlera",
];
const BY_PRODUCTS_TITLE: &str = "RAPORTI SIPAS ARTIKUJVE";
const BY_PRODUCTS_HEADERS: [&str; 6] = [
    "Kodi",
    "Përshkrimi",
    "Njësia",
    "Sasia Totale",
    "Çmimi Mesatar",
    "Vlera Totale",
];

/// Builds a report for `from..=to` from `lines` (already filtered on `issued_on`
/// and in report order).
pub fn build(kind: ReportKind, from: Date, to: Date, lines: &[ReportLine]) -> ReportTable {
    let (title, headers, rows) = match kind {
        ReportKind::Total => (TOTAL_TITLE, &TOTAL_HEADERS[..], total_rows(lines)),
        ReportKind::ByProducts => (
            BY_PRODUCTS_TITLE,
            &BY_PRODUCTS_HEADERS[..],
            by_product_rows(lines),
        ),
    };
    ReportTable {
        title: title.to_owned(),
        subtitle: format::report_range(from, to),
        headers: headers.iter().map(|h| (*h).to_owned()).collect(),
        rows,
    }
}

fn text(s: &str) -> Cell {
    Cell::Text(s.to_owned())
}

/// One row per line; Lloji, Data, Numër and the owner only on an order's first line.
fn total_rows(lines: &[ReportLine]) -> Vec<Vec<Cell>> {
    let mut previous: Option<OrderId> = None;
    lines
        .iter()
        .map(|line| {
            let first = previous != Some(line.order_id);
            previous = Some(line.order_id);
            let head = if first {
                [
                    text(line.order_kind.label()),
                    Cell::Text(format::date(line.issued_on)),
                    Cell::Int(line.order_id.get()),
                    text(&line.owner_name),
                ]
            } else {
                [text(""), text(""), text(""), text("")]
            };
            head.into_iter()
                .chain([
                    text(&line.code),
                    text(&line.name),
                    text(&line.unit),
                    Cell::Int(line.quantity.get()),
                    Cell::Money(line.price),
                    Cell::Money(line.value),
                ])
                .collect()
        })
        .collect()
}

#[derive(PartialEq, Eq, Hash)]
enum Key<'a> {
    Product(ProductId),
    Name(&'a str),
}

struct Aggregate<'a> {
    first: &'a ReportLine,
    quantity: i64,
    total: Money,
}

/// One row per product (or per line name for deleted products), in order of first
/// appearance, with the average price `total / quantity` rounded half-up.
fn by_product_rows(lines: &[ReportLine]) -> Vec<Vec<Cell>> {
    let mut order: Vec<Aggregate<'_>> = Vec::new();
    let mut index: HashMap<Key<'_>, usize> = HashMap::new();
    for line in lines {
        let key = line
            .product_id
            .map_or(Key::Name(line.name.as_str()), Key::Product);
        let slot = *index.entry(key).or_insert_with(|| {
            order.push(Aggregate {
                first: line,
                quantity: 0,
                total: Money::ZERO,
            });
            order.len() - 1
        });
        let agg = &mut order[slot];
        agg.quantity += line.quantity.get();
        agg.total += line.value;
    }
    order
        .into_iter()
        .map(|agg| {
            vec![
                text(&agg.first.code),
                text(&agg.first.name),
                text(&agg.first.unit),
                Cell::Int(agg.quantity),
                Cell::Money(average(agg.total, agg.quantity)),
                Cell::Money(agg.total),
            ]
        })
        .collect()
}

/// `total / quantity` in qindarka, rounded half away from zero; 0 when quantity is 0.
pub(crate) fn average(total: Money, quantity: i64) -> Money {
    if quantity == 0 {
        return Money::ZERO;
    }
    let (t, q) = (i128::from(total.minor()), i128::from(quantity));
    let negative = (t < 0) != (q < 0);
    let (t, q) = (t.abs(), q.abs());
    let rounded = (2 * t + q) / (2 * q);
    let signed = if negative { -rounded } else { rounded };
    Money(i64::try_from(signed).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crm_domain::{OrderKind, Qty};
    use jiff::civil::date;

    #[allow(clippy::too_many_arguments)]
    fn line(
        order: i64,
        kind: OrderKind,
        day: i8,
        owner: &str,
        product: Option<i64>,
        code: &str,
        name: &str,
        qty: i64,
        price: i64,
    ) -> ReportLine {
        ReportLine {
            order_id: OrderId(order),
            order_kind: kind,
            issued_on: date(2026, 10, day),
            owner_name: owner.into(),
            product_id: product.map(ProductId),
            code: code.into(),
            name: name.into(),
            unit: "COPE".into(),
            quantity: Qty(qty),
            price: Money(price),
            value: Money(qty * price),
        }
    }

    fn lines() -> Vec<ReportLine> {
        use OrderKind::{Purchase, Sale};
        vec![
            line(7, Sale, 3, "Bar Alfa", Some(1), "L0009", "BRIZO", 2, 6_500),
            line(
                7,
                Sale,
                3,
                "Bar Alfa",
                Some(2),
                "P0035",
                "BAKE ROLLS",
                1,
                6_600,
            ),
            line(
                9,
                Purchase,
                4,
                "Furnitori",
                Some(1),
                "L0009",
                "BRIZO",
                10,
                6_200,
            ),
            line(
                9,
                Purchase,
                4,
                "Furnitori",
                None,
                "",
                "KAFE E VJETËR",
                3,
                1_000,
            ),
            line(12, Sale, 5, "Bar Beta", None, "", "KAFE E VJETËR", 1, 1_200),
            line(12, Sale, 5, "Bar Beta", Some(1), "L0009", "BRIZO", 1, 6_501),
        ]
    }

    fn t(s: &str) -> Cell {
        text(s)
    }

    #[test]
    fn total_report() {
        let table = build(
            ReportKind::Total,
            date(2026, 10, 1),
            date(2026, 10, 31),
            &lines(),
        );
        assert_eq!(table.title, "REGJISTRI ANALITIK I POROSIVE");
        assert_eq!(table.subtitle, "01.10.2026 - 31.10.2026");
        assert_eq!(table.headers, TOTAL_HEADERS);
        assert_eq!(table.rows.len(), 6);
        assert_eq!(
            table.rows[0],
            [
                t("Shitje"),
                t("03.10.2026"),
                Cell::Int(7),
                t("Bar Alfa"),
                t("L0009"),
                t("BRIZO"),
                t("COPE"),
                Cell::Int(2),
                Cell::Money(Money(6_500)),
                Cell::Money(Money(13_000)),
            ]
        );
        // Later lines of the same order leave the order cells blank.
        assert_eq!(table.rows[1][..4], [t(""), t(""), t(""), t("")]);
        assert_eq!(table.rows[1][5], t("BAKE ROLLS"));
        assert_eq!(
            table.rows[2][..4],
            [t("Blerje"), t("04.10.2026"), Cell::Int(9), t("Furnitori")]
        );
        assert_eq!(table.rows[3][..4], [t(""), t(""), t(""), t("")]);
        assert_eq!(table.rows[4][0], t("Shitje"));
        assert!(table.rows.iter().all(|r| r.len() == 10));
    }

    #[test]
    fn by_products_report() {
        let table = build(
            ReportKind::ByProducts,
            date(2026, 10, 1),
            date(2026, 10, 31),
            &lines(),
        );
        assert_eq!(table.title, "RAPORTI SIPAS ARTIKUJVE");
        assert_eq!(table.subtitle, "01.10.2026 - 31.10.2026");
        assert_eq!(table.headers, BY_PRODUCTS_HEADERS);
        // BRIZO: 2×6500 + 10×6200 + 1×6501 = 81501 over 13 → 6269.3 → 6269.
        // The deleted product groups by name: 3×1000 + 1×1200 = 4200 over 4 = 1050.
        assert_eq!(
            table.rows,
            vec![
                vec![
                    t("L0009"),
                    t("BRIZO"),
                    t("COPE"),
                    Cell::Int(13),
                    Cell::Money(Money(6_269)),
                    Cell::Money(Money(81_501)),
                ],
                vec![
                    t("P0035"),
                    t("BAKE ROLLS"),
                    t("COPE"),
                    Cell::Int(1),
                    Cell::Money(Money(6_600)),
                    Cell::Money(Money(6_600)),
                ],
                vec![
                    t(""),
                    t("KAFE E VJETËR"),
                    t("COPE"),
                    Cell::Int(4),
                    Cell::Money(Money(1_050)),
                    Cell::Money(Money(4_200)),
                ],
            ]
        );
    }

    #[test]
    fn average_rounds_half_up() {
        assert_eq!(average(Money(5), 2), Money(3)); // 2.5 → 3
        assert_eq!(average(Money(7), 3), Money(2)); // 2.33 → 2
        assert_eq!(average(Money(8), 3), Money(3)); // 2.67 → 3
        assert_eq!(average(Money(-5), 2), Money(-3));
        assert_eq!(average(Money(100), 0), Money::ZERO);
        assert_eq!(average(Money(i64::MAX), 1), Money(i64::MAX));
    }

    #[test]
    fn empty_report() {
        let table = build(ReportKind::Total, date(2026, 1, 1), date(2026, 1, 1), &[]);
        assert!(table.rows.is_empty());
        assert_eq!(table.subtitle, "01.01.2026 - 01.01.2026");
    }
}

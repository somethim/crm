//! Table rows for the list windows: formatted cells (through
//! `crm_domain::format` only), numeric sort keys for money and quantities, and
//! the row id kept next to each row for the selection.

use crate::shell::format;
use crm_services::domain::{Owner, ProductRow};
use slint::{ModelRc, SharedString, StandardListViewItem, VecModel};
use std::cmp::Ordering;
use std::rc::Rc;

/// How a column sorts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SortKey {
    /// Case-insensitive text.
    Text(String),
    /// Money in qindarka or a quantity.
    Number(i64),
}

impl SortKey {
    fn text(s: &str) -> SortKey {
        SortKey::Text(s.to_lowercase())
    }

    fn compare(&self, other: &SortKey) -> Ordering {
        match (self, other) {
            (SortKey::Number(a), SortKey::Number(b)) => a.cmp(b),
            (SortKey::Text(a), SortKey::Text(b)) => a.cmp(b),
            (SortKey::Number(_), SortKey::Text(_)) => Ordering::Less,
            (SortKey::Text(_), SortKey::Number(_)) => Ordering::Greater,
        }
    }
}

/// One table row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The product or owner id.
    pub id: i64,
    /// The displayed cells, in column order.
    pub cells: Vec<SharedString>,
    /// The sort key of each cell.
    pub keys: Vec<SortKey>,
}

/// Columns: Kodi, Emri, Njësia, Çmimi i blerjes, Çmimi i shitjes, Gjendja.
pub fn product_row(r: &ProductRow) -> Row {
    Row {
        id: r.product.id.0,
        cells: vec![
            r.sku.code.as_str().into(),
            r.product.name.as_str().into(),
            r.sku.unit.as_str().into(),
            format::amount(r.sku.buy_price).into(),
            format::amount(r.sku.sell_price).into(),
            format::qty(r.sku.quantity).into(),
        ],
        keys: vec![
            SortKey::text(&r.sku.code),
            SortKey::text(&r.product.name),
            SortKey::text(&r.sku.unit),
            SortKey::Number(r.sku.buy_price.0),
            SortKey::Number(r.sku.sell_price.0),
            SortKey::Number(r.sku.quantity.0),
        ],
    }
}

/// Columns: Emri, Përshkrimi, NIPT, Adresa, Llogari.
pub fn owner_row(o: &Owner) -> Row {
    let opt = |v: &Option<String>| v.as_deref().unwrap_or("").to_owned();
    let fields = [
        o.name.clone(),
        opt(&o.description),
        opt(&o.nipt),
        opt(&o.address),
        opt(&o.account_code),
    ];
    Row {
        id: o.id.0,
        cells: fields.iter().map(|f| f.as_str().into()).collect(),
        keys: fields.iter().map(|f| SortKey::text(f)).collect(),
    }
}

/// Sorts `rows` by `column` (stable, so equal keys keep the load order).
pub fn sort(rows: &mut [Row], column: usize, ascending: bool) {
    rows.sort_by(|a, b| {
        let ord = match (a.keys.get(column), b.keys.get(column)) {
            (Some(x), Some(y)) => x.compare(y),
            _ => Ordering::Equal,
        };
        if ascending { ord } else { ord.reverse() }
    });
}

/// The Slint model of `rows`.
pub fn table_rows(rows: &[Row]) -> Vec<ModelRc<StandardListViewItem>> {
    rows.iter()
        .map(|r| {
            let cells: Vec<StandardListViewItem> = r
                .cells
                .iter()
                .map(|c| StandardListViewItem::from(c.clone()))
                .collect();
            ModelRc::from(Rc::new(VecModel::from(cells)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: i64, key: SortKey) -> Row {
        Row {
            id,
            cells: vec![],
            keys: vec![key],
        }
    }

    #[test]
    fn money_sorts_numerically_and_text_ignores_case() {
        let mut rows = vec![
            row(1, SortKey::Number(1_000)),
            row(2, SortKey::Number(200)),
            row(3, SortKey::Number(30_000)),
        ];
        sort(&mut rows, 0, true);
        assert_eq!(rows.iter().map(|r| r.id).collect::<Vec<_>>(), [2, 1, 3]);
        sort(&mut rows, 0, false);
        assert_eq!(rows.iter().map(|r| r.id).collect::<Vec<_>>(), [3, 1, 2]);

        let mut rows = vec![row(1, SortKey::text("beta")), row(2, SortKey::text("Alfa"))];
        sort(&mut rows, 0, true);
        assert_eq!(rows[0].id, 2);
    }
}

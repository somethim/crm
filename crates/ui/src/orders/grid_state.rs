//! The lines of one invoice document as the grid edits them (no Slint).
//!
//! The grid shows the entries plus a trailing placeholder row; this state holds
//! the entries only. Row `entries.len()` is the placeholder. Values are typed
//! as text and committed through the domain parsers.

use super::grid_nav::{PRICE_COL, SELL_COL};
use super::texts;
use crm_services::domain::format::{self, ParseMode};
use crm_services::domain::{
    DraftLine, Money, Order, OrderDraft, OrderKind, ProductId, ProductRow, Qty,
};
use std::collections::HashMap;

/// A status-bar message for a rejected value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlashMsg(pub String);

/// One entry row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Line {
    /// The chosen product; `None` for an empty row or a free-text purchase line.
    pub product_id: Option<ProductId>,
    /// The product name (or the typed name of a free-text line).
    pub name: String,
    /// "Sasia".
    pub qty: Qty,
    /// Sale "Çmimi" (0 = free goods; a blank cell takes `list_price`, F-012).
    pub price: Money,
    /// Purchase "Çmimi i blerjes".
    pub buy: Money,
    /// Purchase "Çmimi i shitjes".
    pub sell: Money,
    /// The SKU stock ("/ N"), when the line has a product.
    pub stock: Option<Qty>,
    /// Sale: the SKU sell price a blank "Çmimi" falls back to (0 when the
    /// product is unknown).
    pub list_price: Money,
    /// A saved line whose product was deleted since (F-017): only Sasia and
    /// the removal can change; re-saving keeps its snapshot.
    pub orphan: bool,
}

impl Line {
    /// A line of `product` (quantity 0, prices from the SKU; a sale uses only
    /// `price`, a purchase only `buy` and `sell`).
    fn from_product(kind: OrderKind, row: &ProductRow) -> Line {
        let (price, buy, sell) = match kind {
            OrderKind::Sale => (row.sku.sell_price, Money::ZERO, Money::ZERO),
            OrderKind::Purchase => (row.sku.buy_price, row.sku.buy_price, row.sku.sell_price),
        };
        Line {
            product_id: Some(row.product.id),
            name: row.product.name.clone(),
            qty: Qty::ZERO,
            price,
            buy,
            sell,
            stock: Some(row.sku.quantity),
            list_price: row.sku.sell_price,
            orphan: false,
        }
    }

    /// Whether nothing was entered in the row.
    pub fn is_empty(&self) -> bool {
        self.product_id.is_none() && self.name.trim().is_empty() && self.qty == Qty::ZERO
    }
}

/// Looks up a product's current row (stock and prices) by id.
pub type Lookup<'a> = &'a dyn Fn(ProductId) -> Option<ProductRow>;

/// The lines of a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridState {
    kind: OrderKind,
    entries: Vec<Line>,
    /// Sale: the quantity of each product already in the saved invoice (an edit
    /// may sell up to the stock plus this).
    saved_qty: HashMap<ProductId, Qty>,
}

impl GridState {
    /// A fresh document: one empty entry (plus the placeholder).
    pub fn new(kind: OrderKind) -> GridState {
        GridState {
            kind,
            entries: vec![Line::default()],
            saved_qty: HashMap::new(),
        }
    }

    /// The lines of a saved invoice; stock and current SKU prices from `lookup`.
    pub fn from_order(order: &Order, lookup: Lookup<'_>) -> GridState {
        let entries = order
            .lines
            .iter()
            .map(|l| {
                let row = l.product_id.and_then(lookup);
                let (buy, sell) = match order.kind {
                    OrderKind::Purchase => (
                        l.price,
                        row.as_ref().map_or(Money::ZERO, |r| r.sku.sell_price),
                    ),
                    OrderKind::Sale => (Money::ZERO, Money::ZERO),
                };
                Line {
                    product_id: l.product_id,
                    name: l.name.clone(),
                    qty: l.quantity,
                    price: l.price,
                    buy,
                    sell,
                    list_price: row.as_ref().map_or(Money::ZERO, |r| r.sku.sell_price),
                    stock: row.map(|r| r.sku.quantity),
                    // A stored line without a product: its product was deleted.
                    orphan: l.product_id.is_none(),
                }
            })
            .collect::<Vec<_>>();
        let mut state = GridState {
            kind: order.kind,
            entries,
            saved_qty: saved_quantities(order),
        };
        state.ensure_entry();
        state
    }

    /// The lines of a persisted draft (`saved`: the invoice it edits, if any).
    pub fn from_draft(draft: &OrderDraft, saved: Option<&Order>, lookup: Lookup<'_>) -> GridState {
        // The saved lines of deleted products; a product-less draft line with
        // the same name is one of them (claimed once, as `save_order` does).
        let mut orphans: Vec<&str> = saved
            .map(|o| {
                o.lines
                    .iter()
                    .filter(|l| l.product_id.is_none())
                    .map(|l| l.name.trim())
                    .collect()
            })
            .unwrap_or_default();
        let entries = draft
            .lines
            .iter()
            .map(|l| {
                let row = l.product_id.and_then(lookup);
                Line {
                    product_id: l.product_id,
                    name: l.name.clone(),
                    qty: l.quantity,
                    price: l.price,
                    buy: l.buy_price.unwrap_or(Money::ZERO),
                    sell: l.sell_price.unwrap_or(Money::ZERO),
                    list_price: row.as_ref().map_or(Money::ZERO, |r| r.sku.sell_price),
                    stock: row.map(|r| r.sku.quantity),
                    orphan: l.product_id.is_none()
                        && orphans
                            .iter()
                            .position(|n| *n == l.name.trim())
                            .map(|at| orphans.swap_remove(at))
                            .is_some(),
                }
            })
            .collect();
        let mut state = GridState {
            kind: draft.kind,
            entries,
            saved_qty: saved.map(saved_quantities).unwrap_or_default(),
        };
        state.ensure_entry();
        state
    }

    fn ensure_entry(&mut self) {
        if self.entries.is_empty() {
            self.entries.push(Line::default());
        }
    }

    /// Shitje or Blerje.
    pub fn kind(&self) -> OrderKind {
        self.kind
    }

    /// The entries (without the placeholder).
    pub fn entries(&self) -> &[Line] {
        &self.entries
    }

    /// Rows shown, the placeholder included.
    pub fn rows(&self) -> usize {
        self.entries.len() + 1
    }

    /// Whether `row` is the placeholder.
    pub fn is_placeholder(&self, row: usize) -> bool {
        row >= self.entries.len()
    }

    /// The entry at `row` (`None` for the placeholder).
    pub fn line(&self, row: usize) -> Option<&Line> {
        self.entries.get(row)
    }

    /// Whether `row` is a deleted product's line (read-only but Sasia, F-017).
    pub fn is_orphan(&self, row: usize) -> bool {
        self.entries.get(row).is_some_and(|l| l.orphan)
    }

    /// The most a sale line of `product` may sell: the stock plus what the
    /// saved invoice already sells of it.
    pub fn max_qty(&self, row: usize) -> Option<Qty> {
        if self.kind != OrderKind::Sale {
            return None;
        }
        let line = self.entries.get(row)?;
        let stock = line.stock?;
        let saved = line
            .product_id
            .and_then(|id| self.saved_qty.get(&id).copied())
            .unwrap_or(Qty::ZERO);
        Some(stock + saved)
    }

    /// Chooses `product` for `row`: on the placeholder it appends an entry,
    /// elsewhere it replaces the row. The quantity starts at 0. Returns the row.
    pub fn choose_product(&mut self, row: usize, product: &ProductRow) -> usize {
        if self.is_orphan(row) {
            return row;
        }
        let line = Line::from_product(self.kind, product);
        if self.is_placeholder(row) {
            self.entries.push(line);
            self.entries.len() - 1
        } else {
            self.entries[row] = line;
            row
        }
    }

    /// Commits Sasia. Blank = 0. A non-number or (sale) a quantity above
    /// [`GridState::max_qty`] resets it to 0 and returns the flash text.
    /// Returns whether the value changed. The placeholder ignores it.
    pub fn set_qty(&mut self, row: usize, text: &str) -> Result<bool, (bool, FlashMsg)> {
        let max = self.max_qty(row);
        let Some(line) = self.entries.get_mut(row) else {
            return Ok(false);
        };
        let before = line.qty;
        let parsed = if text.trim().is_empty() {
            Ok(Qty::ZERO)
        } else {
            format::parse_qty(text).map_err(|_| FlashMsg(texts::INVALID_QTY.to_owned()))
        };
        let checked = parsed.and_then(|q| match max {
            Some(max) if q > max => Err(FlashMsg(texts::max_qty(max))),
            _ => Ok(q),
        });
        match checked {
            Ok(q) => {
                line.qty = q;
                Ok(q != before)
            }
            Err(msg) => {
                line.qty = Qty::ZERO;
                Err((before != Qty::ZERO, msg))
            }
        }
    }

    /// Commits a price cell (`col` 2: sale Çmimi / purchase Çmimi i blerjes;
    /// 3: purchase Çmimi i shitjes). A blank sale price takes the SKU sell price
    /// back, a typed 0 stays 0 (free goods, F-012); a blank purchase price is 0.
    /// An invalid amount keeps the old value and returns the flash text.
    /// Returns whether the value changed.
    pub fn set_price(&mut self, row: usize, col: usize, text: &str) -> Result<bool, FlashMsg> {
        let kind = self.kind;
        let Some(line) = self.entries.get_mut(row).filter(|l| !l.orphan) else {
            return Ok(false);
        };
        let value = if text.trim().is_empty() {
            match (kind, col) {
                (OrderKind::Sale, PRICE_COL) => line.list_price,
                _ => Money::ZERO,
            }
        } else {
            format::parse_amount(text, ParseMode::Input).map_err(|e| FlashMsg(e.user_message()))?
        };
        if value < Money::ZERO {
            return Err(FlashMsg(
                format::ParseError::Amount(text.trim().to_owned()).user_message(),
            ));
        }
        let slot = match (kind, col) {
            (OrderKind::Sale, PRICE_COL) => &mut line.price,
            (OrderKind::Purchase, PRICE_COL) => &mut line.buy,
            (OrderKind::Purchase, SELL_COL) => &mut line.sell,
            _ => return Ok(false),
        };
        let changed = *slot != value;
        *slot = value;
        Ok(changed)
    }

    /// Purchase: typed Artikulli text that differs from the row's name becomes
    /// a new-name line (on the placeholder: a new entry). Sale: ignored (the
    /// caller reverts the text). Returns whether anything changed.
    pub fn set_free_text(&mut self, row: usize, text: &str) -> bool {
        if self.kind != OrderKind::Purchase {
            return false;
        }
        let text = text.trim();
        match self.entries.get_mut(row) {
            Some(line) if line.orphan => false,
            Some(line) => {
                if line.name == text {
                    return false;
                }
                line.product_id = None;
                line.name = text.to_owned();
                line.stock = None;
                true
            }
            None if text.is_empty() => false,
            None => {
                self.entries.push(Line {
                    name: text.to_owned(),
                    ..Line::default()
                });
                true
            }
        }
    }

    /// Appends an empty entry and returns its row (the old placeholder's).
    pub fn add_empty_row(&mut self) -> usize {
        self.entries.push(Line::default());
        self.entries.len() - 1
    }

    /// Removes an entry (not the placeholder); one empty entry stays when the
    /// last one goes. Returns whether a row was removed.
    pub fn remove(&mut self, row: usize) -> bool {
        if row >= self.entries.len() {
            return false;
        }
        self.entries.remove(row);
        self.ensure_entry();
        true
    }

    /// Refreshes the stock and prices of every line of `product` after an edit
    /// in the product editor.
    pub fn refresh_product(&mut self, product: &ProductRow) {
        for line in &mut self.entries {
            if line.product_id == Some(product.product.id) {
                line.stock = Some(product.sku.quantity);
                line.name = product.product.name.clone();
            }
        }
    }

    /// "Vlera" of a row: quantity × unit price (purchase: the buy price).
    pub fn value(&self, row: usize) -> Money {
        self.entries
            .get(row)
            .map_or(Money::ZERO, |l| line_value(self.kind, l))
    }

    /// "Totali".
    pub fn total(&self) -> Money {
        self.entries
            .iter()
            .map(|l| line_value(self.kind, l))
            .fold(Money::ZERO, |a, b| a.checked_add(b).unwrap_or(a))
    }

    /// The draft lines (empty entries included; the placeholder not).
    pub fn draft_lines(&self) -> Vec<DraftLine> {
        self.entries
            .iter()
            .map(|l| match self.kind {
                OrderKind::Sale => DraftLine {
                    product_id: l.product_id,
                    name: l.name.clone(),
                    quantity: l.qty,
                    price: l.price,
                    buy_price: None,
                    sell_price: None,
                },
                OrderKind::Purchase => DraftLine {
                    product_id: l.product_id,
                    name: l.name.clone(),
                    quantity: l.qty,
                    price: l.buy,
                    buy_price: Some(l.buy),
                    sell_price: Some(l.sell),
                },
            })
            .collect()
    }
}

fn line_value(kind: OrderKind, line: &Line) -> Money {
    let unit = match kind {
        OrderKind::Sale => line.price,
        OrderKind::Purchase => line.buy,
    };
    unit.checked_mul_qty(line.qty).unwrap_or(Money::ZERO)
}

fn saved_quantities(order: &Order) -> HashMap<ProductId, Qty> {
    let mut map = HashMap::new();
    if order.kind == OrderKind::Sale {
        for line in &order.lines {
            if let Some(id) = line.product_id {
                *map.entry(id).or_insert(Qty::ZERO) += line.quantity;
            }
        }
    }
    map
}

/// The text of a quantity cell: blank for 0.
pub fn qty_text(q: Qty) -> String {
    if q == Qty::ZERO {
        String::new()
    } else {
        format::qty(q)
    }
}

/// The text of a price cell: blank for 0.
pub fn price_text(m: Money) -> String {
    if m == Money::ZERO {
        String::new()
    } else {
        format::amount(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crm_services::domain::{OrderId, OrderLine, OwnerSnapshot, Product, Sku, SkuId};

    fn product(id: i64, name: &str, stock: i64) -> ProductRow {
        ProductRow {
            product: Product {
                id: ProductId(id),
                sku_id: SkuId(id),
                name: name.into(),
            },
            sku: Sku {
                id: SkuId(id),
                code: format!("L{id:04}"),
                unit: "SHISHE".into(),
                buy_price: Money(5_000),
                sell_price: Money(6_500),
                quantity: Qty(stock),
            },
        }
    }

    #[test]
    fn a_fresh_document_has_one_entry_and_the_placeholder() {
        let g = GridState::new(OrderKind::Sale);
        assert_eq!(g.rows(), 2);
        assert!(g.line(0).unwrap().is_empty());
        assert!(g.is_placeholder(1));
        assert_eq!(g.total(), Money::ZERO);
    }

    #[test]
    fn choosing_on_the_placeholder_appends_elsewhere_replaces() {
        let mut g = GridState::new(OrderKind::Sale);
        assert_eq!(g.choose_product(1, &product(1, "BRIZO", 10)), 1);
        assert_eq!(g.rows(), 3);
        assert_eq!(g.choose_product(0, &product(2, "COCA", 4)), 0);
        assert_eq!(g.rows(), 3);
        let l = g.line(0).unwrap();
        assert_eq!(
            (l.product_id, l.name.as_str(), l.qty, l.price, l.stock),
            (
                Some(ProductId(2)),
                "COCA",
                Qty::ZERO,
                Money(6_500),
                Some(Qty(4))
            )
        );
        let mut p = GridState::new(OrderKind::Purchase);
        p.choose_product(0, &product(1, "BRIZO", 10));
        let l = p.line(0).unwrap();
        assert_eq!(
            (l.price, l.buy, l.sell),
            (Money(5_000), Money(5_000), Money(6_500))
        );
        // Replacing a row resets its quantity.
        g.set_qty(0, "3").unwrap();
        g.choose_product(0, &product(1, "BRIZO", 10));
        assert_eq!(g.line(0).unwrap().qty, Qty::ZERO);
    }

    #[test]
    fn sale_quantity_is_capped_by_the_stock() {
        let mut g = GridState::new(OrderKind::Sale);
        g.choose_product(0, &product(1, "BRIZO", 5));
        assert_eq!(g.set_qty(0, "5"), Ok(true));
        assert_eq!(
            g.set_qty(0, "6"),
            Err((true, FlashMsg("Maksimumi është 5".into())))
        );
        assert_eq!(g.line(0).unwrap().qty, Qty::ZERO);
        assert_eq!(
            g.set_qty(0, "abc"),
            Err((false, FlashMsg("Sasi e pavlefshme".into())))
        );
        assert_eq!(
            g.set_qty(0, "1,5"),
            Err((false, FlashMsg("Sasi e pavlefshme".into())))
        );
        assert_eq!(
            g.set_qty(0, "1.000").map_err(|e| e.1),
            Err(FlashMsg("Maksimumi është 5".into()))
        );
        assert_eq!(g.set_qty(0, ""), Ok(false));
        // The placeholder ignores commits.
        assert_eq!(g.set_qty(1, "3"), Ok(false));
        assert_eq!(g.rows(), 2);
    }

    #[test]
    fn purchase_quantity_has_no_cap() {
        let mut g = GridState::new(OrderKind::Purchase);
        g.choose_product(0, &product(1, "BRIZO", 0));
        assert_eq!(g.set_qty(0, "1.200"), Ok(true));
        assert_eq!(g.line(0).unwrap().qty, Qty(1_200));
        assert_eq!(g.max_qty(0), None);
    }

    #[test]
    fn editing_a_saved_sale_allows_the_stock_plus_its_own_quantity() {
        let order = Order {
            id: OrderId(3),
            kind: OrderKind::Sale,
            owner_id: None,
            owner: OwnerSnapshot {
                name: "K".into(),
                description: None,
                nipt: None,
                address: None,
            },
            issued_on: jiff::civil::date(2026, 10, 14),
            total: Money(13_000),
            created_at: jiff::Timestamp::UNIX_EPOCH,
            updated_at: jiff::Timestamp::UNIX_EPOCH,
            lines: vec![OrderLine {
                position: 0,
                product_id: Some(ProductId(1)),
                sku_id: Some(SkuId(1)),
                name: "BRIZO".into(),
                code: "L0001".into(),
                unit: "SHISHE".into(),
                quantity: Qty(2),
                price: Money(6_500),
                value: Money(13_000),
            }],
        };
        let lookup = |id: ProductId| (id == ProductId(1)).then(|| product(1, "BRIZO", 1));
        let mut g = GridState::from_order(&order, &lookup);
        assert_eq!(g.rows(), 2);
        assert_eq!(g.max_qty(0), Some(Qty(3)));
        assert_eq!(g.set_qty(0, "3"), Ok(true));
        assert!(g.set_qty(0, "4").is_err());
        assert_eq!(g.total(), Money::ZERO);
        g.set_qty(0, "2").unwrap();
        assert_eq!(g.total(), Money(13_000));
        // The draft keeps the cap.
        let draft = OrderDraft {
            kind: OrderKind::Sale,
            owner_id: None,
            owner_name: "K".into(),
            owner_description: String::new(),
            issued_on: order.issued_on,
            lines: g.draft_lines(),
        };
        let g2 = GridState::from_draft(&draft, Some(&order), &lookup);
        assert_eq!(g2.max_qty(0), Some(Qty(3)));
        assert_eq!(g2.entries(), g.entries());
    }

    #[test]
    fn prices_parse_blank_as_zero_and_reject_garbage() {
        let mut g = GridState::new(OrderKind::Purchase);
        g.choose_product(0, &product(1, "BRIZO", 0));
        assert_eq!(g.set_price(0, PRICE_COL, "1.234,5"), Ok(true));
        assert_eq!(g.line(0).unwrap().buy, Money(123_450));
        assert_eq!(g.set_price(0, SELL_COL, ""), Ok(true));
        assert_eq!(g.line(0).unwrap().sell, Money::ZERO);
        assert!(g.set_price(0, SELL_COL, "x").is_err());
        assert!(g.set_price(0, SELL_COL, "-3").is_err());
        assert_eq!(g.line(0).unwrap().sell, Money::ZERO);
        g.set_qty(0, "2").unwrap();
        assert_eq!(g.value(0), Money(246_900));
        assert_eq!(g.total(), Money(246_900));
        assert_eq!(price_text(Money::ZERO), "");
        assert_eq!(price_text(Money(123_450)), "1.234,50");
        assert_eq!(qty_text(Qty(0)), "");
    }

    #[test]
    fn purchase_free_text_becomes_a_new_name_line() {
        let mut g = GridState::new(OrderKind::Purchase);
        g.choose_product(0, &product(1, "BRIZO", 3));
        assert!(!g.set_free_text(0, "BRIZO"));
        assert!(g.set_free_text(0, " LENGU I RI "));
        let l = g.line(0).unwrap();
        assert_eq!(
            (l.product_id, l.name.as_str(), l.stock),
            (None, "LENGU I RI", None)
        );
        // On the placeholder: a new entry; blank text: nothing.
        assert!(!g.set_free_text(1, " "));
        assert!(g.set_free_text(1, "UJE"));
        assert_eq!(g.rows(), 3);
        let lines = g.draft_lines();
        assert_eq!(lines[1].name, "UJE");
        assert_eq!(lines[1].buy_price, Some(Money::ZERO));
        // A sale ignores free text.
        let mut s = GridState::new(OrderKind::Sale);
        assert!(!s.set_free_text(0, "x"));
    }

    #[test]
    fn remove_keeps_one_entry_and_add_appends() {
        let mut g = GridState::new(OrderKind::Sale);
        assert_eq!(g.add_empty_row(), 1);
        assert_eq!(g.rows(), 3);
        assert!(g.remove(0));
        assert!(g.remove(0));
        assert_eq!(g.rows(), 2, "one empty entry stays");
        assert!(!g.remove(1), "not the placeholder");
    }

    #[test]
    fn draft_lines_follow_the_kind() {
        let mut s = GridState::new(OrderKind::Sale);
        s.choose_product(0, &product(1, "BRIZO", 9));
        s.set_qty(0, "2").unwrap();
        s.set_price(0, PRICE_COL, "").unwrap();
        let d = &s.draft_lines()[0];
        assert_eq!(
            (d.quantity, d.price, d.buy_price),
            (Qty(2), Money(6_500), None)
        );
        let mut p = GridState::new(OrderKind::Purchase);
        p.choose_product(0, &product(1, "BRIZO", 9));
        let d = &p.draft_lines()[0];
        assert_eq!(
            (d.price, d.buy_price, d.sell_price),
            (Money(5_000), Some(Money(5_000)), Some(Money(6_500)))
        );
    }

    #[test]
    fn refresh_product_updates_the_stock() {
        let mut g = GridState::new(OrderKind::Sale);
        g.choose_product(0, &product(1, "BRIZO", 2));
        g.refresh_product(&product(1, "BRIZO 1L", 7));
        assert_eq!(g.line(0).unwrap().stock, Some(Qty(7)));
        assert_eq!(g.line(0).unwrap().name, "BRIZO 1L");
    }

    #[test]
    fn a_blank_sale_price_refills_the_sku_price_a_typed_zero_stays() {
        let mut g = GridState::new(OrderKind::Sale);
        g.choose_product(0, &product(1, "BRIZO", 9));
        assert_eq!(g.set_price(0, PRICE_COL, "0"), Ok(true));
        assert_eq!(g.line(0).unwrap().price, Money::ZERO, "free goods");
        assert_eq!(g.draft_lines()[0].price, Money::ZERO);
        assert_eq!(g.set_price(0, PRICE_COL, " "), Ok(true));
        assert_eq!(g.line(0).unwrap().price, Money(6_500), "the SKU price");
        // Lines of a saved invoice and of a draft know the SKU price too.
        let order = Order {
            id: crm_services::domain::OrderId(1),
            kind: OrderKind::Sale,
            owner_id: None,
            owner: OwnerSnapshot {
                name: "K".into(),
                description: None,
                nipt: None,
                address: None,
            },
            issued_on: jiff::civil::date(2026, 10, 14),
            total: Money::ZERO,
            created_at: jiff::Timestamp::UNIX_EPOCH,
            updated_at: jiff::Timestamp::UNIX_EPOCH,
            lines: vec![OrderLine {
                position: 0,
                product_id: Some(ProductId(1)),
                sku_id: Some(SkuId(1)),
                name: "BRIZO".into(),
                code: "L0001".into(),
                unit: "SHISHE".into(),
                quantity: Qty(1),
                price: Money::ZERO,
                value: Money::ZERO,
            }],
        };
        let lookup = |id: ProductId| (id == ProductId(1)).then(|| product(1, "BRIZO", 9));
        let mut saved = GridState::from_order(&order, &lookup);
        assert_eq!(saved.line(0).unwrap().price, Money::ZERO, "stays free");
        saved.set_price(0, PRICE_COL, "").unwrap();
        assert_eq!(saved.line(0).unwrap().price, Money(6_500));
        let draft = OrderDraft {
            kind: OrderKind::Sale,
            owner_id: None,
            owner_name: "K".into(),
            owner_description: String::new(),
            issued_on: order.issued_on,
            lines: g.draft_lines(),
        };
        let mut from_draft = GridState::from_draft(&draft, None, &lookup);
        from_draft.set_price(0, PRICE_COL, "").unwrap();
        assert_eq!(from_draft.line(0).unwrap().price, Money(6_500));
        // Purchase prices: blank is still 0.
        let mut p = GridState::new(OrderKind::Purchase);
        p.choose_product(0, &product(1, "BRIZO", 9));
        p.set_price(0, PRICE_COL, "").unwrap();
        assert_eq!(p.line(0).unwrap().buy, Money::ZERO);
    }

    #[test]
    fn a_deleted_products_line_changes_only_its_quantity() {
        let line = |product_id: Option<i64>, name: &str| OrderLine {
            position: 0,
            product_id: product_id.map(ProductId),
            sku_id: Some(SkuId(9)),
            name: name.into(),
            code: "L0009".into(),
            unit: "COPE".into(),
            quantity: Qty(10),
            price: Money(200),
            value: Money(2_000),
        };
        let order = Order {
            id: crm_services::domain::OrderId(1),
            kind: OrderKind::Purchase,
            owner_id: None,
            owner: OwnerSnapshot {
                name: "F".into(),
                description: None,
                nipt: None,
                address: None,
            },
            issued_on: jiff::civil::date(2026, 10, 14),
            total: Money(4_000),
            created_at: jiff::Timestamp::UNIX_EPOCH,
            updated_at: jiff::Timestamp::UNIX_EPOCH,
            lines: vec![line(None, "XGONE"), line(Some(1), "BRIZO")],
        };
        let lookup = |id: ProductId| (id == ProductId(1)).then(|| product(1, "BRIZO", 9));
        let mut g = GridState::from_order(&order, &lookup);
        assert!(g.is_orphan(0));
        assert!(!g.is_orphan(1));
        assert!(!g.set_free_text(0, "TJETËR"));
        assert_eq!(g.set_price(0, PRICE_COL, "5"), Ok(false));
        assert_eq!(g.set_price(0, SELL_COL, "5"), Ok(false));
        assert_eq!(g.choose_product(0, &product(1, "BRIZO", 9)), 0);
        let l = g.line(0).unwrap();
        assert_eq!(
            (l.product_id, l.name.as_str(), l.buy),
            (None, "XGONE", Money(200))
        );
        assert_eq!(g.set_qty(0, "4"), Ok(true));
        // The draft keeps the line as a deleted product's line (claimed once).
        let mut draft = OrderDraft {
            kind: OrderKind::Purchase,
            owner_id: None,
            owner_name: "F".into(),
            owner_description: String::new(),
            issued_on: order.issued_on,
            lines: g.draft_lines(),
        };
        draft.lines.push(draft.lines[0].clone());
        let g2 = GridState::from_draft(&draft, Some(&order), &lookup);
        assert!(g2.is_orphan(0));
        assert!(!g2.is_orphan(2), "a second line of that name is free text");
        assert!(!GridState::from_draft(&draft, None, &lookup).is_orphan(0));
        assert!(g.remove(0));
        assert!(!g.is_orphan(0));
    }
}

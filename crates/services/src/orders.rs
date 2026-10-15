//! Saving, deleting and listing invoices (001 Behaviour contract).

use crate::catalogue::{insert_sku, next_code};
use crate::crm::Crm;
use crate::error::{Result, ServiceError};
use crate::messages;
use crm_domain::{
    DEFAULT_UNIT, Money, Order, OrderDraft, OrderId, OrderKind, OrderLine, Owner, OwnerId,
    OwnerSnapshot, ProductId, ProductRow, Qty, SkuId, format, stock, time, validate,
};
use crm_search::{Query, classify_query};
use crm_storage::Tx;
use crm_storage::repo::orders::{OrderFilter, OrderSummary};
use crm_storage::repo::skus::SkuPatch;
use crm_storage::repo::{orders, owners, products, skus, tabs};
use std::collections::HashMap;

/// What a committed save changed, for the index update.
struct Saved {
    id: OrderId,
    new_products: Vec<ProductId>,
    new_owner: Option<Owner>,
}

fn non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// The owner on save (AC 4): the selected id, else a folded-name match within
/// the kind, else a new owner without a description. Returns the owner and
/// whether it was created.
fn resolve_owner(tx: &Tx<'_>, draft: &OrderDraft) -> Result<(Owner, bool)> {
    let kind = draft.kind.owner_kind();
    if let Some(id) = draft.owner_id {
        match owners::get(tx, id) {
            Ok(owner) if owner.kind == kind => return Ok((owner, false)),
            // Deleted since it was picked, or of the other kind: resolve by name.
            Ok(_) | Err(crm_storage::StorageError::NotFound) => {}
            Err(e) => return Err(e.into()),
        }
    }
    let name = draft.owner_name.trim();
    if let Some(owner) = owners::by_name_key(tx, kind, name)? {
        return Ok((owner, false));
    }
    let new = Owner {
        id: OwnerId(0),
        kind,
        name: name.to_owned(),
        description: None,
        nipt: None,
        address: None,
        account_code: None,
    };
    let id = owners::insert(tx, &new)?;
    Ok((owners::get(tx, id)?, true))
}

/// A purchase's free-text product: an existing product with the same folded
/// name, else a new SKU `A####` (unit SHISHE, stock 0 — the purchase delta adds
/// the line quantity) with one product.
fn resolve_free_text(
    tx: &Tx<'_>,
    name: &str,
    buy_price: Money,
    sell_price: Money,
    created: &mut Vec<ProductId>,
) -> Result<ProductRow> {
    if let Some(row) = products::by_name_key(tx, name)? {
        return Ok(row);
    }
    let code = next_code(tx)?;
    let sku_id = insert_sku(
        tx,
        code,
        DEFAULT_UNIT.to_owned(),
        buy_price,
        sell_price,
        Qty::ZERO,
    )?;
    let id = products::insert(tx, sku_id, name)?;
    created.push(id);
    Ok(products::get_row(tx, id)?)
}

fn save_in(
    tx: &Tx<'_>,
    draft: OrderDraft,
    existing: Option<OrderId>,
    tab_key: Option<String>,
) -> Result<Saved> {
    // 1. Validate.
    validate::draft(&draft)?;
    let kind = draft.kind;

    // 4 (early). The old order, so a kind change is refused before anything is written.
    let old = match existing {
        Some(id) => {
            let old = orders::get(tx, id)?;
            if old.kind != kind {
                return Err(ServiceError::field("kind", messages::ORDER_KIND_CHANGE));
            }
            Some(old)
        }
        None => None,
    };

    // 2. Owner.
    let (owner, owner_created) = resolve_owner(tx, &draft)?;
    let snapshot = OwnerSnapshot {
        name: draft.owner_name.trim().to_owned(),
        description: non_empty(&draft.owner_description).or_else(|| owner.description.clone()),
        nipt: owner.nipt.clone(),
        address: owner.address.clone(),
    };

    // F-017: the old lines whose product was deleted since (`product_id` NULL).
    // A draft line without a product and with the same name re-uses such a
    // line's snapshot instead of turning into free text (each one once).
    let mut orphans: Vec<&OrderLine> = old
        .as_ref()
        .map(|o| o.lines.iter().filter(|l| l.product_id.is_none()).collect())
        .unwrap_or_default();

    // 3 + 5. Products (purchase free text creates them) and prices.
    let mut new_products = Vec::new();
    let mut lines: Vec<OrderLine> = Vec::new();
    let mut price_updates: Vec<(SkuId, Option<Money>, Option<Money>)> = Vec::new();
    for (i, line) in draft.lines.iter().enumerate() {
        if line.is_placeholder() {
            continue;
        }
        if line.product_id.is_none()
            && let Some(at) = orphans.iter().position(|o| o.name == line.name.trim())
        {
            let orphan = orphans.swap_remove(at);
            lines.push(orphan_line(i, orphan, line, lines.len())?);
            continue;
        }
        let row = match line.product_id {
            Some(id) => match products::get_row(tx, id) {
                Ok(row) => row,
                Err(crm_storage::StorageError::NotFound) => {
                    return Err(ServiceError::line(
                        i,
                        "product_id",
                        messages::product_missing(line.name.trim()),
                    ));
                }
                Err(e) => return Err(e.into()),
            },
            None => {
                // `validate::draft` guarantees a purchase with both prices here.
                let buy = line.buy_price.unwrap_or(Money::ZERO);
                let sell = line.sell_price.unwrap_or(Money::ZERO);
                resolve_free_text(tx, line.name.trim(), buy, sell, &mut new_products)?
            }
        };
        let price = match kind {
            // The price as given: a typed 0 (free goods) stays 0; the grid fills a
            // blank cell with the SKU sell price itself (F-012).
            OrderKind::Sale => line.price,
            // The line price of a purchase is the buy price.
            OrderKind::Purchase => line.buy_price.unwrap_or(row.sku.buy_price),
        };
        if kind == OrderKind::Purchase && (line.buy_price.is_some() || line.sell_price.is_some()) {
            price_updates.push((row.sku.id, line.buy_price, line.sell_price));
        }
        let value = price.checked_mul_qty(line.quantity).ok_or_else(|| {
            ServiceError::line(
                i,
                "price",
                crm_domain::messages::amount_out_of_range(&format::amount(price)),
            )
        })?;
        lines.push(OrderLine {
            position: lines.len() as i64,
            product_id: Some(row.product.id),
            sku_id: Some(row.sku.id),
            name: row.product.name.clone(),
            code: row.sku.code.clone(),
            unit: row.sku.unit.clone(),
            quantity: line.quantity,
            price,
            value,
        });
    }

    // 6. Net stock delta per SKU (old lines with a deleted SKU are skipped).
    let old_lines: &[OrderLine] = old.as_ref().map_or(&[], |o| o.lines.as_slice());
    apply_stock(tx, kind, old_lines, &lines)?;

    // 7. Q11: purchase prices overwrite the SKU prices.
    for (sku_id, buy_price, sell_price) in price_updates {
        skus::update_fields(
            tx,
            sku_id,
            &SkuPatch {
                buy_price,
                sell_price,
                ..SkuPatch::default()
            },
        )?;
    }

    // 8. The order with its snapshots.
    let mut total = Money::ZERO;
    for line in &lines {
        total = total.checked_add(line.value).ok_or_else(|| {
            ServiceError::field(
                "lines",
                crm_domain::messages::amount_out_of_range(&format::amount(total)),
            )
        })?;
    }
    let now = time::now();
    let mut order = Order {
        id: existing.unwrap_or(OrderId(0)),
        kind,
        owner_id: Some(owner.id),
        owner: snapshot,
        issued_on: draft.issued_on,
        total,
        created_at: old.as_ref().map_or(now, |o| o.created_at),
        updated_at: now,
        lines,
    };
    let id = match existing {
        Some(_) => {
            orders::replace(tx, &order)?;
            order.id
        }
        None => {
            let id = orders::insert(tx, &order)?;
            order.id = id;
            id
        }
    };

    // 9. The tab now shows the saved order: no unsaved draft.
    if let Some(key) = tab_key
        && let Some(mut tab) = tabs::list(tx)?.into_iter().find(|t| t.tab_key == key)
    {
        tab.draft_json = None;
        tab.order_id = Some(id);
        tab.updated_at = now;
        tabs::upsert(tx, &tab)?;
    }

    Ok(Saved {
        id,
        new_products,
        new_owner: owner_created.then_some(owner),
    })
}

/// A re-saved line of a deleted product (F-017): the stored snapshot (no
/// product, the SKU it moved, name, code, unit) with the draft's quantity and
/// price. It never touches the SKU prices: the grid knows no sell price for it.
fn orphan_line(
    i: usize,
    orphan: &OrderLine,
    line: &crm_domain::DraftLine,
    position: usize,
) -> Result<OrderLine> {
    let price = line.buy_price.unwrap_or(line.price);
    let value = price.checked_mul_qty(line.quantity).ok_or_else(|| {
        ServiceError::line(
            i,
            "price",
            crm_domain::messages::amount_out_of_range(&format::amount(price)),
        )
    })?;
    Ok(OrderLine {
        position: position as i64,
        product_id: None,
        quantity: line.quantity,
        price,
        value,
        ..orphan.clone()
    })
}

/// Plans the change with `stock::plan_change` and applies the deltas.
fn apply_stock(tx: &Tx<'_>, kind: OrderKind, old: &[OrderLine], new: &[OrderLine]) -> Result<()> {
    let mut current: HashMap<SkuId, Qty> = HashMap::new();
    for sku_id in old.iter().chain(new).filter_map(|l| l.sku_id) {
        if let std::collections::hash_map::Entry::Vacant(slot) = current.entry(sku_id) {
            // An old line whose SKU row vanished counts as 0 (FK normally nulls it).
            let qty = match skus::get(tx, sku_id) {
                Ok(sku) => sku.quantity,
                Err(crm_storage::StorageError::NotFound) => Qty::ZERO,
                Err(e) => return Err(e.into()),
            };
            slot.insert(qty);
        }
    }
    let deltas = stock::plan_change(kind, old, new, &|id| {
        current.get(&id).copied().unwrap_or(Qty::ZERO)
    })?;
    for (sku_id, delta) in deltas {
        skus::apply_delta(tx, sku_id, delta)?;
    }
    Ok(())
}

impl Crm {
    /// Saves a new order (`existing = None`) or replaces one, all in one
    /// transaction:
    /// 1. `validate::draft`;
    /// 2. resolve the owner (selected id → folded name → new);
    /// 3. a purchase's free-text lines reuse a product with the same folded name
    ///    or create SKU `A####` + product; a product-less line named like a
    ///    saved line of a since-deleted product keeps that line's snapshot (F-017);
    /// 4. load the old lines;
    /// 5. line prices: a sale line keeps the price it was given (0 included,
    ///    F-012); a purchase line's price is its buy price;
    /// 6. apply the net stock delta (sale shortage / Q6 → [`ServiceError::Stock`],
    ///    nothing written);
    /// 7. purchase buy/sell prices overwrite the SKU prices (Q11);
    /// 8. write the order with its snapshots and total;
    /// 9. clear the draft of `tab_key`'s tab.
    ///
    /// Then the index learns new products/owners and the order counts (Q2).
    pub fn save_order(
        &self,
        draft: OrderDraft,
        existing: Option<OrderId>,
        tab_key: Option<String>,
    ) -> Result<OrderId> {
        let saved = self.write(move |tx| save_in(tx, draft, existing, tab_key))?;
        self.sync_products(&saved.new_products, true);
        if let Some(owner) = &saved.new_owner {
            self.index_owner(owner);
        }
        Ok(saved.id)
    }

    /// Deletes an order and reverses its stock. Deleting a purchase whose goods
    /// were sold is blocked (Q6); lines whose SKU was deleted are skipped. The
    /// order's open tab goes with it.
    pub fn delete_order(&self, id: OrderId) -> Result<()> {
        self.write(move |tx| {
            let old = orders::get(tx, id)?;
            apply_stock(tx, old.kind, &old.lines, &[])?;
            Ok(orders::delete(tx, id)?)
        })?;
        self.sync_products(&[], true);
        Ok(())
    }

    /// One order with its lines.
    pub fn get_order(&self, id: OrderId) -> Result<Order> {
        self.read(|c| Ok(orders::get(c, id)?))
    }

    /// The order list. A digits-only text is the invoice number (dates ignored);
    /// other text filters owner name/description. Newest `issued_on` first.
    pub fn list_orders(&self, filter: OrderFilter) -> Result<Vec<OrderSummary>> {
        let text = match filter.text.as_deref().map(classify_query) {
            None | Some(Query::Empty) => None,
            Some(Query::InvoiceNumber(n)) => Some(n.to_string()),
            Some(Query::Text(t)) => Some(t),
        };
        let filter = OrderFilter { text, ..filter };
        self.read(|c| Ok(orders::list(c, &filter)?))
    }

    /// The list filter on start: sales issued today (old `page.tsx:53`).
    pub fn default_filter() -> OrderFilter {
        let today = time::today();
        OrderFilter {
            kind: Some(OrderKind::Sale),
            from: today,
            to: today,
            text: None,
        }
    }
}

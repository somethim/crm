//! Stock delta math for creating, editing and deleting orders (001 Behaviour
//! contract, Q6).
//!
//! A save is planned as one net delta per SKU (Σ new effects − Σ old effects),
//! not as reverse-then-apply. Editing a purchase from 10 to 12 after 5 were sold
//! therefore never fails transiently. Stock may never end below zero.

use crate::ids::SkuId;
use crate::kind::OrderKind;
use crate::messages;
use crate::qty::Qty;

/// What the stock planner needs from an order line.
pub trait StockLine {
    /// The SKU whose stock the line moves; `None` when the SKU was deleted.
    fn sku_id(&self) -> Option<SkuId>;
    /// The product name, used in error messages.
    fn product_name(&self) -> &str;
    /// The line quantity (≥ 1 for saved lines).
    fn quantity(&self) -> Qty;
}

/// The signed stock effect of one line: `+qty` for a purchase, `-qty` for a sale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineEffect {
    /// The SKU whose stock moves.
    pub sku_id: SkuId,
    /// The product name on the line.
    pub product_name: String,
    /// Signed change to the SKU's stock.
    pub qty: Qty,
}

/// Which change to a purchase was blocked (Q6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PurchaseAction {
    /// Deleting the purchase invoice.
    Delete,
    /// Editing the purchase invoice down.
    Edit,
}

impl PurchaseAction {
    /// The Albanian verb: `fshihet` / `ndryshohet`.
    pub const fn verb(self) -> &'static str {
        match self {
            Self::Delete => messages::VERB_DELETE,
            Self::Edit => messages::VERB_EDIT,
        }
    }
}

/// A save that would make some SKU's stock negative.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StockError {
    /// A sale needs more than is available.
    #[error("{}", messages::stock_insufficient(.product_name, .available.0, .required.0))]
    Insufficient {
        /// The SKU short of stock.
        sku_id: SkuId,
        /// The product name on the offending line.
        product_name: String,
        /// Stock available to this order (current stock plus what the order's
        /// old lines already took).
        available: Qty,
        /// What the order's new lines take.
        required: Qty,
    },
    /// Removing or shrinking purchase lines would take back goods already sold (Q6).
    #[error(
        "{}",
        messages::purchase_already_sold(.action.verb(), .product_name, .available.0, .required.0)
    )]
    PurchaseAlreadySold {
        /// Delete or edit.
        action: PurchaseAction,
        /// The SKU short of stock.
        sku_id: SkuId,
        /// The product name on the purchase line.
        product_name: String,
        /// Current stock of the SKU ("gjendja").
        available: Qty,
        /// How much the change takes out ("duhet të hiqen").
        required: Qty,
    },
}

impl StockError {
    /// The Albanian message shown to the user.
    pub fn user_message(&self) -> String {
        self.to_string()
    }
}

/// The signed stock effects of `lines` for an order of `kind`. Lines without a
/// SKU (deleted since) are skipped.
pub fn effects<L: StockLine>(kind: OrderKind, lines: &[L]) -> Vec<LineEffect> {
    lines
        .iter()
        .filter_map(|line| {
            let sku_id = line.sku_id()?;
            let qty = match kind {
                OrderKind::Purchase => line.quantity(),
                OrderKind::Sale => -line.quantity(),
            };
            Some(LineEffect {
                sku_id,
                product_name: line.product_name().to_owned(),
                qty,
            })
        })
        .collect()
}

/// Plans a create (`old` empty), edit, or delete (`new` empty) of an order of `kind`.
///
/// Returns the non-zero net stock delta per SKU, sorted by [`SkuId`], or the
/// first SKU (in line order) whose stock would end below zero:
/// - a sale → [`StockError::Insufficient`];
/// - a purchase → [`StockError::PurchaseAlreadySold`] (`Delete` when `new` is empty).
///
/// `stock` returns a SKU's current stock, which still includes the old lines.
pub fn plan_change<O: StockLine, N: StockLine>(
    kind: OrderKind,
    old: &[O],
    new: &[N],
    stock: &impl Fn(SkuId) -> Qty,
) -> Result<Vec<(SkuId, Qty)>, StockError> {
    struct Acc {
        sku_id: SkuId,
        old: Qty,
        new: Qty,
        old_name: Option<String>,
        new_name: Option<String>,
    }

    let mut accs: Vec<Acc> = Vec::new();
    let mut entry = |sku_id: SkuId| -> usize {
        if let Some(i) = accs.iter().position(|a| a.sku_id == sku_id) {
            return i;
        }
        accs.push(Acc {
            sku_id,
            old: Qty::ZERO,
            new: Qty::ZERO,
            old_name: None,
            new_name: None,
        });
        accs.len() - 1
    };
    let mut positions = Vec::new();
    for effect in effects(kind, new) {
        let i = entry(effect.sku_id);
        positions.push((i, effect, true));
    }
    for effect in effects(kind, old) {
        let i = entry(effect.sku_id);
        positions.push((i, effect, false));
    }
    for (i, effect, is_new) in positions {
        let acc = &mut accs[i];
        if is_new {
            acc.new += effect.qty;
            acc.new_name.get_or_insert(effect.product_name);
        } else {
            acc.old += effect.qty;
            acc.old_name.get_or_insert(effect.product_name);
        }
    }

    for acc in &accs {
        let delta = acc.new - acc.old;
        let current = stock(acc.sku_id);
        if !delta.is_negative() || !(current + delta).is_negative() {
            continue;
        }
        let name = |first: &Option<String>, second: &Option<String>| {
            first.clone().or_else(|| second.clone()).unwrap_or_default()
        };
        return Err(match kind {
            OrderKind::Sale => StockError::Insufficient {
                sku_id: acc.sku_id,
                product_name: name(&acc.new_name, &acc.old_name),
                available: current - acc.old,
                required: -acc.new,
            },
            OrderKind::Purchase => StockError::PurchaseAlreadySold {
                action: if new.is_empty() {
                    PurchaseAction::Delete
                } else {
                    PurchaseAction::Edit
                },
                sku_id: acc.sku_id,
                product_name: name(&acc.old_name, &acc.new_name),
                available: current,
                required: -delta,
            },
        });
    }

    let mut deltas: Vec<(SkuId, Qty)> = accs
        .iter()
        .map(|a| (a.sku_id, a.new - a.old))
        .filter(|(_, d)| *d != Qty::ZERO)
        .collect();
    deltas.sort_by_key(|(sku, _)| *sku);
    Ok(deltas)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct L(Option<i64>, &'static str, i64);

    impl StockLine for L {
        fn sku_id(&self) -> Option<SkuId> {
            self.0.map(SkuId)
        }
        fn product_name(&self) -> &str {
            self.1
        }
        fn quantity(&self) -> Qty {
            Qty(self.2)
        }
    }

    const NONE: &[L] = &[];

    fn stock_of(pairs: &'static [(i64, i64)]) -> impl Fn(SkuId) -> Qty {
        move |sku| {
            Qty(pairs
                .iter()
                .find(|(id, _)| *id == sku.0)
                .map(|(_, q)| *q)
                .unwrap_or(0))
        }
    }

    #[test]
    fn effects_sign_by_kind_and_skip_missing_skus() {
        let lines = [L(Some(1), "A", 3), L(None, "B", 9)];
        assert_eq!(
            effects(OrderKind::Purchase, &lines),
            [LineEffect {
                sku_id: SkuId(1),
                product_name: "A".into(),
                qty: Qty(3)
            }]
        );
        assert_eq!(effects(OrderKind::Sale, &lines)[0].qty, Qty(-3));
    }

    #[test]
    fn sale_reduces_and_purchase_increases() {
        let s = stock_of(&[(1, 10)]);
        let new = [L(Some(1), "A", 4)];
        assert_eq!(
            plan_change(OrderKind::Sale, NONE, &new, &s),
            Ok(vec![(SkuId(1), Qty(-4))])
        );
        assert_eq!(
            plan_change(OrderKind::Purchase, NONE, &new, &s),
            Ok(vec![(SkuId(1), Qty(4))])
        );
    }

    #[test]
    fn unchanged_edit_has_no_deltas() {
        let s = stock_of(&[(1, 0)]);
        let lines = [L(Some(1), "A", 4)];
        assert_eq!(plan_change(OrderKind::Sale, &lines, &lines, &s), Ok(vec![]));
    }

    #[test]
    fn deltas_sorted_by_sku() {
        let s = stock_of(&[(1, 10), (2, 10), (3, 10)]);
        let new = [L(Some(3), "C", 1), L(Some(1), "A", 1), L(Some(2), "B", 1)];
        let got = plan_change(OrderKind::Sale, NONE, &new, &s).unwrap();
        assert_eq!(got.iter().map(|(k, _)| k.0).collect::<Vec<_>>(), [1, 2, 3]);
    }

    #[test]
    fn sale_edit_counts_the_old_reservation() {
        // Stock 2 after the old sale of 5; editing to 8 needs 3 more: ka 7, kërkohen 8.
        let s = stock_of(&[(1, 2)]);
        let err = plan_change(
            OrderKind::Sale,
            &[L(Some(1), "A", 5)],
            &[L(Some(1), "A", 8)],
            &s,
        )
        .unwrap_err();
        assert_eq!(
            err.user_message(),
            "Gjendje e pamjaftueshme për «A»: ka 7, kërkohen 8."
        );
        assert_eq!(
            plan_change(
                OrderKind::Sale,
                &[L(Some(1), "A", 5)],
                &[L(Some(1), "A", 7)],
                &s
            ),
            Ok(vec![(SkuId(1), Qty(-2))])
        );
    }

    #[test]
    fn first_failing_line_is_reported() {
        let s = stock_of(&[(1, 0), (2, 0)]);
        let new = [L(Some(2), "B", 1), L(Some(1), "A", 1)];
        let err = plan_change(OrderKind::Sale, NONE, &new, &s).unwrap_err();
        assert!(matches!(
            err,
            StockError::Insufficient {
                sku_id: SkuId(2),
                ..
            }
        ));
    }

    #[test]
    fn edit_with_a_new_sku_in_a_purchase_is_fine() {
        let s = stock_of(&[(1, 3)]);
        let old = [L(Some(1), "A", 3)];
        let new = [L(Some(1), "A", 3), L(Some(2), "B", 6)];
        assert_eq!(
            plan_change(OrderKind::Purchase, &old, &new, &s),
            Ok(vec![(SkuId(2), Qty(6))])
        );
    }

    #[test]
    fn purchase_action_verbs() {
        assert_eq!(PurchaseAction::Delete.verb(), "fshihet");
        assert_eq!(PurchaseAction::Edit.verb(), "ndryshohet");
    }
}

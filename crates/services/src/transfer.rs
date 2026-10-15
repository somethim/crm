//! CSV/XLSX import, CSV export and the order reports.

use crate::catalogue::{Page, insert_sku};
use crate::crm::{Crm, Fault};
use crate::error::Result;
use crate::messages;
use crm_domain::{Money, OrderKind, Owner, OwnerId, OwnerKind, format, time};
use crm_io::export::ProductExportRow;
use crm_io::report::{self, Cell, PREVIEW_LIMIT, ReportKind, ReportLine, ReportTable};
use crm_io::{OwnerImportRow, ProductImportRow, RowError, table};
use crm_storage::repo::orders::OrderFilter;
use crm_storage::repo::skus::SkuPatch;
use crm_storage::repo::{orders, owners, products, skus};
use crm_storage::{BackupReason, Conn, StorageError, Tx};
use jiff::civil::Date;
use std::path::Path;

/// What an import or export file holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImportKind {
    /// "Artikujt" (ARTIKUJ).
    Products,
    /// "Klientët" (KLIENTET).
    Clients,
    /// "Furnitorët" (FURNITORET).
    Suppliers,
}

impl ImportKind {
    /// The backup suffix part: `pre-import-<slug>`.
    pub const fn slug(self) -> &'static str {
        match self {
            ImportKind::Products => "products",
            ImportKind::Clients => "clients",
            ImportKind::Suppliers => "suppliers",
        }
    }

    fn owner_kind(self) -> Option<OwnerKind> {
        match self {
            ImportKind::Products => None,
            ImportKind::Clients => Some(OwnerKind::Client),
            ImportKind::Suppliers => Some(OwnerKind::Supplier),
        }
    }
}

/// The outcome of an import. `created + updated + skipped + failed` = data rows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// New products / owners.
    pub created: usize,
    /// Existing rows whose data changed.
    pub updated: usize,
    /// Existing rows already up to date.
    pub skipped: usize,
    /// Rejected rows (= `errors.len()`).
    pub failed: usize,
    /// One error per rejected row ("Rreshti n: …").
    pub errors: Vec<RowError>,
}

/// Which report, for which issue dates (Q3), of which order kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportParams {
    /// "REGJISTRI ANALITIK I POROSIVE" or "RAPORTI SIPAS ARTIKUJVE".
    pub kind: ReportKind,
    /// First `issued_on` included.
    pub from: Date,
    /// Last `issued_on` included.
    pub to: Date,
    /// Only sales or purchases; `None` = both.
    pub order_kind: Option<OrderKind>,
}

/// The in-app report preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportPreview {
    /// The report with at most [`PREVIEW_LIMIT`] rows.
    pub table: ReportTable,
    /// Rows in the full report.
    pub total: usize,
    /// True when rows were cut off.
    pub truncated: bool,
}

/// The transaction body of one import.
type ApplyRows = Box<dyn FnOnce(&Tx<'_>) -> Result<Counts> + Send>;

#[derive(Default)]
struct Counts {
    created: usize,
    updated: usize,
    skipped: usize,
}

/// Products (old `Import.php:219`): SKUs group by base code; a missing SKU is
/// created from the first row seen for it; only a base row updates an existing
/// SKU's unit, prices and stock; a product is found by `(sku, name)` or created.
fn apply_products(tx: &Tx<'_>, rows: &[ProductImportRow]) -> Result<Counts> {
    let mut counts = Counts::default();
    for row in rows {
        let (sku_id, sku_changed) = match skus::by_code(tx, &row.base_code)? {
            None => {
                let id = insert_sku(
                    tx,
                    row.base_code.clone(),
                    row.unit.clone(),
                    row.buy_price,
                    row.sell_price,
                    row.quantity,
                )?;
                (id, true)
            }
            Some(sku) if row.is_base_row => {
                let changed = sku.unit != row.unit
                    || sku.buy_price != row.buy_price
                    || sku.sell_price != row.sell_price
                    || sku.quantity != row.quantity;
                if changed {
                    skus::update_fields(
                        tx,
                        sku.id,
                        &SkuPatch {
                            code: None,
                            unit: Some(row.unit.clone()),
                            buy_price: Some(row.buy_price),
                            sell_price: Some(row.sell_price),
                            quantity: Some(row.quantity),
                        },
                    )?;
                }
                (sku.id, changed)
            }
            Some(sku) => (sku.id, false),
        };
        if products::by_sku_and_name(tx, sku_id, &row.name)?.is_none() {
            products::insert(tx, sku_id, &row.name)?;
            counts.created += 1;
        } else if sku_changed {
            counts.updated += 1;
        } else {
            counts.skipped += 1;
        }
    }
    Ok(counts)
}

fn trimmed(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

/// The owner a row stands for (001 A1): the account code when the row has one;
/// else its NIPT when exactly one owner of the kind has it; else the folded name.
fn match_owner(c: &Conn<'_>, kind: OwnerKind, row: &OwnerImportRow) -> Result<Option<Owner>> {
    if let Some(account) = trimmed(&row.account_code) {
        return Ok(owners::by_account(c, kind, &account)?);
    }
    if let Some(nipt) = trimmed(&row.nipt) {
        let mut hits = owners::by_nipt(c, kind, &nipt)?;
        if hits.len() == 1 {
            return Ok(hits.pop());
        }
    }
    Ok(owners::by_name_key(c, kind, row.name.trim())?)
}

/// Owners (Q4, 001 A1): a matched owner takes the row's non-empty fields
/// (`updated` when one differs, else `skipped`); an empty cell never clears a
/// stored value. Unmatched rows are created.
fn apply_owners(tx: &Tx<'_>, kind: OwnerKind, rows: &[OwnerImportRow]) -> Result<Counts> {
    let mut counts = Counts::default();
    for row in rows {
        let incoming = Owner {
            id: OwnerId(0),
            kind,
            name: row.name.trim().to_owned(),
            description: trimmed(&row.description),
            nipt: trimmed(&row.nipt),
            address: trimmed(&row.address),
            account_code: trimmed(&row.account_code),
        };
        match match_owner(tx, kind, row)? {
            None => {
                owners::insert(tx, &incoming)?;
                counts.created += 1;
            }
            Some(stored) => {
                let merged = Owner {
                    id: stored.id,
                    kind,
                    name: incoming.name,
                    description: incoming.description.or(stored.description.clone()),
                    nipt: incoming.nipt.or(stored.nipt.clone()),
                    address: incoming.address.or(stored.address.clone()),
                    account_code: incoming.account_code.or(stored.account_code.clone()),
                };
                if merged == stored {
                    counts.skipped += 1;
                } else {
                    owners::update(tx, &merged)?;
                    counts.updated += 1;
                }
            }
        }
    }
    Ok(counts)
}

impl Crm {
    /// Imports a CSV/XLSX file:
    /// 1. read and parse it (`crm-io`; a file-level problem fails before any change);
    /// 2. take a `pre-import-<kind>` backup;
    /// 3. apply the valid rows in one transaction (a database error rolls back
    ///    everything; rejected rows are only counted in `failed`);
    /// 4. rebuild the search index.
    pub fn import(&self, kind: ImportKind, path: &Path) -> Result<ImportReport> {
        let raw = table::read(path)?;
        let (apply, errors): (ApplyRows, _) = match kind.owner_kind() {
            None => {
                let parsed = crm_io::parse_products(&raw)?;
                let rows: Vec<ProductImportRow> = parsed.rows.into_iter().map(|(_, r)| r).collect();
                (Box::new(move |tx| apply_products(tx, &rows)), parsed.errors)
            }
            Some(owner_kind) => {
                let parsed = crm_io::parse_owners(&raw)?;
                let rows: Vec<OwnerImportRow> = parsed.rows.into_iter().map(|(_, r)| r).collect();
                (
                    Box::new(move |tx| apply_owners(tx, owner_kind, &rows)),
                    parsed.errors,
                )
            }
        };
        self.with_db_committing(|db| Ok(db.backup_now(BackupReason::PreImport(kind.slug()))?))?;
        // Captured here: the closure runs on the writer thread.
        let fail = self.take_fault(Fault::ImportAfterApply);
        let counts = self.write(move |tx| {
            let counts = apply(tx)?;
            if fail {
                return Err(
                    StorageError::DeviceUnavailable("injected import failure".into()).into(),
                );
            }
            Ok(counts)
        })?;
        self.rebuild_index_after_commit();
        Ok(ImportReport {
            created: counts.created,
            updated: counts.updated,
            skipped: counts.skipped,
            failed: errors.len(),
            errors,
        })
    }

    /// Writes every product (with its SKU code) or every owner of the kind to a
    /// CSV in the import layout. Returns the number of rows written.
    pub fn export_csv(&self, kind: ImportKind, path: &Path) -> Result<usize> {
        match kind.owner_kind() {
            None => {
                let rows = self.list_products("", Page::ALL)?;
                let out: Vec<ProductExportRow> = rows
                    .into_iter()
                    .map(|r| ProductExportRow {
                        code: r.sku.code,
                        name: r.product.name,
                        unit: r.sku.unit,
                        quantity: r.sku.quantity,
                        buy_price: r.sku.buy_price,
                        sell_price: r.sku.sell_price,
                    })
                    .collect();
                crm_io::write_products(path, &out)?;
                Ok(out.len())
            }
            Some(owner_kind) => {
                let all = self.list_owners(owner_kind, "", Page::ALL)?;
                crm_io::write_owners(path, owner_kind, &all)?;
                Ok(all.len())
            }
        }
    }

    /// The full report, filtered on `issued_on` (Q3).
    pub fn report(&self, params: ReportParams) -> Result<ReportTable> {
        let rows = self.read(|c| {
            Ok(orders::lines_for_report(
                c,
                params.from,
                params.to,
                params.order_kind,
            )?)
        })?;
        let lines: Vec<ReportLine> = rows
            .into_iter()
            .map(|(header, line)| ReportLine {
                order_id: header.id,
                order_kind: header.kind,
                issued_on: header.issued_on,
                owner_name: header.owner.name,
                product_id: line.product_id,
                code: line.code,
                name: line.name,
                unit: line.unit,
                quantity: line.quantity,
                price: line.price,
                value: line.value,
            })
            .collect();
        Ok(report::build(params.kind, params.from, params.to, &lines))
    }

    /// The report capped at [`PREVIEW_LIMIT`] (500) rows, with the full row
    /// count and whether it was cut off.
    pub fn report_preview(&self, params: ReportParams) -> Result<ReportPreview> {
        let mut table = self.report(params)?;
        let total = table.rows.len();
        table.rows.truncate(PREVIEW_LIMIT);
        Ok(ReportPreview {
            table,
            total,
            truncated: total > PREVIEW_LIMIT,
        })
    }

    /// Writes the full report as XLSX.
    pub fn export_report_xlsx(&self, params: ReportParams, path: &Path) -> Result<()> {
        let table = self.report(params)?;
        Ok(crm_io::write_xlsx(path, &table)?)
    }

    /// The order list as shown under `filter` (same kind, dates and search
    /// text, in the default order), as a table for the XLSX export: Nr., Lloji,
    /// Klienti/Furnitori, Përshkrimi, Totali (LEK) as `1.234,56` text, Data,
    /// Modifikuar, then a Totali row.
    pub fn order_list_table(&self, filter: OrderFilter) -> Result<ReportTable> {
        let kind = filter.kind.unwrap_or(OrderKind::Sale);
        let subtitle = messages::order_list_subtitle(
            &format::report_range(filter.from, filter.to),
            kind.label(),
            filter.text.as_deref(),
        );
        let owner = match kind {
            OrderKind::Sale => "Klienti",
            OrderKind::Purchase => "Furnitori",
        };
        let orders = self.list_orders(filter)?;
        let tz = time::tirane();
        let mut sum = Money(0);
        let mut rows: Vec<Vec<Cell>> = orders
            .iter()
            .map(|o| {
                sum = sum.checked_add(o.total).unwrap_or(if o.total.0 < 0 {
                    Money(i64::MIN)
                } else {
                    Money(i64::MAX)
                });
                vec![
                    Cell::Int(o.id.0),
                    Cell::Text(o.kind.label().to_owned()),
                    Cell::Text(o.owner_name.clone()),
                    Cell::Text(o.owner_description.clone().unwrap_or_default()),
                    Cell::AmountText(o.total),
                    Cell::Text(format::date(o.issued_on)),
                    Cell::Text(format::timestamp(o.updated_at, &tz)),
                ]
            })
            .collect();
        rows.push(vec![
            Cell::Text(messages::ORDER_LIST_TOTAL.to_owned()),
            Cell::Text(String::new()),
            Cell::Text(String::new()),
            Cell::Text(String::new()),
            Cell::AmountText(sum),
            Cell::Text(String::new()),
            Cell::Text(String::new()),
        ]);
        Ok(ReportTable {
            title: messages::ORDER_LIST_TITLE.to_owned(),
            subtitle,
            headers: messages::order_list_headers(owner).into(),
            rows,
        })
    }

    /// Writes [`Crm::order_list_table`] as XLSX (atomically).
    pub fn export_order_list_xlsx(&self, filter: OrderFilter, path: &Path) -> Result<()> {
        let table = self.order_list_table(filter)?;
        Ok(crm_io::write_xlsx(path, &table)?)
    }
}

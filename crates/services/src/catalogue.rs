//! Products, SKUs, clients and suppliers.

use crate::crm::Crm;
use crate::error::{Result, ServiceError};
use crate::messages;
use crm_domain::{
    Money, Owner, OwnerId, OwnerInput, OwnerKind, ProductId, ProductInput, ProductRow, Qty, Sku,
    SkuId, sku, validate,
};
use crm_storage::repo::skus::SkuPatch;
use crm_storage::repo::{owners, products, skus};
use crm_storage::{Conn, Tx};

/// Default page size of the catalogue lists.
pub const DEFAULT_PAGE_SIZE: i64 = 200;

/// Default number of search results.
pub const DEFAULT_SEARCH_LIMIT: usize = 50;

/// A page of a list: skip `offset` rows, return at most `limit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    /// Rows to skip.
    pub offset: i64,
    /// Maximum rows returned.
    pub limit: i64,
}

impl Default for Page {
    fn default() -> Self {
        Page {
            offset: 0,
            limit: DEFAULT_PAGE_SIZE,
        }
    }
}

impl Page {
    /// Every row.
    pub const ALL: Page = Page {
        offset: 0,
        limit: i64::MAX,
    };
}

/// Changes to a product; `None` leaves a field as it is. The name belongs to the
/// product; code, unit, prices and stock belong to its SKU and so change every
/// product sharing it (old `UpdateProduct.php:23`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProductPatch {
    /// "Emri".
    pub name: Option<String>,
    /// "Kodi"; a flavour code (`X/n`) is reduced to its base. Blank is ignored.
    pub code: Option<String>,
    /// "Njësia".
    pub unit: Option<String>,
    /// "Çmimi i blerjes".
    pub buy_price: Option<Money>,
    /// "Çmimi i shitjes".
    pub sell_price: Option<Money>,
    /// "Gjendja" (absolute).
    pub quantity: Option<Qty>,
}

/// What `delete_product` removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeleteOutcome {
    /// True when the product was the last one of its SKU, so the SKU (code,
    /// prices, stock) was deleted too (001 A11).
    pub sku_deleted: bool,
}

fn opt(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn owner_from_input(id: OwnerId, input: &OwnerInput) -> Owner {
    Owner {
        id,
        kind: input.kind,
        name: input.name.trim().to_owned(),
        description: opt(&input.description),
        nipt: opt(&input.nipt),
        address: opt(&input.address),
        account_code: opt(&input.account_code),
    }
}

/// Refuses an account code ("Llogari") already used by another owner of the
/// same kind with an `account_code` field error; the unique index
/// `owners_kind_account` stays as the backstop.
fn check_account_free(c: &Conn<'_>, owner: &Owner) -> Result<()> {
    if let Some(code) = &owner.account_code
        && let Some(other) = owners::by_account(c, owner.kind, code)?
        && other.id != owner.id
    {
        return Err(ServiceError::field(
            "account_code",
            messages::account_taken(owner.kind),
        ));
    }
    Ok(())
}

/// The next free automatic SKU code (`A####`, 001 A10).
pub(crate) fn next_code(c: &Conn<'_>) -> Result<String> {
    let codes = skus::all_codes(c)?;
    Ok(sku::next_auto_code(codes.iter().map(String::as_str)))
}

/// Inserts a SKU and returns its id.
pub(crate) fn insert_sku(
    tx: &Tx<'_>,
    code: String,
    unit: String,
    buy_price: Money,
    sell_price: Money,
    quantity: Qty,
) -> Result<SkuId> {
    Ok(skus::insert(
        tx,
        &Sku {
            id: SkuId(0),
            code,
            unit,
            buy_price,
            sell_price,
            quantity,
        },
    )?)
}

/// Every product sharing the SKU of `row`, except `row` itself.
fn siblings_of(c: &Conn<'_>, row: &ProductRow) -> Result<Vec<ProductRow>> {
    // `list_rows` matches the code as a substring; keep the exact SKU only.
    Ok(products::list_rows(c, &row.sku.code, 0, i64::MAX)?
        .into_iter()
        .filter(|r| r.sku.id == row.sku.id && r.product.id != row.product.id)
        .collect())
}

impl Crm {
    // -- products ----------------------------------------------------------

    /// Creates a product. A given code is reduced to its base code; when that
    /// SKU exists the product joins it, and the typed unit, prices and stock are
    /// ignored (old `CreateProduct.php:39`). An empty code gets the next `A####`.
    pub fn create_product(&self, input: ProductInput) -> Result<ProductRow> {
        validate::product(&input)?;
        let row = self.write(move |tx| {
            let code = sku::base_code(&input.code).to_owned();
            let existing = if code.is_empty() {
                None
            } else {
                skus::by_code(tx, &code)?
            };
            let sku_id = match existing {
                Some(sku) => sku.id,
                None => {
                    let code = if code.is_empty() {
                        next_code(tx)?
                    } else {
                        code
                    };
                    insert_sku(
                        tx,
                        code,
                        input.unit.trim().to_owned(),
                        input.buy_price,
                        input.sell_price,
                        input.quantity,
                    )?
                }
            };
            let name = input.name.trim();
            if products::by_sku_and_name(tx, sku_id, name)?.is_some() {
                return Err(ServiceError::field("name", messages::NAME_TAKEN));
            }
            let id = products::insert(tx, sku_id, name)?;
            Ok(products::get_row(tx, id)?)
        })?;
        self.sync_products(&[row.product.id], false);
        Ok(row)
    }

    /// Updates only the fields set in `patch`. Code, unit, prices and stock
    /// change the SKU and so every product sharing it. A code already used by
    /// another SKU fails with a `code` validation error ("Ky kod përdoret nga
    /// një artikull tjetër.").
    pub fn update_product(&self, id: ProductId, patch: ProductPatch) -> Result<ProductRow> {
        let (row, touched) = self.write(move |tx| {
            let current = products::get_row(tx, id)?;
            let code = patch
                .code
                .as_deref()
                .map(|c| sku::base_code(c).to_owned())
                .filter(|c| !c.is_empty());
            let merged = ProductInput {
                code: code.clone().unwrap_or_else(|| current.sku.code.clone()),
                name: patch
                    .name
                    .clone()
                    .unwrap_or_else(|| current.product.name.clone()),
                unit: patch
                    .unit
                    .clone()
                    .unwrap_or_else(|| current.sku.unit.clone()),
                buy_price: patch.buy_price.unwrap_or(current.sku.buy_price),
                sell_price: patch.sell_price.unwrap_or(current.sku.sell_price),
                quantity: patch.quantity.unwrap_or(current.sku.quantity),
            };
            validate::product(&merged)?;
            if let Some(code) = &code
                && let Some(other) = skus::by_code(tx, code)?
                && other.id != current.sku.id
            {
                return Err(ServiceError::field("code", messages::CODE_TAKEN));
            }
            if let Some(name) = &patch.name {
                let name = name.trim();
                if let Some(other) = products::by_sku_and_name(tx, current.sku.id, name)?
                    && other.id != id
                {
                    return Err(ServiceError::field("name", messages::NAME_TAKEN));
                }
                products::update_name(tx, id, name)?;
            }
            let sku_patch = SkuPatch {
                code: code.filter(|c| *c != current.sku.code),
                unit: patch.unit.as_deref().map(|u| u.trim().to_owned()),
                buy_price: patch.buy_price,
                sell_price: patch.sell_price,
                quantity: patch.quantity,
            };
            let code_changed = sku_patch.code.is_some();
            if sku_patch != SkuPatch::default() {
                skus::update_fields(tx, current.sku.id, &sku_patch)?;
            }
            let row = products::get_row(tx, id)?;
            let mut touched = vec![id];
            if code_changed {
                touched.extend(siblings_of(tx, &row)?.iter().map(|r| r.product.id));
            }
            Ok((row, touched))
        })?;
        self.sync_products(&touched, false);
        Ok(row)
    }

    /// Deletes a product. Its order lines keep their snapshots. When it was the
    /// last product of its SKU, the SKU goes too (001 A11); the UI asks first.
    pub fn delete_product(&self, id: ProductId) -> Result<DeleteOutcome> {
        let sku_deleted = self.write(move |tx| {
            let sku_id = products::delete(tx, id)?;
            Ok(skus::delete_if_orphan(tx, sku_id)?)
        })?;
        self.index_mut().remove_product(id);
        Ok(DeleteOutcome { sku_deleted })
    }

    /// The other products sharing this product's SKU ("Ky kod ndahet me N
    /// artikuj të tjerë").
    pub fn product_siblings(&self, id: ProductId) -> Result<Vec<ProductRow>> {
        self.read(|c| {
            let row = products::get_row(c, id)?;
            siblings_of(c, &row)
        })
    }

    /// One product with its SKU.
    pub fn get_product(&self, id: ProductId) -> Result<ProductRow> {
        self.read(|c| Ok(products::get_row(c, id)?))
    }

    /// A page of products whose name or code contains every word of `filter`
    /// (all when blank), by code then name.
    pub fn list_products(&self, filter: &str, page: Page) -> Result<Vec<ProductRow>> {
        self.read(|c| Ok(products::list_rows(c, filter, page.offset, page.limit)?))
    }

    /// Fuzzy product search through the index (an empty query lists by
    /// popularity), hydrated from the database.
    pub fn search_products(&self, q: &str, limit: usize) -> Result<Vec<ProductRow>> {
        Ok(self.search_products_counted(q, limit)?.0)
    }

    /// [`Crm::search_products`], plus how many products match in all ("50/124
    /// sugjerime"). Rows deleted since the index was built are skipped and
    /// not counted.
    pub fn search_products_counted(
        &self,
        q: &str,
        limit: usize,
    ) -> Result<(Vec<ProductRow>, usize)> {
        let (ids, total) = self.index().search_products_counted(q, limit);
        self.read(|c| {
            let mut rows = Vec::with_capacity(ids.len());
            let mut gone = 0;
            for id in ids {
                match products::get_row(c, id) {
                    Ok(row) => rows.push(row),
                    // Deleted since the index was built.
                    Err(crm_storage::StorageError::NotFound) => gone += 1,
                    Err(e) => return Err(e.into()),
                }
            }
            Ok((rows, total.saturating_sub(gone)))
        })
    }

    // -- owners ------------------------------------------------------------

    /// Creates a client or supplier. Blank optional fields are stored as empty.
    pub fn create_owner(&self, input: OwnerInput) -> Result<Owner> {
        validate::owner(&input)?;
        let owner = self.write(move |tx| {
            let owner = owner_from_input(OwnerId(0), &input);
            check_account_free(tx, &owner)?;
            let id = owners::insert(tx, &owner)?;
            Ok(owners::get(tx, id)?)
        })?;
        self.index_owner(&owner);
        Ok(owner)
    }

    /// Overwrites every field of the owner with the dialog input (a blank field
    /// clears it, old `UpdateSupplierTest.php:31`).
    pub fn update_owner(&self, id: OwnerId, input: OwnerInput) -> Result<Owner> {
        validate::owner(&input)?;
        let owner = self.write(move |tx| {
            let owner = owner_from_input(id, &input);
            check_account_free(tx, &owner)?;
            owners::update(tx, &owner)?;
            Ok(owners::get(tx, id)?)
        })?;
        self.index_owner(&owner);
        Ok(owner)
    }

    /// Deletes an owner; its orders keep their snapshots.
    pub fn delete_owner(&self, id: OwnerId) -> Result<()> {
        self.write(move |tx| Ok(owners::delete(tx, id)?))?;
        self.unindex_owner(id);
        Ok(())
    }

    /// One owner.
    pub fn get_owner(&self, id: OwnerId) -> Result<Owner> {
        self.read(|c| Ok(owners::get(c, id)?))
    }

    /// A page of owners of `kind` whose name, NIPT or account contains every
    /// word of `filter` (all when blank), by name in byte order.
    pub fn list_owners(&self, kind: OwnerKind, filter: &str, page: Page) -> Result<Vec<Owner>> {
        self.read(|c| Ok(owners::list(c, kind, filter, page.offset, page.limit)?))
    }

    /// Fuzzy owner search within `kind` through the index, hydrated from the database.
    pub fn search_owners(&self, kind: OwnerKind, q: &str, limit: usize) -> Result<Vec<Owner>> {
        Ok(self.search_owners_counted(kind, q, limit)?.0)
    }

    /// [`Crm::search_owners`], plus how many owners of `kind` match in all.
    /// Rows deleted since the index was built are skipped and not counted.
    pub fn search_owners_counted(
        &self,
        kind: OwnerKind,
        q: &str,
        limit: usize,
    ) -> Result<(Vec<Owner>, usize)> {
        let (ids, total) = self.index().search_owners_counted(kind, q, limit);
        self.read(|c| {
            let mut found = Vec::with_capacity(ids.len());
            let mut gone = 0;
            for id in ids {
                match owners::get(c, id) {
                    Ok(owner) => found.push(owner),
                    Err(crm_storage::StorageError::NotFound) => gone += 1,
                    Err(e) => return Err(e.into()),
                }
            }
            Ok((found, total.saturating_sub(gone)))
        })
    }
}

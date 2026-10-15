//! Keeping the in-memory [`SearchIndex`] in step with the database.
//!
//! Every write use case updates the index in-process after its commit; changes
//! made elsewhere are picked up by [`Crm::refresh_if_changed`](crate::Crm::refresh_if_changed).
//!
//! An index update after a commit is best-effort (F-013): the data is already
//! committed, so a failed read must not turn the use case into an error (the
//! UI would keep the draft and a retry would save it twice). It logs, marks the
//! index dirty, and the next poll rebuilds the whole index.

use crate::crm::{Crm, Fault};
use crate::error::{Result, ServiceError};
use crm_domain::{Owner, OwnerId, ProductId};
use crm_search::{OwnerDoc, ProductDoc, SearchIndex};
use crm_storage::Conn;
use crm_storage::repo::{owners, products};
use std::collections::HashMap;

fn product_doc(row: &products::ProductIndexRow) -> ProductDoc {
    ProductDoc {
        id: row.id,
        name: row.name.clone(),
        code: row.code.clone(),
        order_count: count(row.order_count),
    }
}

fn count(n: i64) -> u32 {
    u32::try_from(n.max(0)).unwrap_or(u32::MAX)
}

/// Builds the whole index from the database.
pub(crate) fn load(c: &Conn<'_>) -> Result<SearchIndex> {
    let products = products::all_for_index(c)?;
    let owners = owners::all_for_index(c)?;
    Ok(SearchIndex::build(
        products.iter().map(product_doc),
        owners.iter().map(OwnerDoc::from),
    ))
}

impl Crm {
    /// Reads the database for the index (the [`Fault::IndexRead`] seam).
    fn read_for_index<T>(&self, f: impl FnOnce(&Conn<'_>) -> Result<T>) -> Result<T> {
        if self.take_fault(Fault::IndexRead) {
            return Err(crm_storage::StorageError::DeviceUnavailable(
                "injected index read failure".into(),
            )
            .into());
        }
        self.read(f)
    }

    /// Rebuilds the whole index from the database.
    pub(crate) fn rebuild_index(&self) -> Result<()> {
        let index = self.read_for_index(load)?;
        *self.index_mut() = index;
        Ok(())
    }

    /// [`Crm::rebuild_index`] after a commit: a failure is logged and leaves the
    /// index dirty for the next poll.
    pub(crate) fn rebuild_index_after_commit(&self) {
        if let Err(e) = self.rebuild_index() {
            self.index_update_failed(&e);
        }
    }

    fn index_update_failed(&self, e: &ServiceError) {
        tracing::warn!("search index update after a commit failed: {e}");
        self.mark_index_dirty();
    }

    /// Re-indexes the given products from the database (removing the ones that
    /// no longer exist). With `counts`, also refreshes every `order_count` (Q2).
    /// Called after a commit, so best-effort (see the module docs).
    pub(crate) fn sync_products(&self, ids: &[ProductId], counts: bool) {
        if let Err(e) = self.try_sync_products(ids, counts) {
            self.index_update_failed(&e);
        }
    }

    fn try_sync_products(&self, ids: &[ProductId], counts: bool) -> Result<()> {
        if ids.is_empty() && !counts {
            return Ok(());
        }
        let rows = self.read_for_index(|c| Ok(products::all_for_index(c)?))?;
        let mut index = self.index_mut();
        for id in ids {
            match rows.iter().find(|r| r.id == *id) {
                Some(row) => index.upsert_product(product_doc(row)),
                None => {
                    index.remove_product(*id);
                }
            }
        }
        if counts {
            let map: HashMap<ProductId, u32> =
                rows.iter().map(|r| (r.id, count(r.order_count))).collect();
            index.set_order_counts(map);
        }
        Ok(())
    }

    /// Adds or replaces one owner in the index.
    pub(crate) fn index_owner(&self, owner: &Owner) {
        self.index_mut().upsert_owner(OwnerDoc::from(owner));
    }

    /// Removes one owner from the index.
    pub(crate) fn unindex_owner(&self, id: OwnerId) {
        self.index_mut().remove_owner(id);
    }
}

//! The Artikujt list: loading, the delete question and the dialog.

use crate::UiContext;
use crate::catalogue::list::SEARCH_LIMIT;
use crate::catalogue::models::{self, Row};
use crate::catalogue::{dialogs, texts};
use crate::shell::modal::{Modal, ModalOwner};
use crm_services::domain::ProductId;
use crm_services::{Crm, Page, ServiceError};
use std::rc::Rc;

/// Every product by code (empty query) or the search results (by relevance).
pub fn load(crm: &Crm, query: &str) -> Result<Vec<Row>, ServiceError> {
    let found = if query.is_empty() {
        crm.list_products("", Page::ALL)?
    } else {
        crm.search_products(query, SEARCH_LIMIT)?
    };
    Ok(found.iter().map(models::product_row).collect())
}

/// "Ta fshij «emri»? …", plus the SKU note for the last product of its code.
pub fn delete_question(crm: &Crm, id: i64) -> Result<String, ServiceError> {
    let row = crm.get_product(ProductId(id))?;
    let mut text = texts::delete_question(&row.product.name);
    if crm.product_siblings(ProductId(id))?.is_empty() {
        text.push_str(&texts::sku_deleted_too(&row.sku.code));
    }
    Ok(text)
}

/// Deletes the product (and its SKU when it was the last one).
pub fn delete(crm: &Crm, id: i64) -> Result<(), ServiceError> {
    crm.delete_product(ProductId(id)).map(|_| ())
}

/// Opens the product dialog over `owner` (`None` = new product).
pub fn open_dialog<O: ModalOwner>(
    ctx: &Rc<UiContext>,
    owner: &O,
    id: Option<i64>,
    on_saved: Rc<dyn Fn(i64)>,
) -> Option<Modal> {
    dialogs::open_product(ctx, owner, id.map(ProductId), on_saved).map(|(_, modal)| modal)
}

//! The Klientët and Furnitorët lists: loading, the delete question and the dialog.

use crate::UiContext;
use crate::catalogue::list::SEARCH_LIMIT;
use crate::catalogue::models::{self, Row};
use crate::catalogue::{dialogs, texts};
use crate::shell::modal::{Modal, ModalOwner};
use crm_services::domain::{OwnerId, OwnerKind};
use crm_services::{Crm, Page, ServiceError};
use std::rc::Rc;

/// Every owner of `kind` by name in byte order (empty query) or the search
/// results (by relevance).
pub fn load(crm: &Crm, kind: OwnerKind, query: &str) -> Result<Vec<Row>, ServiceError> {
    let found = if query.is_empty() {
        crm.list_owners(kind, "", Page::ALL)?
    } else {
        crm.search_owners(kind, query, SEARCH_LIMIT)?
    };
    Ok(found.iter().map(models::owner_row).collect())
}

/// "Ta fshij «emri»? Ky veprim nuk mund të zhbëhet."
pub fn delete_question(crm: &Crm, id: i64) -> Result<String, ServiceError> {
    let owner = crm.get_owner(OwnerId(id))?;
    Ok(texts::delete_question(&owner.name))
}

/// Deletes the owner (its orders keep their snapshots).
pub fn delete(crm: &Crm, id: i64) -> Result<(), ServiceError> {
    crm.delete_owner(OwnerId(id))
}

/// Opens the owner dialog over `owner` (`None` = new client or supplier).
pub fn open_dialog<O: ModalOwner>(
    ctx: &Rc<UiContext>,
    owner: &O,
    kind: OwnerKind,
    id: Option<i64>,
    on_saved: Rc<dyn Fn(i64)>,
) -> Option<Modal> {
    dialogs::open_owner(ctx, owner, kind, id.map(OwnerId), on_saved).map(|(_, modal)| modal)
}

//! Document tabs: opening, activating, closing, and their persistence in
//! `open_tabs` (the drafts replace the old localStorage).

use super::grid_state::GridState;
use super::texts;
use super::workspace::{Workspace, Ws};
use crm_services::domain::{Order, OrderDraft, OrderId, OrderKind, OwnerId, ProductId, time};
use crm_services::{Crm, OpenTab, ServiceError};
use jiff::civil::Date;
use std::hash::{BuildHasher, Hasher};

/// The header fields of a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// The owner picked from the suggestions (cleared when the text is edited).
    pub owner_id: Option<OwnerId>,
    /// The owner name as typed or picked.
    pub owner_text: String,
    /// The name of the picked owner (to notice edits).
    pub owner_label: String,
    /// NIPT of the picked owner (read-only).
    pub nipt: String,
    /// Adresa of the picked owner (read-only).
    pub address: String,
    /// "Përshkrimi".
    pub description: String,
    /// "Data" (`issued_on`).
    pub date: Date,
}

impl Header {
    /// An empty header dated today.
    pub fn new() -> Header {
        Header {
            owner_id: None,
            owner_text: String::new(),
            owner_label: String::new(),
            nipt: String::new(),
            address: String::new(),
            description: String::new(),
            date: time::today(),
        }
    }

    /// The header of a saved invoice.
    pub fn from_order(order: &Order) -> Header {
        Header {
            owner_id: order.owner_id,
            owner_text: order.owner.name.clone(),
            owner_label: order.owner.name.clone(),
            nipt: order.owner.nipt.clone().unwrap_or_default(),
            address: order.owner.address.clone().unwrap_or_default(),
            description: order.owner.description.clone().unwrap_or_default(),
            date: order.issued_on,
        }
    }
}

impl Default for Header {
    fn default() -> Self {
        Header::new()
    }
}

/// One open document.
#[derive(Debug, Clone)]
pub struct DocTab {
    /// The persisted key (16 hex digits).
    pub key: String,
    /// Shitje or Blerje (fixed per document).
    pub kind: OrderKind,
    /// The saved invoice, or `None` for a new one.
    pub order_id: Option<OrderId>,
    /// The tab title (without the "•").
    pub title: String,
    /// The lines.
    pub grid: GridState,
    /// The header.
    pub header: Header,
    /// Unsaved changes.
    pub dirty: bool,
    /// Changed since the last write to `open_tabs`.
    pub unsynced: bool,
}

impl DocTab {
    /// A new, empty document.
    pub fn new(kind: OrderKind) -> DocTab {
        DocTab {
            key: new_key(),
            kind,
            order_id: None,
            title: kind.new_tab_title().to_owned(),
            grid: GridState::new(kind),
            header: Header::new(),
            dirty: false,
            unsynced: true,
        }
    }

    /// A tab showing saved invoice `order`.
    pub fn from_order(crm: &Crm, order: &Order) -> DocTab {
        DocTab {
            key: new_key(),
            kind: order.kind,
            order_id: Some(order.id),
            title: texts::saved_title(order.kind, order.id, &order.owner.name),
            grid: GridState::from_order(order, &|id| lookup(crm, id)),
            header: Header::from_order(order),
            dirty: false,
            unsynced: true,
        }
    }

    /// Reloads the tab from its saved invoice (after a save).
    pub fn reload(&mut self, crm: &Crm, order: &Order) {
        self.order_id = Some(order.id);
        self.title = texts::saved_title(order.kind, order.id, &order.owner.name);
        self.grid = GridState::from_order(order, &|id| lookup(crm, id));
        self.header = Header::from_order(order);
        self.dirty = false;
        self.unsynced = true;
    }

    /// The draft of the document as it is now.
    pub fn draft(&self) -> OrderDraft {
        OrderDraft {
            kind: self.kind,
            owner_id: self.header.owner_id,
            owner_name: self.header.owner_text.trim().to_owned(),
            owner_description: self.header.description.clone(),
            issued_on: self.header.date,
            lines: self.grid.draft_lines(),
        }
    }

    /// The row persisted for this tab.
    pub fn to_open_tab(&self, position: usize, active: bool) -> OpenTab {
        OpenTab {
            tab_key: self.key.clone(),
            position: i64::try_from(position).unwrap_or(i64::MAX),
            kind: self.kind,
            order_id: self.order_id,
            title: self.title.clone(),
            active,
            draft: self.dirty.then(|| self.draft()),
        }
    }

    /// Rebuilds a persisted tab. `None` when its invoice no longer exists.
    pub fn restore(crm: &Crm, stored: &OpenTab) -> Option<DocTab> {
        let saved = match stored.order_id {
            Some(id) => match crm.get_order(id) {
                Ok(order) => Some(order),
                Err(ServiceError::NotFound) => return None,
                Err(e) => {
                    tracing::warn!("restoring tab {}: {e}", stored.tab_key);
                    return None;
                }
            },
            None => None,
        };
        let (grid, header, dirty) = match (&stored.draft, &saved) {
            (Some(draft), _) => (
                GridState::from_draft(draft, saved.as_ref(), &|id| lookup(crm, id)),
                header_from_draft(crm, draft, saved.as_ref()),
                true,
            ),
            (None, Some(order)) => (
                GridState::from_order(order, &|id| lookup(crm, id)),
                Header::from_order(order),
                false,
            ),
            (None, None) => (GridState::new(stored.kind), Header::new(), false),
        };
        // The stored title text may predate a rename ("Faturë Shitje (e re)"):
        // it is rebuilt with the current texts and written back if it differs.
        let title = saved.as_ref().map_or_else(
            || stored.kind.new_tab_title().to_owned(),
            |order| texts::saved_title(order.kind, order.id, &order.owner.name),
        );
        let unsynced = title != stored.title;
        Some(DocTab {
            key: stored.tab_key.clone(),
            kind: stored.kind,
            order_id: stored.order_id,
            title,
            grid,
            header,
            dirty,
            unsynced,
        })
    }
}

fn header_from_draft(crm: &Crm, draft: &OrderDraft, saved: Option<&Order>) -> Header {
    let mut header = Header {
        owner_id: draft.owner_id,
        owner_text: draft.owner_name.clone(),
        owner_label: String::new(),
        nipt: String::new(),
        address: String::new(),
        description: draft.owner_description.clone(),
        date: draft.issued_on,
    };
    if let Some(id) = draft.owner_id {
        match crm.get_owner(id) {
            Ok(owner) => {
                header.owner_label = owner.name.clone();
                header.nipt = owner.nipt.unwrap_or_default();
                header.address = owner.address.unwrap_or_default();
            }
            Err(_) => header.owner_id = None,
        }
    } else if let Some(order) = saved
        && order.owner.name == draft.owner_name
    {
        header.nipt = order.owner.nipt.clone().unwrap_or_default();
        header.address = order.owner.address.clone().unwrap_or_default();
    }
    header
}

/// The current product row (stock and prices) of `id`.
pub fn lookup(crm: &Crm, id: ProductId) -> Option<crm_services::domain::ProductRow> {
    crm.get_product(id).ok()
}

/// A random 16-hex-digit tab key.
pub fn new_key() -> String {
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    format!("{:016x}", hasher.finish())
}

impl Workspace {
    /// Writes the tab at `index` now.
    pub fn persist(&mut self, index: usize) {
        let Some(ctx) = self.ctx() else { return };
        let active = self.active == Some(index);
        let Some(tab) = self.tabs.get_mut(index) else {
            return;
        };
        match ctx.crm.save_tab(tab.to_open_tab(index, active)) {
            Ok(()) => tab.unsynced = false,
            Err(e) => tracing::warn!("saving tab {}: {e}", tab.key),
        }
    }

    /// Writes every tab changed since its last write (and the positions).
    pub fn flush(&mut self) {
        self.draft_timer.stop();
        for i in 0..self.tabs.len() {
            if self.tabs[i].unsynced {
                self.persist(i);
            }
        }
    }

    /// Restores the persisted tabs (order, active tab, drafts). Tabs whose
    /// invoice was deleted are dropped.
    pub fn restore(&mut self) {
        let Some(ctx) = self.ctx() else { return };
        let stored = match ctx.crm.list_tabs() {
            Ok(tabs) => tabs,
            Err(e) => {
                tracing::warn!("listing tabs: {e}");
                return;
            }
        };
        let mut active = None;
        for tab in &stored {
            match DocTab::restore(&ctx.crm, tab) {
                Some(doc) => {
                    if tab.active {
                        active = Some(self.tabs.len());
                    }
                    self.tabs.push(doc);
                }
                None => {
                    if let Err(e) = ctx.crm.remove_tab(&tab.tab_key) {
                        tracing::warn!("removing tab {}: {e}", tab.tab_key);
                    }
                }
            }
        }
        self.active = active.or(if self.tabs.is_empty() { None } else { Some(0) });
        self.sync_tabs();
        self.show_current();
    }

    /// After a restore (F-016): drops every open tab without writing it (their
    /// order ids and drafts belong to the replaced database; a reused invoice
    /// number would make a stale tab overwrite another invoice) and loads the
    /// restored `open_tabs`.
    pub fn reload_from_db(&mut self) {
        self.draft_timer.stop();
        self.pending = None;
        self.focus = None;
        self.last_click = None;
        self.tabs.clear();
        self.active = None;
        self.restore();
    }

    /// Adds `tab` at the end and activates it.
    pub fn push_tab(&mut self, tab: DocTab) {
        self.tabs.push(tab);
        let index = self.tabs.len() - 1;
        self.activate(index);
    }

    /// Activates the tab at `index` (persisting the choice).
    pub fn activate(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        self.commit_pending();
        self.active = Some(index);
        self.tabs[index].unsynced = true;
        self.persist(index);
        self.sync_tabs();
        self.show_current();
    }

    /// Removes the tabs for which `close` is true; the active tab becomes the
    /// one at the same index (clamped) when it was closed.
    pub fn remove_tabs(&mut self, close: impl Fn(usize, &DocTab) -> bool) {
        let Some(ctx) = self.ctx() else { return };
        self.commit_pending();
        let old_active = self.active;
        let mut kept = Vec::new();
        let mut new_active = None;
        let mut active_closed = false;
        for (i, tab) in std::mem::take(&mut self.tabs).into_iter().enumerate() {
            if close(i, &tab) {
                if let Err(e) = ctx.crm.remove_tab(&tab.key) {
                    tracing::warn!("removing tab {}: {e}", tab.key);
                }
                if old_active == Some(i) {
                    active_closed = true;
                }
            } else {
                if old_active == Some(i) {
                    new_active = Some(kept.len());
                }
                kept.push(tab);
            }
        }
        self.tabs = kept;
        if active_closed {
            let at = old_active.unwrap_or(0);
            new_active = if self.tabs.is_empty() {
                None
            } else {
                Some(at.min(self.tabs.len() - 1))
            };
        }
        self.active = new_active;
        for tab in &mut self.tabs {
            tab.unsynced = true;
        }
        self.flush();
        self.sync_tabs();
        self.show_current();
    }

    /// Opens saved invoice `id`: activates its tab, or opens a new one.
    pub fn open_order(&mut self, id: OrderId) {
        if let Some(i) = self.index_of_order(id) {
            self.activate(i);
            return;
        }
        let Some(ctx) = self.ctx() else { return };
        match ctx.crm.get_order(id) {
            Ok(order) => {
                let tab = DocTab::from_order(&ctx.crm, &order);
                self.push_tab(tab);
            }
            Err(e) => super::prompt::error(&ctx, &e),
        }
    }
}

/// Asks before closing when any of the tabs chosen by `close` has unsaved
/// changes, then closes them.
pub fn close_with_prompt(ws: &Ws, close: impl Fn(usize, &DocTab) -> bool + 'static) {
    let (ctx, dirty, count) = {
        let w = ws.borrow();
        let chosen: Vec<&DocTab> = w
            .tabs
            .iter()
            .enumerate()
            .filter(|(i, t)| close(*i, t))
            .map(|(_, t)| t)
            .collect();
        (w.ctx(), chosen.iter().any(|t| t.dirty), chosen.len())
    };
    if count == 0 {
        return;
    }
    // Ask by key: indices may not survive the dialog.
    let keys: Vec<String> = {
        let w = ws.borrow();
        w.tabs
            .iter()
            .enumerate()
            .filter(|(i, t)| close(*i, t))
            .map(|(_, t)| t.key.clone())
            .collect()
    };
    let run = {
        let ws = ws.clone();
        move || {
            ws.borrow_mut()
                .remove_tabs(move |_, t| keys.contains(&t.key));
        }
    };
    match ctx {
        Some(ctx) if dirty => {
            let text = if count == 1 {
                texts::CLOSE_DIRTY_TEXT
            } else {
                texts::CLOSE_DIRTY_MANY_TEXT
            };
            super::prompt::confirm(
                &ctx,
                texts::CLOSE_DIRTY_TITLE,
                text,
                texts::CLOSE_LABEL,
                move |ok| {
                    if ok {
                        run();
                    }
                },
            );
        }
        _ => run(),
    }
}

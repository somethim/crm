//! The document header (owner search, Përshkrimi, Data) and the document
//! commands: Ruaj, Printo, Fshi.

use super::grid_nav::{PRODUCT_COL, Pos};
use super::texts;
use super::workspace::{SUGGESTION_LIMIT, Workspace, Ws};
use crate::{SuggestionItem, UiContext};
use crm_services::domain::format;
use crm_services::domain::time;
use std::rc::Rc;

impl Workspace {
    /// Searches the owners of the active document's kind and opens the list.
    pub fn query_owners(&mut self, text: &str) {
        let Some(ctx) = self.ctx() else { return };
        let Some(kind) = self.current().map(|t| t.kind.owner_kind()) else {
            return;
        };
        match ctx.crm.search_owners_counted(kind, text, SUGGESTION_LIMIT) {
            Ok((hits, total)) => {
                let items: Vec<SuggestionItem> = hits
                    .iter()
                    .map(|o| SuggestionItem {
                        label: o.name.clone().into(),
                        note: texts::owner_note(
                            o.address.as_deref().unwrap_or_default(),
                            o.description.as_deref().unwrap_or_default(),
                        )
                        .into(),
                        detail: o.nipt.as_deref().unwrap_or_default().trim().into(),
                    })
                    .collect();
                let count = if hits.is_empty() {
                    String::new()
                } else {
                    texts::suggestion_count(hits.len(), total)
                };
                self.owner_hits = hits;
                self.owner_items.set_vec(items);
                self.bridge(|b| {
                    b.set_owner_count_text(count.into());
                    b.set_owner_highlighted(-1);
                    b.set_owner_open(true);
                });
            }
            Err(e) => tracing::warn!("owner search: {e}"),
        }
    }
}

/// `owner-focused`: shows the suggestions at once.
pub fn owner_focused(ws: &Ws, text: &str) {
    let mut w = ws.borrow_mut();
    w.last_focus = None;
    w.query_owners(text);
}

/// `owner-edited`: the typed text; editing after a selection clears the
/// selected owner, so the save resolves the owner by name.
pub fn owner_edited(ws: &Ws, text: &str) {
    let mut w = ws.borrow_mut();
    let Some(tab) = w.current_mut() else { return };
    tab.header.owner_text = text.to_owned();
    if tab.header.owner_id.is_some() && tab.header.owner_label != text {
        tab.header.owner_id = None;
        tab.header.owner_label.clear();
        tab.header.nipt.clear();
        tab.header.address.clear();
        w.sync_owner();
    }
    w.touch();
    w.query_owners(text);
}

/// `owner-chosen`: fills the owner (name, NIPT, Adresa) and closes the list.
pub fn owner_chosen(ws: &Ws, index: usize) {
    let mut w = ws.borrow_mut();
    let Some(owner) = w.owner_hits.get(index).cloned() else {
        return;
    };
    let Some(tab) = w.current_mut() else { return };
    tab.header.owner_id = Some(owner.id);
    tab.header.owner_text = owner.name.clone();
    tab.header.owner_label = owner.name.clone();
    tab.header.nipt = owner.nipt.clone().unwrap_or_default();
    tab.header.address = owner.address.clone().unwrap_or_default();
    w.sync_owner();
    w.bridge(|b| {
        b.set_owner_open(false);
        b.set_owner_highlighted(-1);
    });
    w.touch();
}

/// `description-edited`.
pub fn description_edited(ws: &Ws, text: &str) {
    let mut w = ws.borrow_mut();
    let Some(tab) = w.current_mut() else { return };
    tab.header.description = text.to_owned();
    w.touch();
}

/// `date-edited`: a valid date was typed or stepped to.
pub fn date_edited(ws: &Ws, iso: &str) {
    let Ok(date) = format::parse_date(iso) else {
        return;
    };
    let mut w = ws.borrow_mut();
    let Some(tab) = w.current_mut() else { return };
    if tab.header.date != date {
        tab.header.date = date;
        w.touch();
    }
}

/// `save-current` (Ruaj, Ctrl+S): a new invoice saves at once, a saved one
/// after "Konfirmo përditësimin".
pub fn save(ws: &Ws) {
    let Some((ctx, saved)) = commit_header(ws) else {
        return;
    };
    if saved {
        let ws = ws.clone();
        super::prompt::confirm(
            &ctx,
            texts::UPDATE_TITLE,
            texts::UPDATE_TEXT,
            texts::CONTINUE_LABEL,
            move |ok| {
                if ok {
                    save_now(&ws);
                }
            },
        );
    } else {
        save_now(ws);
    }
}

/// Commits the pending cell and the date field into the active document:
/// `(ctx, whether it is a saved invoice)`, or `None` (an invalid date shows
/// in a message; no document).
fn commit_header(ws: &Ws) -> Option<(Rc<UiContext>, bool)> {
    let mut w = ws.borrow_mut();
    w.commit_pending();
    let date_text = w
        .bridge(|b| b.get_doc_date().to_string())
        .unwrap_or_default();
    let ctx = w.ctx()?;
    let tab = w.current_mut()?;
    match format::parse_date(&date_text) {
        Ok(date) => tab.header.date = date,
        Err(_) => {
            drop(w);
            super::prompt::message(&ctx, crate::shell::texts::ERROR_TITLE, texts::INVALID_DATE);
            return None;
        }
    }
    let saved = tab.order_id.is_some();
    Some((ctx, saved))
}

/// Saves the active document. On success the tab shows the saved invoice
/// (`true`); a service error (validation, stock, Q6) shows in a message and
/// the draft stays (`false`), also when the committed invoice cannot be
/// reloaded (F-013).
pub fn save_now(ws: &Ws) -> bool {
    let mut w = ws.borrow_mut();
    let Some(ctx) = w.ctx() else { return false };
    let Some(index) = w.active else { return false };
    let (draft, existing, key) = {
        let tab = &w.tabs[index];
        (tab.draft(), tab.order_id, tab.key.clone())
    };
    // Synchronous: a save is one short transaction (see the Implementation record).
    let id = match ctx.crm.save_order(draft, existing, Some(key)) {
        Ok(id) => id,
        Err(e) => {
            drop(w);
            super::prompt::error(&ctx, &e);
            return false;
        }
    };
    match load_saved(&ctx.crm, id) {
        Ok(order) => {
            // The rows are rebuilt, so the focused cell is gone: focus it again
            // (none: the placeholder's Artikulli), also after the confirm (F-008).
            let focus = w.last_focus;
            let tab = &mut w.tabs[index];
            tab.reload(&ctx.crm, &order);
            w.persist(index);
            w.sync_tabs();
            w.show_current();
            let rows = w.current().map_or(1, |t| t.grid.rows());
            w.last_focus = Some(focus.unwrap_or(Pos::new(rows - 1, PRODUCT_COL)));
            w.refocus();
            drop(w);
            // Every view refreshes (the invoice list, a visible Artikujt list's
            // Gjendja): the 2 s poll ignores this process's own commits (F-001).
            ctx.notify_data_changed();
            ctx.flash(if existing.is_some() {
                texts::UPDATED
            } else {
                texts::CREATED
            });
            true
        }
        Err(e) => {
            // F-013: the invoice is committed even though it cannot be shown;
            // the tab must point at it, or the next Ruaj inserts it (and moves
            // the stock) a second time. The draft stays on screen.
            let tab = &mut w.tabs[index];
            tab.order_id = Some(id);
            tab.title = texts::saved_title(tab.kind, id, tab.header.owner_text.trim());
            tab.dirty = false;
            tab.unsynced = true;
            w.persist(index);
            w.sync_tabs();
            w.bridge(|b| b.set_doc_number(id.0.to_string().into()));
            drop(w);
            ctx.notify_data_changed();
            super::prompt::error(&ctx, &e);
            false
        }
    }
}

#[cfg(test)]
thread_local! {
    /// Makes the next read of a just-saved invoice fail (F-013 tests).
    pub(crate) static FAIL_LOAD_SAVED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Reads the invoice a save just committed.
fn load_saved(
    crm: &crm_services::Crm,
    id: crm_services::domain::OrderId,
) -> Result<crm_services::domain::Order, crm_services::ServiceError> {
    #[cfg(test)]
    if FAIL_LOAD_SAVED.with(|f| f.replace(false)) {
        return Err(crm_services::ServiceError::Closed);
    }
    crm.get_order(id)
}

/// `delete-current` (Fshi): after "Konfirmo fshirjen" the invoice is deleted
/// and its tab closes; Q6 shows in a message.
pub fn delete(ws: &Ws) {
    let (ctx, id) = {
        let mut w = ws.borrow_mut();
        w.commit_pending();
        (w.ctx(), w.current().and_then(|t| t.order_id))
    };
    let (Some(ctx), Some(id)) = (ctx, id) else {
        return;
    };
    let ws = ws.clone();
    let inner = ctx.clone();
    super::prompt::confirm(
        &ctx,
        texts::DELETE_TITLE,
        texts::DELETE_TEXT,
        texts::CONTINUE_LABEL,
        move |ok| {
            if !ok {
                return;
            }
            match inner.crm.delete_order(id) {
                Ok(()) => {
                    let mut w = ws.borrow_mut();
                    w.remove_tabs(|_, t| t.order_id == Some(id));
                    drop(w);
                    // As after a save: every view refreshes (F-001).
                    inner.notify_data_changed();
                    inner.flash(texts::DELETED);
                }
                Err(e) => super::prompt::error(&inner, &e),
            }
        },
    );
}

/// `print-current`: opens the receipt preview (task 012) of the active saved
/// invoice, for both kinds. A document that is not saved yet has nothing to
/// print (the command is disabled for it). The receipt is rendered from the
/// saved invoice, so one with unsaved edits is saved first, after "Ruaj para
/// printimit?" (F-025): the preview shows what is on screen and stored, and
/// Anulo (or a failed save) opens no preview.
pub fn print(ws: &Ws) {
    let (ctx, id, dirty) = {
        let mut w = ws.borrow_mut();
        w.commit_pending();
        let tab = w.current();
        let id = tab.and_then(|t| t.order_id);
        let dirty = tab.is_some_and(|t| t.dirty);
        (w.ctx(), id, dirty)
    };
    let (Some(ctx), Some(id)) = (ctx, id) else {
        return;
    };
    if !dirty {
        crate::print::open_preview(&ctx, id);
        return;
    }
    let ws = ws.clone();
    let inner = ctx.clone();
    super::prompt::confirm(
        &ctx,
        texts::PRINT_UNSAVED_TITLE,
        texts::PRINT_UNSAVED_TEXT,
        texts::SAVE_LABEL,
        move |ok| {
            if !ok || commit_header(&ws).is_none() || !save_now(&ws) {
                return;
            }
            let id = ws.borrow().current().and_then(|t| t.order_id);
            if let Some(id) = id {
                crate::print::open_preview(&inner, id);
            }
        },
    );
}

/// Today as `dd.MM.yyyy`.
pub fn today_text() -> String {
    format::date(time::today())
}

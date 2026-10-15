//! The product and owner property dialogs (Ruaj / Fshi / Anulo), the shared
//! delete flow behind Fshi, and the confirm and message boxes modal over a
//! list window or a property dialog.
//!
//! Money is parsed with `parse_amount(…, Input)` and quantities with
//! `parse_qty` (blank = 0); `ServiceError::Validation` field keys go to the
//! inline error under their field, anything else to a message box.

use crate::catalogue::{ListKind, owners, products, texts};
use crate::shell::format;
use crate::shell::modal::{self, Modal, ModalOwner};
use crate::shell::texts as shell_texts;
use crate::{ConfirmDialog, MessageDialog, OwnerDialog, ProductDialog, UiContext};
use crm_services::domain::{
    Money, Owner, OwnerId, OwnerInput, OwnerKind, ParseMode, ProductId, ProductInput, ProductRow,
    Qty, ValidationErrors,
};
use crm_services::{ProductPatch, ServiceError};
use slint::{ComponentHandle, SharedString};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

thread_local! {
    static LAST_PRODUCT: RefCell<Option<slint::Weak<ProductDialog>>> = const { RefCell::new(None) };
    static LAST_OWNER: RefCell<Option<slint::Weak<OwnerDialog>>> = const { RefCell::new(None) };
    static LAST_CONFIRM: RefCell<Option<slint::Weak<ConfirmDialog>>> = const { RefCell::new(None) };
    static LAST_MESSAGE: RefCell<Option<slint::Weak<MessageDialog>>> = const { RefCell::new(None) };
    /// The open dialogs that act on a row id (product/owner dialogs, confirm
    /// boxes), closed by [`close_after_restore`].
    static BY_ID: RefCell<Vec<Modal>> = const { RefCell::new(Vec::new()) };
}

/// Remembers `modal` until a restore (closed ones are dropped on the way).
fn track(modal: &Modal) {
    BY_ID.with(|m| {
        let mut m = m.borrow_mut();
        m.retain(Modal::is_open);
        m.push(modal.clone());
    });
}

/// After "Rikthe nga rezervimi" (F-016): the row ids held by the open product
/// and owner dialogs and confirm boxes may name a different row (or none) in
/// the restored database, so they close without saving (a confirm box answers
/// "Anulo").
pub fn close_after_restore() {
    // Taken out first: closing runs hooks that may open or track dialogs.
    // Newest first, so a confirm box closes before the dialog it covers.
    let open = BY_ID.with(|m| std::mem::take(&mut *m.borrow_mut()));
    for modal in open.into_iter().rev() {
        modal.close();
    }
}

/// The product dialog opened last, while it is shown.
pub fn open_product_dialog() -> Option<ProductDialog> {
    LAST_PRODUCT
        .with(|d| d.borrow().as_ref().and_then(slint::Weak::upgrade))
        .filter(|d| d.window().is_visible())
}

/// The owner dialog opened last, while it is shown.
pub fn open_owner_dialog() -> Option<OwnerDialog> {
    LAST_OWNER
        .with(|d| d.borrow().as_ref().and_then(slint::Weak::upgrade))
        .filter(|d| d.window().is_visible())
}

/// The confirm box opened last over a list window, while it is shown.
pub fn open_confirm() -> Option<ConfirmDialog> {
    LAST_CONFIRM
        .with(|d| d.borrow().as_ref().and_then(slint::Weak::upgrade))
        .filter(|d| d.window().is_visible())
}

/// The message box opened last over a list window, while it is shown.
pub fn open_message() -> Option<MessageDialog> {
    LAST_MESSAGE
        .with(|d| d.borrow().as_ref().and_then(slint::Weak::upgrade))
        .filter(|d| d.window().is_visible())
}

/// Shows `dialog` modal over `owner` and registers it with the lock.
pub fn open_modal<O: ModalOwner, D: ComponentHandle + 'static>(
    ctx: &UiContext,
    owner: &O,
    dialog: &D,
) -> Modal {
    ctx.register_window(Box::new(dialog.as_weak()));
    modal::open(owner, dialog)
}

/// A message box modal over `owner`.
pub fn message_over<O: ModalOwner>(
    ctx: &UiContext,
    owner: &O,
    title: &str,
    text: &str,
) -> Option<MessageDialog> {
    let dialog = match MessageDialog::new() {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("message dialog: {e}");
            return None;
        }
    };
    dialog.set_caption(title.into());
    dialog.set_text(text.into());
    let modal = open_modal(ctx, owner, &dialog);
    dialog.on_closed(move || modal.close());
    LAST_MESSAGE.with(|d| *d.borrow_mut() = Some(dialog.as_weak()));
    Some(dialog)
}

/// A yes/no box modal over `owner`; `done` gets `true` for `ok_label`, `false`
/// for "Anulo" or closing the box.
pub fn confirm_over<O: ModalOwner>(
    ctx: &UiContext,
    owner: &O,
    title: &str,
    text: &str,
    ok_label: &str,
    done: impl FnOnce(bool) + 'static,
) -> Option<ConfirmDialog> {
    let dialog = match ConfirmDialog::new() {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("confirm dialog: {e}");
            return None;
        }
    };
    dialog.set_caption(title.into());
    dialog.set_text(text.into());
    dialog.set_ok_label(ok_label.into());
    let modal = open_modal(ctx, owner, &dialog);
    type Answer = Box<dyn FnOnce(bool)>;
    let done: Rc<RefCell<Option<Answer>>> = Rc::new(RefCell::new(Some(Box::new(done))));
    let answer = move |ok: bool| {
        let f = done.borrow_mut().take();
        if let Some(f) = f {
            f(ok);
        }
    };
    // The answer runs once the box is closed and its owner unblocked, so it
    // may close the owner too (Fshi in a property dialog).
    let choice = Rc::new(Cell::new(false));
    let close = modal.clone();
    let chosen = choice.clone();
    dialog.on_answered(move |ok| {
        chosen.set(ok);
        close.close();
    });
    modal.on_closed(move || answer(choice.get()));
    track(&modal);
    LAST_CONFIRM.with(|d| *d.borrow_mut() = Some(dialog.as_weak()));
    Some(dialog)
}

/// Fshi: asks "Konfirmo fshirjen" over `owner`, then deletes row `id` of
/// `kind` and tells every window (F-001). `done` gets `Ok(true)` after the
/// delete, `Ok(false)` for "Anulo" (or while a background job runs, F-029),
/// and the error when the question or the delete failed. Used by the list
/// windows and the property dialogs alike.
pub fn confirm_delete<O: ModalOwner>(
    ctx: &Rc<UiContext>,
    owner: &O,
    kind: ListKind,
    id: i64,
    done: impl FnOnce(Result<bool, ServiceError>) + 'static,
) {
    let question = match kind.owner_kind() {
        None => products::delete_question(&ctx.crm, id),
        Some(_) => owners::delete_question(&ctx.crm, id),
    };
    let question = match question {
        Ok(q) => q,
        Err(e) => return done(Err(e)),
    };
    let ctx_weak = Rc::downgrade(ctx);
    confirm_over(
        ctx,
        owner,
        texts::DELETE_TITLE,
        &question,
        texts::DELETE_OK,
        move |yes| {
            let Some(ctx) = ctx_weak.upgrade() else {
                return;
            };
            if !yes {
                return done(Ok(false));
            }
            // F-029: a background job (a restore) is running.
            if ctx.is_busy() {
                tracing::warn!("delete ignored: a background job is running");
                return done(Ok(false));
            }
            let deleted = match kind.owner_kind() {
                None => products::delete(&ctx.crm, id),
                Some(_) => owners::delete(&ctx.crm, id),
            };
            match deleted {
                Ok(()) => {
                    done(Ok(true));
                    // F-001: own writes reach the other windows directly.
                    ctx.notify_data_changed();
                }
                Err(e) => done(Err(e)),
            }
        },
    );
}

/// Fshi in a property dialog: the confirm box opens over the dialog; a
/// delete closes it, a failure shows the error over it and keeps it open.
fn delete_from_dialog<D: ModalOwner>(
    ctx: &Rc<UiContext>,
    dialog: &D,
    kind: ListKind,
    id: i64,
    modal: Option<Modal>,
) {
    let ctx_weak = Rc::downgrade(ctx);
    let dialog_weak = dialog.as_weak();
    confirm_delete(ctx, dialog, kind, id, move |result| match result {
        Ok(true) => {
            if let Some(modal) = modal {
                modal.close();
            }
        }
        Ok(false) => {}
        Err(e) => {
            tracing::warn!("{e}");
            if let (Some(ctx), Some(d)) = (ctx_weak.upgrade(), dialog_weak.upgrade()) {
                message_over(&ctx, &d, shell_texts::ERROR_TITLE, &e.user_message());
            }
        }
    });
}

impl ModalOwner for ProductDialog {
    fn set_input_blocked(&self, blocked: bool) {
        self.set_input_blocked(blocked);
    }

    fn native_title(&self) -> String {
        self.get_caption().to_string()
    }
}

impl ModalOwner for OwnerDialog {
    fn set_input_blocked(&self, blocked: bool) {
        self.set_input_blocked(blocked);
    }

    fn native_title(&self) -> String {
        self.get_caption().to_string()
    }
}

/// Shows error texts in a message box over the dialog's owner.
type ShowError = Rc<dyn Fn(&str)>;

fn error_shower<O: ModalOwner>(ctx: &Rc<UiContext>, owner: &O) -> ShowError {
    let ctx = Rc::downgrade(ctx);
    let owner = owner.as_weak();
    Rc::new(move |text: &str| {
        if let (Some(ctx), Some(owner)) = (ctx.upgrade(), owner.upgrade()) {
            message_over(&ctx, &owner, shell_texts::ERROR_TITLE, text);
        }
    })
}

/// Blank = 0; otherwise an amount typed in the app.
pub fn parse_money(text: &str) -> Result<Money, String> {
    if text.trim().is_empty() {
        return Ok(Money(0));
    }
    format::parse_amount(text, ParseMode::Input).map_err(|e| e.user_message())
}

/// Blank = 0; otherwise a whole quantity.
pub fn parse_quantity(text: &str) -> Result<Qty, String> {
    if text.trim().is_empty() {
        return Ok(Qty(0));
    }
    format::parse_qty(text).map_err(|e| e.user_message())
}

/// The messages of `errors` for `field`, one per line.
fn field_text(errors: &ValidationErrors, field: &str) -> SharedString {
    errors.messages_for(field).join("\n").into()
}

/// The messages of the fields not in `known` (shown in a message box).
fn other_text(errors: &ValidationErrors, known: &[&str]) -> String {
    errors
        .errors()
        .iter()
        .filter(|e| !known.contains(&e.field))
        .map(|e| e.message.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn report(e: &ServiceError, show: &ShowError) {
    tracing::warn!("{e}");
    show(&e.user_message());
}

// -- products --------------------------------------------------------------

struct ProductForm {
    ctx: Weak<UiContext>,
    dialog: slint::Weak<ProductDialog>,
    editing: Cell<Option<ProductId>>,
    loaded: RefCell<Option<ProductRow>>,
    on_saved: Rc<dyn Fn(i64)>,
    show_error: ShowError,
    modal: RefCell<Option<Modal>>,
}

const PRODUCT_FIELDS: &[&str] = &[
    "code",
    "name",
    "unit",
    "buy_price",
    "sell_price",
    "quantity",
];

impl ProductForm {
    fn fill(&self, ctx: &UiContext, d: &ProductDialog, row: &ProductRow) {
        d.set_caption(texts::product_caption(&row.product.name).into());
        d.set_code(row.sku.code.as_str().into());
        d.set_name(row.product.name.as_str().into());
        d.set_unit(row.sku.unit.as_str().into());
        d.set_buy_price(format::amount(row.sku.buy_price).into());
        d.set_sell_price(format::amount(row.sku.sell_price).into());
        d.set_quantity(format::qty(row.sku.quantity).into());
        let note = match ctx.crm.product_siblings(row.product.id) {
            Ok(others) if !others.is_empty() => texts::shared_sku_note(others.len()),
            Ok(_) => String::new(),
            Err(e) => {
                tracing::warn!("product siblings: {e}");
                String::new()
            }
        };
        d.set_note(note.into());
    }

    fn clear_errors(d: &ProductDialog) {
        for set in [
            ProductDialog::set_code_error,
            ProductDialog::set_name_error,
            ProductDialog::set_unit_error,
            ProductDialog::set_buy_price_error,
            ProductDialog::set_sell_price_error,
            ProductDialog::set_quantity_error,
        ] {
            set(d, SharedString::new());
        }
    }

    fn show_validation(&self, d: &ProductDialog, errors: &ValidationErrors) {
        d.set_code_error(field_text(errors, "code"));
        d.set_name_error(field_text(errors, "name"));
        d.set_unit_error(field_text(errors, "unit"));
        d.set_buy_price_error(field_text(errors, "buy_price"));
        d.set_sell_price_error(field_text(errors, "sell_price"));
        d.set_quantity_error(field_text(errors, "quantity"));
        let other = other_text(errors, PRODUCT_FIELDS);
        if !other.is_empty() {
            (self.show_error)(&other);
        }
    }

    /// Only the fields that differ from the loaded product (a blank code is
    /// ignored by the services).
    fn patch(&self, d: &ProductDialog, buy: Money, sell: Money, qty: Qty) -> ProductPatch {
        let loaded = self.loaded.borrow();
        let name = d.get_name().to_string();
        let code = d.get_code().trim().to_owned();
        let unit = d.get_unit().to_string();
        let Some(cur) = loaded.as_ref() else {
            return ProductPatch {
                name: Some(name),
                code: Some(code),
                unit: Some(unit),
                buy_price: Some(buy),
                sell_price: Some(sell),
                quantity: Some(qty),
            };
        };
        ProductPatch {
            name: (name.trim() != cur.product.name).then_some(name),
            code: (!code.is_empty() && code != cur.sku.code).then_some(code),
            unit: (unit.trim() != cur.sku.unit).then_some(unit),
            buy_price: (buy != cur.sku.buy_price).then_some(buy),
            sell_price: (sell != cur.sku.sell_price).then_some(sell),
            quantity: (qty != cur.sku.quantity).then_some(qty),
        }
    }

    fn close(&self) {
        let modal = self.modal.borrow().clone();
        if let Some(modal) = modal {
            modal.close();
        }
    }

    /// Fshi (a saved product only).
    fn delete(&self) {
        let (Some(ctx), Some(d), Some(id)) = (
            self.ctx.upgrade(),
            self.dialog.upgrade(),
            self.editing.get(),
        ) else {
            return;
        };
        let modal = self.modal.borrow().clone();
        delete_from_dialog(&ctx, &d, ListKind::Products, id.0, modal);
    }

    /// Ruaj: validates, saves and closes (stays open with the errors shown).
    fn save(&self) {
        let (Some(ctx), Some(d)) = (self.ctx.upgrade(), self.dialog.upgrade()) else {
            return;
        };
        // F-029: a background job (a restore) is running; the dialog stays
        // open and saves once it ends.
        if ctx.is_busy() {
            tracing::warn!("save ignored: a background job is running");
            return;
        }
        Self::clear_errors(&d);
        let buy = parse_money(&d.get_buy_price());
        let sell = parse_money(&d.get_sell_price());
        let qty = parse_quantity(&d.get_quantity());
        let (buy, sell, qty) = match (buy, sell, qty) {
            (Ok(b), Ok(s), Ok(q)) => (b, s, q),
            (b, s, q) => {
                d.set_buy_price_error(b.err().unwrap_or_default().into());
                d.set_sell_price_error(s.err().unwrap_or_default().into());
                d.set_quantity_error(q.err().unwrap_or_default().into());
                return;
            }
        };
        let saved = match self.editing.get() {
            Some(id) => ctx.crm.update_product(id, self.patch(&d, buy, sell, qty)),
            None => ctx.crm.create_product(ProductInput {
                code: d.get_code().trim().to_owned(),
                name: d.get_name().to_string(),
                unit: d.get_unit().to_string(),
                buy_price: buy,
                sell_price: sell,
                quantity: qty,
            }),
        };
        match saved {
            Ok(row) => {
                let id = row.product.id;
                self.editing.set(Some(id));
                self.fill(&ctx, &d, &row);
                *self.loaded.borrow_mut() = Some(row);
                (self.on_saved)(id.0);
                self.close();
            }
            Err(ServiceError::Validation(errors)) => self.show_validation(&d, &errors),
            Err(e) => report(&e, &self.show_error),
        }
    }
}

/// Opens the product dialog over `owner`: create mode for `None`, else edit
/// mode filled from the database (with Fshi). `on_saved` gets the product id
/// after a successful Ruaj; a delete through Fshi notifies every window instead.
pub fn open_product<O: ModalOwner>(
    ctx: &Rc<UiContext>,
    owner: &O,
    id: Option<ProductId>,
    on_saved: Rc<dyn Fn(i64)>,
) -> Option<(ProductDialog, Modal)> {
    let show_error = error_shower(ctx, owner);
    let loaded = match id.map(|id| ctx.crm.get_product(id)).transpose() {
        Ok(row) => row,
        Err(e) => {
            report(&e, &show_error);
            return None;
        }
    };
    let dialog = match ProductDialog::new() {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("product dialog: {e}");
            return None;
        }
    };
    let form = Rc::new(ProductForm {
        ctx: Rc::downgrade(ctx),
        dialog: dialog.as_weak(),
        editing: Cell::new(id),
        loaded: RefCell::new(None),
        on_saved,
        show_error,
        modal: RefCell::new(None),
    });
    match &loaded {
        Some(row) => form.fill(ctx, &dialog, row),
        None => dialog.set_caption(texts::NEW_PRODUCT.into()),
    }
    dialog.set_editing(loaded.is_some());
    *form.loaded.borrow_mut() = loaded;
    let f = Rc::downgrade(&form);
    dialog.on_ok(move || {
        if let Some(f) = f.upgrade() {
            f.save();
        }
    });
    let f = Rc::downgrade(&form);
    dialog.on_delete(move || {
        if let Some(f) = f.upgrade() {
            f.delete();
        }
    });
    let f = Rc::downgrade(&form);
    dialog.on_cancel(move || {
        if let Some(f) = f.upgrade() {
            f.close();
        }
    });
    let modal = open_modal(ctx, owner, &dialog);
    *form.modal.borrow_mut() = Some(modal.clone());
    // The open modal keeps the form alive until the dialog closes.
    let keep = form.clone();
    modal.on_closed(move || drop(keep));
    track(&modal);
    LAST_PRODUCT.with(|d| *d.borrow_mut() = Some(dialog.as_weak()));
    Some((dialog, modal))
}

// -- owners ----------------------------------------------------------------

struct OwnerForm {
    ctx: Weak<UiContext>,
    dialog: slint::Weak<OwnerDialog>,
    kind: OwnerKind,
    editing: Cell<Option<OwnerId>>,
    on_saved: Rc<dyn Fn(i64)>,
    show_error: ShowError,
    modal: RefCell<Option<Modal>>,
}

const OWNER_FIELDS: &[&str] = &["name", "description", "nipt", "address", "account_code"];

/// "Klient i ri" / "Furnitor i ri".
pub fn new_owner_caption(kind: OwnerKind) -> &'static str {
    match kind {
        OwnerKind::Client => texts::NEW_CLIENT,
        OwnerKind::Supplier => texts::NEW_SUPPLIER,
    }
}

/// "Klienti: {emri}" / "Furnitori: {emri}".
pub fn owner_caption(kind: OwnerKind, name: &str) -> String {
    match kind {
        OwnerKind::Client => texts::client_caption(name),
        OwnerKind::Supplier => texts::supplier_caption(name),
    }
}

impl OwnerForm {
    fn fill(d: &OwnerDialog, owner: &Owner) {
        let opt = |v: &Option<String>| SharedString::from(v.as_deref().unwrap_or(""));
        d.set_caption(owner_caption(owner.kind, &owner.name).into());
        d.set_name(owner.name.as_str().into());
        d.set_description(opt(&owner.description));
        d.set_nipt(opt(&owner.nipt));
        d.set_address(opt(&owner.address));
        d.set_account(opt(&owner.account_code));
    }

    fn close(&self) {
        let modal = self.modal.borrow().clone();
        if let Some(modal) = modal {
            modal.close();
        }
    }

    /// Fshi (a saved client or supplier only).
    fn delete(&self) {
        let (Some(ctx), Some(d), Some(id)) = (
            self.ctx.upgrade(),
            self.dialog.upgrade(),
            self.editing.get(),
        ) else {
            return;
        };
        let kind = match self.kind {
            OwnerKind::Client => ListKind::Clients,
            OwnerKind::Supplier => ListKind::Suppliers,
        };
        let modal = self.modal.borrow().clone();
        delete_from_dialog(&ctx, &d, kind, id.0, modal);
    }

    /// Ruaj: saves and closes (stays open with the errors shown).
    fn save(&self) {
        let (Some(ctx), Some(d)) = (self.ctx.upgrade(), self.dialog.upgrade()) else {
            return;
        };
        // F-029: a background job (a restore) is running; the dialog stays
        // open and saves once it ends.
        if ctx.is_busy() {
            tracing::warn!("save ignored: a background job is running");
            return;
        }
        for set in [
            OwnerDialog::set_name_error,
            OwnerDialog::set_description_error,
            OwnerDialog::set_nipt_error,
            OwnerDialog::set_address_error,
            OwnerDialog::set_account_error,
        ] {
            set(&d, SharedString::new());
        }
        let input = OwnerInput {
            kind: self.kind,
            name: d.get_name().to_string(),
            description: d.get_description().to_string(),
            nipt: d.get_nipt().to_string(),
            address: d.get_address().to_string(),
            account_code: d.get_account().to_string(),
        };
        let saved = match self.editing.get() {
            Some(id) => ctx.crm.update_owner(id, input),
            None => ctx.crm.create_owner(input),
        };
        match saved {
            Ok(owner) => {
                self.editing.set(Some(owner.id));
                Self::fill(&d, &owner);
                (self.on_saved)(owner.id.0);
                self.close();
            }
            Err(ServiceError::Validation(errors)) => {
                d.set_name_error(field_text(&errors, "name"));
                d.set_description_error(field_text(&errors, "description"));
                d.set_nipt_error(field_text(&errors, "nipt"));
                d.set_address_error(field_text(&errors, "address"));
                d.set_account_error(field_text(&errors, "account_code"));
                let other = other_text(&errors, OWNER_FIELDS);
                if !other.is_empty() {
                    (self.show_error)(&other);
                }
            }
            Err(e) => report(&e, &self.show_error),
        }
    }
}

/// Opens the client/supplier dialog over `owner` (`None` = create mode; else
/// edit mode with Fshi).
pub fn open_owner<O: ModalOwner>(
    ctx: &Rc<UiContext>,
    owner: &O,
    kind: OwnerKind,
    id: Option<OwnerId>,
    on_saved: Rc<dyn Fn(i64)>,
) -> Option<(OwnerDialog, Modal)> {
    let show_error = error_shower(ctx, owner);
    let loaded = match id.map(|id| ctx.crm.get_owner(id)).transpose() {
        Ok(o) => o,
        Err(e) => {
            report(&e, &show_error);
            return None;
        }
    };
    let dialog = match OwnerDialog::new() {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("owner dialog: {e}");
            return None;
        }
    };
    match &loaded {
        Some(o) => OwnerForm::fill(&dialog, o),
        None => dialog.set_caption(new_owner_caption(kind).into()),
    }
    dialog.set_editing(loaded.is_some());
    let form = Rc::new(OwnerForm {
        ctx: Rc::downgrade(ctx),
        dialog: dialog.as_weak(),
        kind,
        editing: Cell::new(id),
        on_saved,
        show_error,
        modal: RefCell::new(None),
    });
    let f = Rc::downgrade(&form);
    dialog.on_ok(move || {
        if let Some(f) = f.upgrade() {
            f.save();
        }
    });
    let f = Rc::downgrade(&form);
    dialog.on_delete(move || {
        if let Some(f) = f.upgrade() {
            f.delete();
        }
    });
    let f = Rc::downgrade(&form);
    dialog.on_cancel(move || {
        if let Some(f) = f.upgrade() {
            f.close();
        }
    });
    let modal = open_modal(ctx, owner, &dialog);
    *form.modal.borrow_mut() = Some(modal.clone());
    // The open modal keeps the form alive until the dialog closes.
    let keep = form.clone();
    modal.on_closed(move || drop(keep));
    track(&modal);
    LAST_OWNER.with(|d| *d.borrow_mut() = Some(dialog.as_weak()));
    Some((dialog, modal))
}

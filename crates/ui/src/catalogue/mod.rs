//! Catalogue windows and dialogs (task 009): the Artikujt, Klientët and
//! Furnitorët list windows, the product and owner property dialogs, and the
//! import and export flows.
//!
//! Contract (008): `CatalogueBridge.open-list(kind)`, `import(kind)` and
//! `export(kind)` (0 = Artikujt, 1 = Klientët, 2 = Furnitorët) are called by
//! the main menu and toolbar; [`install`] registers the product editor used by
//! the order grid (task 010) through [`UiContext::set_product_editor`].
//!
//! Layout: [`list`] is the controller shared by the three windows, [`models`]
//! builds their rows, [`products`] and [`owners`] hold the per-kind parts,
//! [`dialogs`] the property dialogs, [`transfer`] import and export, [`texts`]
//! the Albanian strings.

pub mod dialogs;
pub mod list;
pub mod models;
pub mod owners;
pub mod products;
pub mod texts;
pub mod transfer;

use crate::{CatalogueBridge, ClientsWindow, ProductsWindow, SuppliersWindow, UiContext};
use crm_services::domain::OwnerKind;
use crm_services::{ImportKind, ServiceError};
use list::{ListController, ListWindow};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

/// The three catalogue lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ListKind {
    /// "Artikujt".
    Products,
    /// "Klientët".
    Clients,
    /// "Furnitorët".
    Suppliers,
}

impl ListKind {
    /// The bridge index (0, 1, 2).
    pub fn from_index(index: i32) -> Option<ListKind> {
        match index {
            0 => Some(ListKind::Products),
            1 => Some(ListKind::Clients),
            2 => Some(ListKind::Suppliers),
            _ => None,
        }
    }

    /// The bridge index.
    pub fn index(self) -> i32 {
        match self {
            ListKind::Products => 0,
            ListKind::Clients => 1,
            ListKind::Suppliers => 2,
        }
    }

    /// The owner kind of the Klientët / Furnitorët lists.
    pub fn owner_kind(self) -> Option<OwnerKind> {
        match self {
            ListKind::Products => None,
            ListKind::Clients => Some(OwnerKind::Client),
            ListKind::Suppliers => Some(OwnerKind::Supplier),
        }
    }

    /// The import/export file kind.
    pub fn import_kind(self) -> ImportKind {
        match self {
            ListKind::Products => ImportKind::Products,
            ListKind::Clients => ImportKind::Clients,
            ListKind::Suppliers => ImportKind::Suppliers,
        }
    }

    /// The status line: "{n} artikuj", "{n} klientë", "{n} furnitorë".
    pub fn count_text(self, n: usize) -> String {
        match self {
            ListKind::Products => texts::products_count(n),
            ListKind::Clients => texts::clients_count(n),
            ListKind::Suppliers => texts::suppliers_count(n),
        }
    }
}

/// The list windows of one [`UiContext`], created on first open and reused.
#[derive(Default)]
struct Lists {
    products: RefCell<Option<Rc<ListController<ProductsWindow>>>>,
    clients: RefCell<Option<Rc<ListController<ClientsWindow>>>>,
    suppliers: RefCell<Option<Rc<ListController<SuppliersWindow>>>>,
}

thread_local! {
    /// The lists per context (by address). Strong references live in the
    /// main window's `CatalogueBridge` handlers, so they go with the window.
    static LISTS: RefCell<Vec<(usize, Weak<Lists>)>> = const { RefCell::new(Vec::new()) };
}

fn key(ctx: &UiContext) -> usize {
    std::ptr::from_ref(ctx) as usize
}

fn lists(ctx: &UiContext) -> Option<Rc<Lists>> {
    LISTS.with(|l| {
        let mut l = l.borrow_mut();
        l.retain(|(_, w)| w.strong_count() > 0);
        l.iter()
            .find(|(k, _)| *k == key(ctx))
            .and_then(|(_, w)| w.upgrade())
    })
}

fn open_in<W: ListWindow>(
    ctx: &Rc<UiContext>,
    slot: &RefCell<Option<Rc<ListController<W>>>>,
) -> Option<Rc<ListController<W>>> {
    let existing = slot.borrow().clone();
    let ctrl = match existing {
        Some(c) => c,
        None => {
            let c = ListController::<W>::new(ctx)?;
            *slot.borrow_mut() = Some(c.clone());
            c
        }
    };
    ctrl.open();
    Some(ctrl)
}

/// Opens (or focuses) the list window of `kind`.
pub fn open_list(ctx: &Rc<UiContext>, kind: ListKind) {
    let Some(lists) = lists(ctx) else { return };
    match kind {
        ListKind::Products => {
            open_in(ctx, &lists.products);
        }
        ListKind::Clients => {
            open_in(ctx, &lists.clients);
        }
        ListKind::Suppliers => {
            open_in(ctx, &lists.suppliers);
        }
    }
}

/// Opens the list window of `kind` and its empty new-item dialog, as Shto / Ins
/// does there (Ctrl+R / Ctrl+K / Ctrl+F, task 018).
pub fn create(ctx: &Rc<UiContext>, kind: ListKind) {
    let Some(lists) = lists(ctx) else { return };
    match kind {
        ListKind::Products => {
            if let Some(c) = open_in(ctx, &lists.products) {
                c.add();
            }
        }
        ListKind::Clients => {
            if let Some(c) = open_in(ctx, &lists.clients) {
                c.add();
            }
        }
        ListKind::Suppliers => {
            if let Some(c) = open_in(ctx, &lists.suppliers) {
                c.add();
            }
        }
    }
}

/// Reloads the list window of `kind` when it is shown (after an import or an
/// edit made elsewhere).
pub fn reload_list(ctx: &Rc<UiContext>, kind: ListKind) {
    fn reload<W: ListWindow>(slot: &RefCell<Option<Rc<ListController<W>>>>) {
        let ctrl = slot.borrow().clone();
        if let Some(c) = ctrl
            && c.window().window().is_visible()
        {
            c.reload();
        }
    }
    let Some(lists) = lists(ctx) else { return };
    match kind {
        ListKind::Products => reload(&lists.products),
        ListKind::Clients => reload(&lists.clients),
        ListKind::Suppliers => reload(&lists.suppliers),
    }
}

/// The Artikujt controller, once opened.
pub fn products_list(ctx: &UiContext) -> Option<Rc<ListController<ProductsWindow>>> {
    lists(ctx)?.products.borrow().clone()
}

/// The Klientët controller, once opened.
pub fn clients_list(ctx: &UiContext) -> Option<Rc<ListController<ClientsWindow>>> {
    lists(ctx)?.clients.borrow().clone()
}

/// The Furnitorët controller, once opened.
pub fn suppliers_list(ctx: &UiContext) -> Option<Rc<ListController<SuppliersWindow>>> {
    lists(ctx)?.suppliers.borrow().clone()
}

/// Installs the catalogue handlers on the main window's `CatalogueBridge` and
/// registers the product editor.
pub fn install(ctx: &Rc<UiContext>) {
    let Some(app) = ctx.app() else { return };
    let state = Rc::new(Lists::default());
    LISTS.with(|l| l.borrow_mut().push((key(ctx), Rc::downgrade(&state))));
    let bridge = app.global::<CatalogueBridge>();

    let on = |f: fn(&Rc<UiContext>, ListKind)| {
        let weak = Rc::downgrade(ctx);
        let state = state.clone();
        move |index: i32| {
            let _keep = &state;
            if let (Some(ctx), Some(kind)) = (weak.upgrade(), ListKind::from_index(index)) {
                f(&ctx, kind);
            }
        }
    };
    bridge.on_open_list(on(open_list));
    bridge.on_create(on(create));
    bridge.on_import(on(|ctx, kind| {
        if let Some(app) = ctx.app() {
            transfer::open_import(ctx, &app, kind);
        }
    }));
    bridge.on_export(on(|ctx, kind| {
        if let Some(app) = ctx.app() {
            transfer::export(ctx, &app, kind);
        }
    }));

    // F-016: no open dialog may keep acting on pre-restore row ids.
    ctx.on_restored(dialogs::close_after_restore);

    ctx.set_product_editor(Box::new(|ctx, id, on_saved| {
        if let Err(ServiceError::NotFound) = ctx.crm.get_product(id) {
            // Deleted meanwhile: nothing to edit. (PRODUCT_EDITOR_MISSING is
            // only for "no editor registered".)
            ctx.flash(texts::PRODUCT_DELETED);
            return;
        }
        let Some(app) = ctx.app() else { return };
        let on_saved: Rc<dyn Fn()> = Rc::from(on_saved);
        let weak = Rc::downgrade(ctx);
        dialogs::open_product(
            ctx,
            &app,
            Some(id),
            Rc::new(move |_| {
                on_saved();
                // F-001: own writes are not reported by the poll; this also
                // reloads a shown Artikujt list.
                if let Some(ctx) = weak.upgrade() {
                    ctx.notify_data_changed();
                }
            }),
        );
    }));
}

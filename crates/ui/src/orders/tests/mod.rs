//! Headless tests of the orders workspace (i-slint-backend-testing): the line
//! grid keyboard table (AC 13/15), documents (save, edit, delete, restore) and
//! the list filters.

mod calendar;
mod documents;
mod header;
mod keyboard;
mod list;
mod shortcuts;

use super::Ws;
use super::grid_nav::Pos;
use crate::{AppWindow, OrdersBridge, ShellBridge, UiContext};
use crm_services::domain::{Money, OwnerInput, OwnerKind, ProductInput, ProductRow, Qty};
use crm_services::{Crm, DataPaths};
use i_slint_backend_testing::ElementHandle;
use slint::platform::{Key, WindowEvent};
use slint::{ComponentHandle, Model, SharedString};
use std::rc::Rc;

pub(super) struct Harness {
    pub app: AppWindow,
    pub ctx: Rc<UiContext>,
    pub ws: Ws,
    pub dir: Rc<tempfile::TempDir>,
}

/// A fresh database (unlocked).
pub(super) fn harness() -> Harness {
    harness_in(Rc::new(tempfile::tempdir().unwrap()))
}

/// The app over the database in `dir` (unlocked).
pub(super) fn harness_in(dir: Rc<tempfile::TempDir>) -> Harness {
    thread_local! {
        static INIT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    if !INIT.with(|i| i.replace(true)) {
        i_slint_backend_testing::init_no_event_loop();
    }
    let paths = DataPaths::at(dir.path());
    paths.ensure_dirs().unwrap();
    let crm = Crm::open(paths.clone(), &crm_services::DbKey::for_tests(1)).unwrap();
    let (app, ctx) = crate::build(
        crm,
        crm_services::Vault::with_params(paths.clone(), crm_services::KdfParams::TEST),
        paths,
    )
    .unwrap();
    app.show().unwrap();
    ctx.unlock();
    let ws = super::last_workspace().expect("orders installed");
    let h = Harness { app, ctx, ws, dir };
    h.pump();
    h
}

impl Harness {
    /// Instantiates the element tree (no renderer runs headless, so `if`/`for`
    /// elements appear on the first walk) and runs the pending change handlers
    /// (focus requests, has-focus changes).
    pub fn pump(&self) {
        for _ in 0..3 {
            let _ = ElementHandle::find_by_element_id(&self.app, "GridCell::edit").count();
            slint::platform::update_timers_and_animations();
        }
    }

    pub fn bridge(&self) -> OrdersBridge<'_> {
        self.app.global::<OrdersBridge>()
    }

    /// Exit as the shell does: flush the drafts, close the database.
    pub fn close(self) -> Rc<tempfile::TempDir> {
        self.bridge().invoke_flush_drafts();
        self.ctx.crm.close().unwrap();
        let dir = self.dir.clone();
        drop(self);
        dir
    }

    pub fn product(&self, name: &str, stock: i64, sell: i64) -> ProductRow {
        self.ctx
            .crm
            .create_product(ProductInput {
                code: String::new(),
                name: name.into(),
                unit: "SHISHE".into(),
                buy_price: Money(sell * 80),
                sell_price: Money(sell * 100),
                quantity: Qty(stock),
            })
            .unwrap()
    }

    pub fn owner(&self, kind: OwnerKind, name: &str) -> crm_services::domain::Owner {
        self.ctx
            .crm
            .create_owner(OwnerInput {
                kind,
                name: name.into(),
                description: "Tiranë".into(),
                nipt: "M12345678A".into(),
                address: "Rruga e Pishave".into(),
                account_code: String::new(),
            })
            .unwrap()
    }

    /// The focused grid cell, as the grid reports it.
    pub fn focused(&self) -> (i32, i32) {
        (
            self.bridge().get_focused_row(),
            self.bridge().get_focused_col(),
        )
    }

    pub fn focus(&self, row: usize, col: usize) {
        self.ws.borrow_mut().focus_cell(Pos::new(row, col));
        self.pump();
    }

    pub fn rows(&self) -> usize {
        self.bridge().get_grid_rows().row_count()
    }

    pub fn row(&self, i: usize) -> crate::GridRow {
        self.bridge().get_grid_rows().row_data(i).unwrap()
    }

    fn send(&self, text: impl Into<SharedString>, pressed: bool) {
        let text = text.into();
        self.app.window().dispatch_event(if pressed {
            WindowEvent::KeyPressed { text }
        } else {
            WindowEvent::KeyReleased { text }
        });
    }

    /// Presses and releases `key` (with Shift / Ctrl held), then runs the
    /// change handlers.
    pub fn press(&self, key: impl Into<SharedString>, shift: bool, ctrl: bool) {
        let key = key.into();
        if ctrl {
            self.send(Key::Control, true);
        }
        if shift {
            self.send(Key::Shift, true);
        }
        self.send(key.clone(), true);
        self.send(key, false);
        if shift {
            self.send(Key::Shift, false);
        }
        if ctrl {
            self.send(Key::Control, false);
        }
        self.pump();
    }

    pub fn tap(&self, key: Key) {
        self.press(key, false, false);
    }

    /// Types `text` into the focused field.
    pub fn type_text(&self, text: &str) {
        for c in text.chars() {
            self.press(SharedString::from(c.to_string()), false, false);
        }
    }

    /// The text shown by the LineEdit of grid cell (row, col).
    pub fn cell_text(&self, row: usize, col: usize) -> String {
        let cols = if self.bridge().get_doc_kind() == 1 {
            4
        } else {
            3
        };
        let cells: Vec<ElementHandle> =
            ElementHandle::find_by_element_id(&self.app, "GridCell::edit").collect();
        cells[row * cols + col]
            .accessible_value()
            .unwrap_or_default()
            .to_string()
    }

    pub fn flash(&self) -> String {
        self.app
            .global::<ShellBridge>()
            .get_flash_text()
            .to_string()
    }

    pub fn new_document(&self, kind: i32) {
        self.bridge().invoke_new_document(kind);
        self.pump();
    }
}

/// The last confirm dialog the workspace opened (taken).
pub(super) fn take_confirm() -> Option<crate::ConfirmDialog> {
    super::prompt::LAST_CONFIRM.with(|d| d.borrow_mut().take())
}

/// The last message dialog the workspace opened (taken).
pub(super) fn take_message() -> Option<crate::MessageDialog> {
    super::prompt::LAST_MESSAGE.with(|d| d.borrow_mut().take())
}

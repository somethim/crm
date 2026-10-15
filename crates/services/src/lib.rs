//! Application services and use cases (orders, catalogue, import, reports), and
//! the key vault of the encrypted database (password, recovery key, wipe code).
//!
//! Boundary: orchestrates `crm-domain`, `crm-storage`, `crm-search` and `crm-io`.
//! It exposes no `rusqlite` or `slint` types. Used by `crm-ui` and `apps/crm`.
//!
//! The UI holds one [`Crm`] (cheap to clone, `Send + Sync`). Every write use
//! case runs in one storage transaction and then updates the in-memory search
//! index; [`Crm::refresh_if_changed`] picks up everything else. Errors are
//! [`ServiceError`]s; show [`ServiceError::user_message`] (Albanian).
//!
//! The storage and io types that appear in this API are re-exported here, so
//! the UI needs no direct dependency on `crm-storage` or `crm-io`.

#![warn(missing_docs)]

mod backup;
mod catalogue;
mod crm;
mod error;
mod index_sync;
pub mod messages;
mod orders;
mod secure;
mod session;
mod transfer;
mod vault;

pub use backup::BackupEntry;
pub use catalogue::{DEFAULT_PAGE_SIZE, DEFAULT_SEARCH_LIMIT, DeleteOutcome, Page, ProductPatch};
pub use crm::Crm;
#[doc(hidden)]
pub use crm::Fault;
pub use error::{Result, ServiceError, VaultError};
#[doc(hidden)]
pub use secure::create_legacy_plaintext;
pub use secure::secure_storage;
pub use session::{
    IDLE_LOCK_RANGE, OpenTab, RECEIPT_FONT_RANGE, RECEIPT_WIDTH_RANGE, ReceiptSettings,
};
pub use transfer::{ImportKind, ImportReport, ReportParams, ReportPreview};
#[doc(hidden)]
pub use vault::{DataKey, KEY_FILE_LEN, KeyFile};
pub use vault::{
    Entry, KdfParams, LOCKOUT, MAX_ATTEMPTS, PASSWORD_MAX_CHARS, PASSWORD_MIN_CHARS, PendingSetup,
    RecoveryKey, RecoveryToken, StartState, Unlock, Vault, WipeCode, check_new_password, classify,
};

/// The domain crate, for the UI's convenience.
pub use crm_domain as domain;
pub use crm_io::report::{PREVIEW_LIMIT, file_name as report_file_name, order_list_file_name};
pub use crm_io::{Cell, IoError, ReportKind, ReportTable, RowError};
pub use crm_storage::repo::orders::{OrderFilter, OrderSummary};
pub use crm_storage::{BackupName, DataPaths, DbKey, InstanceLock, StorageError};

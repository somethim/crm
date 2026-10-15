//! SQLite storage layer: schema migrations, the writer thread, the reader pool,
//! backups and the single-instance lock.
//!
//! Boundary: depends on `crm-domain` only. `rusqlite` types never leave this
//! crate; the public API speaks in domain types and this crate's error type.
//! Used by `crm-services` and `apps/crm`.
//!
//! Modules:
//! - [`paths`], [`guard`]: the data layout next to the exe, the network-drive
//!   refusal and the single-instance lock;
//! - [`Db`]: pragmas, one writer thread (`BEGIN IMMEDIATE`), a reader pool,
//!   `data_version` polling, checkpoint on close;
//! - [`migrations`]: schema v1;
//! - [`repo`]: repositories over [`Conn`] (reads) and [`Tx`] (writes);
//! - [`backup`]: `VACUUM INTO` backups, rotation, restore, sibling `.key` files;
//! - [`DbKey`]: the SQLite3 Multiple Ciphers (ChaCha20-Poly1305) data key; the
//!   keyed opens ([`Db::open`]) and the plaintext → encrypted primitives
//!   ([`encrypt_in_place`], [`encrypt_copy`]).

#![warn(missing_docs)]

pub mod backup;
mod conn;
mod db;
mod encrypt;
mod error;
pub mod guard;
mod key;
pub mod migrations;
pub mod paths;
pub mod repo;

pub use backup::{BackupName, BackupReason, RotationPlan, plan_rotation, restore_from};
pub use conn::{BUSY_TIMEOUT, Conn, ConnPragmas, JOURNAL_SIZE_LIMIT, Tx};
pub use db::Db;
pub use encrypt::{
    CopyStats, FileKind, encrypt_copy, encrypt_in_place, file_kind, overwrite_and_delete,
};
#[doc(hidden)]
pub use encrypt::{Stage, database_stats, encrypt_in_place_until};
pub use error::{Result, StorageError};
pub use guard::{InstanceLock, check_local_drive, is_network_path};
pub use key::DbKey;
pub use paths::DataPaths;

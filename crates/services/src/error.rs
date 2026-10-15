//! The services error type. Every variant has an Albanian [`ServiceError::user_message`].

use crate::messages;
use crm_domain::validate::FieldError;
use crm_domain::{StockError, ValidationErrors};
use crm_io::IoError;
use crm_storage::StorageError;

/// Result alias of this crate.
pub type Result<T, E = ServiceError> = std::result::Result<T, E>;

/// A password, key-file or vault problem (plans 013 and 015).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VaultError {
    /// A new password shorter than 6 characters.
    #[error("password too short")]
    TooShort,
    /// A new password shaped like a recovery key or a wipe code (or too long).
    #[error("password not allowed")]
    PasswordNotAllowed,
    /// The two new-password fields differ (checked by the UI).
    #[error("passwords differ")]
    Mismatch,
    /// The current password was wrong (password change).
    #[error("wrong password")]
    Wrong,
    /// Five failures in a row: wait this many seconds.
    #[error("locked for {0} s")]
    LockedFor(u64),
    /// The key file is malformed, of an unknown version, or has out-of-range parameters.
    #[error("key file invalid")]
    KeyFileInvalid,
    /// No usable key file.
    #[error("key file missing")]
    KeyMissing,
    /// Reading or writing a key file failed.
    #[error("key file io: {0}")]
    Io(String),
    /// Randomness, Argon2 or the AEAD failed.
    #[error("crypto failure")]
    Crypto,
}

impl VaultError {
    /// The Albanian message.
    pub fn user_message(&self) -> String {
        match self {
            VaultError::TooShort => messages::PASSWORD_TOO_SHORT.to_owned(),
            VaultError::PasswordNotAllowed => messages::PASSWORD_NOT_ALLOWED.to_owned(),
            VaultError::Mismatch => messages::PASSWORD_MISMATCH.to_owned(),
            VaultError::Wrong => messages::PASSWORD_WRONG.to_owned(),
            VaultError::LockedFor(secs) => messages::locked_for(*secs),
            VaultError::KeyFileInvalid | VaultError::KeyMissing => messages::KEY_MISSING.to_owned(),
            VaultError::Io(_) => messages::KEY_FILE_IO.to_owned(),
            VaultError::Crypto => messages::CRYPTO_FAILED.to_owned(),
        }
    }
}

/// Everything a use case can fail with. `Display` is technical (for the log);
/// show [`ServiceError::user_message`] to the user.
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    /// Invalid input; field keys as in `crm_domain::validate` (plus `code` for a
    /// taken SKU code and `value` for out-of-range settings).
    #[error("validation: {0}")]
    Validation(ValidationErrors),
    /// The save would make some stock negative (sale shortage or Q6).
    #[error("stock: {0}")]
    Stock(StockError),
    /// A database failure.
    #[error("storage: {0}")]
    Storage(StorageError),
    /// A file import/export failure.
    #[error("io: {0}")]
    Io(IoError),
    /// A password or key-file problem.
    #[error("vault: {0}")]
    Vault(VaultError),
    /// The requested row does not exist.
    #[error("not found")]
    NotFound,
    /// The database is closed (after [`Crm::close`](crate::Crm::close) or a failed restore).
    #[error("database closed")]
    Closed,
}

impl ServiceError {
    /// The Albanian message shown to the user.
    pub fn user_message(&self) -> String {
        match self {
            ServiceError::Validation(e) => e.user_message(),
            ServiceError::Stock(e) => e.user_message(),
            ServiceError::Storage(e) => e.user_message(),
            ServiceError::Io(e) => e.user_message(),
            ServiceError::Vault(e) => e.user_message(),
            ServiceError::NotFound => StorageError::NotFound.user_message(),
            ServiceError::Closed => messages::DB_CLOSED.to_owned(),
        }
    }

    /// A one-field validation error.
    pub(crate) fn field(field: &'static str, message: impl Into<String>) -> Self {
        ServiceError::Validation(ValidationErrors(vec![FieldError {
            field,
            line: None,
            message: message.into(),
        }]))
    }

    /// A validation error on order line `line` (0-based index into the draft).
    pub(crate) fn line(line: usize, field: &'static str, message: impl Into<String>) -> Self {
        ServiceError::Validation(ValidationErrors(vec![FieldError {
            field,
            line: Some(line),
            message: message.into(),
        }]))
    }
}

impl From<StorageError> for ServiceError {
    fn from(e: StorageError) -> Self {
        match e {
            StorageError::NotFound => ServiceError::NotFound,
            other => ServiceError::Storage(other),
        }
    }
}

impl From<ValidationErrors> for ServiceError {
    fn from(e: ValidationErrors) -> Self {
        ServiceError::Validation(e)
    }
}

impl From<StockError> for ServiceError {
    fn from(e: StockError) -> Self {
        ServiceError::Stock(e)
    }
}

impl From<IoError> for ServiceError {
    fn from(e: IoError) -> Self {
        ServiceError::Io(e)
    }
}

impl From<VaultError> for ServiceError {
    fn from(e: VaultError) -> Self {
        ServiceError::Vault(e)
    }
}

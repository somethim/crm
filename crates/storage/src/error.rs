//! The storage error type and its mapping from SQLite failures.

use rusqlite::ErrorCode;

/// Result alias used across the crate.
pub type Result<T, E = StorageError> = std::result::Result<T, E>;

/// Everything that can go wrong on disk. `Display` is technical (for logs);
/// [`StorageError::user_message`] is the Albanian text shown to the user.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StorageError {
    /// The database path is on a network share or a mapped network drive.
    #[error("database path is on a network drive")]
    NetworkDrive,
    /// The program folder cannot be written to.
    #[error("the data location is read-only")]
    ReadOnlyLocation,
    /// Another instance holds `data/crm.lock`.
    #[error("another instance is already running")]
    AlreadyRunning,
    /// The database stayed locked longer than `busy_timeout`.
    #[error("database is busy")]
    Busy,
    /// `quick_check` failed or SQLite reported corruption.
    #[error("database is corrupt: {details}")]
    Corrupt {
        /// What SQLite reported.
        details: String,
    },
    /// The key does not open the database: another key, or not a database at all
    /// (`SQLITE_NOTADB` on a keyed open).
    #[error("wrong key or not a database")]
    WrongKey,
    /// The storage device went away or cannot be opened (`SQLITE_IOERR*`, `SQLITE_CANTOPEN`).
    #[error("storage device unavailable: {0}")]
    DeviceUnavailable(String),
    /// The disk is full (`SQLITE_FULL`, `ErrorKind::StorageFull`).
    #[error("disk is full: {0}")]
    DiskFull(String),
    /// A UNIQUE, CHECK or FOREIGN KEY constraint rejected the change.
    #[error("constraint failed: {what}")]
    Constraint {
        /// The constraint SQLite named.
        what: String,
    },
    /// The requested row does not exist.
    #[error("not found")]
    NotFound,
    /// Anything else.
    #[error("{0}")]
    Other(String),
}

impl StorageError {
    /// The Albanian message for the user.
    pub fn user_message(&self) -> String {
        match self {
            StorageError::NetworkDrive => {
                "Baza e të dhënave nuk mund të hapet nga një disk rrjeti. Kopjoni programin në një disk lokal ose USB."
            }
            StorageError::ReadOnlyLocation => {
                "Programi nuk mund të shkruajë në dosjen ku ndodhet. Kopjoni programin në një disk ku lejohet shkrimi."
            }
            StorageError::AlreadyRunning => "Programi është tashmë i hapur.",
            StorageError::Busy => "Baza e të dhënave është e zënë. Provoni përsëri pas pak.",
            StorageError::Corrupt { .. } => {
                "Të dhënat janë ndryshuar ose janë dëmtuar. Riktheni të dhënat nga një rezervim."
            }
            StorageError::WrongKey => {
                "Të dhënat nuk mund të hapen me këtë çelës. Skedari mund të jetë zëvendësuar ose dëmtuar. Riktheni të dhënat nga një rezervim."
            }
            StorageError::DeviceUnavailable(_) => {
                "Pajisja e ruajtjes nuk është e disponueshme. Kontrolloni USB-në dhe provoni përsëri."
            }
            StorageError::DiskFull(_) => {
                "Disku është plot. Lironi hapësirë në USB (p.sh. fshini kopje të vjetra në dosjen backups) dhe provoni përsëri."
            }
            StorageError::Constraint { .. } => {
                "Ndryshimi nuk u ruajt sepse bie ndesh me të dhënat ekzistuese."
            }
            StorageError::NotFound => "Të dhënat e kërkuara nuk u gjetën.",
            StorageError::Other(_) => "Ndodhi një gabim gjatë punës me bazën e të dhënave.",
        }
        .to_owned()
    }

    pub(crate) fn other(message: impl Into<String>) -> Self {
        StorageError::Other(message.into())
    }
}

impl From<rusqlite::Error> for StorageError {
    fn from(err: rusqlite::Error) -> Self {
        match &err {
            rusqlite::Error::QueryReturnedNoRows => StorageError::NotFound,
            rusqlite::Error::SqliteFailure(code, message) => {
                let text = message.clone().unwrap_or_else(|| code.to_string());
                match code.code {
                    ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked => StorageError::Busy,
                    ErrorCode::DiskFull => StorageError::DiskFull(text),
                    ErrorCode::SystemIoFailure | ErrorCode::CannotOpen => {
                        StorageError::DeviceUnavailable(text)
                    }
                    ErrorCode::ConstraintViolation => StorageError::Constraint { what: text },
                    ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase => {
                        StorageError::Corrupt { details: text }
                    }
                    _ => StorageError::Other(err.to_string()),
                }
            }
            rusqlite::Error::ToSqlConversionFailure(inner) => {
                match inner.downcast_ref::<StorageError>() {
                    Some(storage) => storage.clone(),
                    None => StorageError::Other(err.to_string()),
                }
            }
            _ => StorageError::Other(err.to_string()),
        }
    }
}

impl From<rusqlite_migration::Error> for StorageError {
    fn from(err: rusqlite_migration::Error) -> Self {
        match err {
            rusqlite_migration::Error::RusqliteError { err, .. } => err.into(),
            other => StorageError::Other(format!("migration: {other}")),
        }
    }
}

impl From<r2d2::Error> for StorageError {
    fn from(err: r2d2::Error) -> Self {
        StorageError::Other(format!("reader pool: {err}"))
    }
}

impl From<std::io::Error> for StorageError {
    fn from(err: std::io::Error) -> Self {
        match err.kind() {
            std::io::ErrorKind::StorageFull => StorageError::DiskFull(format!("io: {err}")),
            _ => StorageError::Other(format!("io: {err}")),
        }
    }
}

/// Wraps a [`StorageError`] so it can travel through a rusqlite callback
/// (r2d2 `with_init`) and come back out intact.
pub(crate) fn to_sqlite(err: StorageError) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(err))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failure(code: i32) -> rusqlite::Error {
        rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(code), Some("x".into()))
    }

    #[test]
    fn sqlite_codes_map_to_variants() {
        use rusqlite::ffi;
        assert_eq!(
            StorageError::from(failure(ffi::SQLITE_BUSY)),
            StorageError::Busy
        );
        assert_eq!(
            StorageError::from(failure(ffi::SQLITE_LOCKED)),
            StorageError::Busy
        );
        assert!(matches!(
            StorageError::from(failure(ffi::SQLITE_IOERR_WRITE)),
            StorageError::DeviceUnavailable(_)
        ));
        assert!(matches!(
            StorageError::from(failure(ffi::SQLITE_CANTOPEN)),
            StorageError::DeviceUnavailable(_)
        ));
        assert!(matches!(
            StorageError::from(failure(ffi::SQLITE_FULL)),
            StorageError::DiskFull(_)
        ));
        assert!(matches!(
            StorageError::from(failure(ffi::SQLITE_CONSTRAINT_CHECK)),
            StorageError::Constraint { .. }
        ));
        assert!(matches!(
            StorageError::from(failure(ffi::SQLITE_CORRUPT)),
            StorageError::Corrupt { .. }
        ));
        assert!(matches!(
            StorageError::from(failure(ffi::SQLITE_NOTADB)),
            StorageError::Corrupt { .. }
        ));
        assert_eq!(
            StorageError::from(rusqlite::Error::QueryReturnedNoRows),
            StorageError::NotFound
        );
        assert_eq!(
            StorageError::from(to_sqlite(StorageError::NetworkDrive)),
            StorageError::NetworkDrive
        );
    }

    #[test]
    fn wrong_key_and_corrupt_have_their_own_texts() {
        assert_eq!(
            StorageError::Corrupt {
                details: "x".into()
            }
            .user_message(),
            "Të dhënat janë ndryshuar ose janë dëmtuar. Riktheni të dhënat nga një rezervim."
        );
        assert_eq!(
            StorageError::WrongKey.user_message(),
            "Të dhënat nuk mund të hapen me këtë çelës. Skedari mund të jetë zëvendësuar ose dëmtuar. Riktheni të dhënat nga një rezervim."
        );
        assert_eq!(
            StorageError::WrongKey.to_string(),
            "wrong key or not a database"
        );
    }

    #[test]
    fn io_errors_map_disk_full_and_nothing_else() {
        let full = std::io::Error::from(std::io::ErrorKind::StorageFull);
        assert!(matches!(
            StorageError::from(full),
            StorageError::DiskFull(_)
        ));
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert!(matches!(StorageError::from(denied), StorageError::Other(_)));
    }

    #[test]
    fn disk_full_tells_the_user_to_free_space() {
        assert_eq!(
            StorageError::DiskFull("x".into()).user_message(),
            "Disku është plot. Lironi hapësirë në USB (p.sh. fshini kopje të vjetra në dosjen backups) dhe provoni përsëri."
        );
        assert_ne!(
            StorageError::DiskFull("x".into()).user_message(),
            StorageError::DeviceUnavailable("x".into()).user_message()
        );
    }

    #[test]
    fn user_messages_are_albanian() {
        assert_eq!(
            StorageError::NetworkDrive.user_message(),
            "Baza e të dhënave nuk mund të hapet nga një disk rrjeti. Kopjoni programin në një disk lokal ose USB."
        );
        assert_eq!(
            StorageError::DeviceUnavailable("x".into()).user_message(),
            "Pajisja e ruajtjes nuk është e disponueshme. Kontrolloni USB-në dhe provoni përsëri."
        );
        assert_eq!(
            StorageError::AlreadyRunning.user_message(),
            "Programi është tashmë i hapur."
        );
    }
}

//! The crate error type, with Albanian user messages.

use std::path::PathBuf;

/// A file-level import or export failure (row-level problems are [`crate::RowError`]s).
///
/// `Display` is the Albanian [`IoError::user_message`], as for the domain errors;
/// the underlying `reason` is kept for the log.
#[derive(Debug, thiserror::Error)]
#[error("{}", self.user_message())]
pub enum IoError {
    /// The file extension is not `.csv`, `.txt`, `.xlsx` or `.xls`.
    UnsupportedFile(PathBuf),
    /// The file could not be opened or parsed.
    Unreadable {
        /// The file.
        path: PathBuf,
        /// The underlying error, for the log.
        reason: String,
    },
    /// The import file is over the size or row limit (it is refused before or
    /// instead of being read whole).
    TooLarge {
        /// The file.
        path: PathBuf,
        /// What was exceeded, for the log (e.g. `60000000 bytes > 52428800`).
        reason: String,
    },
    /// Required columns are missing; holds their Albanian labels.
    MissingColumns(Vec<&'static str>),
    /// The export file could not be written.
    Unwritable {
        /// The file.
        path: PathBuf,
        /// The underlying error, for the log.
        reason: String,
    },
}

impl IoError {
    pub(crate) fn unreadable(path: impl Into<PathBuf>, reason: impl ToString) -> Self {
        Self::Unreadable {
            path: path.into(),
            reason: reason.to_string(),
        }
    }

    pub(crate) fn unwritable(path: impl Into<PathBuf>, reason: impl ToString) -> Self {
        Self::Unwritable {
            path: path.into(),
            reason: reason.to_string(),
        }
    }

    /// The Albanian message shown to the user.
    pub fn user_message(&self) -> String {
        match self {
            Self::UnsupportedFile(_) => {
                "Lloji i skedarit nuk mbështetet. Zgjidhni një skedar CSV ose Excel (.xlsx, .xls)."
                    .to_owned()
            }
            Self::Unreadable { path, .. } => {
                format!("Skedari «{}» nuk mund të lexohet.", file_name(path))
            }
            Self::TooLarge { path, .. } => format!(
                "Skedari «{}» është shumë i madh për t'u importuar. Kontrolloni që keni zgjedhur skedarin e duhur.",
                file_name(path)
            ),
            Self::MissingColumns(labels) => {
                format!("Skedari nuk ka kolonat e nevojshme: {}", labels.join(", "))
            }
            Self::Unwritable { path, .. } => {
                format!("Skedari «{}» nuk mund të ruhet.", file_name(path))
            }
        }
    }
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_messages() {
        assert_eq!(
            IoError::MissingColumns(vec!["Kodi", "Përshkrimi"]).user_message(),
            "Skedari nuk ka kolonat e nevojshme: Kodi, Përshkrimi"
        );
        assert_eq!(
            IoError::unreadable("/x/ARTIKUJ.csv", "boom").user_message(),
            "Skedari «ARTIKUJ.csv» nuk mund të lexohet."
        );
        assert!(
            IoError::UnsupportedFile("a.pdf".into())
                .user_message()
                .contains("CSV")
        );
        assert_eq!(
            IoError::unwritable("out.csv", "denied").user_message(),
            "Skedari «out.csv» nuk mund të ruhet."
        );
        assert_eq!(
            IoError::TooLarge {
                path: "/x/big.csv".into(),
                reason: "1 > 0".into()
            }
            .user_message(),
            "Skedari «big.csv» është shumë i madh për t'u importuar. Kontrolloni që keni zgjedhur skedarin e duhur."
        );
        let e = IoError::MissingColumns(vec!["Emër"]);
        assert_eq!(e.to_string(), e.user_message());
    }
}

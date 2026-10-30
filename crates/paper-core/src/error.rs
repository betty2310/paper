/// Every error the engine can return, one variant per subsystem.
///
/// The engine returns this type from every method, and `paper-ffi` mirrors it
/// so the Swift side gets one thrown error type to switch on.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PaperError {
    /// The text is neither a DOI nor an arXiv ID.
    #[error("not a DOI or arXiv ID: {text}")]
    InvalidIdentifier { text: String },
    /// The DOI is already in the library. `item_id` is the existing item, so
    /// the app can select it instead of adding a duplicate.
    #[error("already in the library: {doi}")]
    Duplicate { doi: String, item_id: String },
    /// No row with this id, or the row is deleted.
    #[error("not found: {id}")]
    NotFound { id: String },
    /// The folder is not a library or the library folder already exists.
    #[error("{message}")]
    Library { message: String },
    #[error("database error: {message}")]
    Database { message: String },
    #[error("{message}")]
    Io { message: String },
    /// The request did not complete: offline, timeout, or a server error.
    #[error("network error: {message}")]
    Network { message: String },
    /// The lookup ran but no source knows the DOI. `item_id` is the item that
    /// was created for it; it stays in the library with a failed lookup state.
    #[error("no metadata found for {doi}")]
    MetadataNotFound { doi: String, item_id: String },
    /// The lookup failed after the item was created. The item stays in the
    /// library in the failed state so the app can offer a retry.
    #[error("could not look up {doi}: {message}")]
    Lookup {
        doi: String,
        item_id: String,
        message: String,
    },
    #[error("settings error: {message}")]
    Settings { message: String },
    /// The engine was closed; open the library again.
    #[error("the library is closed")]
    Closed,
}

impl From<rusqlite::Error> for PaperError {
    fn from(error: rusqlite::Error) -> Self {
        PaperError::Database {
            message: error.to_string(),
        }
    }
}

impl From<std::io::Error> for PaperError {
    fn from(error: std::io::Error) -> Self {
        PaperError::Io {
            message: error.to_string(),
        }
    }
}

impl From<serde_json::Error> for PaperError {
    fn from(error: serde_json::Error) -> Self {
        PaperError::Settings {
            message: error.to_string(),
        }
    }
}

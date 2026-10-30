//! Domain types, SQLite storage, the `Library` service, and settings.
//!
//! This crate depends on nothing else in the workspace and knows nothing about
//! the network or any UI toolkit. `paper-fetch` fills the types defined here,
//! and `paper-engine` wraps `Library` and `SettingsStore` into the public API.

mod error;
mod library;
mod settings;
pub mod time;
mod types;

pub use error::PaperError;
pub use library::{
    DoiMatch, LIBRARY_DB_NAME, LIBRARY_FILES_DIR, Library, RECENTLY_ADDED_DAYS, format_authors,
};
pub use settings::{MAX_RECENT_LIBRARIES, Settings, SettingsStore, WindowState};
pub use types::*;

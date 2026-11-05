//! Machine settings: `settings.json` in the platform config directory.
//!
//! These are deliberately not in the library database, so a library folder can
//! be shared between machines without sharing one machine's window size or
//! API keys.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::PaperError;

/// How many libraries `recent_libraries` remembers.
pub const MAX_RECENT_LIBRARIES: usize = 10;

const FILE_NAME: &str = "settings.json";

/// The main window frame in screen points.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Settings the user edits or the app remembers. Empty strings mean unset.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Sent in the user agent and `mailto` parameter of metadata requests.
    /// Crossref serves requests with an email from its faster polite pool.
    pub contact_email: String,
    pub openalex_api_key: String,
    /// Anthropic API key. When set, `import_pdf` sends page 1 of a PDF that
    /// does not print its identifier to a Claude model to read. Unset, PDFs
    /// never leave the machine.
    pub anthropic_api_key: String,
    pub window: Option<WindowState>,
    pub sidebar_width: Option<f64>,
    pub detail_width: Option<f64>,
}

/// What is written to disk: the settings plus the recent library list, which
/// the engine maintains itself.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
struct SettingsFile {
    #[serde(flatten)]
    settings: Settings,
    recent_libraries: Vec<String>,
}

/// Reads and writes `settings.json`. Every write goes to disk at once.
pub struct SettingsStore {
    path: PathBuf,
    file: Mutex<SettingsFile>,
}

impl SettingsStore {
    /// The platform config directory for Paper:
    /// `~/Library/Application Support/com.betty2310.paper` on macOS,
    /// `%APPDATA%\betty2310\paper\config` on Windows, `~/.config/paper` on Linux.
    pub fn default_dir() -> Result<PathBuf, PaperError> {
        directories::ProjectDirs::from("com", "betty2310", "paper")
            .map(|dirs| dirs.config_dir().to_path_buf())
            .ok_or_else(|| PaperError::Settings {
                message: "no home directory for settings".to_string(),
            })
    }

    /// Opens the store in `dir`, reading `settings.json` if it exists. A
    /// missing or unreadable file starts from defaults; the first write
    /// replaces it.
    pub fn open(dir: impl AsRef<Path>) -> SettingsStore {
        let path = dir.as_ref().join(FILE_NAME);
        let file = fs::read_to_string(&path)
            .ok()
            .and_then(|text| match serde_json::from_str::<SettingsFile>(&text) {
                Ok(file) => Some(file),
                Err(error) => {
                    tracing::warn!(path = %path.display(), %error, "ignoring unreadable settings");
                    None
                }
            })
            .unwrap_or_default();
        SettingsStore {
            path,
            file: Mutex::new(file),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn settings(&self) -> Settings {
        self.file.lock().unwrap().settings.clone()
    }

    pub fn update_settings(&self, settings: Settings) -> Result<(), PaperError> {
        let mut file = self.file.lock().unwrap();
        file.settings = settings;
        self.write(&file)
    }

    /// Most recently opened first.
    pub fn recent_libraries(&self) -> Vec<String> {
        self.file.lock().unwrap().recent_libraries.clone()
    }

    /// Moves `path` to the front of the recent list.
    pub fn remember_library(&self, path: &Path) -> Result<(), PaperError> {
        let text = path.to_string_lossy().into_owned();
        let mut file = self.file.lock().unwrap();
        file.recent_libraries.retain(|known| *known != text);
        file.recent_libraries.insert(0, text);
        file.recent_libraries.truncate(MAX_RECENT_LIBRARIES);
        self.write(&file)
    }

    /// Removes a library that no longer exists from the recent list.
    pub fn forget_library(&self, path: &Path) -> Result<(), PaperError> {
        let text = path.to_string_lossy().into_owned();
        let mut file = self.file.lock().unwrap();
        file.recent_libraries.retain(|known| *known != text);
        self.write(&file)
    }

    fn write(&self, file: &SettingsFile) -> Result<(), PaperError> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(file)?;
        // Write next to the target and rename, so a crash never leaves half a file.
        let temp = self.path.with_extension("json.tmp");
        fs::write(&temp, json)?;
        fs::rename(&temp, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_and_recent_libraries_persist_across_opens() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("nested/config");
        {
            let store = SettingsStore::open(&nested);
            assert_eq!(store.settings(), Settings::default());
            store
                .update_settings(Settings {
                    contact_email: "me@example.org".into(),
                    sidebar_width: Some(240.0),
                    ..Settings::default()
                })
                .unwrap();
            for name in ["a", "b", "a"] {
                store.remember_library(Path::new(name)).unwrap();
            }
        }
        let store = SettingsStore::open(&nested);
        assert_eq!(store.settings().contact_email, "me@example.org");
        assert_eq!(store.settings().sidebar_width, Some(240.0));
        assert_eq!(
            store.recent_libraries(),
            vec!["a", "b"],
            "most recent first, no duplicates"
        );

        for i in 0..20 {
            store
                .remember_library(Path::new(&format!("lib{i}")))
                .unwrap();
        }
        assert_eq!(store.recent_libraries().len(), MAX_RECENT_LIBRARIES);
        assert_eq!(store.recent_libraries()[0], "lib19");

        store.forget_library(Path::new("lib19")).unwrap();
        assert_eq!(store.recent_libraries()[0], "lib18");
    }

    #[test]
    fn unreadable_settings_file_starts_from_defaults() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(FILE_NAME), "{not json").unwrap();
        let store = SettingsStore::open(dir.path());
        assert_eq!(store.settings(), Settings::default());
        assert!(store.recent_libraries().is_empty());
    }

    #[test]
    fn unknown_fields_in_the_file_are_tolerated() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(FILE_NAME),
            r#"{"contact_email":"x@y.z","future_field":1,"recent_libraries":["/lib"]}"#,
        )
        .unwrap();
        let store = SettingsStore::open(dir.path());
        assert_eq!(store.settings().contact_email, "x@y.z");
        assert_eq!(store.recent_libraries(), vec!["/lib"]);
    }
}

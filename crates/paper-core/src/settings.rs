//! Machine settings: `settings.json` in the platform config directory.
//!
//! These are deliberately not in the library database, so a library folder can
//! be shared between machines without sharing one machine's window size or
//! API keys.

use std::collections::BTreeMap;
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
    /// Whether the user hid the sidebar or the detail panel, so the window
    /// opens the way it was left.
    pub sidebar_collapsed: bool,
    pub detail_collapsed: bool,
}

/// What is written to disk: the settings plus the lists the engine
/// maintains itself, the recent libraries and each library's pinned tabs.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
struct SettingsFile {
    #[serde(flatten)]
    settings: Settings,
    recent_libraries: Vec<String>,
    /// Library path to the item ids of its pinned reader tabs, in tab order.
    pinned_tabs: BTreeMap<String, Vec<String>>,
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
        let file = read(&path).unwrap_or_default();
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
        let mut file = self.fresh();
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
        let mut file = self.fresh();
        file.recent_libraries.retain(|known| *known != text);
        file.recent_libraries.insert(0, text);
        file.recent_libraries.truncate(MAX_RECENT_LIBRARIES);
        self.write(&file)
    }

    /// Removes a library that no longer exists from the recent list.
    pub fn forget_library(&self, path: &Path) -> Result<(), PaperError> {
        let text = path.to_string_lossy().into_owned();
        let mut file = self.fresh();
        file.recent_libraries.retain(|known| *known != text);
        self.write(&file)
    }

    /// The item ids of the library's pinned reader tabs, in tab order.
    pub fn pinned_tabs(&self, library: &Path) -> Vec<String> {
        let key = library.to_string_lossy();
        let file = self.fresh();
        file.pinned_tabs
            .get(key.as_ref())
            .cloned()
            .unwrap_or_default()
    }

    /// Replaces the library's pinned tabs. An empty list forgets the library.
    pub fn set_pinned_tabs(&self, library: &Path, item_ids: Vec<String>) -> Result<(), PaperError> {
        let key = library.to_string_lossy().into_owned();
        let mut file = self.fresh();
        if item_ids.is_empty() {
            file.pinned_tabs.remove(&key);
        } else {
            file.pinned_tabs.insert(key, item_ids);
        }
        self.write(&file)
    }

    /// The file as it is on disk now, locked for a change. Each open library
    /// has its own store on the same file, so a change starts from what the
    /// others wrote rather than from this store's copy.
    fn fresh(&self) -> std::sync::MutexGuard<'_, SettingsFile> {
        let mut file = self.file.lock().unwrap();
        if let Some(disk) = read(&self.path) {
            *file = disk;
        }
        file
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

/// The file at `path`, or `None` when it is missing or unreadable.
fn read(path: &Path) -> Option<SettingsFile> {
    let text = fs::read_to_string(path).ok()?;
    match serde_json::from_str::<SettingsFile>(&text) {
        Ok(file) => Some(file),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "ignoring unreadable settings");
            None
        }
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
                    detail_collapsed: true,
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
        assert!(store.settings().detail_collapsed);
        assert!(!store.settings().sidebar_collapsed);
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
    fn pinned_tabs_are_kept_per_library_across_stores() {
        let dir = tempfile::tempdir().unwrap();
        // Two open libraries each have a store on the same file.
        let first = SettingsStore::open(dir.path());
        let second = SettingsStore::open(dir.path());
        let (a, b) = (Path::new("/libraries/a"), Path::new("/libraries/b"));
        first
            .set_pinned_tabs(a, vec!["x".into(), "y".into()])
            .unwrap();
        second.set_pinned_tabs(b, vec!["z".into()]).unwrap();
        second
            .update_settings(Settings {
                sidebar_width: Some(200.0),
                ..second.settings()
            })
            .unwrap();
        second.remember_library(b).unwrap();
        assert_eq!(
            first.pinned_tabs(a),
            ["x", "y"],
            "the other store's writes keep these pins"
        );

        let reopened = SettingsStore::open(dir.path());
        assert_eq!(reopened.pinned_tabs(a), ["x", "y"]);
        assert_eq!(reopened.pinned_tabs(b), ["z"]);
        assert_eq!(reopened.settings().sidebar_width, Some(200.0));
        reopened.set_pinned_tabs(a, Vec::new()).unwrap();
        assert!(SettingsStore::open(dir.path()).pinned_tabs(a).is_empty());
        assert_eq!(SettingsStore::open(dir.path()).pinned_tabs(b), ["z"]);
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

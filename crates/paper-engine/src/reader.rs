//! Which PDFs the app's reader tabs show, for programs beside the app, such
//! as the MCP server, that answer "the paper I am reading".
//!
//! While an engine has a library open it holds an exclusive lock on
//! `cache/app.lock`, and it writes the app's tabs to `cache/reader.json`.
//! Another process reads that file only while it cannot take the lock. The
//! operating system drops a process's locks when it ends, so a file left by an
//! app that crashed, or by Paper on another machine that syncs the library
//! folder, is never taken as what the user reads now.

use std::fs::{File, TryLockError};
use std::path::Path;

use serde::{Deserialize, Serialize};

const CACHE_DIR: &str = "cache";
const LOCK_FILE: &str = "app.lock";
const TABS_FILE: &str = "reader.json";

/// The PDF attachments open in the app's reader tabs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReaderTabs {
    /// Attachment ids in tab order.
    pub attachment_ids: Vec<String>,
    /// The tab in front. While the library tab is in front, the reader tab
    /// that was in front last, if it is still open.
    pub front: Option<String>,
}

/// Takes the lock that tells other processes an app has the library open.
/// `None` when it cannot: the folder is read-only, or another app holds it.
pub(crate) fn hold_app_lock(library_dir: &Path) -> Option<File> {
    let dir = library_dir.join(CACHE_DIR);
    let file = std::fs::create_dir_all(&dir).and_then(|()| {
        File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join(LOCK_FILE))
    });
    let file = match file {
        Ok(file) => file,
        Err(error) => {
            tracing::warn!(%error, "could not make the app lock file");
            return None;
        }
    };
    match file.try_lock() {
        Ok(()) => Some(file),
        Err(error) => {
            tracing::warn!(%error, "could not take the app lock");
            None
        }
    }
}

/// Ends what `hold_app_lock` and `write_tabs` started: the tabs file goes
/// first, then the lock.
pub(crate) fn release_app_lock(library_dir: &Path, lock: File) {
    let _ = std::fs::remove_file(library_dir.join(CACHE_DIR).join(TABS_FILE));
    drop(lock);
}

/// Replaces the tabs file. Readers never see half a file: it is written
/// beside and renamed over.
pub(crate) fn write_tabs(library_dir: &Path, tabs: &ReaderTabs) -> std::io::Result<()> {
    let dir = library_dir.join(CACHE_DIR);
    std::fs::create_dir_all(&dir)?;
    let partial = dir.join(format!("{TABS_FILE}.partial"));
    std::fs::write(&partial, serde_json::to_vec(tabs)?)?;
    std::fs::rename(partial, dir.join(TABS_FILE))
}

/// The tabs of the app that has the library open, or `None` when no app
/// has it open. An app that has not told its tabs yet shows none.
pub(crate) fn read_tabs(library_dir: &Path) -> Option<ReaderTabs> {
    let dir = library_dir.join(CACHE_DIR);
    // No lock file: no app has opened the library since the cache was made.
    let lock = File::open(dir.join(LOCK_FILE)).ok()?;
    match lock.try_lock_shared() {
        // Free, so no app holds it. Dropping the file lets it go again.
        Ok(()) => return None,
        Err(TryLockError::WouldBlock) => {}
        Err(TryLockError::Error(error)) => {
            tracing::warn!(%error, "could not check the app lock");
            return None;
        }
    }
    let tabs = std::fs::read(dir.join(TABS_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    Some(tabs)
}

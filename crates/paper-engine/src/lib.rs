//! The public API of the Paper engine: one `Engine` per open library.
//!
//! The shape follows `docs/ARCHITECTURE.md`. Reads the UI needs on every
//! keystroke (`list_items`, `item_detail`, `collection_tree`) are synchronous
//! and run against the database on the caller's thread. Anything that touches
//! the network is an `async fn` whose work runs on the engine's own thread
//! pool. Events reach the app through `EngineObserver` on engine threads.
//!
//! Nothing here knows about UniFFI or Swift. `paper-ffi` exports this API.

mod pool;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub use paper_core::{
    AttachmentRow, CollectionNode, Creator, CreatorRole, ItemDetail, ItemFields, ItemKind,
    ItemQuery, ItemRow, LookupStatus, MAX_RECENT_LIBRARIES, MetadataPatch, PaperError,
    RECENTLY_ADDED_DAYS, Scope, Settings, SortColumn, SourceRecord, Tag, WindowState,
};
use paper_core::{Library, SettingsStore};
pub use paper_fetch::HttpClient;
use paper_fetch::{Contact, FetchError, Resolver, UreqClient};
pub use paper_ids::Identifier;

use crate::pool::ThreadPool;

/// Threads for lookups and downloads.
const POOL_SIZE: usize = 4;

type Result<T> = std::result::Result<T, PaperError>;

/// Events from the engine. Called on engine threads, never on the caller's
/// thread, so an app must move to its UI thread before touching a view.
pub trait EngineObserver: Send + Sync {
    /// These items were added, changed, deleted, or restored. Reload the rows.
    fn items_changed(&self, ids: Vec<String>);
    /// The collection tree or its item counts changed.
    fn collections_changed(&self);
    /// The tag list or its item counts changed.
    fn tags_changed(&self);
    /// A metadata lookup for an item started, finished, or failed.
    fn lookup_progress(&self, item_id: String, status: LookupStatus);
}

/// How an engine is built. Tests swap the settings directory and the HTTP
/// client; the app uses `Default`.
#[derive(Default)]
pub struct EngineOptions {
    /// Where `settings.json` lives. Default: the platform config directory.
    pub settings_dir: Option<PathBuf>,
    /// The HTTP client for lookups. Default: `ureq` with the platform roots.
    pub http: Option<Arc<dyn HttpClient>>,
}

/// One open library.
pub struct Engine {
    library: Arc<Library>,
    settings: Arc<SettingsStore>,
    resolver: Arc<Resolver>,
    pool: Mutex<Option<ThreadPool>>,
    observer: Mutex<Option<Arc<dyn EngineObserver>>>,
    /// Batches of ids from `delete_items`, most recent last.
    undo: Mutex<Vec<Vec<String>>>,
    closed: AtomicBool,
}

impl Engine {
    /// Opens the library folder at `path`. Fails if it is not a library.
    pub fn open_library(path: impl AsRef<Path>) -> Result<Engine> {
        Engine::open_library_with(path, EngineOptions::default())
    }

    /// Creates a new library folder at `path` and opens it. Fails if the folder
    /// already holds a library.
    pub fn create_library(path: impl AsRef<Path>) -> Result<Engine> {
        Engine::create_library_with(path, EngineOptions::default())
    }

    pub fn open_library_with(path: impl AsRef<Path>, options: EngineOptions) -> Result<Engine> {
        let library = Library::open(path)?;
        Engine::start(library, options)
    }

    pub fn create_library_with(path: impl AsRef<Path>, options: EngineOptions) -> Result<Engine> {
        let library = Library::create(path)?;
        Engine::start(library, options)
    }

    fn start(library: Library, options: EngineOptions) -> Result<Engine> {
        let settings_dir = match options.settings_dir {
            Some(dir) => dir,
            None => SettingsStore::default_dir()?,
        };
        let settings = SettingsStore::open(settings_dir);
        settings.remember_library(library.dir())?;
        let http = options.http.unwrap_or_else(|| Arc::new(UreqClient::new()));
        tracing::info!(path = %library.dir().display(), "library opened");
        Ok(Engine {
            library: Arc::new(library),
            settings: Arc::new(settings),
            resolver: Arc::new(Resolver::new(http)),
            pool: Mutex::new(Some(ThreadPool::new(POOL_SIZE))),
            observer: Mutex::new(None),
            undo: Mutex::new(Vec::new()),
            closed: AtomicBool::new(false),
        })
    }

    pub fn library_path(&self) -> &Path {
        self.library.dir()
    }

    /// Stops the engine threads. Every later call returns `PaperError::Closed`.
    /// Lookups that are still running finish, then the database closes when
    /// the last reference to the engine goes away.
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        let pool = self.pool.lock().unwrap().take();
        drop(pool);
        *self.observer.lock().unwrap() = None;
        tracing::info!(path = %self.library.dir().display(), "library closed");
    }

    /// Replaces the observer. The engine keeps it alive until replaced or closed.
    pub fn set_observer(&self, observer: Arc<dyn EngineObserver>) {
        *self.observer.lock().unwrap() = Some(observer);
    }

    // ----- items -----

    /// The rows of the library table. Synchronous: this is called on every keystroke.
    pub fn list_items(&self, query: &ItemQuery) -> Result<Vec<ItemRow>> {
        self.ensure_open()?;
        self.library.list_items(query)
    }

    pub fn item_detail(&self, id: &str) -> Result<ItemDetail> {
        self.ensure_open()?;
        self.library.item(id)
    }

    /// Adds a paper by DOI, doi.org URL, or arXiv ID.
    ///
    /// The item appears at once with the identifier as its title and a
    /// `Pending` lookup status, so the table can show a progress line. The
    /// metadata lookup then runs on an engine thread; when it finishes the
    /// item is updated and returned. If the lookup fails the item stays with
    /// a `Failed` status for `retry_lookup`, and the error names it.
    ///
    /// If the DOI is already in the library, `PaperError::Duplicate` carries
    /// the existing item's id so the app can select it. A deleted item with
    /// the same DOI is restored and looked up again.
    pub async fn add_by_identifier(&self, text: &str) -> Result<ItemRow> {
        self.ensure_open()?;
        let identifier = Identifier::parse(text).ok_or_else(|| PaperError::InvalidIdentifier {
            text: text.trim().to_string(),
        })?;
        let doi = identifier.doi();

        let item_id = match self.library.find_item_by_doi(&doi)? {
            Some(found) if !found.deleted => {
                return Err(PaperError::Duplicate {
                    doi,
                    item_id: found.item_id,
                });
            }
            Some(found) => {
                self.library
                    .restore_items(std::slice::from_ref(&found.item_id))?;
                self.library
                    .set_lookup_status(&found.item_id, LookupStatus::Pending)?;
                found.item_id
            }
            None => {
                let placeholder = ItemFields {
                    kind: Some(match identifier {
                        Identifier::Arxiv(_) => ItemKind::Preprint,
                        Identifier::Doi(_) => ItemKind::Other,
                    }),
                    doi: Some(doi.clone()),
                    arxiv_id: identifier.arxiv_id(),
                    title: identifier.to_string(),
                    ..ItemFields::default()
                };
                self.library
                    .insert_item(&placeholder, LookupStatus::Pending)?
            }
        };
        self.notify(|observer| observer.items_changed(vec![item_id.clone()]));
        self.lookup(item_id, identifier).await
    }

    /// Runs the metadata lookup again for an item whose lookup failed or that
    /// was added offline.
    pub async fn retry_lookup(&self, item_id: &str) -> Result<ItemRow> {
        self.ensure_open()?;
        let detail = self.library.item(item_id)?;
        let identifier = match (&detail.fields.arxiv_id, &detail.fields.doi) {
            (Some(arxiv), _) => Identifier::Arxiv(arxiv.clone()),
            (None, Some(doi)) => Identifier::Doi(doi.clone()),
            (None, None) => {
                return Err(PaperError::InvalidIdentifier {
                    text: detail.fields.title,
                });
            }
        };
        self.library
            .set_lookup_status(item_id, LookupStatus::Pending)?;
        self.notify(|observer| observer.items_changed(vec![item_id.to_string()]));
        self.lookup(item_id.to_string(), identifier).await
    }

    async fn lookup(&self, item_id: String, identifier: Identifier) -> Result<ItemRow> {
        let doi = identifier.doi();
        self.notify(|observer| observer.lookup_progress(item_id.clone(), LookupStatus::Pending));

        let resolver = Arc::clone(&self.resolver);
        let contact = Contact::new(self.settings.settings().contact_email);
        let job_identifier = identifier.clone();
        let result = self
            .run(move || resolver.lookup(&job_identifier, &contact))
            .await;

        let outcome = match result {
            Ok(Ok(lookup)) => self
                .library
                .replace_metadata(&item_id, &lookup.fields, LookupStatus::Done)
                .and_then(|()| {
                    for record in &lookup.records {
                        self.library
                            .put_source_record(&item_id, &record.source, &record.json)?;
                    }
                    Ok(())
                }),
            Ok(Err(FetchError::NotFound { doi })) => Err(PaperError::MetadataNotFound {
                doi,
                item_id: item_id.clone(),
            }),
            Ok(Err(error)) => Err(PaperError::Lookup {
                doi: doi.clone(),
                item_id: item_id.clone(),
                message: error.to_string(),
            }),
            Err(closed) => Err(closed),
        };

        match outcome {
            Ok(()) => {
                self.notify(|observer| {
                    observer.lookup_progress(item_id.clone(), LookupStatus::Done)
                });
                self.notify(|observer| observer.items_changed(vec![item_id.clone()]));
                self.library.item_row(&item_id)
            }
            Err(error) => {
                tracing::warn!(%doi, %error, "lookup failed");
                // The item may have been deleted while the lookup ran; that is fine.
                let _ = self
                    .library
                    .set_lookup_status(&item_id, LookupStatus::Failed);
                self.notify(|observer| {
                    observer.lookup_progress(item_id.clone(), LookupStatus::Failed)
                });
                self.notify(|observer| observer.items_changed(vec![item_id.clone()]));
                Err(error)
            }
        }
    }

    /// Applies the fields the user edited in the metadata editor.
    pub fn update_metadata(&self, id: &str, patch: &MetadataPatch) -> Result<()> {
        self.ensure_open()?;
        let mut patch = patch.clone();
        if let Some(doi) = patch.doi.as_deref().filter(|doi| !doi.is_empty()) {
            patch.doi = Some(paper_ids::normalize_doi(doi).ok_or_else(|| {
                PaperError::InvalidIdentifier {
                    text: doi.to_string(),
                }
            })?);
        }
        self.library.update_metadata(id, &patch)?;
        self.notify(|observer| observer.items_changed(vec![id.to_string()]));
        Ok(())
    }

    /// Moves items to the trash. `undo_delete` brings back the last batch.
    pub fn delete_items(&self, ids: &[String]) -> Result<()> {
        self.ensure_open()?;
        let deleted = self.library.delete_items(ids)?;
        if deleted.is_empty() {
            return Ok(());
        }
        self.undo.lock().unwrap().push(deleted.clone());
        self.notify(|observer| observer.items_changed(deleted));
        self.notify(|observer| observer.collections_changed());
        self.notify(|observer| observer.tags_changed());
        Ok(())
    }

    /// Restores the most recent `delete_items` batch and returns the ids that
    /// are live again. Returns an empty list when there is nothing to undo.
    pub fn undo_delete(&self) -> Result<Vec<String>> {
        self.ensure_open()?;
        let Some(batch) = self.undo.lock().unwrap().pop() else {
            return Ok(Vec::new());
        };
        let restored = self.library.restore_items(&batch)?;
        if !restored.is_empty() {
            self.notify(|observer| observer.items_changed(restored.clone()));
            self.notify(|observer| observer.collections_changed());
            self.notify(|observer| observer.tags_changed());
        }
        Ok(restored)
    }

    /// Whether `undo_delete` has something to restore.
    pub fn can_undo_delete(&self) -> bool {
        !self.undo.lock().unwrap().is_empty()
    }

    // ----- collections -----

    /// All collections in depth first order with their depth and item counts.
    pub fn collection_tree(&self) -> Result<Vec<CollectionNode>> {
        self.ensure_open()?;
        self.library.collection_tree()
    }

    /// Creates a collection and returns its id.
    pub fn create_collection(&self, name: &str, parent_id: Option<&str>) -> Result<String> {
        self.ensure_open()?;
        let id = self.library.create_collection(name, parent_id)?;
        self.notify(|observer| observer.collections_changed());
        Ok(id)
    }

    pub fn rename_collection(&self, id: &str, name: &str) -> Result<()> {
        self.ensure_open()?;
        self.library.rename_collection(id, name)?;
        self.notify(|observer| observer.collections_changed());
        Ok(())
    }

    /// Moves a collection under `parent_id` (or to the top level) at `position`.
    pub fn move_collection(&self, id: &str, parent_id: Option<&str>, position: i64) -> Result<()> {
        self.ensure_open()?;
        self.library.move_collection(id, parent_id, position)?;
        self.notify(|observer| observer.collections_changed());
        Ok(())
    }

    /// Deletes a collection and its subcollections. Items stay in the library.
    pub fn delete_collection(&self, id: &str) -> Result<()> {
        self.ensure_open()?;
        self.library.delete_collection(id)?;
        self.notify(|observer| observer.collections_changed());
        Ok(())
    }

    pub fn add_to_collection(&self, collection_id: &str, item_ids: &[String]) -> Result<()> {
        self.ensure_open()?;
        self.library.add_to_collection(collection_id, item_ids)?;
        self.notify(|observer| observer.collections_changed());
        self.notify(|observer| observer.items_changed(item_ids.to_vec()));
        Ok(())
    }

    pub fn remove_from_collection(&self, collection_id: &str, item_ids: &[String]) -> Result<()> {
        self.ensure_open()?;
        self.library
            .remove_from_collection(collection_id, item_ids)?;
        self.notify(|observer| observer.collections_changed());
        self.notify(|observer| observer.items_changed(item_ids.to_vec()));
        Ok(())
    }

    // ----- tags -----

    pub fn tags(&self) -> Result<Vec<Tag>> {
        self.ensure_open()?;
        self.library.tags()
    }

    /// Replaces an item's tags. New names create tags; tags left without
    /// items disappear.
    pub fn set_tags(&self, item_id: &str, names: &[String]) -> Result<()> {
        self.ensure_open()?;
        self.library.set_tags(item_id, names)?;
        self.notify(|observer| observer.tags_changed());
        self.notify(|observer| observer.items_changed(vec![item_id.to_string()]));
        Ok(())
    }

    pub fn set_tag_color(&self, tag_id: &str, color: Option<&str>) -> Result<()> {
        self.ensure_open()?;
        self.library.set_tag_color(tag_id, color)?;
        self.notify(|observer| observer.tags_changed());
        Ok(())
    }

    // ----- notes -----

    pub fn note(&self, item_id: &str) -> Result<Option<String>> {
        self.ensure_open()?;
        self.library.note(item_id)
    }

    /// Saves the item's Markdown note. The app debounces autosave.
    pub fn save_note(&self, item_id: &str, markdown: &str) -> Result<()> {
        self.ensure_open()?;
        self.library.save_note(item_id, markdown)?;
        self.notify(|observer| observer.items_changed(vec![item_id.to_string()]));
        Ok(())
    }

    // ----- settings -----

    pub fn settings(&self) -> Settings {
        self.settings.settings()
    }

    pub fn update_settings(&self, settings: Settings) -> Result<()> {
        self.settings.update_settings(settings)
    }

    /// Libraries opened on this machine, most recent first.
    pub fn recent_libraries(&self) -> Vec<String> {
        self.settings.recent_libraries()
    }

    // ----- internals -----

    fn ensure_open(&self) -> Result<()> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(PaperError::Closed);
        }
        Ok(())
    }

    fn run<T: Send + 'static, F: FnOnce() -> T + Send + 'static>(
        &self,
        job: F,
    ) -> impl Future<Output = Result<T>> + Send + use<T, F> {
        let pool = self.pool.lock().unwrap();
        let future = pool.as_ref().map(|pool| pool.run(job));
        async move {
            match future {
                Some(future) => future.await,
                None => Err(PaperError::Closed),
            }
        }
    }

    fn notify(&self, call: impl FnOnce(&dyn EngineObserver)) {
        // Clone the observer out so the lock is not held during the callback.
        let observer = self.observer.lock().unwrap().clone();
        if let Some(observer) = observer {
            call(observer.as_ref());
        }
    }
}

/// Whether the folder holds a Paper library (`library.db`).
pub fn is_library(path: impl AsRef<Path>) -> bool {
    Library::is_library(path)
}

/// Libraries opened on this machine, most recent first, from the platform
/// settings directory. Folders that no longer exist are left out.
pub fn recent_libraries() -> Vec<String> {
    match SettingsStore::default_dir() {
        Ok(dir) => recent_libraries_in(dir),
        Err(_) => Vec::new(),
    }
}

/// Like `recent_libraries` for a specific settings directory.
pub fn recent_libraries_in(settings_dir: impl AsRef<Path>) -> Vec<String> {
    SettingsStore::open(settings_dir)
        .recent_libraries()
        .into_iter()
        .filter(|path| is_library(path))
        .collect()
}

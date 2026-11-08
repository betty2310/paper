//! The public API of the Paper engine: one `Engine` per open library.
//!
//! The shape follows `docs/ARCHITECTURE.md`. Reads the UI needs on every
//! keystroke (`list_items`, `item_detail`, `collection_tree`) are synchronous
//! and run against the database on the caller's thread. Anything that touches
//! the network is an `async fn` whose work runs on the engine's own thread
//! pool. Events reach the app through `EngineObserver` on engine threads.
//!
//! Nothing here knows about UniFFI or Swift. `paper-ffi` exports this API.

mod import;
mod pool;
mod topics;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub use paper_cite::{Citation, CitationStyle};
pub use paper_core::{
    Annotation, AnnotationKind, AnnotationPatch, AttachmentFile, AttachmentRow, CollectionNode,
    Creator, CreatorRole, Figure, FigureSource, Identification, ImportedPdf, ItemDetail,
    ItemFields, ItemKind, ItemQuery, ItemRow, LookupStatus, MAX_RECENT_LIBRARIES, MetadataPatch,
    NewAnnotation, PagePoint, PageRect, PaperError, PdfStatus, PdfStep, Quad, RECENTLY_ADDED_DAYS,
    Scope, Settings, SortColumn, SourceRecord, Tag, Topic, WindowState,
};
use paper_core::{Library, SettingsStore};
pub use paper_fetch::HttpClient;
use paper_fetch::{Contact, FetchError, PdfQuery, Resolver, UreqClient};
pub use paper_ids::Identifier;

use crate::pool::{PoolError, ThreadPool};
pub use crate::topics::TopicTags;
use crate::topics::{JEV_SOURCE, TopicJob};

/// Threads for lookups and downloads.
const POOL_SIZE: usize = 4;
/// How many `delete_items` batches `undo_delete` remembers.
const MAX_UNDO_BATCHES: usize = 100;

type Result<T> = std::result::Result<T, PaperError>;

type SharedObserver = Arc<Mutex<Option<Arc<dyn EngineObserver>>>>;
/// Items whose PDF search is running, so one item never has two.
type PdfSearches = Arc<Mutex<HashSet<String>>>;
/// Batches of ids from `delete_items`, most recent last.
type UndoBatches = Arc<Mutex<Vec<Vec<String>>>>;
type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// Events from the engine. An event caused by a synchronous call arrives on
/// the caller's thread before that call returns; everything a lookup does
/// after it leaves for the network arrives on an engine thread. An app must
/// therefore hop to its UI thread asynchronously (never with a blocking
/// dispatch) before touching a view.
pub trait EngineObserver: Send + Sync {
    /// These items were added, changed, deleted, or restored. Reload the rows.
    fn items_changed(&self, ids: Vec<String>);
    /// The collection tree or its item counts changed.
    fn collections_changed(&self);
    /// The tag list or its item counts changed.
    fn tags_changed(&self);
    /// A metadata lookup for an item started, finished, or failed.
    fn lookup_progress(&self, item_id: String, status: LookupStatus);
    /// The open access PDF search for an item started, found a PDF, found
    /// none, or failed. `items_changed` follows a found PDF.
    fn pdf_progress(&self, item_id: String, status: PdfStatus);
    /// A running PDF search moved on: it asks another source, or more of the
    /// file arrived. Download steps come at most every `PDF_STEP_INTERVAL`,
    /// plus the last one.
    fn pdf_step(&self, item_id: String, step: PdfStep);
    /// A paper that was just added got these tags from the topic list.
    /// `tags_changed` comes first. `tag_with_topics` returns its tags
    /// instead of sending this.
    fn topics_added(&self, item_id: String, tags: Vec<String>);
}

/// How an engine is built. Tests swap the settings directory and the HTTP
/// client; the app uses `Default`.
#[derive(Default)]
pub struct EngineOptions {
    /// Where `settings.json` lives. Default: the platform config directory.
    pub settings_dir: Option<PathBuf>,
    /// The HTTP client for lookups. Default: `ureq` with the platform roots.
    pub http: Option<Arc<dyn HttpClient>>,
    /// Do not search for a PDF after a lookup; only `find_pdf` does. Tests
    /// that count requests or events use it.
    pub manual_pdf_search: bool,
}

/// One open library.
pub struct Engine {
    library: Arc<Library>,
    settings: Arc<SettingsStore>,
    resolver: Arc<Resolver>,
    pool: Mutex<Option<ThreadPool>>,
    /// Shared with lookup jobs, so `close` and `set_observer` reach them too.
    observer: SharedObserver,
    undo: UndoBatches,
    pdf_searches: PdfSearches,
    manual_pdf_search: bool,
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
        // A read-only config directory must not keep the user out of their library.
        if let Err(error) = settings.remember_library(library.dir()) {
            tracing::warn!(%error, "could not add the library to the recent list");
        }
        // Libraries from before citation keys get theirs once, here.
        match library.items_without_citation_key(LookupStatus::Done) {
            Ok(ids) => {
                give_citation_keys(&library, &ids);
            }
            Err(error) => tracing::warn!(%error, "could not list items without citation keys"),
        }
        let http = options.http.unwrap_or_else(|| Arc::new(UreqClient::new()));
        tracing::info!(path = %library.dir().display(), "library opened");
        Ok(Engine {
            library: Arc::new(library),
            settings: Arc::new(settings),
            resolver: Arc::new(Resolver::new(http)),
            pool: Mutex::new(Some(ThreadPool::new(POOL_SIZE))),
            observer: Arc::new(Mutex::new(None)),
            undo: Arc::new(Mutex::new(Vec::new())),
            pdf_searches: Arc::new(Mutex::new(HashSet::new())),
            manual_pdf_search: options.manual_pdf_search,
            closed: AtomicBool::new(false),
        })
    }

    pub fn library_path(&self) -> &Path {
        self.library.dir()
    }

    /// Stops accepting work; every later call returns `PaperError::Closed`.
    /// Returns at once. A lookup that is still waiting on the network finishes
    /// on its own thread without telling the observer, and the database closes
    /// when the last of the engine and such lookups goes away.
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

        let (item_id, placeholder) = match self.library.find_item_by_doi(&doi)? {
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
                forget_undo(&self.undo, &found.item_id);
                (found.item_id, false)
            }
            None => {
                let placeholder = ItemFields {
                    kind: Some(if identifier.arxiv_id().is_some() {
                        ItemKind::Preprint
                    } else {
                        ItemKind::Other
                    }),
                    doi: Some(doi.clone()),
                    arxiv_id: identifier.arxiv_id(),
                    title: identifier.to_string(),
                    ..ItemFields::default()
                };
                let id = self
                    .library
                    .insert_item(&placeholder, LookupStatus::Pending)?;
                (id, true)
            }
        };
        self.notify(|observer| observer.items_changed(vec![item_id.clone()]));
        self.lookup(item_id, identifier, placeholder).await
    }

    /// Runs the metadata lookup again for an item whose lookup failed or that
    /// was added offline. Every field is replaced by what the lookup returns.
    pub async fn retry_lookup(&self, item_id: &str) -> Result<ItemRow> {
        self.ensure_open()?;
        let detail = self.library.item(item_id)?;
        let identifier = match (&detail.fields.doi, &detail.fields.arxiv_id) {
            // The arXiv ID the user typed beats its own DataCite DOI, but a
            // journal DOI the user entered beats the arXiv ID.
            (Some(doi), Some(arxiv)) if Identifier::Doi(doi.clone()).arxiv_id().is_some() => {
                Identifier::Arxiv(arxiv.clone())
            }
            (Some(doi), _) => Identifier::Doi(doi.clone()),
            (None, Some(arxiv)) => Identifier::Arxiv(arxiv.clone()),
            (None, None) => {
                return Err(PaperError::InvalidIdentifier {
                    text: detail.fields.title,
                });
            }
        };
        self.library
            .set_lookup_status(item_id, LookupStatus::Pending)?;
        self.notify(|observer| observer.items_changed(vec![item_id.to_string()]));
        self.lookup(item_id.to_string(), identifier, false).await
    }

    /// Looks up `identifier` and writes the result into the item, all on an
    /// engine thread: the caller only awaits the final row. `placeholder` says
    /// the row was inserted for this lookup and holds nothing the user made.
    async fn lookup(
        &self,
        item_id: String,
        identifier: Identifier,
        placeholder: bool,
    ) -> Result<ItemRow> {
        let job = LookupJob {
            library: Arc::clone(&self.library),
            observer: Arc::clone(&self.observer),
            item_id,
            doi: identifier.doi(),
        };
        let fallback = job.clone();
        let resolver = Arc::clone(&self.resolver);
        let contact = Contact::new(self.settings.settings().contact_email);
        match self
            .run(move || job.run(resolver.lookup(&identifier, &contact), placeholder))
            .await
        {
            Ok(Ok(row)) => {
                // The PDF search runs on its own; the caller only waits for
                // the metadata. Its progress arrives through the observer.
                if !self.manual_pdf_search && !row.has_pdf {
                    match self.start_pdf_search(&row.id) {
                        Ok(PdfSearch::Running(search)) => drop(search),
                        Ok(PdfSearch::Attached(_)) => {}
                        Err(error) => tracing::info!(%error, "no PDF search after the lookup"),
                    }
                }
                self.start_topic_tags(&row.id);
                Ok(row)
            }
            Ok(outcome) => outcome,
            Err(PoolError::Closed) => Err(PaperError::Closed),
            // The job died before it could mark the row, so do that here.
            Err(PoolError::Panicked(message)) => fallback.fail(PaperError::Lookup {
                doi: fallback.doi.clone(),
                item_id: fallback.item_id.clone(),
                message: format!("the lookup crashed: {message}"),
            }),
        }
    }

    /// Adds a paper from a PDF file, such as one dropped on the window.
    ///
    /// A row named after the file appears at once with a `Pending` lookup.
    /// On an engine thread the engine then works out which paper the file is
    /// (see `Identification`), fills in the metadata, and copies the PDF into
    /// the library. The original file is left where it was.
    ///
    /// When nothing identifies the paper, the item stays with the PDF, the
    /// title found in it, and a `Failed` lookup, so the user can enter a DOI.
    /// When the paper is in the library already, its item gets the PDF if it
    /// had none, the new row goes away, and `PaperError::Duplicate` names the
    /// existing item. Fails with `NotPdf` for anything but a PDF.
    pub async fn import_pdf(&self, path: &str) -> Result<ImportedPdf> {
        self.ensure_open()?;
        let settings = self.settings.settings();
        let job = import::ImportJob {
            library: Arc::clone(&self.library),
            observer: Arc::clone(&self.observer),
            resolver: Arc::clone(&self.resolver),
            undo: Arc::clone(&self.undo),
            contact: Contact::new(settings.contact_email),
            api_key: Some(settings.anthropic_api_key.trim().to_string())
                .filter(|key| !key.is_empty()),
        };
        let path = PathBuf::from(path);
        let outcome = match self.run(move || job.run(&path)).await {
            Ok(outcome) => outcome,
            Err(PoolError::Closed) => Err(PaperError::Closed),
            Err(PoolError::Panicked(message)) => Err(PaperError::Io { message }),
        };
        if let Ok(imported) = &outcome
            && !matches!(
                imported.identification,
                Identification::NotFound | Identification::Failed { .. }
            )
        {
            self.start_topic_tags(&imported.item.id);
        }
        outcome
    }

    /// Adds a paper the user typed in, such as a manuscript that has no DOI
    /// yet. Only the title is required. Nothing is looked up: the item keeps
    /// exactly these fields, and `attach_file` gives it a PDF.
    ///
    /// Fails with `Duplicate` when the DOI is in the library already.
    pub fn create_item(&self, fields: &ItemFields) -> Result<ItemRow> {
        self.ensure_open()?;
        let mut fields = fields.clone();
        fields.title = fields.title.trim().to_string();
        if fields.title.is_empty() {
            return Err(PaperError::Library {
                message: "A paper needs a title.".to_string(),
            });
        }
        fields.doi = normalized_doi(fields.doi.as_deref())?;
        fields.arxiv_id = normalized_arxiv_id(fields.arxiv_id.as_deref())?;
        let id = self.library.insert_item(&fields, LookupStatus::Done)?;
        give_citation_keys(&self.library, std::slice::from_ref(&id));
        self.notify(|observer| observer.items_changed(vec![id.clone()]));
        self.library.item_row(&id)
    }

    /// Applies the fields the user edited in the metadata editor.
    pub fn update_metadata(&self, id: &str, patch: &MetadataPatch) -> Result<()> {
        self.ensure_open()?;
        let mut patch = patch.clone();
        if let Some(doi) = patch.doi.as_deref().filter(|doi| !doi.is_empty()) {
            patch.doi = normalized_doi(Some(doi))?;
        }
        if let Some(arxiv) = patch.arxiv_id.as_deref().filter(|id| !id.is_empty()) {
            patch.arxiv_id = normalized_arxiv_id(Some(arxiv))?;
        }
        if let Some(key) = &mut patch.citation_key {
            *key = key.trim().to_string();
            if !key.is_empty() && !paper_cite::is_valid_key(key) {
                return Err(PaperError::Library {
                    message: format!(
                        "“{key}” cannot be a citation key. Use letters, digits, and - _ : . / without spaces."
                    ),
                });
            }
        }
        self.library.update_metadata(id, &patch)?;
        // An emptied key is made again from the metadata as it is now.
        if patch.citation_key.as_deref() == Some("") {
            give_citation_keys(&self.library, &[id.to_string()]);
        }
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
        let mut undo = self.undo.lock().unwrap();
        undo.push(deleted.clone());
        if undo.len() > MAX_UNDO_BATCHES {
            undo.remove(0);
        }
        drop(undo);
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

    // ----- attachments -----

    /// Looks for an open access PDF (arXiv, OpenAlex, Unpaywall, Europe PMC),
    /// downloads it into the library, and attaches it. Adding a paper starts
    /// this on its own; the app calls it to search again. An item that has a
    /// PDF already returns it without touching the network.
    ///
    /// Fails with `PdfNotFound` when no source offers one, and with `Network`
    /// when the search could not finish.
    pub async fn find_pdf(&self, item_id: &str) -> Result<AttachmentRow> {
        match self.start_pdf_search(item_id)? {
            PdfSearch::Attached(row) => Ok(row),
            PdfSearch::Running(search) => search.await,
        }
    }

    /// Queues the PDF search on the pool at once, so dropping the returned
    /// future does not cancel it.
    fn start_pdf_search(&self, item_id: &str) -> Result<PdfSearch> {
        self.ensure_open()?;
        let detail = self.library.item(item_id)?;
        if let Some(pdf) = detail.attachments.iter().find(|a| a.mime == PDF_MIME) {
            return Ok(PdfSearch::Attached(pdf.clone()));
        }
        if detail.fields.doi.is_none() && detail.fields.arxiv_id.is_none() {
            return Err(PaperError::PdfNotFound {
                item_id: item_id.to_string(),
            });
        }
        if !self
            .pdf_searches
            .lock()
            .unwrap()
            .insert(item_id.to_string())
        {
            return Err(PaperError::Library {
                message: "Paper is already looking for this PDF.".to_string(),
            });
        }
        let settings = self.settings.settings();
        let job = PdfJob {
            library: Arc::clone(&self.library),
            observer: Arc::clone(&self.observer),
            searches: Arc::clone(&self.pdf_searches),
            item_id: item_id.to_string(),
        };
        let query = PdfQuery {
            doi: detail.fields.doi,
            arxiv_id: detail.fields.arxiv_id,
            openalex_api_key: Some(settings.openalex_api_key).filter(|key| !key.is_empty()),
        };
        let contact = Contact::new(settings.contact_email);
        job.report(PdfStatus::Searching);
        let fallback = job.clone();
        let resolver = Arc::clone(&self.resolver);
        let running = self.run(move || job.run(&resolver, &query, &contact));
        Ok(PdfSearch::Running(Box::pin(async move {
            match running.await {
                Ok(outcome) => outcome,
                Err(PoolError::Closed) => {
                    fallback.finish();
                    Err(PaperError::Closed)
                }
                Err(PoolError::Panicked(message)) => {
                    fallback.finish();
                    fallback.report(PdfStatus::Failed);
                    Err(PaperError::Network { message })
                }
            }
        })))
    }

    /// Copies a PDF the user chose into the library and attaches it. The
    /// original file is left where it was.
    pub async fn attach_file(&self, item_id: &str, path: &str) -> Result<AttachmentRow> {
        self.ensure_open()?;
        let library = Arc::clone(&self.library);
        let observer = Arc::clone(&self.observer);
        let item_id = item_id.to_string();
        let path = PathBuf::from(path);
        let outcome = self
            .run(move || {
                let bytes = std::fs::read(&path)?;
                if !paper_fetch::is_pdf(&bytes) {
                    return Err(PaperError::NotPdf {
                        path: path.display().to_string(),
                    });
                }
                let row = library.add_pdf(&item_id, &bytes, None)?;
                notify(&observer, |observer| {
                    observer.items_changed(vec![item_id.clone()])
                });
                Ok(row)
            })
            .await;
        match outcome {
            Ok(outcome) => outcome,
            Err(PoolError::Closed) => Err(PaperError::Closed),
            Err(PoolError::Panicked(message)) => Err(PaperError::Io { message }),
        }
    }

    /// Where an attachment's file is and where the reader left it.
    pub fn attachment_file(&self, id: &str) -> Result<AttachmentFile> {
        self.ensure_open()?;
        let row = self.library.attachment(id)?;
        Ok(AttachmentFile {
            path: self.library.attachment_path(&row).display().to_string(),
            id: row.id,
            item_id: row.item_id,
            last_page: row.last_page,
            last_zoom: row.last_zoom,
        })
    }

    /// Where the first figure of a PDF attachment is: the region above its
    /// "Figure 1" caption in the first pages, or else its largest embedded
    /// image. `None` when the PDF has neither. Reading the file takes a
    /// second or two for a large PDF, so the app caches what it draws from
    /// the answer.
    pub async fn first_figure(&self, attachment_id: &str) -> Result<Option<Figure>> {
        self.ensure_open()?;
        let row = self.library.attachment(attachment_id)?;
        let path = self.library.attachment_path(&row);
        let outcome = self
            .run(move || {
                let bytes = std::fs::read(&path)?;
                let Ok(file) = paper_identify::PdfFile::open(bytes) else {
                    return Ok(None);
                };
                Ok(file.first_figure().map(|region| Figure {
                    page_index: region.page_index as i32,
                    rect: PageRect {
                        x0: region.rect.x0.into(),
                        y0: region.rect.y0.into(),
                        x1: region.rect.x1.into(),
                        y1: region.rect.y1.into(),
                    },
                    source: match region.source {
                        paper_identify::FigureSource::Caption => FigureSource::Caption,
                        paper_identify::FigureSource::LargestImage => FigureSource::LargestImage,
                    },
                }))
            })
            .await;
        match outcome {
            Ok(outcome) => outcome,
            Err(PoolError::Closed) => Err(PaperError::Closed),
            Err(PoolError::Panicked(message)) => Err(PaperError::Io { message }),
        }
    }

    /// Remembers the reader's page (0 based) and zoom for an attachment. The
    /// first call takes the item out of the Unread list, so only that one
    /// tells the observer.
    pub fn set_reading_position(&self, id: &str, page: i32, zoom: f64) -> Result<()> {
        self.ensure_open()?;
        let before = self.library.attachment(id)?;
        self.library.set_reading_position(id, page, zoom)?;
        if before.last_page.is_none() {
            self.notify(|observer| observer.items_changed(vec![before.item_id.clone()]));
        }
        Ok(())
    }

    // ----- annotations -----

    /// The highlights and notes of an attachment in reading order.
    /// Synchronous: the reader calls it once when a PDF opens.
    pub fn annotations(&self, attachment_id: &str) -> Result<Vec<Annotation>> {
        self.ensure_open()?;
        self.library.annotations(attachment_id)
    }

    /// Stores a highlight or note. Its text joins the item's search index, so
    /// the observer hears that the item changed.
    pub fn create_annotation(&self, new: &NewAnnotation) -> Result<Annotation> {
        self.ensure_open()?;
        let annotation = self.library.create_annotation(new)?;
        self.annotations_changed(&annotation.attachment_id);
        Ok(annotation)
    }

    pub fn update_annotation(&self, id: &str, patch: &AnnotationPatch) -> Result<Annotation> {
        self.ensure_open()?;
        let annotation = self.library.update_annotation(id, patch)?;
        self.annotations_changed(&annotation.attachment_id);
        Ok(annotation)
    }

    pub fn delete_annotation(&self, id: &str) -> Result<()> {
        self.ensure_open()?;
        let annotation = self.library.annotation(id)?;
        self.library.delete_annotation(id)?;
        self.annotations_changed(&annotation.attachment_id);
        Ok(())
    }

    fn annotations_changed(&self, attachment_id: &str) {
        if let Ok(attachment) = self.library.attachment(attachment_id) {
            self.notify(|observer| observer.items_changed(vec![attachment.item_id]));
        }
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

    /// Marks papers as favorites, which every list shows first, or clears
    /// the mark.
    pub fn set_favorite(&self, item_ids: &[String], favorite: bool) -> Result<()> {
        self.ensure_open()?;
        let changed = self.library.set_favorite(item_ids, favorite)?;
        if !changed.is_empty() {
            self.notify(|observer| observer.items_changed(changed));
        }
        Ok(())
    }

    pub fn set_tag_color(&self, tag_id: &str, color: Option<&str>) -> Result<()> {
        self.ensure_open()?;
        self.library.set_tag_color(tag_id, color)?;
        self.notify(|observer| observer.tags_changed());
        Ok(())
    }

    // ----- topics -----

    /// The library's topic list, which automatic tags come from.
    pub fn topics(&self) -> Result<Vec<Topic>> {
        self.ensure_open()?;
        self.library.topics()
    }

    /// Replaces the topic list. Tags already given stay on their papers.
    pub fn set_topics(&self, topics: &[Topic]) -> Result<()> {
        self.ensure_open()?;
        self.library.set_topics(topics)
    }

    /// Asks Jev again which topics each paper is about and adds the ones it
    /// lacks as tags, even to papers tagged before. Returns the papers that
    /// got tags. Fails with `Settings` when no Jev API key is set or TypeSafe
    /// refuses it, and with the first error when no paper could be asked.
    pub async fn tag_with_topics(&self, item_ids: &[String]) -> Result<Vec<TopicTags>> {
        self.ensure_open()?;
        let Some(job) = self.topic_job() else {
            return Err(PaperError::Settings {
                message: "Add a Jev API key in Settings to tag papers with topics.".to_string(),
            });
        };
        // Queued together, so the pool's threads ask about several papers at once.
        let runs: Vec<_> = item_ids
            .iter()
            .map(|id| {
                let job = job.clone();
                let id = id.clone();
                self.run(move || job.run(&id).map(|tags| TopicTags { item_id: id, tags }))
            })
            .collect();
        let mut tagged = Vec::new();
        let mut asked = 0;
        let mut first_error = None;
        for run in runs {
            match run.await {
                Ok(Ok(result)) => {
                    asked += 1;
                    if !result.tags.is_empty() {
                        tagged.push(result);
                    }
                }
                Ok(Err(error @ PaperError::Settings { .. })) => return Err(error),
                Ok(Err(error)) => {
                    tracing::warn!(%error, "could not tag a paper with topics");
                    first_error.get_or_insert(error);
                }
                Err(PoolError::Closed) => return Err(PaperError::Closed),
                Err(PoolError::Panicked(message)) => {
                    first_error.get_or_insert(PaperError::Network { message });
                }
            }
        }
        match first_error {
            Some(error) if asked == 0 => Err(error),
            _ => Ok(tagged),
        }
    }

    /// Tags a paper that was just added, once: a paper Jev was asked about
    /// before keeps the tags the user left it. Runs on its own and reports
    /// through `topics_added`.
    fn start_topic_tags(&self, item_id: &str) {
        let Some(job) = self.topic_job() else { return };
        match self.library.has_source_record(item_id, JEV_SOURCE) {
            Ok(false) => {}
            Ok(true) => return,
            Err(error) => {
                tracing::warn!(%error, "could not check for earlier topics");
                return;
            }
        }
        let item_id = item_id.to_string();
        drop(self.run(move || match job.run(&item_id) {
            Ok(tags) if !tags.is_empty() => {
                notify(&job.observer, |o| o.topics_added(item_id.clone(), tags));
            }
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, %item_id, "could not tag the paper with topics"),
        }));
    }

    /// The tagging job, or `None` without a Jev API key.
    fn topic_job(&self) -> Option<TopicJob> {
        let settings = self.settings.settings();
        let jev_api_key = settings.jev_api_key.trim().to_string();
        if jev_api_key.is_empty() {
            return None;
        }
        Some(TopicJob {
            library: Arc::clone(&self.library),
            observer: Arc::clone(&self.observer),
            resolver: Arc::clone(&self.resolver),
            contact: Contact::new(settings.contact_email),
            openalex_api_key: Some(settings.openalex_api_key).filter(|key| !key.is_empty()),
            jev_api_key,
        })
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

    // ----- citations -----

    /// BibTeX entries for the items, sorted by citation key, ready for the
    /// clipboard or a `.bib` file. An item without a key gets its key now and
    /// keeps it; one whose lookup is still running is cited under the key it
    /// would get, without storing it.
    pub fn bibtex(&self, ids: &[String]) -> Result<String> {
        self.ensure_open()?;
        Ok(bibtex_of(&self.cited_items(ids)?))
    }

    /// The items' reference list entries in `style`, in the style's order,
    /// as plain text and as HTML. The style's processor runs on an engine
    /// thread: loading a style takes a moment the first time.
    pub async fn citation(&self, ids: &[String], style: CitationStyle) -> Result<Citation> {
        self.ensure_open()?;
        let items = self.cited_items(ids)?;
        let outcome = self
            .run(move || {
                let cited: Vec<paper_cite::CiteItem<'_>> = items
                    .iter()
                    .map(|(key, fields)| paper_cite::CiteItem { key, fields })
                    .collect();
                paper_cite::citation(&cited, style)
            })
            .await;
        match outcome {
            Ok(citation) => Ok(citation),
            Err(PoolError::Closed) => Err(PaperError::Closed),
            Err(PoolError::Panicked(message)) => Err(PaperError::Library { message }),
        }
    }

    /// Writes `bibtex(ids)` to a `.bib` file at `path`, replacing it, and
    /// returns how many entries it holds.
    pub async fn export_bib(&self, ids: &[String], path: &str) -> Result<u32> {
        self.ensure_open()?;
        let items = self.cited_items(ids)?;
        let bibtex = bibtex_of(&items);
        let count = items.len() as u32;
        let path = PathBuf::from(path);
        let outcome = self
            .run(move || -> Result<u32> {
                std::fs::write(&path, bibtex)?;
                Ok(count)
            })
            .await;
        match outcome {
            Ok(outcome) => outcome,
            Err(PoolError::Closed) => Err(PaperError::Closed),
            Err(PoolError::Panicked(message)) => Err(PaperError::Io { message }),
        }
    }

    /// Each item's key and fields, sorted by key, giving keys to items that
    /// have finished their lookup and have none yet.
    fn cited_items(&self, ids: &[String]) -> Result<Vec<(String, ItemFields)>> {
        let mut unique = HashSet::new();
        let ids: Vec<String> = ids
            .iter()
            .filter(|id| unique.insert(id.as_str()))
            .cloned()
            .collect();
        let mut waiting = Vec::new();
        for id in &ids {
            if self.library.item(id)?.lookup_status != LookupStatus::Pending {
                waiting.push(id.clone());
            }
        }
        let given = give_citation_keys(&self.library, &waiting);
        if !given.is_empty() {
            self.notify(|observer| observer.items_changed(given));
        }
        let mut items = Vec::with_capacity(ids.len());
        for id in &ids {
            let detail = self.library.item(id)?;
            let key = detail
                .citation_key
                .unwrap_or_else(|| paper_cite::key_base(&detail.fields));
            items.push((key, detail.fields));
        }
        items.sort_by_key(|(key, _)| key.to_lowercase());
        Ok(items)
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

    /// The item ids of this library's pinned reader tabs, in tab order. They
    /// are machine settings like the window layout, kept per library.
    pub fn pinned_tabs(&self) -> Vec<String> {
        self.settings.pinned_tabs(self.library.dir())
    }

    pub fn set_pinned_tabs(&self, item_ids: Vec<String>) -> Result<()> {
        self.settings.set_pinned_tabs(self.library.dir(), item_ids)
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
    ) -> impl Future<Output = std::result::Result<T, PoolError>> + Send + use<T, F> {
        let pool = self.pool.lock().unwrap();
        let future = pool.as_ref().map(|pool| pool.run(job));
        async move {
            match future {
                Some(future) => future.await,
                None => Err(PoolError::Closed),
            }
        }
    }

    fn notify(&self, call: impl FnOnce(&dyn EngineObserver)) {
        notify(&self.observer, call);
    }
}

/// The canonical form of a DOI the user typed; `None` for nothing typed.
fn normalized_doi(text: Option<&str>) -> Result<Option<String>> {
    match text.map(str::trim).filter(|text| !text.is_empty()) {
        None => Ok(None),
        Some(text) => {
            paper_ids::normalize_doi(text)
                .map(Some)
                .ok_or_else(|| PaperError::InvalidIdentifier {
                    text: text.to_string(),
                })
        }
    }
}

/// The canonical form of an arXiv ID the user typed; `None` for nothing typed.
fn normalized_arxiv_id(text: Option<&str>) -> Result<Option<String>> {
    match text.map(str::trim).filter(|text| !text.is_empty()) {
        None => Ok(None),
        Some(text) => paper_ids::normalize_arxiv_id(text)
            .map(Some)
            .ok_or_else(|| PaperError::InvalidIdentifier {
                text: text.to_string(),
            }),
    }
}

/// Drops an item that is live again from the `undo_delete` batches.
fn forget_undo(undo: &UndoBatches, item_id: &str) {
    let mut undo = undo.lock().unwrap();
    for batch in undo.iter_mut() {
        batch.retain(|id| id != item_id);
    }
    undo.retain(|batch| !batch.is_empty());
}

fn bibtex_of(items: &[(String, ItemFields)]) -> String {
    let cited: Vec<paper_cite::CiteItem<'_>> = items
        .iter()
        .map(|(key, fields)| paper_cite::CiteItem { key, fields })
        .collect();
    paper_cite::bibtex(&cited)
}

/// Gives each of these items that has no citation key one made from its
/// metadata, and returns the ids that got one. Keys are a convenience next to
/// the work that triggers them, so a failure is logged, not returned.
fn give_citation_keys(library: &Library, ids: &[String]) -> Vec<String> {
    let mut bases = Vec::new();
    for id in ids {
        match library.item(id) {
            Ok(detail) if detail.citation_key.is_none() => {
                bases.push((id.clone(), paper_cite::key_base(&detail.fields)));
            }
            // Deleted items get theirs when they are back and cited.
            Ok(_) | Err(PaperError::NotFound { .. }) => {}
            Err(error) => {
                tracing::warn!(%error, item_id = %id, "could not read an item for its citation key")
            }
        }
    }
    if let Err(error) = library.assign_citation_keys(&bases) {
        tracing::warn!(%error, "could not give citation keys");
        return Vec::new();
    }
    bases.into_iter().map(|(id, _)| id).collect()
}

fn notify(observer: &SharedObserver, call: impl FnOnce(&dyn EngineObserver)) {
    // Clone the observer out so the lock is not held during the callback.
    let observer = observer.lock().unwrap().clone();
    if let Some(observer) = observer {
        call(observer.as_ref());
    }
}

/// The part of a lookup that runs on an engine thread: the network call, the
/// database writes, and the events. Nothing here runs on the caller's thread.
#[derive(Clone)]
struct LookupJob {
    library: Arc<Library>,
    observer: SharedObserver,
    item_id: String,
    doi: String,
}

impl LookupJob {
    fn run(
        &self,
        result: std::result::Result<paper_fetch::Lookup, FetchError>,
        placeholder: bool,
    ) -> Result<ItemRow> {
        notify(&self.observer, |observer| {
            observer.lookup_progress(self.item_id.clone(), LookupStatus::Pending)
        });
        let outcome = match result {
            Ok(lookup) => self
                .library
                .replace_metadata(&self.item_id, &lookup.fields, LookupStatus::Done)
                .and_then(|()| {
                    for record in &lookup.records {
                        self.library.put_source_record(
                            &self.item_id,
                            &record.source,
                            &record.json,
                        )?;
                    }
                    Ok(())
                }),
            Err(FetchError::NotFound { doi }) => Err(PaperError::MetadataNotFound {
                doi,
                item_id: self.item_id.clone(),
            }),
            Err(error) => Err(PaperError::Lookup {
                doi: self.doi.clone(),
                item_id: self.item_id.clone(),
                message: error.to_string(),
            }),
        };
        match outcome {
            Ok(()) => {
                give_citation_keys(&self.library, std::slice::from_ref(&self.item_id));
                notify(&self.observer, |observer| {
                    observer.lookup_progress(self.item_id.clone(), LookupStatus::Done)
                });
                notify(&self.observer, |observer| {
                    observer.items_changed(vec![self.item_id.clone()])
                });
                self.library.item_row(&self.item_id)
            }
            // The source answered with a DOI that another item already has
            // (an alias of the typed one). A placeholder for it is junk.
            Err(error @ PaperError::Duplicate { .. }) if placeholder => {
                tracing::warn!(doi = %self.doi, %error, "lookup found an existing item");
                self.library.purge_item(&self.item_id)?;
                notify(&self.observer, |observer| {
                    observer.items_changed(vec![self.item_id.clone()])
                });
                Err(error)
            }
            Err(error) => self.fail(error),
        }
    }

    /// Leaves the item in the `Failed` state for `retry_lookup` and returns `error`.
    fn fail(&self, error: PaperError) -> Result<ItemRow> {
        tracing::warn!(doi = %self.doi, %error, "lookup failed");
        if let Err(status_error) = self
            .library
            .set_lookup_status(&self.item_id, LookupStatus::Failed)
        {
            tracing::warn!(item_id = %self.item_id, %status_error, "could not mark the lookup failed");
        }
        notify(&self.observer, |observer| {
            observer.lookup_progress(self.item_id.clone(), LookupStatus::Failed)
        });
        notify(&self.observer, |observer| {
            observer.items_changed(vec![self.item_id.clone()])
        });
        Err(error)
    }
}

const PDF_MIME: &str = "application/pdf";

/// The shortest time between two download progress events for one search.
pub const PDF_STEP_INTERVAL: Duration = Duration::from_millis(50);

enum PdfSearch {
    /// The item has a PDF already.
    Attached(AttachmentRow),
    /// The search is queued; the future resolves with the attached PDF.
    Running(BoxFuture<Result<AttachmentRow>>),
}

/// The part of a PDF search that runs on an engine thread: the requests, the
/// download, the file write, and the events.
#[derive(Clone)]
struct PdfJob {
    library: Arc<Library>,
    observer: SharedObserver,
    searches: PdfSearches,
    item_id: String,
}

impl PdfJob {
    fn run(
        &self,
        resolver: &Resolver,
        query: &PdfQuery,
        contact: &Contact,
    ) -> Result<AttachmentRow> {
        let mut last_download: Option<Instant> = None;
        let search = resolver.find_pdf(query, contact, &mut |step| {
            if let PdfStep::Downloading {
                received_bytes,
                total_bytes,
                ..
            } = &step
            {
                let finished = Some(*received_bytes) == *total_bytes;
                let now = Instant::now();
                if !finished && last_download.is_some_and(|last| now - last < PDF_STEP_INTERVAL) {
                    return;
                }
                last_download = Some(now);
            }
            notify(&self.observer, |observer| {
                observer.pdf_step(self.item_id.clone(), step)
            });
        });
        for record in &search.records {
            if let Err(error) =
                self.library
                    .put_source_record(&self.item_id, &record.source, &record.json)
            {
                tracing::warn!(%error, source = record.source, "could not keep a source record");
            }
        }
        let outcome = match search.result {
            Ok(found) => self
                .library
                .add_pdf(&self.item_id, &found.bytes, Some(&found.url)),
            Err(FetchError::NoPdf { .. }) => Err(PaperError::PdfNotFound {
                item_id: self.item_id.clone(),
            }),
            Err(error) => Err(PaperError::Network {
                message: error.to_string(),
            }),
        };
        self.finish();
        match &outcome {
            Ok(_) => {
                self.report(PdfStatus::Found);
                notify(&self.observer, |observer| {
                    observer.items_changed(vec![self.item_id.clone()])
                });
            }
            Err(PaperError::PdfNotFound { .. }) => self.report(PdfStatus::NotFound),
            Err(error) => {
                tracing::warn!(item_id = %self.item_id, %error, "PDF search failed");
                self.report(PdfStatus::Failed);
            }
        }
        outcome
    }

    fn report(&self, status: PdfStatus) {
        notify(&self.observer, |observer| {
            observer.pdf_progress(self.item_id.clone(), status)
        });
    }

    /// Lets a later search for the same item start.
    fn finish(&self) {
        self.searches.lock().unwrap().remove(&self.item_id);
    }
}

/// The canonical form of user input that `add_by_identifier` accepts
/// (`10.1038/nature14539` or `arXiv:1706.03762`), or `None` when the text is
/// neither a DOI nor an arXiv ID and should be treated as a search.
pub fn parse_identifier(text: &str) -> Option<String> {
    Identifier::parse(text).map(|identifier| identifier.to_string())
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

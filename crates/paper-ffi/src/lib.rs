//! UniFFI exports of `paper-engine`.
//!
//! This crate only translates. Records and enums are the engine's own types,
//! described to UniFFI with `remote` so the engine stays free of FFI code.
//! `Engine` and `EngineObserver` are thin wrappers, because UniFFI needs an
//! object it owns and a trait it can implement in Swift. Method names and
//! argument orders match `paper_engine::Engine` one to one.

use std::sync::Arc;
use std::time::SystemTime;

use paper_engine as engine;
use paper_engine::{
    Annotation, AnnotationKind, AnnotationPatch, AttachmentFile, AttachmentRow, Citation,
    CitationStyle, CollectionNode, Creator, CreatorRole, Figure, FigureSource, Identification,
    ImportedPdf, ItemDetail, ItemFields, ItemKind, ItemQuery, ItemRow, LookupStatus, MetadataPatch,
    NewAnnotation, PagePoint, PageRect, PaperError, PdfStatus, PdfStep, Quad, Scope, Settings,
    SortColumn, Tag, Topic, TopicTags, WindowState,
};

uniffi::setup_scaffolding!();

// ----- enums -----

#[uniffi::remote(Enum)]
pub enum ItemKind {
    JournalArticle,
    ConferencePaper,
    Preprint,
    Other,
}

#[uniffi::remote(Enum)]
pub enum LookupStatus {
    Pending,
    Done,
    Failed,
}

#[uniffi::remote(Enum)]
pub enum PdfStatus {
    Searching,
    Found,
    NotFound,
    Failed,
}

#[uniffi::remote(Enum)]
pub enum PdfStep {
    Checking {
        source: String,
    },
    Downloading {
        source: String,
        received_bytes: u64,
        total_bytes: Option<u64>,
    },
}

#[uniffi::remote(Enum)]
pub enum Identification {
    FileText,
    TitleSearch,
    Model,
    NotFound,
    Failed { message: String },
}

#[uniffi::remote(Enum)]
pub enum FigureSource {
    Caption,
    LargestImage,
}

#[uniffi::remote(Enum)]
pub enum CreatorRole {
    Author,
    Editor,
}

#[uniffi::remote(Enum)]
pub enum Scope {
    All,
    Unread,
    RecentlyAdded,
    NoPdf,
    Collection { id: String },
    Tag { id: String },
}

#[uniffi::remote(Enum)]
pub enum SortColumn {
    Title,
    FirstAuthor,
    Journal,
    Year,
    Added,
}

#[uniffi::remote(Enum)]
pub enum AnnotationKind {
    Highlight,
    Note,
}

#[uniffi::remote(Enum)]
pub enum CitationStyle {
    Apa,
}

#[uniffi::remote(Error)]
pub enum PaperError {
    InvalidIdentifier {
        text: String,
    },
    Duplicate {
        doi: String,
        item_id: String,
    },
    NotFound {
        id: String,
    },
    Library {
        message: String,
    },
    Database {
        message: String,
    },
    Io {
        message: String,
    },
    Network {
        message: String,
    },
    MetadataNotFound {
        doi: String,
        item_id: String,
    },
    Lookup {
        doi: String,
        item_id: String,
        message: String,
    },
    PdfNotFound {
        item_id: String,
    },
    NotPdf {
        path: String,
    },
    Settings {
        message: String,
    },
    Closed,
}

// ----- records -----

#[uniffi::remote(Record)]
pub struct Creator {
    pub role: Option<CreatorRole>,
    pub family: Option<String>,
    pub given: Option<String>,
    pub literal: Option<String>,
    pub orcid: Option<String>,
}

#[uniffi::remote(Record)]
pub struct ItemFields {
    pub kind: Option<ItemKind>,
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    pub title: String,
    pub creators: Vec<Creator>,
    pub container_title: Option<String>,
    pub container_short: Option<String>,
    pub publisher: Option<String>,
    pub year: Option<i32>,
    pub published_date: Option<String>,
    pub volume: Option<String>,
    pub issue: Option<String>,
    pub pages: Option<String>,
    pub abstract_text: Option<String>,
    pub url: Option<String>,
    pub issn: Option<String>,
    pub language: Option<String>,
    pub extra: Option<String>,
}

#[uniffi::remote(Record)]
pub struct MetadataPatch {
    pub kind: Option<ItemKind>,
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    pub title: Option<String>,
    pub creators: Option<Vec<Creator>>,
    pub container_title: Option<String>,
    pub container_short: Option<String>,
    pub publisher: Option<String>,
    pub year: Option<i32>,
    pub published_date: Option<String>,
    pub volume: Option<String>,
    pub issue: Option<String>,
    pub pages: Option<String>,
    pub abstract_text: Option<String>,
    pub url: Option<String>,
    pub citation_key: Option<String>,
}

#[uniffi::remote(Record)]
pub struct ItemRow {
    pub id: String,
    pub kind: ItemKind,
    pub title: String,
    pub authors: String,
    pub journal: Option<String>,
    pub year: Option<i32>,
    pub doi: Option<String>,
    pub has_pdf: bool,
    pub favorite: bool,
    pub lookup_status: LookupStatus,
    pub added_at: SystemTime,
}

#[uniffi::remote(Record)]
pub struct AttachmentRow {
    pub id: String,
    pub item_id: String,
    pub relative_path: String,
    pub mime: String,
    pub byte_size: i64,
    pub page_count: Option<i32>,
    pub last_page: Option<i32>,
    pub last_zoom: Option<f64>,
    pub added_at: SystemTime,
}

#[uniffi::remote(Record)]
pub struct ImportedPdf {
    pub item: ItemRow,
    pub identification: Identification,
}

#[uniffi::remote(Record)]
pub struct AttachmentFile {
    pub id: String,
    pub item_id: String,
    pub path: String,
    pub last_page: Option<i32>,
    pub last_zoom: Option<f64>,
}

#[uniffi::remote(Record)]
pub struct Tag {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
    pub item_count: i64,
}

#[uniffi::remote(Record)]
pub struct Topic {
    pub name: String,
    pub description: String,
}

#[uniffi::remote(Record)]
pub struct TopicTags {
    pub item_id: String,
    pub tags: Vec<String>,
}

#[uniffi::remote(Record)]
pub struct ItemDetail {
    pub id: String,
    pub fields: ItemFields,
    pub lookup_status: LookupStatus,
    pub citation_key: Option<String>,
    pub tags: Vec<Tag>,
    pub collection_ids: Vec<String>,
    pub attachments: Vec<AttachmentRow>,
    pub note: Option<String>,
    pub added_at: SystemTime,
    pub modified_at: SystemTime,
}

#[uniffi::remote(Record)]
pub struct ItemQuery {
    pub text: String,
    pub scope: Scope,
    pub sort: SortColumn,
    pub ascending: bool,
}

#[uniffi::remote(Record)]
pub struct CollectionNode {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub position: i64,
    pub depth: i64,
    pub item_count: i64,
}

#[uniffi::remote(Record)]
pub struct PagePoint {
    pub x: f64,
    pub y: f64,
}

#[uniffi::remote(Record)]
pub struct Quad {
    pub top_left: PagePoint,
    pub top_right: PagePoint,
    pub bottom_left: PagePoint,
    pub bottom_right: PagePoint,
}

#[uniffi::remote(Record)]
pub struct PageRect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

#[uniffi::remote(Record)]
pub struct Figure {
    pub page_index: i32,
    pub rect: PageRect,
    pub source: FigureSource,
}

#[uniffi::remote(Record)]
pub struct Annotation {
    pub id: String,
    pub attachment_id: String,
    pub kind: AnnotationKind,
    pub page_index: i32,
    pub quads: Vec<Quad>,
    pub rect: PageRect,
    pub color: String,
    pub selected_text: Option<String>,
    pub comment: Option<String>,
    pub created_at: SystemTime,
    pub modified_at: SystemTime,
}

#[uniffi::remote(Record)]
pub struct NewAnnotation {
    pub attachment_id: String,
    pub kind: AnnotationKind,
    pub page_index: i32,
    pub quads: Vec<Quad>,
    pub rect: Option<PageRect>,
    pub color: String,
    pub selected_text: Option<String>,
    pub comment: Option<String>,
    pub text_before: Option<String>,
    pub text_after: Option<String>,
}

#[uniffi::remote(Record)]
pub struct AnnotationPatch {
    pub color: Option<String>,
    pub comment: Option<String>,
}

#[uniffi::remote(Record)]
pub struct Citation {
    pub text: String,
    pub html: String,
}

#[uniffi::remote(Record)]
pub struct WindowState {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[uniffi::remote(Record)]
pub struct Settings {
    pub contact_email: String,
    pub openalex_api_key: String,
    pub anthropic_api_key: String,
    pub jev_api_key: String,
    pub window: Option<WindowState>,
    pub sidebar_width: Option<f64>,
    pub detail_width: Option<f64>,
    pub sidebar_collapsed: bool,
    pub detail_collapsed: bool,
}

// ----- observer -----

/// Implemented in Swift. Called on engine threads.
#[uniffi::export(with_foreign)]
pub trait EngineObserver: Send + Sync {
    fn items_changed(&self, ids: Vec<String>);
    fn collections_changed(&self);
    fn tags_changed(&self);
    fn lookup_progress(&self, item_id: String, status: LookupStatus);
    fn pdf_progress(&self, item_id: String, status: PdfStatus);
    fn pdf_step(&self, item_id: String, step: PdfStep);
    fn topics_added(&self, item_id: String, tags: Vec<String>);
}

/// Passes engine events to the Swift observer.
struct ObserverAdapter(Arc<dyn EngineObserver>);

impl engine::EngineObserver for ObserverAdapter {
    fn items_changed(&self, ids: Vec<String>) {
        self.0.items_changed(ids);
    }

    fn collections_changed(&self) {
        self.0.collections_changed();
    }

    fn tags_changed(&self) {
        self.0.tags_changed();
    }

    fn lookup_progress(&self, item_id: String, status: LookupStatus) {
        self.0.lookup_progress(item_id, status);
    }

    fn pdf_progress(&self, item_id: String, status: PdfStatus) {
        self.0.pdf_progress(item_id, status);
    }

    fn pdf_step(&self, item_id: String, step: PdfStep) {
        self.0.pdf_step(item_id, step);
    }

    fn topics_added(&self, item_id: String, tags: Vec<String>) {
        self.0.topics_added(item_id, tags);
    }
}

// ----- free functions -----

/// The canonical DOI or arXiv ID in `text`, or `None` when it is neither.
#[uniffi::export]
pub fn parse_identifier(text: String) -> Option<String> {
    engine::parse_identifier(&text)
}

/// Whether the folder holds a Paper library.
#[uniffi::export]
pub fn is_library(path: String) -> bool {
    engine::is_library(path)
}

/// Libraries opened on this machine that still exist, most recent first.
#[uniffi::export]
pub fn recent_libraries() -> Vec<String> {
    engine::recent_libraries()
}

/// Days covered by `Scope::RecentlyAdded`.
#[uniffi::export]
pub fn recently_added_days() -> i64 {
    engine::RECENTLY_ADDED_DAYS
}

// ----- engine -----

#[derive(uniffi::Object)]
pub struct Engine(engine::Engine);

#[uniffi::export]
impl Engine {
    #[uniffi::constructor]
    pub fn open_library(path: String) -> Result<Arc<Self>, PaperError> {
        engine::Engine::open_library(path).map(|engine| Arc::new(Self(engine)))
    }

    #[uniffi::constructor]
    pub fn create_library(path: String) -> Result<Arc<Self>, PaperError> {
        engine::Engine::create_library(path).map(|engine| Arc::new(Self(engine)))
    }

    pub fn library_path(&self) -> String {
        self.0.library_path().display().to_string()
    }

    pub fn close(&self) {
        self.0.close();
    }

    pub fn set_observer(&self, observer: Arc<dyn EngineObserver>) {
        self.0.set_observer(Arc::new(ObserverAdapter(observer)));
    }

    // items

    pub fn list_items(&self, query: ItemQuery) -> Result<Vec<ItemRow>, PaperError> {
        self.0.list_items(&query)
    }

    pub fn item_detail(&self, id: String) -> Result<ItemDetail, PaperError> {
        self.0.item_detail(&id)
    }

    pub async fn add_by_identifier(&self, text: String) -> Result<ItemRow, PaperError> {
        self.0.add_by_identifier(&text).await
    }

    pub async fn retry_lookup(&self, item_id: String) -> Result<ItemRow, PaperError> {
        self.0.retry_lookup(&item_id).await
    }

    pub async fn import_pdf(&self, path: String) -> Result<ImportedPdf, PaperError> {
        self.0.import_pdf(&path).await
    }

    pub fn create_item(&self, fields: ItemFields) -> Result<ItemRow, PaperError> {
        self.0.create_item(&fields)
    }

    pub fn update_metadata(&self, id: String, patch: MetadataPatch) -> Result<(), PaperError> {
        self.0.update_metadata(&id, &patch)
    }

    pub fn delete_items(&self, ids: Vec<String>) -> Result<(), PaperError> {
        self.0.delete_items(&ids)
    }

    pub fn undo_delete(&self) -> Result<Vec<String>, PaperError> {
        self.0.undo_delete()
    }

    pub fn can_undo_delete(&self) -> bool {
        self.0.can_undo_delete()
    }

    // attachments

    pub async fn find_pdf(&self, item_id: String) -> Result<AttachmentRow, PaperError> {
        self.0.find_pdf(&item_id).await
    }

    pub async fn attach_file(
        &self,
        item_id: String,
        path: String,
    ) -> Result<AttachmentRow, PaperError> {
        self.0.attach_file(&item_id, &path).await
    }

    pub fn attachment_file(&self, id: String) -> Result<AttachmentFile, PaperError> {
        self.0.attachment_file(&id)
    }

    pub async fn first_figure(&self, attachment_id: String) -> Result<Option<Figure>, PaperError> {
        self.0.first_figure(&attachment_id).await
    }

    pub fn set_reading_position(&self, id: String, page: i32, zoom: f64) -> Result<(), PaperError> {
        self.0.set_reading_position(&id, page, zoom)
    }

    // annotations

    pub fn annotations(&self, attachment_id: String) -> Result<Vec<Annotation>, PaperError> {
        self.0.annotations(&attachment_id)
    }

    pub fn create_annotation(&self, new: NewAnnotation) -> Result<Annotation, PaperError> {
        self.0.create_annotation(&new)
    }

    pub fn update_annotation(
        &self,
        id: String,
        patch: AnnotationPatch,
    ) -> Result<Annotation, PaperError> {
        self.0.update_annotation(&id, &patch)
    }

    pub fn delete_annotation(&self, id: String) -> Result<(), PaperError> {
        self.0.delete_annotation(&id)
    }

    // collections

    pub fn collection_tree(&self) -> Result<Vec<CollectionNode>, PaperError> {
        self.0.collection_tree()
    }

    pub fn create_collection(
        &self,
        name: String,
        parent_id: Option<String>,
    ) -> Result<String, PaperError> {
        self.0.create_collection(&name, parent_id.as_deref())
    }

    pub fn rename_collection(&self, id: String, name: String) -> Result<(), PaperError> {
        self.0.rename_collection(&id, &name)
    }

    pub fn move_collection(
        &self,
        id: String,
        parent_id: Option<String>,
        position: i64,
    ) -> Result<(), PaperError> {
        self.0.move_collection(&id, parent_id.as_deref(), position)
    }

    pub fn delete_collection(&self, id: String) -> Result<(), PaperError> {
        self.0.delete_collection(&id)
    }

    pub fn add_to_collection(
        &self,
        collection_id: String,
        item_ids: Vec<String>,
    ) -> Result<(), PaperError> {
        self.0.add_to_collection(&collection_id, &item_ids)
    }

    pub fn remove_from_collection(
        &self,
        collection_id: String,
        item_ids: Vec<String>,
    ) -> Result<(), PaperError> {
        self.0.remove_from_collection(&collection_id, &item_ids)
    }

    // tags

    pub fn tags(&self) -> Result<Vec<Tag>, PaperError> {
        self.0.tags()
    }

    pub fn set_tags(&self, item_id: String, names: Vec<String>) -> Result<(), PaperError> {
        self.0.set_tags(&item_id, &names)
    }

    pub fn set_favorite(&self, item_ids: Vec<String>, favorite: bool) -> Result<(), PaperError> {
        self.0.set_favorite(&item_ids, favorite)
    }

    pub fn set_tag_color(&self, tag_id: String, color: Option<String>) -> Result<(), PaperError> {
        self.0.set_tag_color(&tag_id, color.as_deref())
    }

    // topics

    pub fn topics(&self) -> Result<Vec<Topic>, PaperError> {
        self.0.topics()
    }

    pub fn set_topics(&self, topics: Vec<Topic>) -> Result<(), PaperError> {
        self.0.set_topics(&topics)
    }

    pub async fn tag_with_topics(
        &self,
        item_ids: Vec<String>,
    ) -> Result<Vec<TopicTags>, PaperError> {
        self.0.tag_with_topics(&item_ids).await
    }

    // notes

    pub fn note(&self, item_id: String) -> Result<Option<String>, PaperError> {
        self.0.note(&item_id)
    }

    pub fn save_note(&self, item_id: String, markdown: String) -> Result<(), PaperError> {
        self.0.save_note(&item_id, &markdown)
    }

    // citations

    pub fn bibtex(&self, ids: Vec<String>) -> Result<String, PaperError> {
        self.0.bibtex(&ids)
    }

    pub async fn citation(
        &self,
        ids: Vec<String>,
        style: CitationStyle,
    ) -> Result<Citation, PaperError> {
        self.0.citation(&ids, style).await
    }

    pub async fn export_bib(&self, ids: Vec<String>, path: String) -> Result<u32, PaperError> {
        self.0.export_bib(&ids, &path).await
    }

    // settings

    pub fn settings(&self) -> Settings {
        self.0.settings()
    }

    pub fn update_settings(&self, settings: Settings) -> Result<(), PaperError> {
        self.0.update_settings(settings)
    }

    pub fn recent_libraries(&self) -> Vec<String> {
        self.0.recent_libraries()
    }

    pub fn pinned_tabs(&self) -> Vec<String> {
        self.0.pinned_tabs()
    }

    pub fn set_pinned_tabs(&self, item_ids: Vec<String>) -> Result<(), PaperError> {
        self.0.set_pinned_tabs(item_ids)
    }
}

//! Plain data types shared by every crate and mirrored one to one by the FFI.
//!
//! Records hold only strings, numbers, booleans, `SystemTime`, options, and
//! vectors of those, so UniFFI can pass them to Swift unchanged. Timestamps
//! are `SystemTime` here and RFC 3339 text in the database (see `time`).

use std::time::SystemTime;

/// What kind of publication an item is. Closed set, stored as snake_case text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemKind {
    JournalArticle,
    ConferencePaper,
    Preprint,
    Other,
}

impl ItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemKind::JournalArticle => "journal_article",
            ItemKind::ConferencePaper => "conference_paper",
            ItemKind::Preprint => "preprint",
            ItemKind::Other => "other",
        }
    }

    pub fn from_db(text: &str) -> ItemKind {
        match text {
            "journal_article" => ItemKind::JournalArticle,
            "conference_paper" => ItemKind::ConferencePaper,
            "preprint" => ItemKind::Preprint,
            _ => ItemKind::Other,
        }
    }
}

/// Where an item is in the metadata lookup. `Pending` rows show a progress
/// line, `Failed` rows show a retry button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LookupStatus {
    Pending,
    Done,
    Failed,
}

impl LookupStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            LookupStatus::Pending => "pending",
            LookupStatus::Done => "done",
            LookupStatus::Failed => "failed",
        }
    }

    pub fn from_db(text: &str) -> LookupStatus {
        match text {
            "pending" => LookupStatus::Pending,
            "failed" => LookupStatus::Failed,
            _ => LookupStatus::Done,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CreatorRole {
    Author,
    Editor,
}

impl CreatorRole {
    pub fn as_str(self) -> &'static str {
        match self {
            CreatorRole::Author => "author",
            CreatorRole::Editor => "editor",
        }
    }

    pub fn from_db(text: &str) -> CreatorRole {
        match text {
            "editor" => CreatorRole::Editor,
            _ => CreatorRole::Author,
        }
    }
}

/// One author or editor. Either `family` (with an optional `given`) or
/// `literal` (for organizations) is set.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Creator {
    pub role: Option<CreatorRole>,
    pub family: Option<String>,
    pub given: Option<String>,
    pub literal: Option<String>,
    pub orcid: Option<String>,
}

impl Creator {
    pub fn person(given: &str, family: &str) -> Creator {
        Creator {
            role: Some(CreatorRole::Author),
            family: Some(family.to_string()),
            given: Some(given.to_string()),
            ..Creator::default()
        }
    }

    /// The name used in author lists and for sorting: the family name, or the
    /// literal name, or the given name when nothing else is there.
    pub fn sort_name(&self) -> &str {
        self.family
            .as_deref()
            .or(self.literal.as_deref())
            .or(self.given.as_deref())
            .unwrap_or("")
    }

    /// "Given Family" or the literal name.
    pub fn display_name(&self) -> String {
        match (&self.given, &self.family, &self.literal) {
            (Some(given), Some(family), _) => format!("{given} {family}"),
            (None, Some(family), _) => family.clone(),
            (_, None, Some(literal)) => literal.clone(),
            (Some(given), None, None) => given.clone(),
            (None, None, None) => String::new(),
        }
    }
}

/// Every metadata field of an item. Used to insert a new item and to replace
/// the metadata of an existing one after a lookup.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ItemFields {
    pub kind: Option<ItemKind>,
    /// Lowercase, no prefix. Unique among live items.
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    pub title: String,
    pub creators: Vec<Creator>,
    /// Journal or proceedings name.
    pub container_title: Option<String>,
    pub container_short: Option<String>,
    pub publisher: Option<String>,
    pub year: Option<i32>,
    /// `YYYY`, `YYYY-MM`, or `YYYY-MM-DD`.
    pub published_date: Option<String>,
    pub volume: Option<String>,
    pub issue: Option<String>,
    pub pages: Option<String>,
    pub abstract_text: Option<String>,
    pub url: Option<String>,
    pub issn: Option<String>,
    pub language: Option<String>,
    /// JSON for fields with no column yet.
    pub extra: Option<String>,
}

/// Fields the user can edit. `None` leaves a field alone; `Some("")` clears it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
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
}

/// One row of the library table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemRow {
    pub id: String,
    pub kind: ItemKind,
    pub title: String,
    /// "Vaswani", "Vaswani and Shazeer", or "Vaswani et al.".
    pub authors: String,
    /// Short journal name, or the full one if there is no short form.
    pub journal: Option<String>,
    pub year: Option<i32>,
    pub doi: Option<String>,
    pub has_pdf: bool,
    pub lookup_status: LookupStatus,
    pub added_at: SystemTime,
}

/// A file attached to an item.
#[derive(Debug, Clone, PartialEq)]
pub struct AttachmentRow {
    pub id: String,
    pub item_id: String,
    /// Relative to the library's `files/` directory.
    pub relative_path: String,
    pub mime: String,
    pub byte_size: i64,
    pub page_count: Option<i32>,
    pub last_page: Option<i32>,
    pub last_zoom: Option<f64>,
    pub added_at: SystemTime,
}

/// Where the search for an item's open access PDF is. Not stored: the app
/// keeps it per item while it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PdfStatus {
    /// Asking the open access sources and downloading.
    Searching,
    /// A PDF was downloaded and attached.
    Found,
    /// No source offers an open access PDF.
    NotFound,
    /// The search could not finish, usually because the network is down.
    Failed,
}

/// What a running PDF search is doing, for progress in the app. `source` is
/// `arxiv`, `openalex`, `unpaywall`, or `europepmc`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdfStep {
    /// Asking a source whether it has an open access copy.
    Checking { source: String },
    /// Downloading a file a source pointed to. `total_bytes` is known when
    /// the server sent a length.
    Downloading {
        source: String,
        received_bytes: u64,
        total_bytes: Option<u64>,
    },
}

/// How `import_pdf` found which paper a file is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identification {
    /// The file prints its DOI or arXiv ID (or carries it in its metadata),
    /// and the registered title is on page 1.
    FileText,
    /// The largest text on page 1 matched a title in Crossref. Titles can
    /// collide, so the app may ask the user to check.
    TitleSearch,
    /// The model read page 1, and Crossref confirmed what it read.
    Model,
    /// Nothing matched. The item keeps the PDF and the title found in it; the
    /// user can enter a DOI and look it up.
    NotFound,
    /// A request failed before anything matched, for example offline or with
    /// a model API key that was not accepted. The item keeps the PDF.
    Failed { message: String },
}

/// What `import_pdf` made: the item holding the PDF, and how it was found.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedPdf {
    pub item: ItemRow,
    pub identification: Identification,
}

/// What the reader needs to open an attachment.
#[derive(Debug, Clone, PartialEq)]
pub struct AttachmentFile {
    pub id: String,
    pub item_id: String,
    /// Absolute path of the PDF inside the library folder.
    pub path: String,
    /// 0 based page the reader was on, if the PDF was opened before.
    pub last_page: Option<i32>,
    pub last_zoom: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub id: String,
    pub name: String,
    /// `#RRGGBB`, if the user picked one.
    pub color: Option<String>,
    /// Live items with this tag.
    pub item_count: i64,
}

/// Everything the detail panel shows for one item.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemDetail {
    pub id: String,
    pub fields: ItemFields,
    pub lookup_status: LookupStatus,
    pub tags: Vec<Tag>,
    /// Ids of the collections the item is in.
    pub collection_ids: Vec<String>,
    pub attachments: Vec<AttachmentRow>,
    /// The item's Markdown note, if one was saved.
    pub note: Option<String>,
    pub added_at: SystemTime,
    pub modified_at: SystemTime,
}

/// Which items `list_items` looks at.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Scope {
    #[default]
    All,
    /// Items whose attachments have never been opened in the reader.
    Unread,
    /// Items added in the last `RECENTLY_ADDED_DAYS` days.
    RecentlyAdded,
    /// Items without a PDF attachment.
    NoPdf,
    /// Items directly in one collection.
    Collection { id: String },
    /// Items with one tag.
    Tag { id: String },
}

/// Table columns the list can be sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortColumn {
    Title,
    FirstAuthor,
    Journal,
    Year,
    #[default]
    Added,
}

/// What `list_items` returns: the rows in `scope` that match `text`, sorted.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ItemQuery {
    /// Words to match against title, authors, journal, abstract, tags, and
    /// notes. Each word matches as a prefix. Empty matches everything.
    pub text: String,
    pub scope: Scope,
    pub sort: SortColumn,
    pub ascending: bool,
}

/// One collection in the sidebar tree. `collection_tree` returns nodes in
/// depth first order, so a flat list rebuilds the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionNode {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    /// Order among siblings.
    pub position: i64,
    /// Nesting level, 0 for top level collections.
    pub depth: i64,
    /// Live items directly in this collection.
    pub item_count: i64,
}

/// What an annotation is. Closed set, stored as text. The schema has room for
/// underline, strikeout, rectangle, and ink; they join this enum when the
/// reader can make them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnnotationKind {
    /// Marked text, drawn over `quads`.
    Highlight,
    /// A sticky note anchored at `rect`, with its text in `comment`.
    Note,
}

impl AnnotationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AnnotationKind::Highlight => "highlight",
            AnnotationKind::Note => "note",
        }
    }

    pub fn from_db(text: &str) -> Option<AnnotationKind> {
        match text {
            "highlight" => Some(AnnotationKind::Highlight),
            "note" => Some(AnnotationKind::Note),
            _ => None,
        }
    }
}

/// A point in PDF user space: points from the bottom left of the unrotated
/// page, the same space PDFKit's page coordinates use.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PagePoint {
    pub x: f64,
    pub y: f64,
}

/// One line of marked text as four corners in PDF user space, in the order
/// of the PDF `QuadPoints` array.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Quad {
    pub top_left: PagePoint,
    pub top_right: PagePoint,
    pub bottom_left: PagePoint,
    pub bottom_right: PagePoint,
}

/// A rectangle in PDF user space. `x0, y0` is the bottom left corner.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PageRect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

/// Where a PDF's first figure is, for the paper's preview picture. The app
/// draws this part of the page itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Figure {
    /// 0 based.
    pub page_index: i32,
    /// In PDF user space, inside the page's media box.
    pub rect: PageRect,
    pub source: FigureSource,
}

/// How `Figure` was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FigureSource {
    /// The drawings and images above the "Figure 1" caption.
    Caption,
    /// No caption led to a figure; this is the largest embedded image.
    LargestImage,
}

/// A highlight or note on one page of a PDF attachment. The PDF file is never
/// changed; annotations live only in the library.
#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    pub id: String,
    pub attachment_id: String,
    pub kind: AnnotationKind,
    /// 0 based.
    pub page_index: i32,
    /// The marked lines of a highlight; empty for a note.
    pub quads: Vec<Quad>,
    /// The union of the quads, or the note's anchor.
    pub rect: PageRect,
    /// `#RRGGBB`.
    pub color: String,
    /// The text under the quads.
    pub selected_text: Option<String>,
    pub comment: Option<String>,
    pub created_at: SystemTime,
    pub modified_at: SystemTime,
}

/// What `create_annotation` needs. A highlight needs `quads` and gets the
/// union of them as its `rect`; a note needs `rect` and has no quads.
#[derive(Debug, Clone, PartialEq)]
pub struct NewAnnotation {
    pub attachment_id: String,
    pub kind: AnnotationKind,
    pub page_index: i32,
    pub quads: Vec<Quad>,
    pub rect: Option<PageRect>,
    /// `#RRGGBB`.
    pub color: String,
    pub selected_text: Option<String>,
    pub comment: Option<String>,
    /// Up to 32 characters before and after the selection on the page, for
    /// re-anchoring if the PDF is ever replaced.
    pub text_before: Option<String>,
    pub text_after: Option<String>,
}

/// Fields of an annotation the user can change. `None` leaves a field alone;
/// `Some("")` clears the comment.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AnnotationPatch {
    pub color: Option<String>,
    pub comment: Option<String>,
}

/// A raw API response kept for re-mapping later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRecord {
    /// `crossref`, `datacite`, `doi_org`, `openalex`, ...
    pub source: String,
    pub json: String,
}

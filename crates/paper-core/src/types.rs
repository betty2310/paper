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

/// A raw API response kept for re-mapping later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRecord {
    /// `crossref`, `datacite`, `doi_org`, `openalex`, ...
    pub source: String,
    pub json: String,
}

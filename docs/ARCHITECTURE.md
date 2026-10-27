# Architecture

## Workspace layout

The repository is a Cargo workspace. Everything that is not UI lives in crates that do not depend on GPUI, so it can be unit tested with plain `cargo test` and reused by a future CLI or sync tool.

```text
paper/
├── Cargo.toml                 workspace
├── crates/
│   ├── paper-core/            domain types, SQLite storage, library service, settings
│   ├── paper-ids/             DOI and arXiv parsing and normalization
│   ├── paper-fetch/           Crossref, OpenAlex, Unpaywall, arXiv, PMC clients and the lookup pipeline
│   ├── paper-pdf/             PDFium worker thread: render, text, search, outline, annotation export
│   ├── paper-cite/            BibTeX and CSL output (hayagriva, biblatex)
│   └── paper-app/             GPUI application: views, actions, keymaps, assets
├── docs/
└── .github/workflows/
```

Dependency direction is strictly `paper-app` → everything else, and `paper-core` depends on nothing else in the workspace. `paper-fetch` and `paper-pdf` return plain data types defined in `paper-core`.

```diagram
┌─────────────────────────────────────────────────────────┐
│ paper-app (GPUI)                                        │
│  LibraryView  ReaderView  AnnotationPanel  NotePanel    │
└───────┬───────────────┬───────────────┬─────────────────┘
        │               │               │
        ▼               ▼               ▼
┌──────────────┐ ┌─────────────┐ ┌──────────────┐ ┌────────────┐
│ paper-core   │ │ paper-pdf   │ │ paper-fetch  │ │ paper-cite │
│ Library,     │ │ PdfWorker   │ │ Resolver     │ │ bibtex,    │
│ SQLite, FTS  │ │ (one thread)│ │ pipeline     │ │ csl        │
└──────────────┘ └─────────────┘ └──────────────┘ └────────────┘
        ▲               ▲               ▲
        └───────────────┴───────────────┘
                   paper-ids (shared types live in paper-core)
```

## Data on disk

The library directory is chosen on first launch. The chooser offers to create a new folder (suggesting `~/Documents/Paper`) or to open an existing one. A folder is recognized as a library when it contains `library.db`. The app keeps a list of recent libraries in its settings and opens the last used one on start.

```text
<library>/
├── library.db          SQLite, WAL mode
├── files/
│   └── <item uuid>/
│       └── <sanitized title>.pdf
└── cache/              page thumbnails, extracted text; safe to delete
```

Settings (recent libraries, window state, theme, contact email for API user agents, OpenAlex key) are stored separately in the platform config directory (`~/Library/Application Support/com.betty2310.paper` on macOS, `%APPDATA%\betty2310\paper` on Windows, `~/.config/paper` on Linux) as `settings.json`, so a library directory can be shared between machines without sharing machine settings.

## Database schema

All primary keys are UUID v7 strings. Every table has `created_at` and `modified_at` as UTC RFC 3339 text and a `deleted_at` column for soft deletes. Both are there so a sync layer can be added later without a migration of user data.

```sql
CREATE TABLE items (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,              -- journal_article | conference_paper | preprint | other
  doi TEXT UNIQUE,                 -- lowercase, no prefix
  arxiv_id TEXT,
  title TEXT NOT NULL,
  container_title TEXT,            -- journal or proceedings
  container_short TEXT,
  publisher TEXT,
  year INTEGER,
  published_date TEXT,             -- YYYY, YYYY-MM, or YYYY-MM-DD
  volume TEXT, issue TEXT, pages TEXT,
  abstract TEXT,
  url TEXT,
  issn TEXT,
  language TEXT,
  oa_status TEXT,                  -- gold | hybrid | green | bronze | closed | unknown
  cited_by_count INTEGER,
  citation_key TEXT UNIQUE,
  extra TEXT,                      -- JSON for fields with no column yet
  created_at TEXT NOT NULL, modified_at TEXT NOT NULL, deleted_at TEXT
);

CREATE TABLE creators (
  id TEXT PRIMARY KEY,
  item_id TEXT NOT NULL REFERENCES items(id),
  role TEXT NOT NULL,              -- author | editor
  position INTEGER NOT NULL,
  family TEXT, given TEXT, literal TEXT, orcid TEXT
);

CREATE TABLE attachments (
  id TEXT PRIMARY KEY,
  item_id TEXT NOT NULL REFERENCES items(id),
  relative_path TEXT NOT NULL,     -- under files/
  mime TEXT NOT NULL,
  sha256 TEXT NOT NULL,
  byte_size INTEGER NOT NULL,
  source_url TEXT,
  page_count INTEGER,
  last_page INTEGER, last_zoom REAL,
  created_at TEXT NOT NULL, modified_at TEXT NOT NULL, deleted_at TEXT
);

CREATE TABLE collections (
  id TEXT PRIMARY KEY,
  parent_id TEXT REFERENCES collections(id),
  name TEXT NOT NULL,
  position INTEGER NOT NULL,
  created_at TEXT NOT NULL, modified_at TEXT NOT NULL, deleted_at TEXT
);
CREATE TABLE collection_items (
  collection_id TEXT NOT NULL REFERENCES collections(id),
  item_id TEXT NOT NULL REFERENCES items(id),
  added_at TEXT NOT NULL,
  PRIMARY KEY (collection_id, item_id)
);

CREATE TABLE tags (id TEXT PRIMARY KEY, name TEXT UNIQUE NOT NULL, color TEXT);
CREATE TABLE item_tags (item_id TEXT NOT NULL, tag_id TEXT NOT NULL, PRIMARY KEY (item_id, tag_id));

CREATE TABLE notes (
  id TEXT PRIMARY KEY,
  item_id TEXT NOT NULL REFERENCES items(id),
  markdown TEXT NOT NULL,
  created_at TEXT NOT NULL, modified_at TEXT NOT NULL, deleted_at TEXT
);

CREATE TABLE annotations (
  id TEXT PRIMARY KEY,
  attachment_id TEXT NOT NULL REFERENCES attachments(id),
  kind TEXT NOT NULL,              -- highlight | underline | strikeout | note | rectangle | ink
  page_index INTEGER NOT NULL,     -- 0 based
  quads TEXT,                      -- JSON [[x1,y1,x2,y2,x3,y3,x4,y4], ...] in PDF user space
  rect TEXT,                       -- JSON [x0,y0,x1,y1], union of quads or note anchor
  paths TEXT,                      -- JSON for ink
  color TEXT NOT NULL,             -- #RRGGBB
  opacity REAL NOT NULL DEFAULT 1.0,
  selected_text TEXT,              -- text under the quads
  comment TEXT,
  text_before TEXT, text_after TEXT,   -- 32 chars of context for re-anchoring after a file swap
  sort_key TEXT NOT NULL,          -- page index + top y, for ordered listing
  created_at TEXT NOT NULL, modified_at TEXT NOT NULL, deleted_at TEXT
);

CREATE TABLE source_records (      -- raw API responses, for re-mapping later
  item_id TEXT NOT NULL, source TEXT NOT NULL, fetched_at TEXT NOT NULL, json TEXT NOT NULL,
  PRIMARY KEY (item_id, source)
);

CREATE VIRTUAL TABLE items_fts USING fts5(title, authors, container_title, abstract, note, annotation_text, content='');
```

The `items_fts` table is an external content index kept in sync by the library service after each write. Full text of PDFs is not indexed in the MVP but the extracted text can be cached under `cache/` for in-document search.

## Threading model

GPUI runs the UI on the main thread and offers a background executor backed by a thread pool. Three kinds of work leave the main thread.

1. PDFium work goes to one dedicated OS thread started at launch. PDFium is not thread safe, so the `PdfWorker` owns the `Pdfium` instance and all open documents, receives requests on a channel, and replies through oneshot channels wrapped as futures. Requests are `Open`, `Close`, `PageSizes`, `RenderPage { page, scale, tile }`, `PageText`, `Search`, `Outline`, `LinkAt`, and `ExportAnnotated`. Render requests carry a generation number so stale requests from a fast scroll are skipped.
2. SQLite work runs in `cx.background_spawn` on a `Mutex<Connection>` owned by the `Library` service. Writes are short transactions. The main thread never holds the connection across an await.
3. HTTP work runs in `cx.background_spawn` with the blocking `ureq` client. The resolver pipeline runs lookups for one item sequentially and several items concurrently, with a per host rate limiter (Crossref 10 per second, arXiv one per three seconds, others 5 per second).

```diagram
 main thread (GPUI)                background executor            pdf worker thread
┌─────────────────────┐   spawn   ┌─────────────────────┐  chan  ┌──────────────────┐
│ views, entities,    │──────────▶│ sqlite, http,       │       │ Pdfium instance   │
│ cx.notify()         │◀──────────│ bitmap -> RenderImage│◀─────▶│ open docs, render │
└─────────────────────┘  update   └─────────────────────┘        └──────────────────┘
```

## PDF viewer pipeline

A `ReaderView` entity holds an open `DocumentHandle`, the page sizes in points, the zoom, and a page cache. The page stack is a GPUI `list(ListState)` because pages have different heights. Each page element is a `canvas()` whose paint callback draws the cached bitmap with `paint_image`, then the annotation overlay with `paint_quad` (highlights use alpha, notes use a small icon, rectangles use a stroked quad), then the current selection.

Rendering a page works as follows. First, when a page becomes visible or within two pages of the viewport, the view asks the worker to render it at `zoom × window scale factor`, in BGRA because that is the format GPUI's `RenderImage` expects and PDFium can emit directly. Second, the worker returns the bytes, and a background task wraps them in an `image::Frame` and an `Arc<RenderImage>`. Third, the view stores the `Arc` in an LRU cache keyed by (page, zoom bucket) and calls `cx.notify()`. The same `Arc` is reused across frames so GPUI uploads the texture once, and `window.drop_image` is called on eviction. Above 200% zoom pages are rendered as 1024 px tiles so no bitmap exceeds the GPU texture limit and memory stays bounded. A quarter scale preview is rendered first and drawn scaled until the full bitmap arrives. The cache is capped at roughly 256 MB per window.

Coordinates are kept in PDF user space (points, origin bottom left, unrotated) in the model. Conversion to screen pixels happens only in paint and hit testing using the page's crop box, rotation, zoom, and scale factor. Text selection asks the worker for the page's character boxes once per page and caches them. Dragging maps the mouse to character indexes, and the selected range is grouped into line rectangles that become the annotation quads.

## Lookup pipeline

```text
input ──▶ normalize (paper-ids) ──▶ Crossref /works/{doi}
                                    │ 404
                                    ▼
                                  /works/{doi}/agency ──▶ doi.org content negotiation (CSL JSON)
                                    │
                                    ▼
                                  item saved, UI updated
                                    │
                                    ▼ (background)
                                  OpenAlex work by DOI ──▶ oa status, pdf_url, topics, pmcid
                                    │
                                    ▼
                                  PDF candidates in order:
                                    arXiv ▶ OpenAlex best_oa_location ▶ Unpaywall ▶ Europe PMC ▶ landing page meta tag
                                    │
                                    ▼
                                  download, verify %PDF, sha256, store, attach
```

Each stage writes its raw response to `source_records` and emits a `LookupEvent` so the UI can show progress per item.

## GPUI application structure

State is held in GPUI entities. `Entity<Library>` is created once in `application().run` and passed to views by handle. It wraps the storage service and emits `LibraryEvent::{ItemsChanged, CollectionsChanged, AttachmentReady, LookupProgress}` through `EventEmitter`. Views subscribe with `cx.subscribe` and keep the returned `Subscription` in their struct so it lives as long as the view.

The window is one `Root` from gpui-kit containing a `Dock` layout. The left panel is `CollectionsPanel` (gpui-component `Tree`), the center is a tab area that holds either the `LibraryView` (gpui-component `Table` with virtual rows) or a `ReaderView` per open PDF, and the right panel holds `ItemDetailPanel`, `AnnotationsPanel`, and `NotePanel` as tabs.

User commands are GPUI actions declared with the `actions!` macro (`AddByDoi`, `ImportFile`, `OpenItem`, `ToggleHighlight`, `FindInDocument`, `CopyBibtex`, `CopyCitation`, `ZoomIn`) and bound in a keymap JSON asset. Menus and toolbar buttons dispatch the same actions, so there is one code path per command.

Error handling follows one rule. Library crates return `Result<T, PaperError>` with `thiserror` variants per subsystem. The app converts errors into a toast notification through gpui-component's notification API and logs them with `tracing`. Network failures during lookup leave the item in a `needs_lookup` state that a retry button clears.

## Testing

`paper-core` tests run against an in-memory SQLite database. `paper-ids` has table driven tests for DOI and arXiv forms. `paper-fetch` tests parse recorded JSON fixtures from each API and never hit the network. `paper-pdf` tests render a small fixture PDF and check page count, text extraction, and that a highlight exported to a copy reads back with the same quads. UI behavior is covered by gpui-kit's headless window tests for the add flow and the reader's selection to highlight flow, and by manual checks on each platform before a release.

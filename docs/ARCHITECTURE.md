# Architecture

## Shape

Paper is a Rust engine with a native app on top of it per platform. The engine owns the library database, metadata lookup, PDF downloads, citations, and settings, and it exposes one API through `paper-engine`. The macOS app is a Swift application that calls the engine in process through UniFFI generated bindings and does everything visual with AppKit, SwiftUI, and PDFKit. Windows and Linux apps come after 1.0 and reuse the engine unchanged. See `docs/TECH_STACK.md` for why.

## Repository layout

The repository holds a Cargo workspace for the engine and an `apps/` directory with one native app per platform. Every crate under `crates/` can be unit tested with plain `cargo test` on any OS, and none of them depends on a UI toolkit.

```text
paper/
├── Cargo.toml                 workspace for the engine crates
├── crates/
│   ├── paper-core/            domain types, SQLite storage, library service, settings
│   ├── paper-ids/             DOI and arXiv parsing and normalization
│   ├── paper-fetch/           Crossref, OpenAlex, Unpaywall, arXiv, PMC clients and the lookup pipeline
│   ├── paper-identify/        reads the first pages of a PDF (pdf_oxide) and ranks its DOI and arXiv candidates
│   ├── paper-cite/            citation keys, BibTeX, and CSL output (hayagriva)
│   ├── paper-engine/          the public API: Engine, commands, view models, observer, threads
│   ├── paper-ffi/             UniFFI exports of paper-engine, builds the static library
│   └── paper-pdf/             PDFium worker for the later Windows and Linux apps, not built on macOS
├── apps/
│   └── macos/
│       ├── Paper.xcodeproj
│       ├── Paper/             Swift sources: AppKit windows and views, PDFKit reader, SwiftUI panels
│       ├── PaperEngine/       Swift package: the XCFramework and the generated bindings
│       ├── PaperTests/        XCTest unit and snapshot tests
│       └── PaperUITests/      XCUITest flows
├── scripts/                   build-xcframework.sh, generate-bindings.sh
├── docs/
└── .github/workflows/
```

Dependency direction is strictly `paper-ffi` → `paper-engine` → (`paper-core`, `paper-ids`, `paper-fetch`, `paper-identify`, `paper-cite`), and `paper-core` depends on nothing else in the workspace. `paper-identify` depends only on `paper-ids` and the pure Rust `pdf_oxide`, never touches the network, and runs the same on every platform; it is how the engine reads a PDF it is given, separate from the app's reader. `paper-fetch` returns plain data types defined in `paper-core`. `paper-pdf` is behind a `pdfium` cargo feature of `paper-engine` that the macOS build leaves off. The Swift app depends only on the `PaperEngine` package and never opens the database, calls the network, or reads API responses itself.

```diagram
┌──────────────────────────────────────────────────────────────┐
│ apps/macos (Swift)                                           │
│  MainWindow  Sidebar  LibraryTable  Reader (PDFKit)  Panels  │
└──────────────────────────────┬───────────────────────────────┘
                               │ PaperEngine package (UniFFI)
                               ▼
┌──────────────────────────────────────────────────────────────┐
│ paper-ffi → paper-engine                                     │
│  Engine, commands, view models, EngineObserver, thread pool  │
└───────┬──────────────────┬──────────────────┬────────────────┘
        ▼                  ▼                  ▼
┌──────────────┐  ┌──────────────┐  ┌────────────┐  ┌──────────────────┐
│ paper-core   │  │ paper-fetch  │  │ paper-cite │  │ paper-pdf        │
│ Library,     │  │ Resolver     │  │ bibtex,    │  │ PDFium worker,   │
│ SQLite, FTS  │  │ pipeline     │  │ csl        │  │ Windows & Linux  │
└──────────────┘  └──────────────┘  └────────────┘  └──────────────────┘
                   paper-ids (shared types live in paper-core)
```

## Engine API

`paper-engine` exposes one `Engine` object per open library. Its surface is small and typed so the generated Swift reads like a native SDK and so a Windows or Linux app can use it unchanged later.

| Group | Methods | Notes |
|---|---|---|
| Library | `open_library(path)`, `create_library(path)`, `recent_libraries()`, `close()` | `Engine` is created by `open_library`. |
| Items | `list_items(ItemQuery) -> Vec<ItemRow>`, `item_detail(id) -> ItemDetail`, `add_by_identifier(text)`, `retry_lookup(id)`, `import_pdf(path)`, `update_metadata(id, MetadataPatch)`, `delete_items(ids)`, `undo_delete()` | `ItemQuery` is text, scope, sort column, and direction; the text runs through FTS5. `scope` is a collection, a tag, or one of the smart lists. `add_by_identifier` inserts a `Pending` placeholder row at once, then fills it in from the lookup; a failed lookup leaves the row `Failed` for `retry_lookup`. `import_pdf` does the same for a PDF file and returns `ImportedPdf` with how the paper was identified (see below); a file nothing identifies stays as an item with its PDF and a `Failed` row, and a paper already in the library gets the PDF and returns `Duplicate`. |
| Collections and tags | `collection_tree() -> Vec<CollectionNode>`, `create_collection`, `rename_collection`, `move_collection`, `add_to_collection`, `tags()`, `set_tags(id, names)`, `set_tag_color(id, color)`, `set_favorite(ids, favorite)` | A tag's color is `#RRGGBB`; the app picks it from its palette when the tag is new. |
| Topics | `topics() -> Vec<Topic>`, `set_topics(topics)`, `tag_with_topics(ids) -> Vec<TopicTags>` | The library's topic list (name and description), which automatic tags come from. With a Jev API key set, a finished lookup or an identified import asks Jev once per paper which topics it is about and adds up to three as ordinary tags (see Topic tags below). `tag_with_topics` asks again for the papers the user picks and returns the tags each one gained. |
| Attachments | `attachment_file(id) -> AttachmentFile` (absolute path, last page, last zoom), `set_reading_position(id, page, zoom)`, `attach_file(item_id, path)`, `find_pdf(item_id)`, `first_figure(id) -> Option<Figure>` | `first_figure` says where a PDF's first figure is, for the paper's preview picture: the drawings and images above the "Figure 1" caption in the first four pages, or else the largest embedded image. The app draws that region with PDFKit and caches it under `cache/previews/`. A finished `add_by_identifier` starts `find_pdf` on its own, and the app follows it through `pdf_progress` and `pdf_step`. `find_pdf` returns the PDF the item already has without touching the network, fails with `PdfNotFound` when no source offers one, and allows one search per item at a time. Zoom 0 means fit width. |
| Annotations | `annotations(attachment_id) -> Vec<Annotation>`, `create_annotation(New)`, `update_annotation(id, Patch)`, `delete_annotation(id)`, `import_embedded(attachment_id, Vec<New>)` | Geometry is in PDF user space, see below. |
| Notes | `note(item_id)`, `save_note(item_id, markdown)` | Autosave calls `save_note` with a debounce in the app. |
| Citations | `bibtex(ids) -> String`, `citation(ids, CitationStyle) -> Citation`, `export_bib(ids, path) -> u32` | Entries come sorted by citation key, and an id listed twice is cited once. `citation` returns the style's reference list as plain text and as HTML with the style's italics, for a rich paste; `CitationStyle` is `Apa` for now. `export_bib` writes the `.bib` file on an engine thread and returns the entry count. See Citation keys below. |
| Settings | `settings()`, `update_settings(Patch)`, `pinned_tabs()`, `set_pinned_tabs(item_ids)` | Pinned tabs are kept per library path, in tab order. Contact email, OpenAlex key, Anthropic key, Jev API key, window state, panel sizes. |
| Events | `set_observer(Arc<dyn EngineObserver>)` with `items_changed(ids)`, `collections_changed()`, `tags_changed()`, `lookup_progress(item_id, LookupStatus)`, `pdf_progress(item_id, PdfStatus)`, `pdf_step(item_id, PdfStep)` (the source being asked, or bytes downloaded of the total, at most every 50 ms), `topics_added(item_id, tags)` (tags Jev gave a paper that was just added) | A foreign trait the app implements. An event caused by a synchronous call arrives on the caller's thread before that call returns; everything a lookup does after it leaves for the network (database writes and events) runs on an engine thread, so the app hops to the main thread asynchronously, never with a blocking dispatch. Errors are not events: every `async fn` returns its own `PaperError`, and lookup errors carry the id of the row they left behind. |

Rules for the API. Reads that the UI needs on every keystroke or scroll, such as `list_items`, are synchronous and return in a few milliseconds from the database. Anything that touches the network or takes more than a few milliseconds is an `async fn` that completes a UniFFI future on the engine's thread pool. Records are plain data (`ItemRow`, `ItemDetail`, `CollectionNode`, `Annotation`, `AttachmentFile`), enums are closed sets (`ItemKind`, `AnnotationKind`, `LookupStatus`, `SortColumn`), errors are one `PaperError` enum with a variant per subsystem, and no method returns a handle that the caller must keep alive except `Engine` itself. The app subscribes once with `set_observer`, so nothing in the engine ever calls back into the UI toolkit.

## Citation keys

Every paper gets a citation key, such as `vaswani2017attention`: the first author's family name, the year, and the first title word that is not a stop word, folded to lowercase ASCII (`Müller` → `muller`). A missing part is left out, and a paper with nothing usable is `paper`. When another paper has the key in any case, the next free of `a` to `z`, `aa`, ... is appended (`lecun2015deep`, `lecun2015deepa`).

A key is given once, when the paper's metadata is in: at the end of a lookup or an import, when a paper is typed in, and for older libraries when the library opens. From then on it never changes by itself, so a `.bib` file and the documents that cite it stay in step when metadata is edited or looked up again. The user can type a key in the metadata editor (it must be free in any case, and may not contain whitespace or `,{}()"#%'=\~`), and clearing the field makes a new key from the metadata as it is then. A paper in the trash keeps its key, so undo brings it back unchanged. A paper whose lookup is still running is cited under the key it would get, without storing it. `paper-cite` makes the key and writes BibTeX and CSL output as pure functions; `paper-core` stores the key in `items.citation_key` and picks the free suffix.

## Identifying a PDF

`import_pdf` reads the first three pages with `paper-identify` and tries three steps, stopping at the first answer that Crossref confirms. The measurements behind the order are in `docs/research/pdf-identification.md`.

1. File text. DOIs and arXiv IDs from the Info dictionary, the XMP packet, `doi.org` links, and the text of the pages, ranked by where they appear and cut off at a references heading. The best four are looked up, and one counts when its registered title is on page 1. `Identification::FileText`.
2. Title search. The first run of lines in the largest font on page 1, searched with Crossref's bibliographic query. A result counts when its title matches (supplementary files and peer reviews that share the title are skipped). `Identification::TitleSearch`; the app may ask the user to check these, because titles collide.
3. Model. Only when the user has set an Anthropic API key: page 1 is rendered to a PNG of at most 1568 px and sent to `claude-haiku-5-5`, which returns the printed DOI, arXiv ID, title, and first author as JSON. A DOI it reads counts when its registered title matches the title it reads; otherwise its title goes through step 2. `Identification::Model`.

Without a key the file never leaves the machine; Crossref only sees the identifiers and title taken from it. The search and model responses are kept in `source_records` (`crossref_search`, `anthropic`) next to the lookup's own records.

## Topic tags

A paper gets up to three tags from the library's topic list without the user typing them, so the TAGS group can filter by subject. The research behind the design is in `docs/research/topic-tags.md`.

When a lookup finishes or an import identifies its paper, and the user has set a Jev API key, a job on the engine's pool does three things. First, it reads the paper's OpenAlex work by DOI for its keywords and abstract. Second, it sends the title, venue, abstract (Crossref's, else OpenAlex's), and keywords to TypeSafe's Jev model with one yes or no question per topic, built from the topic's name and description, in a single request. Third, it adds the topics Jev answers yes to with a probability of at least 0.8, at most three and the most likely first, as ordinary tags, and sends `tags_changed` and then `topics_added` so the app can show a toast. Both responses are kept in `source_records` (`openalex`, `typesafe`).

A paper is tagged automatically once. The `typesafe` record marks it, so a later `retry_lookup` does not bring back a tag the user removed. `tag_with_topics` asks again on request and only adds tags. Without a key nothing leaves the machine for TypeSafe and no tags are added. A key TypeSafe refuses is a `Settings` error.

## Data on disk

The library directory is chosen on first launch. The chooser offers to create a new folder (suggesting `~/Documents/Paper`) or to open an existing one. A folder is recognized as a library when it contains `library.db`. The app keeps a list of recent libraries in its settings and opens the last used one on start.

```text
<library>/
├── library.db          SQLite, WAL mode; the WAL is folded back into this file on close so a synced folder sees one file
├── files/
│   └── <item uuid>/
│       └── <sanitized title>.pdf
└── cache/              page thumbnails, figure previews, extracted text; safe to delete
```

Settings (recent libraries, window state, each library's pinned reader tabs, theme, contact email for API user agents, OpenAlex, Anthropic, and Jev keys) are stored separately in the platform config directory (`~/Library/Application Support/com.betty2310.paper` on macOS, `%APPDATA%\betty2310\paper\config` on Windows, `~/.config/paper` on Linux) as `settings.json`, so a library directory can be shared between machines without sharing machine settings. The sandboxed macOS app also stores a security scoped bookmark for each recent library next to the settings, because the sandbox forgets folder access between launches otherwise.

## Database schema

All primary keys are UUID v7 strings. Every table has `created_at` and `modified_at` as UTC RFC 3339 text and a `deleted_at` column for soft deletes. Both are there so a sync layer can be added later without a migration of user data.

```sql
CREATE TABLE items (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,              -- journal_article | conference_paper | preprint | other
  doi TEXT,                        -- lowercase, no prefix; unique among live rows (partial index)
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
  citation_key TEXT UNIQUE,        -- unique without regard to case (migration 2)
  lookup_status TEXT NOT NULL,     -- pending | done | failed
  favorite INTEGER NOT NULL DEFAULT 0,  -- listed first in every view (migration 3)
  extra TEXT,                      -- JSON for fields with no column yet
  created_at TEXT NOT NULL, modified_at TEXT NOT NULL, deleted_at TEXT
);
CREATE UNIQUE INDEX items_live_doi ON items(doi) WHERE deleted_at IS NULL AND doi IS NOT NULL;

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

CREATE TABLE topics (              -- what automatic tags can be named (migration 4)
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL COLLATE NOCASE UNIQUE,
  description TEXT NOT NULL,       -- read by Jev; says what the topic covers and what it does not
  position INTEGER NOT NULL,
  created_at TEXT NOT NULL, modified_at TEXT NOT NULL
);

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

The engine owns its threads, and the app owns the main thread. Nothing in the engine knows which thread the app calls from, and nothing in the app touches engine internals.

1. Synchronous engine reads (`list_items`, `item_detail`, `annotations`, `collection_tree`) run on the caller's thread against a `Mutex<Connection>`. They are called from the main thread and must return in a few milliseconds, which the 5,000 item target in `docs/PRD.md` holds at. Writes are short transactions on the same connection.
2. Async engine work (lookups, downloads, imports, exports, Zotero import) runs on a small fixed thread pool inside the engine. UniFFI turns each `async fn` into a Swift `async` call. HTTP uses the blocking `ureq` client on these threads. The resolver pipeline runs the lookups for one item sequentially and several items concurrently, with a per host rate limiter (Crossref 10 per second, arXiv one per three seconds, others 5 per second).
3. Events reach the app through `EngineObserver`: on the caller's thread for synchronous calls, on an engine thread for everything a lookup does once it has left for the network. The app's `LibraryStore` forwards each call to the main thread asynchronously before it touches any view. `close()` returns at once; a lookup still waiting on the network finishes on its thread without reaching the observer, and the database closes when the last reference goes away.
4. PDF work on macOS is PDFKit's. `PDFView` renders pages on its own background threads and the app never renders a page bitmap itself. Opening a `PDFDocument` and building the thumbnail strip happen on a utility queue, then the document is handed to the main thread.

```diagram
 main thread (AppKit)                engine thread pool             PDFKit
┌─────────────────────┐  sync call  ┌─────────────────────┐   ┌──────────────────┐
│ views, LibraryStore │────────────▶│ sqlite (mutex),     │   │ PDFView renders  │
│                     │  async fn   │ ureq, pipeline      │   │ on its own       │
│ (owns PDFView)      │◀────────────│ EngineObserver      │   │ threads          │
└─────────────────────┘  hop to main└─────────────────────┘   └──────────────────┘
```

## PDF reader on macOS (PDFKit)

A `ReaderViewController` owns one `PDFView` in continuous single page mode with `autoScales` off, the zoom from the attachment's `last_zoom`, page shadows off, and the warm gray background from the theme. Opening works as follows. First, the app asks the engine for `attachment_file(id)`, which returns the absolute path inside the library folder, the last page, the last zoom, and the page count. Second, on a utility queue it creates `PDFDocument(url:)`, asks the engine for `annotations(attachment_id)`, and turns each row into a `PDFAnnotation` that it adds to the matching `PDFPage`. Third, on the main thread it assigns the document to the view and scrolls to the last page. Fourth, the thumbnail strip asks each page for `thumbnail(of:for:)` on the utility queue and caches the images by page index. The first page is visible as soon as PDFKit draws it, which is what the 300 ms target measures.

The library file is never written. `PDFAnnotation` objects are added to the in memory `PDFDocument` only so that PDFKit draws them, and `PDFDocument.write(to:)` is called only on the "Export PDF with annotations" path, into a new file the user chooses. The reader keeps a dictionary from annotation id to `PDFAnnotation`, so an edit or delete from the annotations panel updates the page without reloading, and each `PDFAnnotation` created by Paper has `userName` set to `Paper` so embedded annotations that came with the file can be told apart. On open, annotations in the file that Paper did not create are listed read only in the panel with an "Import" action that calls `import_embedded`.

Coordinates in the database are PDF user space (points, origin bottom left, unrotated, inside the crop box). PDFKit page space is user space too, so a quad from the database becomes a `PDFAnnotation` with `bounds` set to the union rectangle and `quadrilateralPoints` set to each line rectangle expressed relative to `bounds`, which is the one conversion PDFKit needs because it stores quad points relative to the annotation bounds rather than absolute like PDF QuadPoints. Both directions of this conversion live in one Swift file (`AnnotationGeometry.swift`) with unit tests on rotated and odd crop box fixtures, and spike 2 of the roadmap confirms the crop box origin behavior before any annotation code is written.

Selection and markup. `PDFView` handles drag selection and posts `PDFViewSelectionChanged`. On mouse up with a non empty selection, the reader calls `selection.selectionsByLine()`, takes `bounds(for: page)` of each line in page space, builds the quads, and shows the floating markup toolbar 8 px above the selection in view coordinates. Choosing a markup kind and color calls `create_annotation` with the quads, the selected string from `selection.string`, and 32 characters of context on each side taken from `page.string`, then adds the returned annotation to the page. A sticky note comes from a click with the note tool, converted with `pdfView.convert(point, to: page)`, and is stored as a 20 by 20 pt rectangle. A figure rectangle is drawn with a drag in the rectangle tool and stored as `rect` only. Search uses `PDFDocument.beginFindString` with the delegate callbacks feeding the search bar and `pdfView.highlightedSelections` for the matches. The outline comes from `document.outlineRoot`. Links are handled by `PDFView` itself. The current page is read from `PDFViewPageChanged` and written back with `set_reading_position` after a one second debounce.

The PDFium reader for Windows and Linux (tiles, overlay painting, character boxes) is documented in `docs/research/pdf-engines.md` and will get its own section when those apps are planned. Both readers feed the same annotation table, which is why the geometry is defined in PDF user space and not in either engine's terms.

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

## macOS application structure

One `LibraryStore` object per open library wraps the `Engine`, implements `EngineObserver`, hops every event to the main thread, and publishes changes with Combine (`ObservableObject` with `@Published` snapshots of the item rows, the collection tree, and per item lookup state). AppKit controllers subscribe with Combine and SwiftUI panels observe it with `@ObservedObject`. The store is the only object in the app that holds an `Engine`, and it is created by `AppDelegate` when a library is opened and handed to the window controller.

Each library opens one `MainWindowController`. Its window hides the standard title and uses full size content so the custom 40 px tab bar can sit next to the traffic lights. The content is an `NSSplitViewController` with three items: `SidebarViewController` (an `NSOutlineView` with the smart lists, the collection tree, and the tags), a `TabContainerViewController` that owns the tab bar and swaps between one `LibraryViewController` (an `NSTableView` with view based cells, 44 px rows, and the columns from `docs/UI.md`) and one `ReaderViewController` per open PDF, and a right `NSHostingController` that shows the SwiftUI `ItemDetailPanel` for the library tab or the `ReaderSidePanel` (annotations, outline, notes) for a reader tab. Divider positions are saved through `update_settings`.

User commands are selectors on the responder chain (`addByIdentifier:`, `importFile:`, `openSelected:`, `highlightSelection:`, `findInDocument:`, `copyBibtex:`, `copyCitation:`, `copyCitationKey:`, `exportBibtex:`, `zoomIn:`, and so on). The main menu is built in code with the key equivalents from `docs/UI.md`, and the toolbar buttons, the SwiftUI buttons (`NSApp.sendAction`), and the command palette all send the same selectors, so there is one code path per command and menu items enable and disable themselves through `validateMenuItem`.

Error handling follows one rule. Engine crates return `Result<T, PaperError>` with `thiserror` variants per subsystem, flattened to one `PaperError` enum at the FFI, which UniFFI turns into a thrown Swift error. The app shows errors as a toast at the bottom right and logs them with `os.Logger`. Network failures during lookup leave the item in a `needs_lookup` state that a retry button clears.

## Testing

`paper-core` tests run against an in-memory SQLite database. `paper-ids` has table driven tests for DOI and arXiv forms. `paper-fetch` tests parse recorded JSON fixtures from each API and never hit the network. `paper-engine` tests exercise the public API end to end on a temporary library folder with a fake HTTP layer, and they are the contract that the later Windows and Linux apps can rely on. `paper-pdf` tests run only with the `pdfium` feature on Linux and Windows CI.

The macOS app has XCTest unit tests for `AnnotationGeometry` (database quads to `PDFAnnotation` and back on rotated and odd crop box fixtures), for `LibraryStore` (events arrive on the main thread and update the published snapshots), and for the identifier detection in the search field. Snapshot tests render each screen state (empty library, library with a selection, reader with the selection toolbar, annotations panel with an open comment) from a fixture library at 1x and 2x and compare them with reference images checked in next to the tests, with the mockups in `docs/design/` as the source for the references. XCUITest covers the add by DOI flow with a stubbed engine and the selection to highlight to export flow on a fixture PDF, and the exported copy is checked by reading its annotations back with PDFKit. Manual checks on each supported macOS version happen before a release, because PDFKit behavior changes between releases.

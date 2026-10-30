-- Migration 1: the whole MVP schema from docs/ARCHITECTURE.md.
--
-- All primary keys are UUID v7 strings. Timestamps are UTC RFC 3339 text.
-- Rows are soft deleted with deleted_at so delete can be undone and so a sync
-- layer can be added later without a data migration.

CREATE TABLE items (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,              -- journal_article | conference_paper | preprint | other
  doi TEXT,                        -- lowercase, no prefix; unique among live items (see index)
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
  lookup_status TEXT NOT NULL DEFAULT 'done',  -- pending | done | failed
  extra TEXT,                      -- JSON for fields with no column yet
  created_at TEXT NOT NULL, modified_at TEXT NOT NULL, deleted_at TEXT
);
CREATE UNIQUE INDEX items_live_doi ON items(doi) WHERE deleted_at IS NULL AND doi IS NOT NULL;
CREATE INDEX items_created_at ON items(created_at);

CREATE TABLE creators (
  id TEXT PRIMARY KEY,
  item_id TEXT NOT NULL REFERENCES items(id),
  role TEXT NOT NULL,              -- author | editor
  position INTEGER NOT NULL,
  family TEXT, given TEXT, literal TEXT, orcid TEXT
);
CREATE INDEX creators_item ON creators(item_id, position);

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
CREATE INDEX attachments_item ON attachments(item_id);

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
CREATE INDEX collection_items_item ON collection_items(item_id);

CREATE TABLE tags (id TEXT PRIMARY KEY, name TEXT NOT NULL COLLATE NOCASE UNIQUE, color TEXT);
CREATE TABLE item_tags (item_id TEXT NOT NULL, tag_id TEXT NOT NULL, PRIMARY KEY (item_id, tag_id));
CREATE INDEX item_tags_tag ON item_tags(tag_id);

CREATE TABLE notes (
  id TEXT PRIMARY KEY,
  item_id TEXT NOT NULL UNIQUE REFERENCES items(id),
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
CREATE INDEX annotations_attachment ON annotations(attachment_id, sort_key);

CREATE TABLE source_records (      -- raw API responses, for re-mapping later
  item_id TEXT NOT NULL, source TEXT NOT NULL, fetched_at TEXT NOT NULL, json TEXT NOT NULL,
  PRIMARY KEY (item_id, source)
);

-- Search index, one row per live item, rebuilt by the Library after each write.
CREATE VIRTUAL TABLE items_fts USING fts5(
  item_id UNINDEXED, title, authors, container_title, abstract, tags, note, annotation_text
);

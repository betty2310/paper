//! The `Library`: one SQLite database plus a `files/` folder in a directory the
//! user chose. Every read and write of library data goes through here.
//!
//! The connection sits behind a mutex. Reads the UI needs on every keystroke
//! (`list_items`) are a single query and return in milliseconds at the 5,000
//! item target. Writes are short transactions that also rebuild the search
//! index row of the items they touched.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Row, Transaction, params, params_from_iter,
};

use crate::PaperError;
use crate::time;
use crate::types::*;

pub const LIBRARY_DB_NAME: &str = "library.db";
pub const LIBRARY_FILES_DIR: &str = "files";
/// `Scope::RecentlyAdded` covers this many days.
pub const RECENTLY_ADDED_DAYS: i64 = 30;

const MIGRATIONS: &[&str] = &[
    include_str!("migrations/001_initial.sql"),
    include_str!("migrations/002_citation_keys.sql"),
    include_str!("migrations/003_favorites.sql"),
    include_str!("migrations/004_topics.sql"),
];

/// Separator for names packed into one SQL string column.
const NAME_SEPARATOR: &str = "\u{1f}";

type Result<T> = std::result::Result<T, PaperError>;

pub struct Library {
    dir: PathBuf,
    conn: Mutex<Connection>,
}

impl Library {
    /// A folder is a library when it contains `library.db`.
    pub fn is_library(dir: impl AsRef<Path>) -> bool {
        dir.as_ref().join(LIBRARY_DB_NAME).is_file()
    }

    /// Creates a new library in `dir`, which may not exist yet. Fails if the
    /// folder already holds a library, so an existing one is never overwritten.
    pub fn create(dir: impl AsRef<Path>) -> Result<Library> {
        let dir = dir.as_ref().to_path_buf();
        if Library::is_library(&dir) {
            return Err(PaperError::Library {
                message: format!("{} already contains a library", dir.display()),
            });
        }
        std::fs::create_dir_all(dir.join(LIBRARY_FILES_DIR))?;
        let conn = open_connection(&dir.join(LIBRARY_DB_NAME), true)?;
        Ok(Library {
            dir,
            conn: Mutex::new(conn),
        })
    }

    /// Opens the library in `dir`. Fails if the folder is not a library.
    pub fn open(dir: impl AsRef<Path>) -> Result<Library> {
        let dir = dir.as_ref().to_path_buf();
        if !Library::is_library(&dir) {
            return Err(PaperError::Library {
                message: format!(
                    "{} is not a Paper library (no {LIBRARY_DB_NAME})",
                    dir.display()
                ),
            });
        }
        std::fs::create_dir_all(dir.join(LIBRARY_FILES_DIR))?;
        let conn = open_connection(&dir.join(LIBRARY_DB_NAME), false)?;
        Ok(Library {
            dir,
            conn: Mutex::new(conn),
        })
    }

    /// A library that lives only in memory, for tests.
    pub fn open_in_memory() -> Result<Library> {
        let conn = Connection::open_in_memory()?;
        configure(&conn)?;
        migrate(&conn)?;
        Ok(Library {
            dir: PathBuf::new(),
            conn: Mutex::new(conn),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

impl Drop for Library {
    /// Folds the WAL back into `library.db` so a library in a synced folder
    /// (Dropbox, iCloud Drive) leaves one file behind, not three.
    fn drop(&mut self) {
        if let Ok(conn) = self.conn.lock() {
            let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
        }
    }
}

impl Library {
    pub fn files_dir(&self) -> PathBuf {
        self.dir.join(LIBRARY_FILES_DIR)
    }

    // ----- items -----

    /// Inserts an item and returns its id. The DOI must not belong to a live item.
    pub fn insert_item(&self, fields: &ItemFields, status: LookupStatus) -> Result<String> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        if let Some(doi) = fields.doi.as_deref() {
            check_doi_free(&tx, doi, None)?;
        }
        let id = new_id();
        let now = time::now();
        tx.execute(
            "INSERT INTO items (id, kind, doi, arxiv_id, title, container_title, container_short,
                publisher, year, published_date, volume, issue, pages, abstract, url, issn,
                language, lookup_status, extra, created_at, modified_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?20)",
            params![
                id,
                fields.kind.unwrap_or(ItemKind::Other).as_str(),
                fields.doi,
                fields.arxiv_id,
                fields.title,
                fields.container_title,
                fields.container_short,
                fields.publisher,
                fields.year,
                fields.published_date,
                fields.volume,
                fields.issue,
                fields.pages,
                fields.abstract_text,
                fields.url,
                fields.issn,
                fields.language,
                status.as_str(),
                fields.extra,
                now,
            ],
        )?;
        write_creators(&tx, &id, &fields.creators)?;
        reindex(&tx, &id)?;
        tx.commit()?;
        Ok(id)
    }

    /// Overwrites every metadata field of an item, as after a lookup. Works on
    /// a deleted item too, so a lookup that finishes after the user deleted the
    /// row still lands and an undo brings back the finished item.
    pub fn replace_metadata(
        &self,
        id: &str,
        fields: &ItemFields,
        status: LookupStatus,
    ) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        require_item(&tx, id)?;
        if let Some(doi) = fields.doi.as_deref() {
            check_doi_free(&tx, doi, Some(id))?;
        }
        tx.execute(
            "UPDATE items SET kind = ?2, doi = ?3, arxiv_id = ?4, title = ?5, container_title = ?6,
                container_short = ?7, publisher = ?8, year = ?9, published_date = ?10, volume = ?11,
                issue = ?12, pages = ?13, abstract = ?14, url = ?15, issn = ?16, language = ?17,
                lookup_status = ?18, extra = ?19, modified_at = ?20
             WHERE id = ?1",
            params![
                id,
                fields.kind.unwrap_or(ItemKind::Other).as_str(),
                fields.doi,
                fields.arxiv_id,
                fields.title,
                fields.container_title,
                fields.container_short,
                fields.publisher,
                fields.year,
                fields.published_date,
                fields.volume,
                fields.issue,
                fields.pages,
                fields.abstract_text,
                fields.url,
                fields.issn,
                fields.language,
                status.as_str(),
                fields.extra,
                time::now(),
            ],
        )?;
        tx.execute("DELETE FROM creators WHERE item_id = ?1", params![id])?;
        write_creators(&tx, id, &fields.creators)?;
        reindex(&tx, id)?;
        tx.commit()?;
        Ok(())
    }

    /// Applies the fields the user edited. `None` leaves a field alone and
    /// `Some("")` clears it. An empty title is ignored.
    pub fn update_metadata(&self, id: &str, patch: &MetadataPatch) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        require_live_item(&tx, id)?;
        if let Some(doi) = patch.doi.as_deref().filter(|doi| !doi.is_empty()) {
            check_doi_free(&tx, doi, Some(id))?;
        }
        if let Some(key) = patch.citation_key.as_deref().filter(|key| !key.is_empty()) {
            check_citation_key_free(&tx, key, id)?;
        }

        let mut sets: Vec<String> = vec!["modified_at = ?".into()];
        let mut values: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(time::now())];
        let mut set_text = |column: &str, value: &Option<String>| {
            if let Some(text) = value {
                sets.push(format!("{column} = ?"));
                values.push(Box::new(if text.is_empty() {
                    None
                } else {
                    Some(text.clone())
                }));
            }
        };
        set_text("doi", &patch.doi);
        set_text("arxiv_id", &patch.arxiv_id);
        set_text("container_title", &patch.container_title);
        set_text("container_short", &patch.container_short);
        set_text("publisher", &patch.publisher);
        set_text("published_date", &patch.published_date);
        set_text("volume", &patch.volume);
        set_text("issue", &patch.issue);
        set_text("pages", &patch.pages);
        set_text("abstract", &patch.abstract_text);
        set_text("url", &patch.url);
        set_text("citation_key", &patch.citation_key);
        if let Some(title) = patch
            .title
            .as_deref()
            .filter(|title| !title.trim().is_empty())
        {
            sets.push("title = ?".into());
            values.push(Box::new(title.trim().to_string()));
        }
        if let Some(kind) = patch.kind {
            sets.push("kind = ?".into());
            values.push(Box::new(kind.as_str()));
        }
        if let Some(year) = patch.year {
            sets.push("year = ?".into());
            values.push(Box::new(year));
        }
        values.push(Box::new(id.to_string()));
        tx.execute(
            &format!("UPDATE items SET {} WHERE id = ?", sets.join(", ")),
            params_from_iter(values.iter()),
        )?;
        if let Some(creators) = &patch.creators {
            tx.execute("DELETE FROM creators WHERE item_id = ?1", params![id])?;
            write_creators(&tx, id, creators)?;
        }
        reindex(&tx, id)?;
        tx.commit()?;
        Ok(())
    }

    /// Sets the lookup state of an item, deleted or not (see `replace_metadata`).
    pub fn set_lookup_status(&self, id: &str, status: LookupStatus) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let changed = conn.execute(
            "UPDATE items SET lookup_status = ?2, modified_at = ?3 WHERE id = ?1",
            params![id, status.as_str(), time::now()],
        )?;
        not_found_if_zero(changed, id)
    }

    /// Gives each item that has no citation key one made from its base: the
    /// base itself when no other item has it, else the base followed by `a`,
    /// `b`, ..., `z`, `aa`, and so on. Keys are compared without regard to
    /// case. An item that has a key keeps it, so keys never change on their
    /// own once given. One transaction for the whole list.
    pub fn assign_citation_keys(&self, bases: &[(String, String)]) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        for (id, base) in bases {
            let has_key: Option<bool> = tx
                .query_row(
                    "SELECT citation_key IS NOT NULL FROM items WHERE id = ?1",
                    params![id],
                    |row| row.get(0),
                )
                .optional()?;
            match has_key {
                None => return Err(PaperError::NotFound { id: id.clone() }),
                Some(true) => continue,
                Some(false) => {}
            }
            let mut index = 0;
            let key = loop {
                let candidate = format!("{base}{}", key_suffix(index));
                if !citation_key_taken(&tx, &candidate, id)? {
                    break candidate;
                }
                index += 1;
            };
            tx.execute(
                "UPDATE items SET citation_key = ?2 WHERE id = ?1",
                params![id, key],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Items, deleted or not, that have no citation key and whose lookup is
    /// `status`.
    pub fn items_without_citation_key(&self, status: LookupStatus) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let ids = conn
            .prepare_cached(
                "SELECT id FROM items WHERE citation_key IS NULL AND lookup_status = ?1
                 ORDER BY created_at",
            )?
            .query_map(params![status.as_str()], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(ids)
    }

    /// Removes an item and everything that hangs off it for good. For rows the
    /// user never saw finished, such as a placeholder whose lookup turned out
    /// to be a duplicate; `delete_items` is the undoable delete.
    pub fn purge_item(&self, id: &str) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        require_item(&tx, id)?;
        tx.execute(
            "DELETE FROM annotations WHERE attachment_id IN (SELECT id FROM attachments WHERE item_id = ?1)",
            params![id],
        )?;
        for table in [
            "attachments",
            "creators",
            "collection_items",
            "item_tags",
            "notes",
            "source_records",
            "items_fts",
        ] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE item_id = ?1"),
                params![id],
            )?;
        }
        tx.execute("DELETE FROM items WHERE id = ?1", params![id])?;
        tx.execute(
            "DELETE FROM tags WHERE id NOT IN (SELECT tag_id FROM item_tags)",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// The live item with this DOI, or else the most recently deleted one.
    pub fn find_item_by_doi(&self, doi: &str) -> Result<Option<DoiMatch>> {
        let conn = self.conn.lock().unwrap();
        find_by_doi(&conn, doi)
    }

    pub fn item(&self, id: &str) -> Result<ItemDetail> {
        let conn = self.conn.lock().unwrap();
        let fields = conn
            .query_row(
                "SELECT kind, doi, arxiv_id, title, container_title, container_short, publisher,
                        year, published_date, volume, issue, pages, abstract, url, issn, language,
                        extra, lookup_status, citation_key, created_at, modified_at
                 FROM items WHERE id = ?1 AND deleted_at IS NULL",
                params![id],
                |row| {
                    Ok((
                        ItemFields {
                            kind: Some(ItemKind::from_db(&row.get::<_, String>("kind")?)),
                            doi: row.get("doi")?,
                            arxiv_id: row.get("arxiv_id")?,
                            title: row.get("title")?,
                            creators: Vec::new(),
                            container_title: row.get("container_title")?,
                            container_short: row.get("container_short")?,
                            publisher: row.get("publisher")?,
                            year: row.get("year")?,
                            published_date: row.get("published_date")?,
                            volume: row.get("volume")?,
                            issue: row.get("issue")?,
                            pages: row.get("pages")?,
                            abstract_text: row.get("abstract")?,
                            url: row.get("url")?,
                            issn: row.get("issn")?,
                            language: row.get("language")?,
                            extra: row.get("extra")?,
                        },
                        LookupStatus::from_db(&row.get::<_, String>("lookup_status")?),
                        row.get::<_, Option<String>>("citation_key")?,
                        row.get::<_, String>("created_at")?,
                        row.get::<_, String>("modified_at")?,
                    ))
                },
            )
            .optional()?;
        let Some((mut fields, lookup_status, citation_key, created_at, modified_at)) = fields
        else {
            return Err(PaperError::NotFound { id: id.to_string() });
        };

        fields.creators = conn
            .prepare_cached(
                "SELECT role, family, given, literal, orcid FROM creators
                 WHERE item_id = ?1 ORDER BY position",
            )?
            .query_map(params![id], |row| {
                Ok(Creator {
                    role: Some(CreatorRole::from_db(&row.get::<_, String>("role")?)),
                    family: row.get("family")?,
                    given: row.get("given")?,
                    literal: row.get("literal")?,
                    orcid: row.get("orcid")?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        let tags = conn
            .prepare_cached(
                "SELECT t.id, t.name, t.color,
                        (SELECT count(*) FROM item_tags it2 JOIN items i ON i.id = it2.item_id
                         WHERE it2.tag_id = t.id AND i.deleted_at IS NULL) AS item_count
                 FROM item_tags it JOIN tags t ON t.id = it.tag_id
                 WHERE it.item_id = ?1 ORDER BY t.name COLLATE NOCASE",
            )?
            .query_map(params![id], read_tag)?
            .collect::<rusqlite::Result<_>>()?;

        let collection_ids = conn
            .prepare_cached(
                "SELECT ci.collection_id FROM collection_items ci
                 JOIN collections c ON c.id = ci.collection_id
                 WHERE ci.item_id = ?1 AND c.deleted_at IS NULL ORDER BY ci.added_at",
            )?
            .query_map(params![id], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;

        let attachments = conn
            .prepare_cached(&format!(
                "{ATTACHMENT_SELECT} WHERE item_id = ?1 AND deleted_at IS NULL ORDER BY created_at"
            ))?
            .query_map(params![id], read_attachment)?
            .collect::<rusqlite::Result<_>>()?;

        let note = read_note(&conn, id)?;

        Ok(ItemDetail {
            id: id.to_string(),
            fields,
            lookup_status,
            citation_key,
            tags,
            collection_ids,
            attachments,
            note,
            added_at: time::parse(&created_at),
            modified_at: time::parse(&modified_at),
        })
    }

    /// One table row, for refreshing a single item after a change. Finds a
    /// deleted item too: a lookup that lands after a delete still reports
    /// what it found.
    pub fn item_row(&self, id: &str) -> Result<ItemRow> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            &format!("{ITEM_ROW_SELECT} WHERE i.id = ?1"),
            params![id],
            read_item_row,
        )
        .optional()?
        .ok_or_else(|| PaperError::NotFound { id: id.to_string() })
    }

    /// The rows of the library table for `query`. One SQL statement.
    /// Favorites come first whatever the sort, each group in the sort order.
    pub fn list_items(&self, query: &ItemQuery) -> Result<Vec<ItemRow>> {
        let conn = self.conn.lock().unwrap();
        let mut clauses = vec!["i.deleted_at IS NULL".to_string()];
        let mut values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(match_expr) = fts_match_expression(&query.text) {
            clauses.push("i.id IN (SELECT item_id FROM items_fts WHERE items_fts MATCH ?)".into());
            values.push(Box::new(match_expr));
        }
        match &query.scope {
            Scope::All => {}
            Scope::Unread => clauses.push(
                "NOT EXISTS (SELECT 1 FROM attachments a WHERE a.item_id = i.id
                             AND a.deleted_at IS NULL AND a.last_page IS NOT NULL)"
                    .into(),
            ),
            Scope::RecentlyAdded => {
                clauses.push("i.created_at >= ?".into());
                values.push(Box::new(time::days_ago(RECENTLY_ADDED_DAYS)));
            }
            Scope::NoPdf => clauses.push(format!("NOT {HAS_PDF}")),
            Scope::Collection { id } => {
                clauses.push(
                    "EXISTS (SELECT 1 FROM collection_items ci WHERE ci.item_id = i.id AND ci.collection_id = ?)"
                        .into(),
                );
                values.push(Box::new(id.clone()));
            }
            Scope::Tag { id } => {
                clauses.push(
                    "EXISTS (SELECT 1 FROM item_tags it WHERE it.item_id = i.id AND it.tag_id = ?)"
                        .into(),
                );
                values.push(Box::new(id.clone()));
            }
        }

        let direction = if query.ascending { "ASC" } else { "DESC" };
        let key = match query.sort {
            SortColumn::Title => "i.title COLLATE NOCASE",
            SortColumn::FirstAuthor => "first_author COLLATE NOCASE",
            SortColumn::Journal => "coalesce(i.container_short, i.container_title) COLLATE NOCASE",
            SortColumn::Year => "i.year",
            SortColumn::Added => "i.created_at",
        };
        let sql = format!(
            "{ITEM_ROW_SELECT} WHERE {} ORDER BY i.favorite DESC, {key} {direction} NULLS LAST, i.created_at DESC, i.id",
            clauses.join(" AND ")
        );
        let mut statement = conn.prepare_cached(&sql)?;
        let rows = statement
            .query_map(params_from_iter(values.iter()), read_item_row)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Soft deletes items. Returns the ids that were live and are now deleted.
    pub fn delete_items(&self, ids: &[String]) -> Result<Vec<String>> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let now = time::now();
        let mut deleted = Vec::new();
        for id in ids {
            let changed = tx.execute(
                "UPDATE items SET deleted_at = ?2, modified_at = ?2 WHERE id = ?1 AND deleted_at IS NULL",
                params![id, now],
            )?;
            if changed == 1 {
                reindex(&tx, id)?;
                deleted.push(id.clone());
            }
        }
        tx.commit()?;
        Ok(deleted)
    }

    /// Undoes `delete_items`. An item whose DOI was added again since the
    /// delete stays deleted, so the live DOI stays unique. Returns the ids that
    /// are live again.
    pub fn restore_items(&self, ids: &[String]) -> Result<Vec<String>> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let now = time::now();
        let mut restored = Vec::new();
        for id in ids {
            let doi: Option<Option<String>> = tx
                .query_row(
                    "SELECT doi FROM items WHERE id = ?1 AND deleted_at IS NOT NULL",
                    params![id],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(doi) = doi else { continue };
            if let Some(doi) = doi.as_deref()
                && check_doi_free(&tx, doi, Some(id)).is_err()
            {
                continue;
            }
            tx.execute(
                "UPDATE items SET deleted_at = NULL, modified_at = ?2 WHERE id = ?1",
                params![id, now],
            )?;
            reindex(&tx, id)?;
            restored.push(id.clone());
        }
        tx.commit()?;
        Ok(restored)
    }

    pub fn put_source_record(&self, item_id: &str, source: &str, json: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO source_records (item_id, source, fetched_at, json) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (item_id, source) DO UPDATE SET fetched_at = excluded.fetched_at, json = excluded.json",
            params![item_id, source, time::now(), json],
        )?;
        Ok(())
    }

    /// Whether the item has a raw response from `source`.
    pub fn has_source_record(&self, item_id: &str, source: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .prepare_cached("SELECT 1 FROM source_records WHERE item_id = ?1 AND source = ?2")?
            .exists(params![item_id, source])?)
    }

    pub fn source_records(&self, item_id: &str) -> Result<Vec<SourceRecord>> {
        let conn = self.conn.lock().unwrap();
        let records = conn
            .prepare_cached(
                "SELECT source, json FROM source_records WHERE item_id = ?1 ORDER BY source",
            )?
            .query_map(params![item_id], |row| {
                Ok(SourceRecord {
                    source: row.get(0)?,
                    json: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(records)
    }

    // ----- collections -----

    /// All live collections in depth first order with their nesting depth.
    pub fn collection_tree(&self) -> Result<Vec<CollectionNode>> {
        let conn = self.conn.lock().unwrap();
        let flat: Vec<CollectionNode> = conn
            .prepare_cached(
                "SELECT c.id, c.parent_id, c.name, c.position,
                        (SELECT count(*) FROM collection_items ci JOIN items i ON i.id = ci.item_id
                         WHERE ci.collection_id = c.id AND i.deleted_at IS NULL) AS item_count
                 FROM collections c WHERE c.deleted_at IS NULL
                 ORDER BY c.position, c.name COLLATE NOCASE, c.created_at",
            )?
            .query_map([], |row| {
                Ok(CollectionNode {
                    id: row.get("id")?,
                    parent_id: row.get("parent_id")?,
                    name: row.get("name")?,
                    position: row.get("position")?,
                    depth: 0,
                    item_count: row.get("item_count")?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        let mut tree = Vec::with_capacity(flat.len());
        fn visit(
            parent: Option<&str>,
            depth: i64,
            flat: &[CollectionNode],
            tree: &mut Vec<CollectionNode>,
        ) {
            for node in flat
                .iter()
                .filter(|node| node.parent_id.as_deref() == parent)
            {
                tree.push(CollectionNode {
                    depth,
                    ..node.clone()
                });
                visit(Some(&node.id), depth + 1, flat, tree);
            }
        }
        visit(None, 0, &flat, &mut tree);
        Ok(tree)
    }

    /// Creates a collection at the end of its siblings and returns its id.
    pub fn create_collection(&self, name: &str, parent_id: Option<&str>) -> Result<String> {
        let name = non_empty_name(name)?;
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        if let Some(parent) = parent_id {
            require_live_collection(&tx, parent)?;
        }
        let position: i64 = tx.query_row(
            "SELECT coalesce(max(position) + 1, 0) FROM collections
             WHERE parent_id IS ?1 AND deleted_at IS NULL",
            params![parent_id],
            |row| row.get(0),
        )?;
        let id = new_id();
        let now = time::now();
        tx.execute(
            "INSERT INTO collections (id, parent_id, name, position, created_at, modified_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![id, parent_id, name, position, now],
        )?;
        tx.commit()?;
        Ok(id)
    }

    pub fn rename_collection(&self, id: &str, name: &str) -> Result<()> {
        let name = non_empty_name(name)?;
        let conn = self.conn.lock().unwrap();
        let changed = conn.execute(
            "UPDATE collections SET name = ?2, modified_at = ?3 WHERE id = ?1 AND deleted_at IS NULL",
            params![id, name, time::now()],
        )?;
        not_found_if_zero(changed, id)
    }

    /// Moves a collection under `parent_id` (or to the top level) at `position`
    /// among its new siblings, renumbering them. A collection cannot be moved
    /// into itself or one of its descendants.
    pub fn move_collection(&self, id: &str, parent_id: Option<&str>, position: i64) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        require_live_collection(&tx, id)?;
        if let Some(parent) = parent_id {
            require_live_collection(&tx, parent)?;
            let mut ancestor = Some(parent.to_string());
            while let Some(current) = ancestor {
                if current == id {
                    return Err(PaperError::Library {
                        message: "a collection cannot be moved into itself".to_string(),
                    });
                }
                ancestor = tx.query_row(
                    "SELECT parent_id FROM collections WHERE id = ?1",
                    params![current],
                    |row| row.get(0),
                )?;
            }
        }
        let mut siblings: Vec<String> = tx
            .prepare(
                "SELECT id FROM collections WHERE parent_id IS ?1 AND deleted_at IS NULL AND id != ?2
                 ORDER BY position, name COLLATE NOCASE, created_at",
            )?
            .query_map(params![parent_id, id], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let at = position.clamp(0, siblings.len() as i64) as usize;
        siblings.insert(at, id.to_string());
        let now = time::now();
        for (index, sibling) in siblings.iter().enumerate() {
            tx.execute(
                "UPDATE collections SET parent_id = ?2, position = ?3, modified_at = ?4 WHERE id = ?1",
                params![sibling, parent_id, index as i64, now],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Soft deletes a collection and everything nested in it. Items stay in
    /// the library.
    pub fn delete_collection(&self, id: &str) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        require_live_collection(&tx, id)?;
        tx.execute(
            "WITH RECURSIVE subtree(id) AS (
                 SELECT ?1
                 UNION ALL
                 SELECT c.id FROM collections c JOIN subtree ON c.parent_id = subtree.id
                 WHERE c.deleted_at IS NULL
             )
             UPDATE collections SET deleted_at = ?2, modified_at = ?2 WHERE id IN (SELECT id FROM subtree)",
            params![id, time::now()],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Adds items to a collection. Items already in it are left alone.
    pub fn add_to_collection(&self, collection_id: &str, item_ids: &[String]) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        require_live_collection(&tx, collection_id)?;
        let now = time::now();
        for item_id in item_ids {
            require_live_item(&tx, item_id)?;
            tx.execute(
                "INSERT OR IGNORE INTO collection_items (collection_id, item_id, added_at) VALUES (?1, ?2, ?3)",
                params![collection_id, item_id, now],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn remove_from_collection(&self, collection_id: &str, item_ids: &[String]) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        for item_id in item_ids {
            tx.execute(
                "DELETE FROM collection_items WHERE collection_id = ?1 AND item_id = ?2",
                params![collection_id, item_id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    // ----- tags -----

    /// Every tag on a live item, by name. A tag whose only items are in the
    /// trash is kept for undo but not listed.
    pub fn tags(&self) -> Result<Vec<Tag>> {
        let conn = self.conn.lock().unwrap();
        let tags = conn
            .prepare_cached(
                "SELECT t.id, t.name, t.color,
                        (SELECT count(*) FROM item_tags it JOIN items i ON i.id = it.item_id
                         WHERE it.tag_id = t.id AND i.deleted_at IS NULL) AS item_count
                 FROM tags t
                 WHERE EXISTS (SELECT 1 FROM item_tags it JOIN items i ON i.id = it.item_id
                               WHERE it.tag_id = t.id AND i.deleted_at IS NULL)
                 ORDER BY t.name COLLATE NOCASE",
            )?
            .query_map([], read_tag)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(tags)
    }

    /// Replaces an item's tags with `names`, creating tags that do not exist
    /// yet. Names are trimmed, matched ignoring case, and tags that no item
    /// uses afterwards are removed.
    pub fn set_tags(&self, item_id: &str, names: &[String]) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        require_live_item(&tx, item_id)?;
        tx.execute("DELETE FROM item_tags WHERE item_id = ?1", params![item_id])?;
        for name in names {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            let existing: Option<String> = tx
                .query_row(
                    "SELECT id FROM tags WHERE name = ?1",
                    params![name],
                    |row| row.get(0),
                )
                .optional()?;
            let tag_id = match existing {
                Some(id) => id,
                None => {
                    let id = new_id();
                    tx.execute(
                        "INSERT INTO tags (id, name) VALUES (?1, ?2)",
                        params![id, name],
                    )?;
                    id
                }
            };
            tx.execute(
                "INSERT OR IGNORE INTO item_tags (item_id, tag_id) VALUES (?1, ?2)",
                params![item_id, tag_id],
            )?;
        }
        tx.execute(
            "DELETE FROM tags WHERE id NOT IN (SELECT tag_id FROM item_tags)",
            [],
        )?;
        tx.execute(
            "UPDATE items SET modified_at = ?2 WHERE id = ?1",
            params![item_id, time::now()],
        )?;
        reindex(&tx, item_id)?;
        tx.commit()?;
        Ok(())
    }

    /// Marks live items as favorites or clears the mark. Returns the ids
    /// that changed; unknown, deleted, and unchanged items are skipped.
    pub fn set_favorite(&self, ids: &[String], favorite: bool) -> Result<Vec<String>> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let now = time::now();
        let mut changed = Vec::new();
        for id in ids {
            let count = tx.execute(
                "UPDATE items SET favorite = ?2, modified_at = ?3
                 WHERE id = ?1 AND deleted_at IS NULL AND favorite != ?2",
                params![id, favorite, now],
            )?;
            if count > 0 {
                changed.push(id.clone());
            }
        }
        tx.commit()?;
        Ok(changed)
    }

    /// Sets or clears a tag's `#RRGGBB` color.
    pub fn set_tag_color(&self, tag_id: &str, color: Option<&str>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let changed = conn.execute(
            "UPDATE tags SET color = ?2 WHERE id = ?1",
            params![tag_id, color],
        )?;
        not_found_if_zero(changed, tag_id)
    }

    /// Adds tags to an item and keeps the ones it has, creating tags that do
    /// not exist yet. Names are matched ignoring case. Returns the names of
    /// the tags the item did not have before, as the tags spell them.
    pub fn add_tags(&self, item_id: &str, names: &[String]) -> Result<Vec<String>> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        require_live_item(&tx, item_id)?;
        let mut added = Vec::new();
        for name in names {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            let existing: Option<(String, String)> = tx
                .query_row(
                    "SELECT id, name FROM tags WHERE name = ?1",
                    params![name],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            let (tag_id, tag_name) = match existing {
                Some(tag) => tag,
                None => {
                    let id = new_id();
                    tx.execute(
                        "INSERT INTO tags (id, name) VALUES (?1, ?2)",
                        params![id, name],
                    )?;
                    (id, name.to_string())
                }
            };
            let inserted = tx.execute(
                "INSERT OR IGNORE INTO item_tags (item_id, tag_id) VALUES (?1, ?2)",
                params![item_id, tag_id],
            )?;
            if inserted > 0 {
                added.push(tag_name);
            }
        }
        if !added.is_empty() {
            tx.execute(
                "UPDATE items SET modified_at = ?2 WHERE id = ?1",
                params![item_id, time::now()],
            )?;
            reindex(&tx, item_id)?;
        }
        tx.commit()?;
        Ok(added)
    }

    // ----- topics -----

    /// The library's topic list, in the user's order.
    pub fn topics(&self) -> Result<Vec<Topic>> {
        let conn = self.conn.lock().unwrap();
        let topics = conn
            .prepare_cached("SELECT name, description FROM topics ORDER BY position")?
            .query_map([], |row| {
                Ok(Topic {
                    name: row.get(0)?,
                    description: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(topics)
    }

    /// Replaces the topic list. Names are trimmed and must be unique
    /// ignoring case; a topic keeps its id while its name stays. Tags made
    /// from a removed topic stay on their papers.
    pub fn set_topics(&self, topics: &[Topic]) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let now = time::now();
        let mut kept: Vec<String> = Vec::new();
        for (position, topic) in topics.iter().enumerate() {
            let name = non_empty_name(&topic.name)?;
            if kept.iter().any(|known| known.eq_ignore_ascii_case(name)) {
                return Err(PaperError::Library {
                    message: format!("The topic “{name}” is in the list twice."),
                });
            }
            let description = topic.description.trim();
            let id: Option<String> = tx
                .query_row(
                    "SELECT id FROM topics WHERE name = ?1",
                    params![name],
                    |row| row.get(0),
                )
                .optional()?;
            match id {
                Some(id) => {
                    tx.execute(
                        "UPDATE topics SET name = ?2, description = ?3, position = ?4, modified_at = ?5
                         WHERE id = ?1 AND (name != ?2 COLLATE BINARY OR description != ?3 OR position != ?4)",
                        params![id, name, description, position as i64, now],
                    )?;
                }
                None => {
                    tx.execute(
                        "INSERT INTO topics (id, name, description, position, created_at, modified_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                        params![new_id(), name, description, position as i64, now],
                    )?;
                }
            }
            kept.push(name.to_string());
        }
        let placeholders = vec!["?"; kept.len()].join(", ");
        tx.execute(
            &format!("DELETE FROM topics WHERE name NOT IN ({placeholders})"),
            params_from_iter(&kept),
        )?;
        tx.commit()?;
        Ok(())
    }

    // ----- notes -----

    pub fn note(&self, item_id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        read_note(&conn, item_id)
    }

    /// Saves the item's Markdown note. An empty note is removed.
    pub fn save_note(&self, item_id: &str, markdown: &str) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        require_live_item(&tx, item_id)?;
        let now = time::now();
        if markdown.trim().is_empty() {
            tx.execute("DELETE FROM notes WHERE item_id = ?1", params![item_id])?;
        } else {
            tx.execute(
                "INSERT INTO notes (id, item_id, markdown, created_at, modified_at) VALUES (?1, ?2, ?3, ?4, ?4)
                 ON CONFLICT (item_id) DO UPDATE SET markdown = excluded.markdown,
                     modified_at = excluded.modified_at, deleted_at = NULL",
                params![new_id(), item_id, markdown, now],
            )?;
        }
        tx.execute(
            "UPDATE items SET modified_at = ?2 WHERE id = ?1",
            params![item_id, now],
        )?;
        reindex(&tx, item_id)?;
        tx.commit()?;
        Ok(())
    }
}

impl Library {
    // ----- attachments -----

    /// Copies a PDF into `files/<item id>/<title>.pdf` and attaches it to the
    /// item. The bytes go to a temporary name first and are renamed into
    /// place, so a crash never leaves half a PDF in the library. An existing
    /// file of the same name is never overwritten.
    pub fn add_pdf(
        &self,
        item_id: &str,
        bytes: &[u8],
        source_url: Option<&str>,
    ) -> Result<AttachmentRow> {
        let title: String = {
            let conn = self.conn.lock().unwrap();
            require_live_item(&conn, item_id)?;
            conn.query_row(
                "SELECT title FROM items WHERE id = ?1",
                params![item_id],
                |row| row.get(0),
            )?
        };
        let folder = self.files_dir().join(item_id);
        std::fs::create_dir_all(&folder)?;
        let stem = file_stem(&title);
        let mut name = format!("{stem}.pdf");
        let mut copy = 1;
        while folder.join(&name).exists() {
            copy += 1;
            name = format!("{stem} {copy}.pdf");
        }
        let partial = folder.join(format!(".{name}.part"));
        std::fs::write(&partial, bytes)?;
        std::fs::rename(&partial, folder.join(&name))?;

        let id = new_id();
        let now = time::now();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO attachments (id, item_id, relative_path, mime, sha256, byte_size, source_url,
                created_at, modified_at)
             VALUES (?1, ?2, ?3, 'application/pdf', ?4, ?5, ?6, ?7, ?7)",
            params![
                id,
                item_id,
                format!("{item_id}/{name}"),
                sha256_hex(bytes),
                bytes.len() as i64,
                source_url,
                now
            ],
        )?;
        conn.execute(
            "UPDATE items SET modified_at = ?2 WHERE id = ?1",
            params![item_id, now],
        )?;
        read_live_attachment(&conn, &id)
    }

    pub fn attachment(&self, id: &str) -> Result<AttachmentRow> {
        let conn = self.conn.lock().unwrap();
        read_live_attachment(&conn, id)
    }

    /// The absolute path of an attachment's file.
    pub fn attachment_path(&self, attachment: &AttachmentRow) -> PathBuf {
        self.files_dir().join(&attachment.relative_path)
    }

    /// Remembers where the reader left a PDF. Opening a PDF once takes its
    /// item out of the Unread list.
    pub fn set_reading_position(&self, id: &str, page: i32, zoom: f64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let changed = conn.execute(
            "UPDATE attachments SET last_page = ?2, last_zoom = ?3, modified_at = ?4
             WHERE id = ?1 AND deleted_at IS NULL",
            params![id, page, zoom, time::now()],
        )?;
        not_found_if_zero(changed, id)
    }
}

impl Library {
    // ----- annotations -----

    /// The live annotations of an attachment, in reading order: by page, then
    /// from the top of the page down.
    pub fn annotations(&self, attachment_id: &str) -> Result<Vec<Annotation>> {
        let conn = self.conn.lock().unwrap();
        read_live_attachment(&conn, attachment_id)?;
        let mut statement = conn.prepare_cached(&format!(
            "{ANNOTATION_SELECT} WHERE attachment_id = ?1 AND deleted_at IS NULL
             ORDER BY sort_key, created_at"
        ))?;
        let rows = statement.query_map(params![attachment_id], read_annotation)?;
        let mut annotations = Vec::new();
        for row in rows {
            // Kinds this build cannot show are left out, not errors.
            annotations.extend(row?);
        }
        Ok(annotations)
    }

    pub fn annotation(&self, id: &str) -> Result<Annotation> {
        let conn = self.conn.lock().unwrap();
        read_live_annotation(&conn, id)
    }

    /// Stores a new highlight or note and returns it. A highlight's `rect` is
    /// the union of its quads; a note keeps only its `rect`.
    pub fn create_annotation(&self, new: &NewAnnotation) -> Result<Annotation> {
        let color = normalize_color(&new.color)?;
        let (quads, rect) = match new.kind {
            AnnotationKind::Highlight => {
                let rect = union_of_quads(&new.quads)
                    .ok_or_else(|| invalid_annotation("a highlight needs at least one quad"))?;
                (new.quads.as_slice(), rect)
            }
            AnnotationKind::Note => {
                let rect = new
                    .rect
                    .ok_or_else(|| invalid_annotation("a note needs a rect"))?;
                (&[][..], rect)
            }
        };
        if new.page_index < 0 {
            return Err(invalid_annotation("the page index cannot be negative"));
        }
        let id = new_id();
        let now = time::now();
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let item_id = live_attachment_item(&tx, &new.attachment_id)?;
        tx.execute(
            "INSERT INTO annotations (id, attachment_id, kind, page_index, quads, rect, color,
                selected_text, comment, text_before, text_after, sort_key, created_at, modified_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)",
            params![
                id,
                new.attachment_id,
                new.kind.as_str(),
                new.page_index,
                (!quads.is_empty()).then(|| quads_json(quads)),
                rect_json(&rect),
                color,
                new.selected_text,
                new.comment
                    .as_deref()
                    .filter(|comment| !comment.trim().is_empty()),
                new.text_before,
                new.text_after,
                annotation_sort_key(new.page_index, &rect),
                now,
            ],
        )?;
        reindex(&tx, &item_id)?;
        tx.commit()?;
        read_live_annotation(&conn, &id)
    }

    /// Changes an annotation's color or comment and returns it.
    pub fn update_annotation(&self, id: &str, patch: &AnnotationPatch) -> Result<Annotation> {
        let color = patch.color.as_deref().map(normalize_color).transpose()?;
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let before = read_live_annotation(&tx, id)?;
        let item_id = live_attachment_item(&tx, &before.attachment_id)?;
        let comment = match &patch.comment {
            None => before.comment,
            Some(comment) if comment.trim().is_empty() => None,
            Some(comment) => Some(comment.clone()),
        };
        tx.execute(
            "UPDATE annotations SET color = ?2, comment = ?3, modified_at = ?4 WHERE id = ?1",
            params![id, color.unwrap_or(before.color), comment, time::now()],
        )?;
        reindex(&tx, &item_id)?;
        tx.commit()?;
        read_live_annotation(&conn, id)
    }

    /// Moves an annotation to the trash.
    pub fn delete_annotation(&self, id: &str) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let before = read_live_annotation(&tx, id)?;
        let item_id = live_attachment_item(&tx, &before.attachment_id)?;
        let now = time::now();
        tx.execute(
            "UPDATE annotations SET deleted_at = ?2, modified_at = ?2 WHERE id = ?1",
            params![id, now],
        )?;
        reindex(&tx, &item_id)?;
        tx.commit()?;
        Ok(())
    }
}

/// Result of `find_item_by_doi`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoiMatch {
    pub item_id: String,
    pub deleted: bool,
}

// ----- SQL fragments -----

/// True when the item has a live PDF attachment.
const HAS_PDF: &str = "EXISTS (SELECT 1 FROM attachments a WHERE a.item_id = i.id
                               AND a.deleted_at IS NULL AND a.mime = 'application/pdf')";

const ATTACHMENT_SELECT: &str = "
    SELECT id, item_id, relative_path, mime, byte_size, page_count, last_page, last_zoom, created_at
    FROM attachments";

fn read_attachment(row: &Row<'_>) -> rusqlite::Result<AttachmentRow> {
    Ok(AttachmentRow {
        id: row.get("id")?,
        item_id: row.get("item_id")?,
        relative_path: row.get("relative_path")?,
        mime: row.get("mime")?,
        byte_size: row.get("byte_size")?,
        page_count: row.get("page_count")?,
        last_page: row.get("last_page")?,
        last_zoom: row.get("last_zoom")?,
        added_at: time::parse(&row.get::<_, String>("created_at")?),
    })
}

fn read_live_attachment(conn: &Connection, id: &str) -> Result<AttachmentRow> {
    conn.query_row(
        &format!("{ATTACHMENT_SELECT} WHERE id = ?1 AND deleted_at IS NULL"),
        params![id],
        read_attachment,
    )
    .optional()?
    .ok_or_else(|| PaperError::NotFound { id: id.to_string() })
}

const ANNOTATION_SELECT: &str = "
    SELECT id, attachment_id, kind, page_index, quads, rect, color, selected_text, comment,
           created_at, modified_at
    FROM annotations";

/// `None` for a kind this build does not know or geometry it cannot read.
fn read_annotation(row: &Row<'_>) -> rusqlite::Result<Option<Annotation>> {
    let id: String = row.get("id")?;
    let Some(kind) = AnnotationKind::from_db(&row.get::<_, String>("kind")?) else {
        return Ok(None);
    };
    let quads = match row.get::<_, Option<String>>("quads")? {
        Some(json) => parse_quads(&json),
        None => Some(Vec::new()),
    };
    let rect = row
        .get::<_, Option<String>>("rect")?
        .and_then(|json| parse_rect(&json));
    let (Some(quads), Some(rect)) = (quads, rect) else {
        tracing::warn!(%id, "skipping an annotation with unreadable geometry");
        return Ok(None);
    };
    Ok(Some(Annotation {
        id,
        attachment_id: row.get("attachment_id")?,
        kind,
        page_index: row.get("page_index")?,
        quads,
        rect,
        color: row.get("color")?,
        selected_text: row.get("selected_text")?,
        comment: row.get("comment")?,
        created_at: time::parse(&row.get::<_, String>("created_at")?),
        modified_at: time::parse(&row.get::<_, String>("modified_at")?),
    }))
}

fn read_live_annotation(conn: &Connection, id: &str) -> Result<Annotation> {
    conn.query_row(
        &format!("{ANNOTATION_SELECT} WHERE id = ?1 AND deleted_at IS NULL"),
        params![id],
        read_annotation,
    )
    .optional()?
    .flatten()
    .ok_or_else(|| PaperError::NotFound { id: id.to_string() })
}

/// The item a live attachment belongs to.
fn live_attachment_item(conn: &Connection, attachment_id: &str) -> Result<String> {
    Ok(read_live_attachment(conn, attachment_id)?.item_id)
}

fn invalid_annotation(message: &str) -> PaperError {
    PaperError::Library {
        message: format!("invalid annotation: {message}"),
    }
}

/// `#RRGGBB` in uppercase, or an error for anything else.
fn normalize_color(color: &str) -> Result<String> {
    let hex = color.strip_prefix('#').unwrap_or("");
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(invalid_annotation(&format!(
            "{color} is not a #RRGGBB color"
        )));
    }
    Ok(format!("#{}", hex.to_ascii_uppercase()))
}

fn union_of_quads(quads: &[Quad]) -> Option<PageRect> {
    let points = quads.iter().flat_map(|quad| {
        [
            quad.top_left,
            quad.top_right,
            quad.bottom_left,
            quad.bottom_right,
        ]
    });
    points.fold(None, |rect: Option<PageRect>, point| {
        Some(match rect {
            None => PageRect {
                x0: point.x,
                y0: point.y,
                x1: point.x,
                y1: point.y,
            },
            Some(rect) => PageRect {
                x0: rect.x0.min(point.x),
                y0: rect.y0.min(point.y),
                x1: rect.x1.max(point.x),
                y1: rect.y1.max(point.y),
            },
        })
    })
}

/// Page first, then the top edge from the top of the page down. PDF user
/// space grows upwards, so the key counts down from a height no page reaches.
fn annotation_sort_key(page_index: i32, rect: &PageRect) -> String {
    const ABOVE_ANY_PAGE: f64 = 1_000_000.0;
    let top = rect.y0.max(rect.y1);
    format!(
        "{page_index:06}:{:014.3}",
        (ABOVE_ANY_PAGE - top).clamp(0.0, 2.0 * ABOVE_ANY_PAGE)
    )
}

/// `[[x1,y1,x2,y2,x3,y3,x4,y4], ...]` in `QuadPoints` order.
fn quads_json(quads: &[Quad]) -> String {
    let arrays: Vec<[f64; 8]> = quads
        .iter()
        .map(|q| {
            [
                q.top_left.x,
                q.top_left.y,
                q.top_right.x,
                q.top_right.y,
                q.bottom_left.x,
                q.bottom_left.y,
                q.bottom_right.x,
                q.bottom_right.y,
            ]
        })
        .collect();
    serde_json::to_string(&arrays).expect("numbers serialize")
}

fn parse_quads(json: &str) -> Option<Vec<Quad>> {
    let arrays: Vec<[f64; 8]> = serde_json::from_str(json).ok()?;
    Some(
        arrays
            .into_iter()
            .map(|[x1, y1, x2, y2, x3, y3, x4, y4]| Quad {
                top_left: PagePoint { x: x1, y: y1 },
                top_right: PagePoint { x: x2, y: y2 },
                bottom_left: PagePoint { x: x3, y: y3 },
                bottom_right: PagePoint { x: x4, y: y4 },
            })
            .collect(),
    )
}

/// `[x0, y0, x1, y1]`.
fn rect_json(rect: &PageRect) -> String {
    serde_json::to_string(&[rect.x0, rect.y0, rect.x1, rect.y1]).expect("numbers serialize")
}

fn parse_rect(json: &str) -> Option<PageRect> {
    let [x0, y0, x1, y1]: [f64; 4] = serde_json::from_str(json).ok()?;
    Some(PageRect { x0, y0, x1, y1 })
}

/// A file name from a paper title: no path separators or characters that
/// Windows and macOS reject, no leading dot, at most 120 characters.
fn file_stem(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let short: String = collapsed
        .trim_matches(|c| c == '.' || c == ' ')
        .chars()
        .take(120)
        .collect();
    let short = short.trim_end_matches(['.', ' ']);
    if short.is_empty() {
        "Paper".to_string()
    } else {
        short.to_string()
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The columns of `ItemRow`, plus `first_author` for sorting.
const ITEM_ROW_SELECT: &str = "
    SELECT i.id, i.kind, i.title, i.container_short, i.container_title, i.year, i.doi,
           i.lookup_status, i.created_at, i.favorite,
           (SELECT group_concat(coalesce(family, literal, given), char(31) ORDER BY position)
            FROM creators c WHERE c.item_id = i.id AND c.role = 'author') AS author_names,
           (SELECT coalesce(family, literal, given) FROM creators c
            WHERE c.item_id = i.id AND c.role = 'author' ORDER BY position LIMIT 1) AS first_author,
           EXISTS (SELECT 1 FROM attachments a WHERE a.item_id = i.id
                   AND a.deleted_at IS NULL AND a.mime = 'application/pdf') AS has_pdf
    FROM items i";

fn read_item_row(row: &Row<'_>) -> rusqlite::Result<ItemRow> {
    let names: Option<String> = row.get("author_names")?;
    let container_short: Option<String> = row.get("container_short")?;
    let container_title: Option<String> = row.get("container_title")?;
    Ok(ItemRow {
        id: row.get("id")?,
        kind: ItemKind::from_db(&row.get::<_, String>("kind")?),
        title: row.get("title")?,
        authors: format_authors(names.as_deref().unwrap_or("").split(NAME_SEPARATOR)),
        journal: container_short.or(container_title),
        year: row.get("year")?,
        doi: row.get("doi")?,
        has_pdf: row.get("has_pdf")?,
        favorite: row.get("favorite")?,
        lookup_status: LookupStatus::from_db(&row.get::<_, String>("lookup_status")?),
        added_at: time::parse(&row.get::<_, String>("created_at")?),
    })
}

fn read_tag(row: &Row<'_>) -> rusqlite::Result<Tag> {
    Ok(Tag {
        id: row.get("id")?,
        name: row.get("name")?,
        color: row.get("color")?,
        item_count: row.get("item_count")?,
    })
}

fn read_note(conn: &Connection, item_id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT markdown FROM notes WHERE item_id = ?1 AND deleted_at IS NULL",
            params![item_id],
            |row| row.get(0),
        )
        .optional()?)
}

/// "Vaswani", "Vaswani and Shazeer", "Vaswani et al.", or "" for no authors.
pub fn format_authors<'a>(names: impl IntoIterator<Item = &'a str>) -> String {
    let names: Vec<&str> = names.into_iter().filter(|name| !name.is_empty()).collect();
    match names.as_slice() {
        [] => String::new(),
        [one] => one.to_string(),
        [first, second] => format!("{first} and {second}"),
        [first, ..] => format!("{first} et al."),
    }
}

/// Turns typed text into an FTS5 query: each word is a quoted prefix term, and
/// all words must match. `None` when there is nothing to search for.
fn fts_match_expression(text: &str) -> Option<String> {
    let terms: Vec<String> = text
        .split(|c: char| c.is_whitespace() || c == '"')
        .filter(|word| word.chars().any(char::is_alphanumeric))
        .map(|word| format!("\"{word}\"*"))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

// ----- connection -----

fn open_connection(path: &Path, create: bool) -> Result<Connection> {
    let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    if create {
        flags |= OpenFlags::SQLITE_OPEN_CREATE;
    }
    let conn = Connection::open_with_flags(path, flags)?;
    configure(&conn)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    migrate(&conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> Result<()> {
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(())
}

/// Applies the numbered migrations above the database's `user_version`.
fn migrate(conn: &Connection) -> Result<()> {
    let version: usize =
        conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))? as usize;
    if version > MIGRATIONS.len() {
        return Err(PaperError::Library {
            message: format!(
                "the library was written by a newer version of Paper (schema {version}, this build knows {})",
                MIGRATIONS.len()
            ),
        });
    }
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(version) {
        let next = index + 1;
        tracing::info!(version = next, "applying migration");
        conn.execute_batch(&format!(
            "BEGIN; {sql} PRAGMA user_version = {next}; COMMIT;"
        ))?;
    }
    Ok(())
}

// ----- helpers used inside transactions -----

fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

fn non_empty_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(PaperError::Library {
            message: "the name cannot be empty".to_string(),
        });
    }
    Ok(name)
}

fn not_found_if_zero(changed: usize, id: &str) -> Result<()> {
    if changed == 0 {
        return Err(PaperError::NotFound { id: id.to_string() });
    }
    Ok(())
}

fn require_item(conn: &Connection, id: &str) -> Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM items WHERE id = ?1)",
        params![id],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(PaperError::NotFound { id: id.to_string() });
    }
    Ok(())
}

fn require_live_item(conn: &Connection, id: &str) -> Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM items WHERE id = ?1 AND deleted_at IS NULL)",
        params![id],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(PaperError::NotFound { id: id.to_string() });
    }
    Ok(())
}

fn require_live_collection(conn: &Connection, id: &str) -> Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM collections WHERE id = ?1 AND deleted_at IS NULL)",
        params![id],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(PaperError::NotFound { id: id.to_string() });
    }
    Ok(())
}

fn find_by_doi(conn: &Connection, doi: &str) -> Result<Option<DoiMatch>> {
    Ok(conn
        .query_row(
            "SELECT id, deleted_at IS NOT NULL FROM items WHERE doi = ?1
             ORDER BY deleted_at IS NOT NULL, modified_at DESC LIMIT 1",
            params![doi],
            |row| {
                Ok(DoiMatch {
                    item_id: row.get(0)?,
                    deleted: row.get(1)?,
                })
            },
        )
        .optional()?)
}

/// `""`, then `a` to `z`, then `aa`, `ab`, and so on.
fn key_suffix(mut index: usize) -> String {
    let mut suffix = Vec::new();
    while index > 0 {
        index -= 1;
        suffix.push(b'a' + (index % 26) as u8);
        index /= 26;
    }
    suffix.reverse();
    String::from_utf8(suffix).unwrap()
}

/// Whether an item other than `except`, deleted or not, has this key in any
/// case. Deleted items keep their keys so an undo brings them back intact.
fn citation_key_taken(conn: &Connection, key: &str, except: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM items WHERE citation_key = ?1 COLLATE NOCASE AND id != ?2)",
        params![key, except],
        |row| row.get(0),
    )?)
}

fn check_citation_key_free(conn: &Connection, key: &str, except: &str) -> Result<()> {
    if citation_key_taken(conn, key, except)? {
        return Err(PaperError::Library {
            message: format!("Another paper already has the citation key “{key}”."),
        });
    }
    Ok(())
}

/// Errors with `Duplicate` when a live item other than `except` has this DOI.
fn check_doi_free(conn: &Connection, doi: &str, except: Option<&str>) -> Result<()> {
    match find_by_doi(conn, doi)? {
        Some(found) if !found.deleted && except != Some(found.item_id.as_str()) => {
            Err(PaperError::Duplicate {
                doi: doi.to_string(),
                item_id: found.item_id,
            })
        }
        _ => Ok(()),
    }
}

fn write_creators(tx: &Transaction<'_>, item_id: &str, creators: &[Creator]) -> Result<()> {
    let mut statement = tx.prepare_cached(
        "INSERT INTO creators (id, item_id, role, position, family, given, literal, orcid)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    for (position, creator) in creators.iter().enumerate() {
        statement.execute(params![
            new_id(),
            item_id,
            creator.role.unwrap_or(CreatorRole::Author).as_str(),
            position as i64,
            creator.family,
            creator.given,
            creator.literal,
            creator.orcid,
        ])?;
    }
    Ok(())
}

/// Rebuilds the search index row of one item from the current tables. Deleted
/// items get no row.
fn reindex(tx: &Transaction<'_>, item_id: &str) -> Result<()> {
    tx.execute("DELETE FROM items_fts WHERE item_id = ?1", params![item_id])?;
    tx.execute(
        "INSERT INTO items_fts (item_id, title, authors, container_title, abstract, tags, note, annotation_text)
         SELECT i.id, i.title,
                (SELECT group_concat(trim(coalesce(given, '') || ' ' || coalesce(family, '') || ' ' || coalesce(literal, '')), ', ')
                 FROM creators c WHERE c.item_id = i.id),
                trim(coalesce(i.container_title, '') || ' ' || coalesce(i.container_short, '')),
                i.abstract,
                (SELECT group_concat(t.name, ', ') FROM item_tags it JOIN tags t ON t.id = it.tag_id WHERE it.item_id = i.id),
                (SELECT markdown FROM notes n WHERE n.item_id = i.id AND n.deleted_at IS NULL),
                (SELECT group_concat(coalesce(an.selected_text, '') || ' ' || coalesce(an.comment, ''), ' ')
                 FROM annotations an JOIN attachments at ON at.id = an.attachment_id
                 WHERE at.item_id = i.id AND an.deleted_at IS NULL)
         FROM items i WHERE i.id = ?1 AND i.deleted_at IS NULL",
        params![item_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paper(
        title: &str,
        doi: Option<&str>,
        authors: &[(&str, &str)],
        year: Option<i32>,
    ) -> ItemFields {
        ItemFields {
            kind: Some(ItemKind::JournalArticle),
            doi: doi.map(str::to_string),
            title: title.to_string(),
            creators: authors
                .iter()
                .map(|(given, family)| Creator::person(given, family))
                .collect(),
            year,
            ..ItemFields::default()
        }
    }

    fn titles(library: &Library, query: &ItemQuery) -> Vec<String> {
        library
            .list_items(query)
            .unwrap()
            .into_iter()
            .map(|row| row.title)
            .collect()
    }

    fn search(text: &str) -> ItemQuery {
        ItemQuery {
            text: text.to_string(),
            sort: SortColumn::Title,
            ascending: true,
            ..ItemQuery::default()
        }
    }

    fn seeded() -> (Library, String, String, String) {
        let library = Library::open_in_memory().unwrap();
        let deep = library
            .insert_item(
                &ItemFields {
                    container_title: Some("Nature".into()),
                    abstract_text: Some("Deep learning allows computational models".into()),
                    ..paper(
                        "Deep learning",
                        Some("10.1038/nature14539"),
                        &[
                            ("Yann", "LeCun"),
                            ("Yoshua", "Bengio"),
                            ("Geoffrey", "Hinton"),
                        ],
                        Some(2015),
                    )
                },
                LookupStatus::Done,
            )
            .unwrap();
        let attention = library
            .insert_item(
                &ItemFields {
                    kind: Some(ItemKind::Preprint),
                    arxiv_id: Some("1706.03762".into()),
                    ..paper(
                        "Attention Is All You Need",
                        Some("10.48550/arxiv.1706.03762"),
                        &[("Ashish", "Vaswani"), ("Noam", "Shazeer")],
                        Some(2017),
                    )
                },
                LookupStatus::Done,
            )
            .unwrap();
        let xgboost = library
            .insert_item(
                &ItemFields {
                    container_title: Some("Proceedings of the 22nd ACM SIGKDD".into()),
                    container_short: Some("KDD '16".into()),
                    ..paper(
                        "XGBoost: A Scalable Tree Boosting System",
                        Some("10.1145/2939672.2939785"),
                        &[("Tianqi", "Chen")],
                        None,
                    )
                },
                LookupStatus::Done,
            )
            .unwrap();
        (library, deep, attention, xgboost)
    }

    #[test]
    fn library_folders_are_created_opened_and_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("My Library");
        assert!(!Library::is_library(&path));
        assert!(matches!(
            Library::open(&path),
            Err(PaperError::Library { .. })
        ));

        let library = Library::create(&path).unwrap();
        let id = library
            .insert_item(&paper("Persisted", None, &[], None), LookupStatus::Done)
            .unwrap();
        drop(library);

        assert!(Library::is_library(&path));
        assert!(path.join(LIBRARY_FILES_DIR).is_dir());
        assert!(matches!(
            Library::create(&path),
            Err(PaperError::Library { .. })
        ));
        let reopened = Library::open(&path).unwrap();
        assert_eq!(reopened.item(&id).unwrap().fields.title, "Persisted");
        assert_eq!(reopened.dir(), path);
        drop(reopened);
        assert!(
            !path.join(format!("{LIBRARY_DB_NAME}-wal")).exists(),
            "closing folds the WAL into the database file"
        );

        Connection::open(path.join(LIBRARY_DB_NAME))
            .unwrap()
            .pragma_update(None, "user_version", 99)
            .unwrap();
        assert!(
            matches!(
                Library::open(&path),
                Err(PaperError::Library { message }) if message.contains("newer version")
            ),
            "a database from a newer Paper is refused, not migrated or broken"
        );
    }

    #[test]
    fn purging_an_item_removes_it_and_everything_attached_to_it() {
        let (library, deep, attention, _) = seeded();
        let collection = library.create_collection("Reading", None).unwrap();
        library
            .add_to_collection(&collection, &[deep.clone(), attention.clone()])
            .unwrap();
        library.set_tags(&deep, &["only-here".into()]).unwrap();
        library.save_note(&deep, "gone").unwrap();
        library.put_source_record(&deep, "crossref", "{}").unwrap();

        library.purge_item(&deep).unwrap();

        assert!(matches!(
            library.item(&deep),
            Err(PaperError::NotFound { .. })
        ));
        assert!(matches!(
            library.item_row(&deep),
            Err(PaperError::NotFound { .. })
        ));
        assert!(matches!(
            library.purge_item(&deep),
            Err(PaperError::NotFound { .. })
        ));
        assert!(
            library
                .find_item_by_doi("10.1038/nature14539")
                .unwrap()
                .is_none()
        );
        assert!(library.tags().unwrap().is_empty(), "its tag is pruned");
        assert!(titles(&library, &search("gone")).is_empty());
        assert!(library.source_records(&deep).unwrap().is_empty());
        assert_eq!(library.collection_tree().unwrap()[0].item_count, 1);
        assert_eq!(
            library.item(&attention).unwrap().collection_ids,
            [collection]
        );
    }

    #[test]
    fn rows_format_authors_and_prefer_the_short_journal_name() {
        let (library, ..) = seeded();
        let rows = library.list_items(&search("")).unwrap();
        let by_title: Vec<(&str, &str, Option<&str>)> = rows
            .iter()
            .map(|row| {
                (
                    row.title.as_str(),
                    row.authors.as_str(),
                    row.journal.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            by_title,
            vec![
                ("Attention Is All You Need", "Vaswani and Shazeer", None),
                ("Deep learning", "LeCun et al.", Some("Nature")),
                (
                    "XGBoost: A Scalable Tree Boosting System",
                    "Chen",
                    Some("KDD '16")
                ),
            ]
        );
        assert!(
            rows.iter()
                .all(|row| !row.has_pdf && row.lookup_status == LookupStatus::Done)
        );
        assert_eq!(rows[0].kind, ItemKind::Preprint);
    }

    #[test]
    fn sorting_puts_missing_values_last_in_both_directions() {
        let (library, ..) = seeded();
        let year = |ascending| ItemQuery {
            sort: SortColumn::Year,
            ascending,
            ..ItemQuery::default()
        };
        assert_eq!(
            titles(&library, &year(true)),
            [
                "Deep learning",
                "Attention Is All You Need",
                "XGBoost: A Scalable Tree Boosting System"
            ]
        );
        assert_eq!(
            titles(&library, &year(false)),
            [
                "Attention Is All You Need",
                "Deep learning",
                "XGBoost: A Scalable Tree Boosting System"
            ]
        );
        let author = ItemQuery {
            sort: SortColumn::FirstAuthor,
            ascending: true,
            ..ItemQuery::default()
        };
        assert_eq!(
            titles(&library, &author),
            [
                "XGBoost: A Scalable Tree Boosting System",
                "Deep learning",
                "Attention Is All You Need"
            ]
        );
    }

    #[test]
    fn favorites_come_first_in_every_sort_and_scope() {
        let (library, deep, attention, xgboost) = seeded();
        let changed = library
            .set_favorite(&[xgboost.clone(), deep.clone(), "missing".into()], true)
            .unwrap();
        assert_eq!(changed, [xgboost.clone(), deep.clone()]);
        assert!(
            library
                .set_favorite(std::slice::from_ref(&deep), true)
                .unwrap()
                .is_empty(),
            "an item that is already a favorite does not change"
        );
        let by = |sort, ascending, scope| ItemQuery {
            sort,
            ascending,
            scope,
            ..ItemQuery::default()
        };
        // Each group keeps the sort order, missing years last.
        assert_eq!(
            titles(&library, &by(SortColumn::Year, false, Scope::All)),
            [
                "Deep learning",
                "XGBoost: A Scalable Tree Boosting System",
                "Attention Is All You Need"
            ]
        );
        assert_eq!(
            titles(&library, &by(SortColumn::Title, true, Scope::All)),
            [
                "Deep learning",
                "XGBoost: A Scalable Tree Boosting System",
                "Attention Is All You Need"
            ]
        );
        assert_eq!(
            titles(&library, &by(SortColumn::Title, false, Scope::All)),
            [
                "XGBoost: A Scalable Tree Boosting System",
                "Deep learning",
                "Attention Is All You Need"
            ]
        );
        let collection = library.create_collection("Reading", None).unwrap();
        library
            .add_to_collection(&collection, &[attention.clone(), xgboost.clone()])
            .unwrap();
        assert_eq!(
            titles(
                &library,
                &by(
                    SortColumn::Title,
                    true,
                    Scope::Collection { id: collection }
                )
            ),
            [
                "XGBoost: A Scalable Tree Boosting System",
                "Attention Is All You Need"
            ]
        );
        assert!(library.item_row(&xgboost).unwrap().favorite);
        assert!(!library.item_row(&attention).unwrap().favorite);

        library
            .set_favorite(std::slice::from_ref(&xgboost), false)
            .unwrap();
        assert!(!library.item_row(&xgboost).unwrap().favorite);
        assert_eq!(
            titles(&library, &by(SortColumn::Title, true, Scope::All))[0],
            "Deep learning"
        );
    }

    #[test]
    fn search_matches_prefixes_of_every_word_across_fields() {
        let (library, deep, attention, _) = seeded();
        library
            .set_tags(&attention, &["transformers".into()])
            .unwrap();
        library
            .save_note(&deep, "Read for the seminar on backprop")
            .unwrap();

        assert_eq!(titles(&library, &search("deep")), ["Deep learning"]);
        assert_eq!(
            titles(&library, &search("vasw")),
            ["Attention Is All You Need"],
            "by author prefix"
        );
        assert_eq!(
            titles(&library, &search("transfor")),
            ["Attention Is All You Need"],
            "by tag"
        );
        assert_eq!(
            titles(&library, &search("seminar")),
            ["Deep learning"],
            "by note"
        );
        assert_eq!(
            titles(&library, &search("nature")),
            ["Deep learning"],
            "by journal"
        );
        assert_eq!(
            titles(&library, &search("computational models")),
            ["Deep learning"],
            "by abstract"
        );
        assert_eq!(
            titles(&library, &search("lecun hinton")),
            ["Deep learning"],
            "all words must match"
        );
        assert!(titles(&library, &search("lecun vaswani")).is_empty());
        assert_eq!(
            titles(&library, &search("\"tree\" (")).len(),
            1,
            "punctuation does not break the query"
        );
        assert!(
            titles(&library, &search("deep OR NOT -learn* AND ^\"")).is_empty(),
            "FTS operators are quoted as plain words, not syntax (and no SQL error)"
        );
        assert_eq!(
            titles(&library, &search("   ")).len(),
            3,
            "blank text matches everything"
        );

        library.set_tags(&attention, &[]).unwrap();
        assert!(
            titles(&library, &search("transfor")).is_empty(),
            "the index follows tag changes"
        );
    }

    #[test]
    fn a_doi_is_unique_among_live_items_only() {
        let (library, deep, ..) = seeded();
        let again = paper(
            "Deep learning again",
            Some("10.1038/nature14539"),
            &[],
            None,
        );
        assert_eq!(
            library.insert_item(&again, LookupStatus::Done),
            Err(PaperError::Duplicate {
                doi: "10.1038/nature14539".into(),
                item_id: deep.clone()
            })
        );

        assert_eq!(
            library.delete_items(std::slice::from_ref(&deep)).unwrap(),
            vec![deep.clone()]
        );
        assert_eq!(
            library.find_item_by_doi("10.1038/nature14539").unwrap(),
            Some(DoiMatch {
                item_id: deep.clone(),
                deleted: true
            })
        );
        let replacement = library.insert_item(&again, LookupStatus::Pending).unwrap();
        assert_eq!(
            library.find_item_by_doi("10.1038/nature14539").unwrap(),
            Some(DoiMatch {
                item_id: replacement.clone(),
                deleted: false
            }),
            "the live item wins over the deleted one"
        );
        assert!(
            library
                .restore_items(std::slice::from_ref(&deep))
                .unwrap()
                .is_empty(),
            "restoring would duplicate the DOI"
        );
        assert!(matches!(
            library.item(&deep),
            Err(PaperError::NotFound { .. })
        ));

        let patch = MetadataPatch {
            doi: Some("10.48550/arxiv.1706.03762".into()),
            ..MetadataPatch::default()
        };
        assert!(matches!(
            library.update_metadata(&replacement, &patch),
            Err(PaperError::Duplicate { .. })
        ));
    }

    #[test]
    fn deleted_items_leave_every_view_and_come_back_on_restore() {
        let (library, deep, attention, _) = seeded();
        let collection = library.create_collection("Reading", None).unwrap();
        library
            .add_to_collection(&collection, &[deep.clone(), attention.clone()])
            .unwrap();
        library.set_tags(&deep, &["ml".into()]).unwrap();

        let deleted = library
            .delete_items(&[deep.clone(), "missing".into(), deep.clone()])
            .unwrap();
        assert_eq!(
            deleted,
            vec![deep.clone()],
            "only ids that were live are reported"
        );
        assert_eq!(titles(&library, &search("")).len(), 2);
        assert!(
            titles(&library, &search("lecun")).is_empty(),
            "deleted items are not searchable"
        );
        assert_eq!(library.collection_tree().unwrap()[0].item_count, 1);
        assert!(matches!(
            library.item(&deep),
            Err(PaperError::NotFound { .. })
        ));
        assert!(
            library.tags().unwrap().is_empty(),
            "a tag with only deleted items is not listed"
        );
        library
            .set_lookup_status(&deep, LookupStatus::Done)
            .expect("a lookup that lands after the delete still writes the trash row");
        assert!(matches!(
            library.set_lookup_status("missing", LookupStatus::Done),
            Err(PaperError::NotFound { .. })
        ));

        assert_eq!(library.restore_items(&deleted).unwrap(), vec![deep.clone()]);
        assert!(
            library.restore_items(&deleted).unwrap().is_empty(),
            "already live"
        );
        assert_eq!(titles(&library, &search("lecun")), ["Deep learning"]);
        assert_eq!(library.collection_tree().unwrap()[0].item_count, 2);
        assert_eq!(library.tags().unwrap()[0].item_count, 1);
        assert_eq!(library.item(&deep).unwrap().collection_ids, [collection]);
    }

    #[test]
    fn patches_leave_none_alone_clear_empty_and_ignore_blank_titles() {
        let (library, deep, ..) = seeded();
        let patch = MetadataPatch {
            title: Some("   ".into()),
            container_title: Some(String::new()),
            volume: Some("521".into()),
            year: Some(2016),
            creators: Some(vec![Creator::person("Yann", "LeCun")]),
            ..MetadataPatch::default()
        };
        library.update_metadata(&deep, &patch).unwrap();
        let detail = library.item(&deep).unwrap();
        assert_eq!(
            detail.fields.title, "Deep learning",
            "a blank title is ignored"
        );
        assert_eq!(
            detail.fields.container_title, None,
            "an empty string clears"
        );
        assert_eq!(detail.fields.volume.as_deref(), Some("521"));
        assert_eq!(detail.fields.year, Some(2016));
        assert_eq!(
            detail.fields.doi.as_deref(),
            Some("10.1038/nature14539"),
            "None leaves the field alone"
        );
        assert_eq!(
            detail.fields.creators,
            vec![Creator::person("Yann", "LeCun")]
        );
        assert_eq!(library.item_row(&deep).unwrap().authors, "LeCun");
        assert!(
            titles(&library, &search("hinton")).is_empty(),
            "the index follows the edit"
        );
        assert!(detail.modified_at >= detail.added_at);

        assert!(matches!(
            library.update_metadata("missing", &patch),
            Err(PaperError::NotFound { .. })
        ));
    }

    #[test]
    fn replace_metadata_overwrites_everything_and_records_the_lookup_status() {
        let (library, deep, ..) = seeded();
        library
            .set_lookup_status(&deep, LookupStatus::Pending)
            .unwrap();
        assert_eq!(
            library.item_row(&deep).unwrap().lookup_status,
            LookupStatus::Pending
        );
        let fields = paper(
            "Replaced",
            Some("10.1038/nature14539"),
            &[("A", "Author")],
            Some(2020),
        );
        library
            .replace_metadata(&deep, &fields, LookupStatus::Done)
            .unwrap();
        let detail = library.item(&deep).unwrap();
        assert_eq!(detail.fields, fields);
        assert_eq!(detail.lookup_status, LookupStatus::Done);
        assert_eq!(library.item_row(&deep).unwrap().journal, None);

        library
            .put_source_record(&deep, "crossref", "{\"a\":1}")
            .unwrap();
        library
            .put_source_record(&deep, "crossref", "{\"a\":2}")
            .unwrap();
        library.put_source_record(&deep, "doi_org", "{}").unwrap();
        let records = library.source_records(&deep).unwrap();
        assert_eq!(records.len(), 2, "one record per source, the latest wins");
        assert_eq!(records[0].json, "{\"a\":2}");
    }

    #[test]
    fn collections_nest_move_and_delete_as_a_tree() {
        let (library, deep, attention, _) = seeded();
        let ml = library
            .create_collection("  Machine learning ", None)
            .unwrap();
        let nlp = library.create_collection("NLP", Some(&ml)).unwrap();
        let vision = library.create_collection("Vision", Some(&ml)).unwrap();
        let misc = library.create_collection("Misc", None).unwrap();
        library
            .add_to_collection(&nlp, std::slice::from_ref(&attention))
            .unwrap();
        library
            .add_to_collection(&ml, &[deep.clone(), attention.clone(), attention.clone()])
            .unwrap();
        assert!(matches!(
            library.create_collection(" ", None),
            Err(PaperError::Library { .. })
        ));
        assert!(matches!(
            library.create_collection("x", Some("missing")),
            Err(PaperError::NotFound { .. })
        ));

        let summary = |library: &Library| -> Vec<(String, i64, i64)> {
            library
                .collection_tree()
                .unwrap()
                .into_iter()
                .map(|node| (node.name, node.depth, node.item_count))
                .collect()
        };
        assert_eq!(
            summary(&library),
            vec![
                ("Machine learning".into(), 0, 2),
                ("NLP".into(), 1, 1),
                ("Vision".into(), 1, 0),
                ("Misc".into(), 0, 0),
            ]
        );
        let in_ml = ItemQuery {
            scope: Scope::Collection { id: ml.clone() },
            ..search("")
        };
        assert_eq!(titles(&library, &in_ml).len(), 2);
        let in_nlp = ItemQuery {
            scope: Scope::Collection { id: nlp.clone() },
            ..search("")
        };
        assert_eq!(
            titles(&library, &in_nlp),
            ["Attention Is All You Need"],
            "only direct members"
        );

        assert!(
            matches!(
                library.move_collection(&ml, Some(&nlp), 0),
                Err(PaperError::Library { .. })
            ),
            "no cycles"
        );
        library.move_collection(&misc, Some(&ml), 0).unwrap();
        library.move_collection(&vision, None, 0).unwrap();
        assert_eq!(
            summary(&library)
                .iter()
                .map(|(name, depth, _)| (name.as_str(), *depth))
                .collect::<Vec<_>>(),
            [
                ("Vision", 0),
                ("Machine learning", 0),
                ("Misc", 1),
                ("NLP", 1)
            ]
        );
        let positions: Vec<i64> = library
            .collection_tree()
            .unwrap()
            .iter()
            .map(|node| node.position)
            .collect();
        assert_eq!(positions, [0, 1, 0, 1], "siblings are renumbered");

        library.rename_collection(&nlp, "Language").unwrap();
        library
            .remove_from_collection(&ml, std::slice::from_ref(&deep))
            .unwrap();
        library.delete_collection(&ml).unwrap();
        assert_eq!(
            summary(&library),
            vec![("Vision".into(), 0, 0)],
            "subcollections go with their parent"
        );
        assert!(library.item(&attention).unwrap().collection_ids.is_empty());
        assert!(matches!(
            library.add_to_collection(&ml, &[deep]),
            Err(PaperError::NotFound { .. })
        ));
    }

    #[test]
    fn tags_are_shared_case_insensitively_and_pruned_when_unused() {
        let (library, deep, attention, _) = seeded();
        library
            .set_tags(&deep, &["ML".into(), " to-read ".into(), "".into()])
            .unwrap();
        library
            .set_tags(&attention, &["ml".into(), "nlp".into()])
            .unwrap();
        let tags = library.tags().unwrap();
        let names: Vec<(&str, i64)> = tags
            .iter()
            .map(|tag| (tag.name.as_str(), tag.item_count))
            .collect();
        assert_eq!(names, [("ML", 2), ("nlp", 1), ("to-read", 1)]);

        let ml = tags[0].id.clone();
        library.set_tag_color(&ml, Some("#FF8800")).unwrap();
        assert_eq!(library.tags().unwrap()[0].color.as_deref(), Some("#FF8800"));
        assert!(matches!(
            library.set_tag_color("missing", None),
            Err(PaperError::NotFound { .. })
        ));
        let tagged = ItemQuery {
            scope: Scope::Tag { id: ml.clone() },
            ..search("")
        };
        assert_eq!(titles(&library, &tagged).len(), 2);

        library.set_tags(&deep, &["ML".into()]).unwrap();
        library.set_tags(&attention, &[]).unwrap();
        let names: Vec<String> = library
            .tags()
            .unwrap()
            .into_iter()
            .map(|tag| tag.name)
            .collect();
        assert_eq!(names, ["ML"], "tags nobody uses are removed");
        assert_eq!(
            library.item(&deep).unwrap().tags[0].id,
            ml,
            "a kept tag keeps its id"
        );
    }

    #[test]
    fn notes_are_saved_per_item_and_removed_when_emptied() {
        let (library, deep, ..) = seeded();
        assert_eq!(library.note(&deep).unwrap(), None);
        library.save_note(&deep, "# Summary\nGreat paper").unwrap();
        library
            .save_note(&deep, "# Summary\nGreat paper, second draft")
            .unwrap();
        assert_eq!(
            library.note(&deep).unwrap().as_deref(),
            Some("# Summary\nGreat paper, second draft")
        );
        assert_eq!(
            library.item(&deep).unwrap().note.as_deref(),
            Some("# Summary\nGreat paper, second draft")
        );
        assert_eq!(titles(&library, &search("draft")), ["Deep learning"]);

        library.save_note(&deep, " \n").unwrap();
        assert_eq!(library.note(&deep).unwrap(), None);
        assert!(titles(&library, &search("draft")).is_empty());
        assert!(matches!(
            library.save_note("missing", "x"),
            Err(PaperError::NotFound { .. })
        ));
    }

    #[test]
    fn pdfs_are_copied_into_the_item_folder_and_leave_no_pdf_and_unread() {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::create(dir.path().join("Library")).unwrap();
        let id = library
            .insert_item(
                &paper("A/B testing: why? <part 1>.", None, &[], None),
                LookupStatus::Done,
            )
            .unwrap();
        let scope = |scope: Scope| {
            titles(
                &library,
                &ItemQuery {
                    scope,
                    ..ItemQuery::default()
                },
            )
            .len()
        };

        let first = library
            .add_pdf(&id, b"%PDF-1.7 one", Some("https://arxiv.org/pdf/1"))
            .unwrap();
        let second = library.add_pdf(&id, b"%PDF-1.7 two", None).unwrap();

        assert_eq!(
            first.relative_path,
            format!("{id}/A B testing why part 1.pdf"),
            "separators and characters macOS or Windows reject are gone"
        );
        assert_eq!(
            second.relative_path,
            format!("{id}/A B testing why part 1 2.pdf"),
            "a second file never overwrites the first"
        );
        let path = library.attachment_path(&first);
        assert!(path.starts_with(dir.path().join("Library").join("files")));
        assert_eq!(std::fs::read(&path).unwrap(), b"%PDF-1.7 one");
        assert_eq!(first.byte_size, 12);
        assert_eq!(first.mime, "application/pdf");
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name.to_string_lossy().ends_with(".part"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        let sha: String = library
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT sha256 FROM attachments WHERE id = ?1",
                params![first.id],
                |row| row.get(0),
            )
            .unwrap();
        // printf '%PDF-1.7 one' | sha256sum
        assert_eq!(
            sha,
            "e6d49ab2abf82176003da667a50ba1d708796b091f2957400c19699badfb6a20"
        );
        assert!(library.item_row(&id).unwrap().has_pdf);
        assert_eq!(scope(Scope::NoPdf), 0);
        assert_eq!(scope(Scope::Unread), 1, "attached but never opened");
        assert_eq!(library.item(&id).unwrap().attachments.len(), 2);

        library.set_reading_position(&first.id, 4, 1.25).unwrap();
        let read = library.attachment(&first.id).unwrap();
        assert_eq!((read.last_page, read.last_zoom), (Some(4), Some(1.25)));
        assert_eq!(scope(Scope::Unread), 0);
        assert_eq!(
            library.set_reading_position("nope", 0, 1.0),
            Err(PaperError::NotFound { id: "nope".into() })
        );

        library.delete_items(std::slice::from_ref(&id)).unwrap();
        assert_eq!(
            library.add_pdf(&id, b"%PDF", None),
            Err(PaperError::NotFound { id: id.clone() }),
            "a deleted item takes no files"
        );
        assert_eq!(file_stem("..."), "Paper");
        assert_eq!(file_stem(&"x".repeat(300)).len(), 120);
    }

    #[test]
    fn sidebar_scopes_cover_recent_unread_and_missing_pdfs() {
        let (library, ..) = seeded();
        for scope in [Scope::RecentlyAdded, Scope::Unread, Scope::NoPdf] {
            let query = ItemQuery {
                scope: scope.clone(),
                ..ItemQuery::default()
            };
            assert_eq!(titles(&library, &query).len(), 3, "{scope:?}");
        }
        // Items without attachments can be neither read nor have a PDF.
        {
            let conn = library.conn.lock().unwrap();
            conn.execute(
                "UPDATE items SET created_at = '2000-01-01T00:00:00.000Z' WHERE title = 'Deep learning'",
                [],
            )
            .unwrap();
        }
        let recent = ItemQuery {
            scope: Scope::RecentlyAdded,
            ..ItemQuery::default()
        };
        assert_eq!(titles(&library, &recent).len(), 2);
        let added = ItemQuery {
            sort: SortColumn::Added,
            ascending: true,
            ..ItemQuery::default()
        };
        assert_eq!(titles(&library, &added)[0], "Deep learning");
    }

    #[test]
    fn author_lists_follow_the_house_style() {
        assert_eq!(format_authors([]), "");
        assert_eq!(format_authors(["Vaswani"]), "Vaswani");
        assert_eq!(
            format_authors(["Vaswani", "Shazeer"]),
            "Vaswani and Shazeer"
        );
        assert_eq!(
            format_authors(["Vaswani", "Shazeer", "Parmar"]),
            "Vaswani et al."
        );
        assert_eq!(format_authors(["", "Only"]), "Only");
    }

    fn line(x0: f64, y0: f64, x1: f64, y1: f64) -> Quad {
        Quad {
            top_left: PagePoint { x: x0, y: y1 },
            top_right: PagePoint { x: x1, y: y1 },
            bottom_left: PagePoint { x: x0, y: y0 },
            bottom_right: PagePoint { x: x1, y: y0 },
        }
    }

    #[test]
    fn annotations_list_in_reading_order_search_and_leave_on_delete() {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::create(dir.path().join("Library")).unwrap();
        let item = library
            .insert_item(&paper("Diffusion", None, &[], None), LookupStatus::Done)
            .unwrap();
        let pdf = library.add_pdf(&item, b"%PDF-1.7", None).unwrap();
        let new = |kind, page_index, quads: Vec<Quad>, rect| NewAnnotation {
            attachment_id: pdf.id.clone(),
            kind,
            page_index,
            quads,
            rect,
            color: "#ffe58a".into(),
            selected_text: None,
            comment: None,
            text_before: None,
            text_after: None,
        };

        // Made out of order: page 1, then the bottom of page 0, then its top.
        let later_page = library
            .create_annotation(&new(
                AnnotationKind::Highlight,
                1,
                vec![line(72.0, 700.0, 300.0, 712.0)],
                None,
            ))
            .unwrap();
        let note = library
            .create_annotation(&NewAnnotation {
                comment: Some("check the variance schedule".into()),
                ..new(
                    AnnotationKind::Note,
                    0,
                    // A note's quads are ignored.
                    vec![line(0.0, 0.0, 1.0, 1.0)],
                    Some(PageRect {
                        x0: 40.0,
                        y0: 80.0,
                        x1: 60.0,
                        y1: 100.0,
                    }),
                )
            })
            .unwrap();
        let highlight = library
            .create_annotation(&NewAnnotation {
                selected_text: Some("reverse process learns to denoise".into()),
                // Two lines; the second starts further left and ends lower.
                ..new(
                    AnnotationKind::Highlight,
                    0,
                    vec![
                        line(100.0, 600.0, 500.0, 612.0),
                        line(72.0, 586.0, 250.0, 598.0),
                    ],
                    Some(PageRect {
                        x0: 0.0,
                        y0: 0.0,
                        x1: 1.0,
                        y1: 1.0,
                    }),
                )
            })
            .unwrap();

        assert_eq!(
            highlight.rect,
            PageRect {
                x0: 72.0,
                y0: 586.0,
                x1: 500.0,
                y1: 612.0
            },
            "a highlight's rect is the union of its quads, not what was sent"
        );
        assert_eq!(highlight.color, "#FFE58A");
        assert!(note.quads.is_empty());
        let ids: Vec<_> = library
            .annotations(&pdf.id)
            .unwrap()
            .into_iter()
            .map(|a| a.id)
            .collect();
        assert_eq!(
            ids,
            [highlight.id.clone(), note.id.clone(), later_page.id.clone()]
        );
        assert_eq!(library.annotation(&highlight.id).unwrap(), highlight);
        assert_eq!(titles(&library, &search("denoise")), ["Diffusion"]);
        assert_eq!(titles(&library, &search("variance")), ["Diffusion"]);

        let recolored = library
            .update_annotation(
                &note.id,
                &AnnotationPatch {
                    color: Some("#BFD9F2".into()),
                    comment: None,
                },
            )
            .unwrap();
        assert_eq!(recolored.color, "#BFD9F2");
        assert_eq!(
            recolored.comment, note.comment,
            "None leaves the comment alone"
        );
        let cleared = library
            .update_annotation(
                &note.id,
                &AnnotationPatch {
                    color: None,
                    comment: Some("  ".into()),
                },
            )
            .unwrap();
        assert_eq!(cleared.comment, None);
        assert!(titles(&library, &search("variance")).is_empty());

        library.delete_annotation(&highlight.id).unwrap();
        assert_eq!(library.annotations(&pdf.id).unwrap().len(), 2);
        assert!(titles(&library, &search("denoise")).is_empty());
        assert!(matches!(
            library.delete_annotation(&highlight.id),
            Err(PaperError::NotFound { .. })
        ));

        for bad in [
            new(AnnotationKind::Highlight, 0, vec![], None),
            new(AnnotationKind::Note, 0, vec![], None),
            new(AnnotationKind::Note, -1, vec![], Some(PageRect::default())),
            NewAnnotation {
                color: "yellow".into(),
                ..new(AnnotationKind::Note, 0, vec![], Some(PageRect::default()))
            },
        ] {
            assert!(
                matches!(
                    library.create_annotation(&bad),
                    Err(PaperError::Library { .. })
                ),
                "{bad:?}"
            );
        }
        assert!(matches!(
            library.create_annotation(&NewAnnotation {
                attachment_id: "missing".into(),
                ..new(AnnotationKind::Note, 0, vec![], Some(PageRect::default()))
            }),
            Err(PaperError::NotFound { .. })
        ));
    }
}

//! `import_pdf`: find which paper a PDF is, then file it under that paper.
//!
//! Identification tries, in order, and stops at the first answer Crossref
//! confirms (see `docs/research/pdf-identification.md`):
//!
//! 1. DOIs and arXiv IDs the file prints or carries in its metadata, accepted
//!    when the registered title is on page 1.
//! 2. The largest text on page 1, searched in Crossref by title.
//! 3. With an Anthropic API key in the settings, a Claude model reading an
//!    image of page 1. A DOI it reads must have the title it reads; otherwise
//!    its title is searched like step 2.

use std::path::Path;
use std::sync::{Arc, Mutex};

use paper_core::{
    Identification, ImportedPdf, ItemFields, ItemKind, Library, LookupStatus, PaperError,
    SourceRecord,
};
use paper_fetch::{Contact, FetchError, Lookup, Resolver};
use paper_identify::{
    HEAD_PAGES, IMAGE_MAX_SIDE, PdfFile, SAME_TITLE, TITLE_ON_PAGE, title_coverage,
    title_similarity, words,
};
use paper_ids::Identifier;

use crate::{SharedObserver, notify};

/// Candidates from the file checked with Crossref at most. Each costs a
/// request, and the paper's own identifier is nearly always among the first.
const MAX_CANDIDATES: usize = 4;
/// Titles shorter than this many words match too many papers to search by.
const MIN_TITLE_WORDS: usize = 3;

type Result<T> = std::result::Result<T, PaperError>;

/// The part of an import that runs on an engine thread.
pub(crate) struct ImportJob {
    pub library: Arc<Library>,
    pub observer: SharedObserver,
    pub resolver: Arc<Resolver>,
    /// Batches of deleted ids, so restoring one here takes it out of undo.
    pub undo: Arc<Mutex<Vec<Vec<String>>>>,
    pub contact: Contact,
    pub api_key: Option<String>,
}

impl ImportJob {
    pub fn run(&self, path: &Path) -> Result<ImportedPdf> {
        let bytes = std::fs::read(path)?;
        if !paper_fetch::is_pdf(&bytes) {
            return Err(PaperError::NotPdf {
                path: path.display().to_string(),
            });
        }
        let file_title = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_string());
        let placeholder = ItemFields {
            kind: Some(ItemKind::Other),
            title: file_title.clone(),
            ..ItemFields::default()
        };
        let item_id = self
            .library
            .insert_item(&placeholder, LookupStatus::Pending)?;
        notify(&self.observer, |o| o.items_changed(vec![item_id.clone()]));
        notify(&self.observer, |o| {
            o.lookup_progress(item_id.clone(), LookupStatus::Pending)
        });

        let attempt = identify(
            bytes.clone(),
            &self.resolver,
            &self.contact,
            self.api_key.as_deref(),
        );
        let outcome = match attempt.found {
            Some(found) => self.file_under(&item_id, found, &attempt.records, &bytes),
            None => self.keep_unidentified(
                &item_id,
                attempt.title.unwrap_or(file_title),
                attempt.error,
                &attempt.records,
                &bytes,
            ),
        };
        if let Err(error) = &outcome
            && !matches!(error, PaperError::Duplicate { .. })
        {
            tracing::warn!(%error, "import failed");
            // The row may be gone already; nothing more to tell then.
            if self
                .library
                .set_lookup_status(&item_id, LookupStatus::Failed)
                .is_ok()
            {
                notify(&self.observer, |o| {
                    o.lookup_progress(item_id.clone(), LookupStatus::Failed)
                });
                notify(&self.observer, |o| o.items_changed(vec![item_id.clone()]));
            }
        }
        outcome
    }

    /// Writes the metadata into the new item and attaches the PDF. When the
    /// paper is in the library already, the PDF goes to that item instead
    /// (if it has none) and the new item is removed.
    fn file_under(
        &self,
        item_id: &str,
        found: Found,
        records: &[SourceRecord],
        bytes: &[u8],
    ) -> Result<ImportedPdf> {
        let doi = found
            .lookup
            .fields
            .doi
            .clone()
            .unwrap_or_else(|| found.identifier.doi());
        let mut target = item_id.to_string();
        if let Some(existing) = self.library.find_item_by_doi(&doi)? {
            if !existing.deleted {
                let detail = self.library.item(&existing.item_id)?;
                if detail.attachments.iter().all(|a| a.mime != crate::PDF_MIME) {
                    self.library.add_pdf(&existing.item_id, bytes, None)?;
                }
                self.library.purge_item(item_id)?;
                notify(&self.observer, |o| {
                    o.items_changed(vec![item_id.to_string(), existing.item_id.clone()])
                });
                return Err(PaperError::Duplicate {
                    doi,
                    item_id: existing.item_id,
                });
            }
            // A deleted item of the same paper comes back, as with
            // `add_by_identifier`, and the new row goes.
            self.library
                .restore_items(std::slice::from_ref(&existing.item_id))?;
            crate::forget_undo(&self.undo, &existing.item_id);
            self.library.purge_item(item_id)?;
            notify(&self.observer, |o| {
                o.items_changed(vec![item_id.to_string()])
            });
            target = existing.item_id;
        }

        let mut fields = found.lookup.fields;
        if let Identifier::Arxiv(id) = &found.identifier {
            fields.arxiv_id.get_or_insert_with(|| id.clone());
        }
        self.library
            .replace_metadata(&target, &fields, LookupStatus::Done)?;
        for record in found.lookup.records.iter().chain(records) {
            self.library
                .put_source_record(&target, &record.source, &record.json)?;
        }
        let has_pdf = self
            .library
            .item(&target)?
            .attachments
            .iter()
            .any(|a| a.mime == crate::PDF_MIME);
        if !has_pdf {
            self.library.add_pdf(&target, bytes, None)?;
        }
        crate::give_citation_keys(&self.library, std::slice::from_ref(&target));
        notify(&self.observer, |o| {
            o.lookup_progress(target.clone(), LookupStatus::Done)
        });
        notify(&self.observer, |o| o.items_changed(vec![target.clone()]));
        Ok(ImportedPdf {
            item: self.library.item_row(&target)?,
            identification: found.by,
        })
    }

    /// Keeps the item with the PDF and the best title found, marked failed so
    /// the table shows that it needs a DOI.
    fn keep_unidentified(
        &self,
        item_id: &str,
        title: String,
        error: Option<FetchError>,
        records: &[SourceRecord],
        bytes: &[u8],
    ) -> Result<ImportedPdf> {
        let fields = ItemFields {
            kind: Some(ItemKind::Other),
            title,
            ..ItemFields::default()
        };
        self.library
            .replace_metadata(item_id, &fields, LookupStatus::Failed)?;
        for record in records {
            self.library
                .put_source_record(item_id, &record.source, &record.json)?;
        }
        self.library.add_pdf(item_id, bytes, None)?;
        notify(&self.observer, |o| {
            o.lookup_progress(item_id.to_string(), LookupStatus::Failed)
        });
        notify(&self.observer, |o| {
            o.items_changed(vec![item_id.to_string()])
        });
        let identification = match error {
            None => Identification::NotFound,
            Some(FetchError::Status {
                status: 401 | 403,
                url,
            }) if url == paper_fetch::MESSAGES_URL => Identification::Failed {
                message: "Anthropic did not accept the API key.".to_string(),
            },
            Some(error) => Identification::Failed {
                message: error.to_string(),
            },
        };
        Ok(ImportedPdf {
            item: self.library.item_row(item_id)?,
            identification,
        })
    }
}

/// A confirmed answer: the identifier, its metadata, and which step found it.
struct Found {
    identifier: Identifier,
    lookup: Lookup,
    by: Identification,
}

/// Everything identification learned, found or not.
struct Attempt {
    found: Option<Found>,
    /// The best title for the item if nothing is found.
    title: Option<String>,
    /// The first request that failed, so "offline" is not reported as
    /// "not found".
    error: Option<FetchError>,
    /// Search and model responses, kept in `source_records`.
    records: Vec<SourceRecord>,
}

fn identify(
    bytes: Vec<u8>,
    resolver: &Resolver,
    contact: &Contact,
    api_key: Option<&str>,
) -> Attempt {
    let file = match PdfFile::open(bytes) {
        Ok(file) => Some(file),
        Err(error) => {
            tracing::info!(%error, "the PDF could not be read for identification");
            None
        }
    };
    let head = file
        .as_ref()
        .map(|file| file.head(HEAD_PAGES))
        .unwrap_or_default();
    let page_text = head.first_page_text();
    let mut title = head.title_guess();
    let mut check = Checker {
        resolver,
        contact,
        error: None,
        records: Vec::new(),
    };

    for candidate in head.candidates().into_iter().take(MAX_CANDIDATES) {
        if let Some(lookup) = check.lookup(&candidate.identifier)
            && title_coverage(&lookup.fields.title, &page_text) >= TITLE_ON_PAGE
        {
            return check.found(candidate.identifier, lookup, Identification::FileText);
        }
    }

    if let Some(guess) = &title
        && let Some((identifier, lookup)) = check.search(guess, None)
    {
        return check.found(identifier, lookup, Identification::TitleSearch);
    }

    let (Some(key), Some(file)) = (api_key, &file) else {
        return check.not_found(title);
    };
    let Some(png) = file.render_first_page(IMAGE_MAX_SIDE) else {
        return check.not_found(title);
    };
    let reading = match resolver.read_first_page(&png, key) {
        Ok(reading) => reading,
        Err(error) => {
            tracing::warn!(%error, "the model could not read page 1");
            check.error.get_or_insert(error);
            return check.not_found(title);
        }
    };
    check.records.push(reading.record.clone());
    let printed = reading
        .doi
        .as_deref()
        .and_then(Identifier::parse)
        .or_else(|| reading.arxiv_id.as_deref().and_then(Identifier::parse));
    if let Some(identifier) = printed
        && let Some(lookup) = check.lookup(&identifier)
    {
        // The model may misread a digit; the registered title must agree.
        let agrees = match &reading.title {
            Some(read) => title_similarity(&lookup.fields.title, read) >= SAME_TITLE,
            None => title_coverage(&lookup.fields.title, &page_text) >= TITLE_ON_PAGE,
        };
        if agrees {
            return check.found(identifier, lookup, Identification::Model);
        }
    }
    if let Some(read) = &reading.title {
        if let Some((identifier, lookup)) = check.search(read, reading.first_author.as_deref()) {
            return check.found(identifier, lookup, Identification::Model);
        }
        // The model reads titles better than the layout guess does.
        title = Some(read.clone());
    }
    check.not_found(title)
}

/// The Crossref calls of identification. Remembers the first request that
/// failed and the raw search and model responses.
struct Checker<'a> {
    resolver: &'a Resolver,
    contact: &'a Contact,
    error: Option<FetchError>,
    records: Vec<SourceRecord>,
}

impl Checker<'_> {
    /// The metadata of an identifier, or `None` when it is unknown or the
    /// request failed.
    fn lookup(&mut self, identifier: &Identifier) -> Option<Lookup> {
        match self.resolver.lookup(identifier, self.contact) {
            Ok(lookup) => Some(lookup),
            Err(FetchError::NotFound { .. }) => None,
            Err(error) => {
                tracing::info!(%identifier, %error, "could not check a candidate");
                self.error.get_or_insert(error);
                None
            }
        }
    }

    /// The first search result whose title matches `title`, with its metadata.
    fn search(&mut self, title: &str, author: Option<&str>) -> Option<(Identifier, Lookup)> {
        if words(title).len() < MIN_TITLE_WORDS {
            return None;
        }
        let search = match self.resolver.search_title(title, author, self.contact) {
            Ok(search) => search,
            Err(error) => {
                self.error.get_or_insert(error);
                return None;
            }
        };
        self.records.push(search.record);
        let hit = search
            .matches
            .into_iter()
            .find(|hit| title_similarity(&hit.title, title) >= SAME_TITLE)?;
        let identifier = Identifier::Doi(hit.doi);
        let lookup = self.lookup(&identifier)?;
        Some((identifier, lookup))
    }

    fn found(self, identifier: Identifier, lookup: Lookup, by: Identification) -> Attempt {
        tracing::info!(%identifier, ?by, "identified a PDF");
        Attempt {
            found: Some(Found {
                identifier,
                lookup,
                by,
            }),
            title: None,
            error: None,
            records: self.records,
        }
    }

    fn not_found(self, title: Option<String>) -> Attempt {
        Attempt {
            found: None,
            title,
            error: self.error,
            records: self.records,
        }
    }
}

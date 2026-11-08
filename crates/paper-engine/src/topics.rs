//! Automatic tags: Jev picks up to three entries of the library's topic list
//! for a paper, and they are added to the paper as ordinary tags. See
//! `docs/research/topic-tags.md`.

use std::sync::Arc;

use paper_core::{Library, PaperError};
use paper_fetch::{Contact, FetchError, PaperText, Resolver};

use crate::{SharedObserver, notify};

pub use paper_fetch::JEV_SOURCE;

type Result<T> = std::result::Result<T, PaperError>;

/// The part of tagging that runs on an engine thread.
#[derive(Clone)]
pub(crate) struct TopicJob {
    pub library: Arc<Library>,
    pub observer: SharedObserver,
    pub resolver: Arc<Resolver>,
    pub contact: Contact,
    pub openalex_api_key: Option<String>,
    pub jev_api_key: String,
}

impl TopicJob {
    /// Asks Jev which topics the item is about and adds them as tags.
    /// Returns the tags the item did not have before.
    pub fn run(&self, item_id: &str) -> Result<Vec<String>> {
        let topics = self.library.topics()?;
        if topics.is_empty() {
            return Ok(Vec::new());
        }
        let fields = self.library.item(item_id)?.fields;
        // Without OpenAlex the title and abstract are still enough to go on.
        let work = match fields.doi.as_deref() {
            Some(doi) => self
                .resolver
                .openalex_work(doi, self.openalex_api_key.as_deref(), &self.contact)
                .unwrap_or_else(|error| {
                    tracing::warn!(%error, item_id, "no OpenAlex keywords for topics");
                    None
                }),
            None => None,
        };
        if let Some(work) = &work {
            self.library
                .put_source_record(item_id, &work.record.source, &work.record.json)?;
        }
        let (keywords, openalex_abstract) = work
            .map(|work| (work.keywords, work.abstract_text))
            .unwrap_or_default();
        let paper = PaperText {
            title: fields.title,
            venue: fields.container_title,
            keywords,
            abstract_text: fields.abstract_text.or(openalex_abstract),
        };
        let choice = self
            .resolver
            .choose_topics(&paper, &topics, &self.jev_api_key)
            .map_err(|error| match error {
                FetchError::Status {
                    status: 401 | 403, ..
                } => PaperError::Settings {
                    message: "TypeSafe did not accept the Jev API key.".to_string(),
                },
                error => PaperError::Network {
                    message: error.to_string(),
                },
            })?;
        // Kept even when no topic fits, so the paper is not asked about again.
        self.library
            .put_source_record(item_id, &choice.record.source, &choice.record.json)?;
        let added = self.library.add_tags(item_id, &choice.names)?;
        if !added.is_empty() {
            notify(&self.observer, |o| o.tags_changed());
            notify(&self.observer, |o| {
                o.items_changed(vec![item_id.to_string()])
            });
        }
        Ok(added)
    }
}

/// The tags one paper got from `tag_with_topics`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicTags {
    pub item_id: String,
    /// Tags the paper did not have before, most likely topic first.
    pub tags: Vec<String>,
}

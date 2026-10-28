//! The public API of the Paper engine.
//!
//! This is the minimal version from spike 1 of `docs/ROADMAP.md`. It keeps
//! the shape described in `docs/ARCHITECTURE.md` (one `Engine` per library,
//! plain data records, one error enum, synchronous reads, async work on engine
//! threads, events through an observer) but stores items in memory and fakes
//! the network lookup. SQLite and the real metadata pipeline come later.
//!
//! Nothing here knows about UniFFI or Swift. `paper-ffi` exports this API.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Number of generated items in a newly opened demo library.
pub const DEMO_ITEM_COUNT: usize = 5_000;

/// How long the fake metadata lookup takes, to make the async path visible.
const FAKE_LOOKUP_DELAY: Duration = Duration::from_millis(400);

/// One row of the library table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemRow {
    pub id: String,
    pub title: String,
    pub authors: String,
    pub year: Option<i32>,
    pub doi: Option<String>,
}

/// Every error the engine can return.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PaperError {
    #[error("not a DOI: {text}")]
    InvalidIdentifier { text: String },
    #[error("already in the library: {doi}")]
    Duplicate { doi: String },
    #[error("could not open library: {message}")]
    Io { message: String },
}

/// Events from the engine. Called on engine threads, never on the caller's
/// thread, so an app must move to its UI thread before touching a view.
pub trait EngineObserver: Send + Sync {
    fn items_changed(&self, ids: Vec<String>);
}

/// One open library.
pub struct Engine {
    path: PathBuf,
    items: Mutex<Vec<ItemRow>>,
    observer: Mutex<Option<Arc<dyn EngineObserver>>>,
}

impl Engine {
    /// Opens the library folder at `path`, creating it if needed, and fills it
    /// with generated demo items.
    pub fn open_library(path: impl AsRef<Path>) -> Result<Engine, PaperError> {
        let path = path.as_ref().to_path_buf();
        std::fs::create_dir_all(&path).map_err(|error| PaperError::Io {
            message: format!("{}: {error}", path.display()),
        })?;
        Ok(Engine {
            path,
            items: Mutex::new(demo_items(DEMO_ITEM_COUNT)),
            observer: Mutex::new(None),
        })
    }

    pub fn library_path(&self) -> &Path {
        &self.path
    }

    /// Items whose title or authors contain `query`, ignoring case. An empty
    /// query returns everything. Synchronous: this is called on every keystroke.
    pub fn list_items(&self, query: &str) -> Vec<ItemRow> {
        let items = self.items.lock().unwrap();
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return items.clone();
        }
        items
            .iter()
            .filter(|item| {
                item.title.to_lowercase().contains(&query)
                    || item.authors.to_lowercase().contains(&query)
            })
            .cloned()
            .collect()
    }

    /// Adds a paper by DOI. The lookup runs on an engine thread and the caller
    /// awaits it, so a UI thread never blocks on the network.
    pub async fn add_by_identifier(&self, text: &str) -> Result<ItemRow, PaperError> {
        let doi = normalize_doi(text).ok_or_else(|| PaperError::InvalidIdentifier {
            text: text.to_string(),
        })?;
        self.check_not_present(&doi)?;

        let (sender, receiver) = futures_channel::oneshot::channel();
        thread::spawn(move || {
            let _ = sender.send(fake_lookup(doi));
        });
        let looked_up = receiver
            .await
            .expect("lookup thread ended without a result");

        // Check again: another add of the same DOI may have finished meanwhile.
        let row = {
            let mut items = self.items.lock().unwrap();
            if items.iter().any(|item| item.doi == looked_up.doi) {
                return Err(PaperError::Duplicate {
                    doi: looked_up.doi.unwrap_or_default(),
                });
            }
            let row = ItemRow {
                id: format!("item-{:05}", items.len() + 1),
                ..looked_up
            };
            items.insert(0, row.clone());
            row
        };
        self.notify_items_changed(vec![row.id.clone()]);
        Ok(row)
    }

    /// Replaces the observer. The engine keeps it alive until replaced.
    pub fn set_observer(&self, observer: Arc<dyn EngineObserver>) {
        *self.observer.lock().unwrap() = Some(observer);
    }

    fn check_not_present(&self, doi: &str) -> Result<(), PaperError> {
        let items = self.items.lock().unwrap();
        if items.iter().any(|item| item.doi.as_deref() == Some(doi)) {
            return Err(PaperError::Duplicate {
                doi: doi.to_string(),
            });
        }
        Ok(())
    }

    fn notify_items_changed(&self, ids: Vec<String>) {
        // Clone the observer out so the lock is not held during the callback.
        let observer = self.observer.lock().unwrap().clone();
        if let Some(observer) = observer {
            observer.items_changed(ids);
        }
    }
}

/// Accepts `10.x/y`, `doi:10.x/y`, and doi.org URLs. Returns the lowercase DOI.
fn normalize_doi(text: &str) -> Option<String> {
    let lower = text.trim().to_lowercase();
    let doi = [
        "https://doi.org/",
        "http://doi.org/",
        "https://dx.doi.org/",
        "doi:",
    ]
    .iter()
    .find_map(|prefix| lower.strip_prefix(prefix))
    .unwrap_or(&lower)
    .trim();
    let (prefix, suffix) = doi.split_once('/')?;
    (prefix.starts_with("10.") && prefix.len() > 3 && !suffix.is_empty()).then(|| doi.to_string())
}

/// Stands in for the Crossref lookup in `paper-fetch`. Runs on an engine thread.
fn fake_lookup(doi: String) -> ItemRow {
    thread::sleep(FAKE_LOOKUP_DELAY);
    ItemRow {
        id: String::new(),
        title: format!("Paper with DOI {doi}"),
        authors: "Unknown author".to_string(),
        year: None,
        doi: Some(doi),
    }
}

fn demo_items(count: usize) -> Vec<ItemRow> {
    const ADJECTIVES: [&str; 6] = [
        "Efficient",
        "Robust",
        "Scalable",
        "Sparse",
        "Neural",
        "Bayesian",
    ];
    const TOPICS: [&str; 7] = [
        "Transformers",
        "Retrieval",
        "Graph Learning",
        "Protein Folding",
        "Optimization",
        "Diffusion Models",
        "Reinforcement Learning",
    ];
    const AUTHORS: [&str; 8] = [
        "Vaswani", "Lewis", "Kingma", "Jumper", "Ho", "Silver", "Nguyen", "Tran",
    ];
    (0..count)
        .map(|i| ItemRow {
            id: format!("demo-{i:05}"),
            title: format!(
                "{} {} at Scale, Part {}",
                ADJECTIVES[i % 6],
                TOPICS[i % 7],
                i + 1
            ),
            authors: format!("{} et al.", AUTHORS[i % 8]),
            year: Some(1990 + (i % 36) as i32),
            doi: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_library(name: &str) -> Engine {
        let path =
            std::env::temp_dir().join(format!("paper-engine-test-{name}-{}", std::process::id()));
        Engine::open_library(path).unwrap()
    }

    #[derive(Default)]
    struct Recorder(Mutex<Vec<Vec<String>>>);
    impl EngineObserver for Recorder {
        fn items_changed(&self, ids: Vec<String>) {
            self.0.lock().unwrap().push(ids);
        }
    }

    #[test]
    fn list_items_filters_title_and_authors_ignoring_case() {
        let engine = temp_library("list");
        assert_eq!(engine.list_items("").len(), DEMO_ITEM_COUNT);
        assert_eq!(engine.list_items("  ").len(), DEMO_ITEM_COUNT);

        let by_author = engine.list_items("KINGMA");
        assert_eq!(by_author.len(), DEMO_ITEM_COUNT / 8);
        assert!(by_author.iter().all(|item| item.authors == "Kingma et al."));

        let by_title = engine.list_items("part 4999");
        assert_eq!(by_title.len(), 1);
        assert_eq!(by_title[0].id, "demo-04998");
    }

    #[test]
    fn add_by_identifier_normalizes_doi_and_notifies_observer() {
        let engine = temp_library("add");
        let recorder = Arc::new(Recorder::default());
        engine.set_observer(recorder.clone());

        let row =
            pollster::block_on(engine.add_by_identifier(" https://doi.org/10.1038/NATURE14539 "))
                .unwrap();

        assert_eq!(row.doi.as_deref(), Some("10.1038/nature14539"));
        assert_eq!(engine.list_items("")[0], row, "new items come first");
        assert_eq!(engine.list_items("").len(), DEMO_ITEM_COUNT + 1);
        assert_eq!(*recorder.0.lock().unwrap(), vec![vec![row.id.clone()]]);

        let again = pollster::block_on(engine.add_by_identifier("doi:10.1038/nature14539"));
        assert_eq!(
            again,
            Err(PaperError::Duplicate {
                doi: "10.1038/nature14539".into()
            })
        );
    }

    #[test]
    fn add_by_identifier_rejects_text_that_is_not_a_doi() {
        let engine = temp_library("reject");
        for text in [
            "",
            "attention is all you need",
            "10.1038",
            "10./x",
            "11.1000/x",
            "10.1000/",
        ] {
            let result = pollster::block_on(engine.add_by_identifier(text));
            assert_eq!(
                result,
                Err(PaperError::InvalidIdentifier { text: text.into() }),
                "{text:?}"
            );
        }
        assert_eq!(engine.list_items("").len(), DEMO_ITEM_COUNT);
    }
}

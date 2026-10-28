//! UniFFI exports of `paper-engine`.
//!
//! This crate only translates. Records and the error enum are the engine's own
//! types, described to UniFFI with `remote` so the engine stays free of FFI
//! code. `Engine` and `EngineObserver` are thin wrappers, because UniFFI needs
//! an object it owns and a trait it can implement in Swift.

use std::sync::Arc;

use paper_engine as engine;
use paper_engine::{ItemRow, PaperError};

uniffi::setup_scaffolding!();

#[uniffi::remote(Record)]
pub struct ItemRow {
    pub id: String,
    pub title: String,
    pub authors: String,
    pub year: Option<i32>,
    pub doi: Option<String>,
}

#[uniffi::remote(Error)]
pub enum PaperError {
    InvalidIdentifier { text: String },
    Duplicate { doi: String },
    Io { message: String },
}

/// Implemented in Swift. Called on engine threads.
#[uniffi::export(with_foreign)]
pub trait EngineObserver: Send + Sync {
    fn items_changed(&self, ids: Vec<String>);
}

/// Passes engine events to the Swift observer.
struct ObserverAdapter(Arc<dyn EngineObserver>);

impl engine::EngineObserver for ObserverAdapter {
    fn items_changed(&self, ids: Vec<String>) {
        self.0.items_changed(ids);
    }
}

#[derive(uniffi::Object)]
pub struct Engine(engine::Engine);

#[uniffi::export]
impl Engine {
    #[uniffi::constructor]
    pub fn open_library(path: String) -> Result<Arc<Self>, PaperError> {
        engine::Engine::open_library(path).map(|engine| Arc::new(Self(engine)))
    }

    pub fn library_path(&self) -> String {
        self.0.library_path().display().to_string()
    }

    pub fn list_items(&self, query: String) -> Vec<ItemRow> {
        self.0.list_items(&query)
    }

    pub async fn add_by_identifier(&self, text: String) -> Result<ItemRow, PaperError> {
        self.0.add_by_identifier(&text).await
    }

    pub fn set_observer(&self, observer: Arc<dyn EngineObserver>) {
        self.0.set_observer(Arc::new(ObserverAdapter(observer)));
    }
}

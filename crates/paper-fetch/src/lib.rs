//! Metadata lookup for DOIs and arXiv IDs.
//!
//! `Resolver::lookup` runs the pipeline from `docs/ARCHITECTURE.md`: Crossref
//! first, then DOI content negotiation (CSL JSON) for DataCite DOIs such as
//! arXiv and Zenodo. Each source's raw response comes back as a `SourceRecord`
//! so the engine can keep it. Everything is blocking and meant to run on an
//! engine thread; `HttpClient` is a trait so tests never touch the network.
//!
//! Open access PDF discovery (OpenAlex, Unpaywall, arXiv, Europe PMC) is
//! milestone 2 and will be added next to the metadata sources here.

mod crossref;
mod csl;
mod http;
mod rate_limit;
mod resolver;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
mod text;

pub use http::{Contact, HttpClient, Response, USER_AGENT_PRODUCT, UreqClient};
pub use resolver::{Lookup, Resolver};

/// Errors from the lookup pipeline. The engine maps them onto `PaperError`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FetchError {
    /// The request did not complete: offline, DNS, TLS, or timeout.
    #[error("{message}")]
    Network { message: String },
    /// The server answered with a status the pipeline does not handle.
    #[error("{url} answered {status}")]
    Status { status: u16, url: String },
    /// The body was not the JSON the pipeline expected.
    #[error("could not read the response from {api}: {message}")]
    Parse { api: String, message: String },
    /// No source knows this DOI.
    #[error("no metadata found for {doi}")]
    NotFound { doi: String },
}

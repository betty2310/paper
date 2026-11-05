//! The lookup pipeline: Crossref, then DOI content negotiation.

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use paper_core::{ItemFields, SourceRecord};
use paper_ids::Identifier;
use serde_json::Value;

use crate::http::{Contact, HttpClient, Response};
use crate::rate_limit::RateLimiter;
use crate::{FetchError, crossref, csl};

/// Attempts per request when the server says to slow down or fails.
const ATTEMPTS: u32 = 3;
const DEFAULT_BACKOFF: Duration = Duration::from_secs(1);

/// What a lookup found: the mapped metadata and the raw responses it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lookup {
    pub fields: ItemFields,
    pub records: Vec<SourceRecord>,
}

/// Runs lookups. One per engine; the rate limiter is shared by all of them.
pub struct Resolver {
    client: Arc<dyn HttpClient>,
    limiter: RateLimiter,
}

impl Resolver {
    pub fn new(client: Arc<dyn HttpClient>) -> Resolver {
        Resolver {
            client,
            limiter: RateLimiter::new(),
        }
    }

    /// Looks up the metadata for an identifier. Blocks; run it on a worker.
    pub fn lookup(&self, identifier: &Identifier, contact: &Contact) -> Result<Lookup, FetchError> {
        let doi = identifier.doi();
        let user_agent = contact.user_agent();
        let mut records = Vec::new();

        let crossref_url = crossref::work_url(&doi, contact.email.as_deref());
        let response = self.get(&crossref_url, &[("User-Agent", &user_agent)])?;
        if response.is_success() {
            let json = response.json(crossref::SOURCE)?;
            let message = &json["message"];
            if message.is_object() {
                tracing::info!(%doi, source = crossref::SOURCE, "metadata found");
                records.push(record(crossref::SOURCE, &json));
                let mut fields = crossref::map_work(message);
                fill_arxiv_id(&mut fields, identifier);
                return Ok(Lookup { fields, records });
            }
        } else if response.status != 404 {
            return Err(FetchError::Status {
                status: response.status,
                url: crossref_url,
            });
        }

        // Crossref does not know DataCite DOIs (arXiv, Zenodo); doi.org knows them all.
        let csl_url = csl::url(&doi);
        let response = self.get(
            &csl_url,
            &[("User-Agent", &user_agent), ("Accept", csl::ACCEPT)],
        )?;
        if response.status == 404 || response.status == 406 {
            return Err(FetchError::NotFound { doi });
        }
        if !response.is_success() {
            return Err(FetchError::Status {
                status: response.status,
                url: csl_url,
            });
        }
        let json = response.json(csl::SOURCE)?;
        if !json.is_object() {
            return Err(FetchError::Parse {
                api: csl::SOURCE.to_string(),
                message: "expected a CSL JSON object".to_string(),
            });
        }
        tracing::info!(%doi, source = csl::SOURCE, "metadata found");
        records.push(record(csl::SOURCE, &json));
        let mut fields = csl::map_csl(&json);
        fill_arxiv_id(&mut fields, identifier);
        Ok(Lookup { fields, records })
    }

    /// A rate limited GET that retries on 429 and 5xx, honoring `Retry-After`.
    pub(crate) fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Response, FetchError> {
        self.download(url, headers, &mut |_, _| {})
    }

    /// `get`, reporting the body's progress as `HttpClient::download` does.
    pub(crate) fn download(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<Response, FetchError> {
        self.retrying(url, &mut || self.client.download(url, headers, progress))
    }

    /// A rate limited POST, retried like `get`.
    pub(crate) fn post(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<Response, FetchError> {
        self.retrying(url, &mut || self.client.post(url, headers, body))
    }

    /// Sends a request through the rate limiter and sends it again on 429
    /// and 5xx, honoring `Retry-After`.
    fn retrying(
        &self,
        url: &str,
        send: &mut dyn FnMut() -> Result<Response, FetchError>,
    ) -> Result<Response, FetchError> {
        let mut attempt = 1;
        loop {
            self.limiter.wait_for(url);
            let response = send()?;
            let should_retry = response.status == 429 || response.status >= 500;
            if !should_retry || attempt >= ATTEMPTS {
                return Ok(response);
            }
            let backoff = response
                .retry_after
                .map(Duration::from_secs)
                .unwrap_or(DEFAULT_BACKOFF * attempt)
                .min(Duration::from_secs(30));
            tracing::warn!(url, status = response.status, ?backoff, attempt, "retrying");
            thread::sleep(backoff);
            attempt += 1;
        }
    }
}

pub(crate) fn record(source: &str, json: &Value) -> SourceRecord {
    SourceRecord {
        source: source.to_string(),
        json: json.to_string(),
    }
}

/// The user may have typed an arXiv ID with a version; keep what they typed.
fn fill_arxiv_id(fields: &mut ItemFields, identifier: &Identifier) {
    if let Some(id) = identifier.arxiv_id() {
        fields.arxiv_id = Some(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeHttp, json_response};
    use paper_core::ItemKind;

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!(
            "{}/tests/fixtures/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    #[test]
    fn crossref_answer_is_used_and_recorded() {
        let http = Arc::new(FakeHttp::new());
        http.route(
            "https://api.crossref.org/works/10.1038/nature14539?mailto=me@example.org",
            200,
            fixture("crossref_nature14539.json"),
        );
        let resolver = Resolver::new(http.clone());

        let lookup = resolver
            .lookup(
                &Identifier::Doi("10.1038/nature14539".into()),
                &Contact::new("me@example.org"),
            )
            .unwrap();

        assert_eq!(lookup.fields.title, "Deep learning");
        assert_eq!(lookup.records.len(), 1);
        assert_eq!(lookup.records[0].source, "crossref");
        assert!(
            lookup.records[0]
                .json
                .contains("\"DOI\":\"10.1038/nature14539\"")
        );
        let requests = http.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].headers.get("User-Agent").map(String::as_str),
            Some(Contact::new("me@example.org").user_agent().as_str())
        );
    }

    #[test]
    fn datacite_doi_falls_back_to_content_negotiation() {
        let http = Arc::new(FakeHttp::new());
        http.route(
            "https://api.crossref.org/works/10.48550/arxiv.1706.03762",
            404,
            b"Resource not found.".to_vec(),
        );
        http.route(
            "https://doi.org/10.48550/arxiv.1706.03762",
            200,
            fixture("csl_arxiv_1706_03762.json"),
        );
        let resolver = Resolver::new(http.clone());

        let lookup = resolver
            .lookup(
                &Identifier::Arxiv("1706.03762v7".into()),
                &Contact::default(),
            )
            .unwrap();

        assert_eq!(lookup.fields.title, "Attention Is All You Need");
        assert_eq!(lookup.fields.kind, Some(ItemKind::Preprint));
        assert_eq!(
            lookup.fields.arxiv_id.as_deref(),
            Some("1706.03762v7"),
            "the typed version is kept"
        );
        assert_eq!(lookup.records[0].source, "doi_org");
        let requests = http.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[1].headers.get("Accept").map(String::as_str),
            Some(csl::ACCEPT)
        );
    }

    #[test]
    fn unknown_doi_is_not_found_and_server_errors_are_reported() {
        let http = Arc::new(FakeHttp::new());
        http.route(
            "https://api.crossref.org/works/10.9999/nothing",
            404,
            Vec::new(),
        );
        http.route("https://doi.org/10.9999/nothing", 404, Vec::new());
        let resolver = Resolver::new(http.clone());
        assert_eq!(
            resolver.lookup(
                &Identifier::Doi("10.9999/nothing".into()),
                &Contact::default()
            ),
            Err(FetchError::NotFound {
                doi: "10.9999/nothing".into()
            })
        );

        http.route(
            "https://api.crossref.org/works/10.9999/broken",
            503,
            Vec::new(),
        );
        let result = resolver.lookup(
            &Identifier::Doi("10.9999/broken".into()),
            &Contact::default(),
        );
        assert_eq!(
            result,
            Err(FetchError::Status {
                status: 503,
                url: "https://api.crossref.org/works/10.9999/broken".into()
            })
        );
        let attempts = http
            .requests()
            .iter()
            .filter(|request| request.url.ends_with("broken"))
            .count();
        assert_eq!(attempts, ATTEMPTS as usize, "5xx is retried");
    }

    #[test]
    fn rate_limiting_is_retried_after_the_server_says_so() {
        let http = Arc::new(FakeHttp::new());
        http.route_responses(
            "https://api.crossref.org/works/10.1038/nature14539",
            vec![
                json_response(429, Vec::new(), Some(0)),
                json_response(503, Vec::new(), Some(0)),
                json_response(200, fixture("crossref_nature14539.json"), None),
            ],
        );
        let resolver = Resolver::new(http.clone());
        let started = std::time::Instant::now();
        let lookup = resolver
            .lookup(
                &Identifier::Doi("10.1038/nature14539".into()),
                &Contact::default(),
            )
            .unwrap();
        assert_eq!(lookup.fields.title, "Deep learning");
        assert_eq!(
            http.requests().len(),
            3,
            "429 and 5xx are retried against the same URL"
        );
        assert!(
            started.elapsed() < DEFAULT_BACKOFF,
            "Retry-After: 0 beats the default backoff, took {:?}",
            started.elapsed()
        );

        http.route_responses(
            "https://api.crossref.org/works/10.9999/busy",
            vec![json_response(429, Vec::new(), Some(0))],
        );
        assert_eq!(
            resolver.lookup(&Identifier::Doi("10.9999/busy".into()), &Contact::default()),
            Err(FetchError::Status {
                status: 429,
                url: "https://api.crossref.org/works/10.9999/busy".into()
            }),
            "after the last attempt the status is reported, not retried forever"
        );
    }

    #[test]
    fn network_failure_is_returned_as_is() {
        let http = Arc::new(FakeHttp::new());
        http.fail_with("no route to host");
        let resolver = Resolver::new(http);
        assert_eq!(
            resolver.lookup(
                &Identifier::Doi("10.1038/nature14539".into()),
                &Contact::default()
            ),
            Err(FetchError::Network {
                message: "no route to host".into()
            })
        );
    }
}

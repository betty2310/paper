//! Open access PDF discovery and download, in the order from
//! `docs/research/doi-and-pdf-sources.md`: arXiv, OpenAlex, Unpaywall, then
//! Europe PMC. Only sources that state a file is open access are asked, and a
//! download counts only when the body really is a PDF.

use std::collections::HashSet;

use paper_core::{PdfStep, SourceRecord};
use serde_json::Value;

use crate::FetchError;
use crate::crossref::percent_encode_path;
use crate::http::Contact;
use crate::resolver::{Resolver, record};

pub const OPENALEX: &str = "openalex";
pub const UNPAYWALL: &str = "unpaywall";

/// URLs tried per source at most, so one source with many mirrors cannot
/// stall the search.
const MAX_URLS_PER_SOURCE: usize = 6;
/// How far into the body the `%PDF` signature may start. Some servers put a
/// byte order mark or blank lines first.
const SIGNATURE_WINDOW: usize = 1024;

/// What to look for: the item's DOI, its arXiv ID, or both.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PdfQuery {
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    /// OpenAlex API key from the settings, if the user has one.
    pub openalex_api_key: Option<String>,
}

/// A downloaded PDF and the raw source responses that led to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundPdf {
    pub bytes: Vec<u8>,
    /// The URL the PDF came from.
    pub url: String,
    /// `arxiv`, `openalex`, `unpaywall`, or `europepmc`.
    pub source: String,
}

/// The outcome of `find_pdf`: the PDF if one was found, and every raw API
/// response read on the way, found or not, for `source_records`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfSearch {
    pub result: Result<FoundPdf, FetchError>,
    pub records: Vec<SourceRecord>,
}

impl Resolver {
    /// Finds and downloads an open access PDF. Blocks; run it on a worker.
    /// `on_step` hears each source asked and each download's progress.
    ///
    /// Fails with `NoPdf` when no source offers one, and with the network
    /// error when nothing was found and at least one request did not complete,
    /// so the app can tell "not open access" from "try again later".
    pub fn find_pdf(
        &self,
        query: &PdfQuery,
        contact: &Contact,
        on_step: &mut dyn FnMut(PdfStep),
    ) -> PdfSearch {
        let mut search = Search {
            resolver: self,
            on_step,
            user_agent: contact.user_agent(),
            tried: HashSet::new(),
            network_error: None,
            records: Vec::new(),
        };
        let result = search.run(query, contact);
        PdfSearch {
            result,
            records: search.records,
        }
    }
}

struct Search<'a> {
    resolver: &'a Resolver,
    on_step: &'a mut dyn FnMut(PdfStep),
    user_agent: String,
    tried: HashSet<String>,
    network_error: Option<FetchError>,
    records: Vec<SourceRecord>,
}

impl Search<'_> {
    fn run(&mut self, query: &PdfQuery, contact: &Contact) -> Result<FoundPdf, FetchError> {
        let doi = query.doi.as_deref();
        let arxiv_id = query.arxiv_id.clone().or_else(|| {
            doi.and_then(|doi| doi.strip_prefix("10.48550/arxiv."))
                .map(str::to_string)
        });

        // arXiv PDFs are always open, and its URL needs no lookup.
        if let Some(id) = &arxiv_id
            && let Some(found) = self
                .checking("arxiv")
                .try_urls("arxiv", vec![format!("https://arxiv.org/pdf/{id}")])
        {
            return Ok(found);
        }

        let Some(doi) = doi else {
            return Err(self.give_up(doi.unwrap_or_default()));
        };

        let mut pmcid = None;
        let mut url = format!(
            "https://api.openalex.org/works/https://doi.org/{}",
            percent_encode_path(doi)
        );
        if let Some(key) = query
            .openalex_api_key
            .as_deref()
            .filter(|key| !key.is_empty())
        {
            url.push_str(&format!("?api_key={key}"));
        }
        self.checking(OPENALEX);
        if let Some(work) = self.json(OPENALEX, &url) {
            pmcid = openalex_pmcid(&work);
            if let Some(found) = self.try_urls(OPENALEX, openalex_pdf_urls(&work)) {
                return Ok(found);
            }
        }

        // Unpaywall requires an email address and rejects requests without one.
        if let Some(email) = &contact.email {
            let url = format!(
                "https://api.unpaywall.org/v2/{}?email={email}",
                percent_encode_path(doi)
            );
            self.checking(UNPAYWALL);
            if let Some(json) = self.json(UNPAYWALL, &url)
                && let Some(found) = self.try_urls(UNPAYWALL, unpaywall_pdf_urls(&json))
            {
                return Ok(found);
            }
        }

        if let Some(pmcid) = pmcid
            && let Some(found) = self.checking("europepmc").try_urls(
                "europepmc",
                vec![format!("https://europepmc.org/articles/{pmcid}?pdf=render")],
            )
        {
            return Ok(found);
        }

        Err(self.give_up(doi))
    }

    fn checking(&mut self, source: &str) -> &mut Self {
        (self.on_step)(PdfStep::Checking {
            source: source.to_string(),
        });
        self
    }

    fn give_up(&mut self, doi: &str) -> FetchError {
        self.network_error.take().unwrap_or(FetchError::NoPdf {
            doi: doi.to_string(),
        })
    }

    /// Reads an API's JSON and keeps it as a source record. `None` when the
    /// API does not know the DOI or cannot be reached.
    fn json(&mut self, source: &str, url: &str) -> Option<Value> {
        let response = match self.resolver.get(url, &[("User-Agent", &self.user_agent)]) {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!(source, %error, "PDF source unreachable");
                self.network_error = Some(error);
                return None;
            }
        };
        if !response.is_success() {
            tracing::info!(source, status = response.status, "PDF source has no record");
            return None;
        }
        let json = response.json(source).ok()?;
        self.records.push(record(source, &json));
        Some(json)
    }

    /// Downloads the first URL that answers with a PDF.
    fn try_urls(&mut self, source: &str, urls: Vec<String>) -> Option<FoundPdf> {
        for url in urls.into_iter().take(MAX_URLS_PER_SOURCE) {
            let url = force_https(&url);
            if !self.tried.insert(url.clone()) {
                continue;
            }
            let on_step = &mut *self.on_step;
            let response = match self.resolver.download(
                &url,
                &[
                    ("User-Agent", &self.user_agent),
                    ("Accept", "application/pdf,*/*;q=0.8"),
                ],
                &mut |received_bytes, total_bytes| {
                    on_step(PdfStep::Downloading {
                        source: source.to_string(),
                        received_bytes,
                        total_bytes,
                    })
                },
            ) {
                Ok(response) => response,
                Err(error) => {
                    tracing::warn!(source, url, %error, "PDF download failed");
                    self.network_error = Some(error);
                    continue;
                }
            };
            if response.is_success() && is_pdf(&response.body) {
                tracing::info!(source, url, bytes = response.body.len(), "PDF downloaded");
                return Some(FoundPdf {
                    bytes: response.body,
                    url,
                    source: source.to_string(),
                });
            }
            tracing::info!(source, url, status = response.status, "not a PDF");
        }
        None
    }
}

/// Whether the bytes are a PDF file: `%PDF-` near the start.
pub fn is_pdf(bytes: &[u8]) -> bool {
    let window = &bytes[..bytes.len().min(SIGNATURE_WINDOW)];
    window.windows(5).any(|chunk| chunk == b"%PDF-")
}

fn force_https(url: &str) -> String {
    match url.strip_prefix("http://") {
        Some(rest) => format!("https://{rest}"),
        None => url.to_string(),
    }
}

/// `best_oa_location.pdf_url`, then the other open locations with a PDF,
/// published versions before accepted before submitted.
fn openalex_pdf_urls(work: &Value) -> Vec<String> {
    let mut urls: Vec<String> = work["best_oa_location"]["pdf_url"]
        .as_str()
        .map(str::to_string)
        .into_iter()
        .collect();
    let mut located: Vec<(usize, String)> = work["locations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|location| location["is_oa"].as_bool() == Some(true))
        .filter_map(|location| {
            let url = location["pdf_url"].as_str()?;
            Some((version_rank(location["version"].as_str()), url.to_string()))
        })
        .collect();
    located.sort_by_key(|(rank, _)| *rank);
    urls.extend(located.into_iter().map(|(_, url)| url));
    urls
}

fn version_rank(version: Option<&str>) -> usize {
    match version {
        Some("publishedVersion") => 0,
        Some("acceptedVersion") => 1,
        Some("submittedVersion") => 2,
        _ => 3,
    }
}

/// `PMC5619685` from `ids.pmcid`, which OpenAlex gives as a URL.
fn openalex_pmcid(work: &Value) -> Option<String> {
    let text = work["ids"]["pmcid"].as_str()?;
    let digits = text
        .trim_end_matches('/')
        .rsplit('/')
        .next()?
        .trim_start_matches("PMC");
    (!digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()))
        .then(|| format!("PMC{digits}"))
}

fn unpaywall_pdf_urls(json: &Value) -> Vec<String> {
    let best = json["best_oa_location"]["url_for_pdf"].as_str();
    let others = json["oa_locations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|location| location["url_for_pdf"].as_str());
    best.into_iter().chain(others).map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::Response;
    use crate::testing::FakeHttp;

    const OPENALEX_PLOS: &str =
        "https://api.openalex.org/works/https://doi.org/10.1371/journal.pcbi.1005619";
    const PLOS_PDF: &str = "https://journals.plos.org/ploscompbiol/article/file?id=10.1371/journal.pcbi.1005619&type=printable";
    const BIORXIV_PDF: &str =
        "https://www.biorxiv.org/content/biorxiv/early/2017/05/23/088278.full.pdf";

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!(
            "{}/tests/fixtures/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    fn pdf_response(body: &[u8]) -> Response {
        Response {
            status: 200,
            body: body.to_vec(),
            content_type: Some("application/pdf".into()),
            retry_after: None,
        }
    }

    fn plos_query() -> PdfQuery {
        PdfQuery {
            doi: Some("10.1371/journal.pcbi.1005619".into()),
            ..PdfQuery::default()
        }
    }

    #[test]
    fn arxiv_comes_first_and_needs_no_lookup() {
        let http = Arc::new(FakeHttp::new());
        http.route_responses(
            "https://arxiv.org/pdf/1706.03762v7",
            vec![pdf_response(b"%PDF-1.5 attention")],
        );
        let search = Resolver::new(http.clone()).find_pdf(
            &PdfQuery {
                doi: Some("10.48550/arxiv.1706.03762".into()),
                arxiv_id: Some("1706.03762v7".into()),
                ..PdfQuery::default()
            },
            &Contact::default(),
            &mut |_| {},
        );
        let found = search.result.unwrap();
        assert_eq!(found.source, "arxiv");
        assert_eq!(found.bytes, b"%PDF-1.5 attention");
        assert_eq!(http.requests().len(), 1, "no API call for an arXiv paper");
        assert_eq!(
            http.requests()[0].headers.get("Accept").map(String::as_str),
            Some("application/pdf,*/*;q=0.8")
        );
    }

    #[test]
    fn an_arxiv_doi_alone_finds_the_arxiv_pdf() {
        let http = Arc::new(FakeHttp::new());
        http.route_responses(
            "https://arxiv.org/pdf/1706.03762",
            vec![pdf_response(b"%PDF-1.5")],
        );
        let search = Resolver::new(http).find_pdf(
            &PdfQuery {
                doi: Some("10.48550/arxiv.1706.03762".into()),
                ..PdfQuery::default()
            },
            &Contact::default(),
            &mut |_| {},
        );
        assert_eq!(
            search.result.unwrap().url,
            "https://arxiv.org/pdf/1706.03762"
        );
    }

    #[test]
    fn openalex_html_answers_are_skipped_for_the_next_open_location() {
        let http = Arc::new(FakeHttp::new());
        http.route(OPENALEX_PLOS, 200, fixture("openalex_plos_pcbi.json"));
        // The publisher answers with a landing page instead of the file.
        http.route_responses(
            PLOS_PDF,
            vec![Response {
                status: 200,
                body: b"<!DOCTYPE html><html>".to_vec(),
                content_type: Some("text/html".into()),
                retry_after: None,
            }],
        );
        http.route_responses(BIORXIV_PDF, vec![pdf_response(b"\xef\xbb\xbf\n%PDF-1.4")]);

        let search =
            Resolver::new(http.clone()).find_pdf(&plos_query(), &Contact::default(), &mut |_| {});

        let found = search.result.unwrap();
        assert_eq!(
            found.url, BIORXIV_PDF,
            "the accepted version is next in line"
        );
        assert_eq!(found.source, OPENALEX);
        assert_eq!(search.records.len(), 1);
        assert_eq!(search.records[0].source, OPENALEX);
        let urls: Vec<String> = http.requests().into_iter().map(|r| r.url).collect();
        assert_eq!(
            urls.iter().filter(|url| *url == PLOS_PDF).count(),
            1,
            "best_oa_location and the identical location are tried once: {urls:?}"
        );
        assert!(
            !urls.iter().any(|url| url.contains("unpaywall")),
            "no email, so Unpaywall is never asked: {urls:?}"
        );
    }

    #[test]
    fn unpaywall_and_europe_pmc_follow_when_openalex_has_no_file() {
        let http = Arc::new(FakeHttp::new());
        let mut work: Value = serde_json::from_slice(&fixture("openalex_plos_pcbi.json")).unwrap();
        work["best_oa_location"]["pdf_url"] = Value::Null;
        for location in work["locations"].as_array_mut().unwrap() {
            location["pdf_url"] = Value::Null;
        }
        http.route(OPENALEX_PLOS, 200, serde_json::to_vec(&work).unwrap());
        let unpaywall =
            "https://api.unpaywall.org/v2/10.1371/journal.pcbi.1005619?email=me@example.org";
        http.route(
            unpaywall,
            200,
            br#"{"best_oa_location":{"url_for_pdf":"http://repo.example.org/a.pdf"},"oa_locations":[]}"#.to_vec(),
        );
        http.route_responses(
            "https://europepmc.org/articles/PMC5619685?pdf=render",
            vec![pdf_response(b"%PDF-1.7 pmc")],
        );

        let mut steps = Vec::new();
        let search = Resolver::new(http.clone()).find_pdf(
            &plos_query(),
            &Contact::new("me@example.org"),
            &mut |step| steps.push(step),
        );

        let found = search.result.unwrap();
        assert_eq!(found.source, "europepmc");
        let urls: Vec<String> = http.requests().into_iter().map(|r| r.url).collect();
        assert!(
            urls.contains(&"https://repo.example.org/a.pdf".to_string()),
            "Unpaywall's http URL is tried over https: {urls:?}"
        );
        let sources: Vec<&str> = search.records.iter().map(|r| r.source.as_str()).collect();
        assert_eq!(sources, [OPENALEX, UNPAYWALL]);
        let checking = |source: &str| PdfStep::Checking {
            source: source.into(),
        };
        let downloading = |source: &str, bytes| PdfStep::Downloading {
            source: source.into(),
            received_bytes: bytes,
            total_bytes: Some(bytes),
        };
        assert_eq!(
            steps,
            [
                checking(OPENALEX),
                checking(UNPAYWALL),
                // The repository's 404 page is empty.
                downloading(UNPAYWALL, 0),
                checking("europepmc"),
                downloading("europepmc", 12),
            ]
        );
    }

    #[test]
    fn closed_papers_are_no_pdf_and_offline_is_a_network_error() {
        let http = Arc::new(FakeHttp::new());
        http.route(
            "https://api.openalex.org/works/https://doi.org/10.1038/nature14539",
            200,
            br#"{"open_access":{"is_oa":false},"best_oa_location":null,"locations":[{"is_oa":false,"pdf_url":null}]}"#.to_vec(),
        );
        let resolver = Resolver::new(http.clone());
        let query = PdfQuery {
            doi: Some("10.1038/nature14539".into()),
            ..PdfQuery::default()
        };
        let search = resolver.find_pdf(&query, &Contact::default(), &mut |_| {});
        assert_eq!(
            search.result,
            Err(FetchError::NoPdf {
                doi: "10.1038/nature14539".into()
            })
        );
        assert_eq!(search.records.len(), 1, "the closed answer is still kept");

        http.fail_with("offline");
        assert_eq!(
            resolver
                .find_pdf(&query, &Contact::default(), &mut |_| {})
                .result,
            Err(FetchError::Network {
                message: "offline".into()
            })
        );
    }

    #[test]
    fn the_api_key_goes_to_openalex() {
        let http = Arc::new(FakeHttp::new());
        Resolver::new(http.clone()).find_pdf(
            &PdfQuery {
                openalex_api_key: Some("k3y".into()),
                ..plos_query()
            },
            &Contact::default(),
            &mut |_| {},
        );
        assert_eq!(
            http.requests()[0].url,
            format!("{OPENALEX_PLOS}?api_key=k3y")
        );
    }

    #[test]
    fn pdf_signatures_are_found_near_the_start_only() {
        assert!(is_pdf(b"%PDF-1.4\n"));
        assert!(is_pdf(b"\r\n\r\n%PDF-2.0"));
        let page_about_pdfs = [b"<html>".repeat(200), b"%PDF-1.4".to_vec()].concat();
        assert!(!is_pdf(&page_about_pdfs));
        assert!(!is_pdf(b""));
    }
}

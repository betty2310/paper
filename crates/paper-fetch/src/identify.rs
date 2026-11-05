//! The two network steps of identifying a PDF that does not print its DOI: a
//! Crossref search by title, and asking a Claude model to read page 1.

use base64::Engine as _;
use paper_core::SourceRecord;
use serde_json::{Value, json};

use crate::FetchError;
use crate::crossref;
use crate::http::Contact;
use crate::resolver::{Resolver, record};

/// The source name of a Crossref search in `source_records`.
pub const CROSSREF_SEARCH: &str = "crossref_search";
/// The source name of the model's answer in `source_records`.
pub const MODEL_SOURCE: &str = "anthropic";
/// The model that reads page 1. Haiku read the identifiers of 55 of 57 test
/// papers for about $0.0003 each (`docs/research/pdf-identification.md`).
pub const MODEL: &str = "claude-haiku-5-5";
pub const MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Room for the model's thinking as well as the short JSON answer.
const MAX_TOKENS: u32 = 2000;

const PROMPT: &str = "This is the first page of a scholarly PDF. Identify the paper this page belongs to, not papers it cites. \
Reply with only a JSON object with these keys: \
\"doi\" (the paper's own DOI exactly as printed on the page, or null if none is printed), \
\"arxiv_id\" (the arXiv identifier if printed, such as 1706.03762, or null), \
\"title\" (the paper's full title), \
\"first_author_family_name\" (or null). Do not guess a DOI that is not printed.";

/// One search result: a DOI and its registered title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleMatch {
    pub doi: String,
    pub title: String,
}

/// The search results, best first, and the raw response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleSearch {
    pub matches: Vec<TitleMatch>,
    pub record: SourceRecord,
}

/// What the model read on page 1. Every field is the model's reading and must
/// be checked before use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageReading {
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    pub title: Option<String>,
    pub first_author: Option<String>,
    pub record: SourceRecord,
}

impl Resolver {
    /// Searches Crossref for a title (and author, when known). Parts of a
    /// work that share its title, such as supplementary files and peer
    /// reviews, are left out.
    pub fn search_title(
        &self,
        title: &str,
        author: Option<&str>,
        contact: &Contact,
    ) -> Result<TitleSearch, FetchError> {
        let query = match author {
            Some(author) => format!("{title} {author}"),
            None => title.to_string(),
        };
        let url = crossref::search_url(&query, contact.email.as_deref());
        let response = self.get(&url, &[("User-Agent", &contact.user_agent())])?;
        if !response.is_success() {
            return Err(FetchError::Status {
                status: response.status,
                url,
            });
        }
        let json = response.json(crossref::SOURCE)?;
        let matches = json["message"]["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|item| {
                !matches!(
                    item["type"].as_str(),
                    Some("component" | "peer-review" | "other")
                )
            })
            .filter_map(|item| {
                Some(TitleMatch {
                    doi: item["DOI"].as_str()?.to_lowercase(),
                    title: crossref::first_str(&item["title"])?.to_string(),
                })
            })
            .collect();
        Ok(TitleSearch {
            matches,
            record: record(CROSSREF_SEARCH, &json),
        })
    }

    /// Sends page 1 as a PNG to the model and returns what it read. Fails
    /// with `Status` 401 when the key is wrong.
    pub fn read_first_page(&self, png: &[u8], api_key: &str) -> Result<PageReading, FetchError> {
        let body = json!({
            "model": MODEL,
            "max_tokens": MAX_TOKENS,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "image", "source": {
                        "type": "base64",
                        "media_type": "image/png",
                        "data": base64::engine::general_purpose::STANDARD.encode(png),
                    }},
                    {"type": "text", "text": PROMPT},
                ],
            }],
        });
        let response = self.post(
            MESSAGES_URL,
            &[
                ("x-api-key", api_key),
                ("anthropic-version", ANTHROPIC_VERSION),
                ("content-type", "application/json"),
            ],
            body.to_string().as_bytes(),
        )?;
        if !response.is_success() {
            return Err(FetchError::Status {
                status: response.status,
                url: MESSAGES_URL.to_string(),
            });
        }
        let json = response.json(MODEL_SOURCE)?;
        let answer = parse_answer(&json).ok_or_else(|| FetchError::Parse {
            api: MODEL_SOURCE.to_string(),
            message: "the answer held no JSON object".to_string(),
        })?;
        let field = |key: &str| {
            answer[key]
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty() && *value != "null")
                .map(str::to_string)
        };
        Ok(PageReading {
            doi: field("doi"),
            arxiv_id: field("arxiv_id"),
            title: field("title"),
            first_author: field("first_author_family_name"),
            record: record(MODEL_SOURCE, &json),
        })
    }
}

/// The JSON object in the first text block. The model may think first, and
/// may wrap the object in prose or a code fence.
fn parse_answer(response: &Value) -> Option<Value> {
    let text = response["content"]
        .as_array()?
        .iter()
        .find(|block| block["type"] == "text")?["text"]
        .as_str()?;
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    serde_json::from_str(text.get(start..=end)?).ok()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::testing::FakeHttp;

    #[test]
    fn title_search_skips_parts_that_share_the_title() {
        let http = Arc::new(FakeHttp::new());
        let url = crossref::search_url("Rate vs Free Energy Smith", Some("me@example.org"));
        assert_eq!(
            url,
            "https://api.crossref.org/works?rows=5&select=DOI,title,type&query.bibliographic=Rate%20vs%20Free%20Energy%20Smith&mailto=me@example.org"
        );
        http.route(
            &url,
            200,
            json!({"message": {"items": [
                {"DOI": "10.1021/ACS.JPCB.9B07845.S001", "title": ["Rate vs Free Energy"], "type": "component"},
                {"DOI": "10.1021/ACS.JPCB.9B07845", "title": ["Rate vs Free Energy"], "type": "journal-article"},
                {"DOI": "10.1000/untitled", "type": "journal-article"},
            ]}})
            .to_string()
            .into_bytes(),
        );
        let search = Resolver::new(http)
            .search_title(
                "Rate vs Free Energy",
                Some("Smith"),
                &Contact::new("me@example.org"),
            )
            .unwrap();
        assert_eq!(
            search.matches,
            vec![TitleMatch {
                doi: "10.1021/acs.jpcb.9b07845".into(),
                title: "Rate vs Free Energy".into()
            }]
        );
        assert_eq!(search.record.source, CROSSREF_SEARCH);
    }

    #[test]
    fn the_model_gets_the_page_and_its_answer_is_read_after_its_thinking() {
        let http = Arc::new(FakeHttp::new());
        http.route(
            MESSAGES_URL,
            200,
            json!({"content": [
                {"type": "thinking", "thinking": "The header shows {a DOI}."},
                {"type": "text", "text": "```json\n{\"doi\": \"https://doi.org/10.1038/nature14539\", \"arxiv_id\": null, \"title\": \"Deep learning\", \"first_author_family_name\": \"LeCun\"}\n```"},
            ]})
            .to_string()
            .into_bytes(),
        );
        let reading = Resolver::new(http.clone())
            .read_first_page(b"\x89PNG fake", "sk-test")
            .unwrap();
        assert_eq!(
            reading.doi.as_deref(),
            Some("https://doi.org/10.1038/nature14539")
        );
        assert_eq!(reading.arxiv_id, None);
        assert_eq!(reading.title.as_deref(), Some("Deep learning"));
        assert_eq!(reading.first_author.as_deref(), Some("LeCun"));

        let request = &http.requests()[0];
        assert_eq!(request.method, "POST");
        assert_eq!(request.headers["x-api-key"], "sk-test");
        let sent: Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(sent["model"], MODEL);
        let image = &sent["messages"][0]["content"][0]["source"];
        assert_eq!(image["media_type"], "image/png");
        assert_eq!(
            image["data"],
            base64::engine::general_purpose::STANDARD.encode(b"\x89PNG fake")
        );
    }

    #[test]
    fn a_rejected_key_is_a_status_error() {
        let http = Arc::new(FakeHttp::new());
        http.route(MESSAGES_URL, 401, b"{\"type\":\"error\"}".to_vec());
        assert_eq!(
            Resolver::new(http).read_first_page(b"png", "wrong"),
            Err(FetchError::Status {
                status: 401,
                url: MESSAGES_URL.to_string()
            })
        );
    }
}

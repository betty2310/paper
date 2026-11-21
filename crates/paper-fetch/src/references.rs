//! The works a paper cites, as OpenAlex lists them, for giving the entries
//! of its reference list an identifier. Reading one OpenAlex record is free
//! and has no daily limit; searching is not, so nothing here searches. See
//! `docs/research/references.md`.

use serde_json::Value;

use crate::FetchError;
use crate::http::Contact;
use crate::resolver::Resolver;

/// The source name, in `source_records`, of the cited works' raw records.
pub const OPENALEX_REFERENCES: &str = "openalex_references";

/// One work a paper cites.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CitedWork {
    /// Lowercase, no prefix.
    pub doi: Option<String>,
    pub title: String,
    pub year: Option<i32>,
}

/// The OpenAlex ids, such as `W2963403868`, of the works an OpenAlex work
/// cites, in OpenAlex's order, which is not the paper's.
pub fn referenced_work_ids(work: &Value) -> Vec<String> {
    work["referenced_works"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|url| url.rsplit('/').next())
        .filter(|id| id.starts_with('W') && id[1..].chars().all(|c| c.is_ascii_digit()))
        .map(str::to_string)
        .collect()
}

/// A cited work from the record `Resolver::openalex_cited_work` returns.
/// `None` without a title.
pub fn cited_work(record: &Value) -> Option<CitedWork> {
    let title = record["title"].as_str()?.trim();
    if title.is_empty() {
        return None;
    }
    Some(CitedWork {
        doi: record["doi"]
            .as_str()
            .map(|doi| doi.trim_start_matches("https://doi.org/").to_lowercase()),
        title: title.to_string(),
        year: record["publication_year"]
            .as_i64()
            .and_then(|year| i32::try_from(year).ok()),
    })
}

/// The URL `Resolver::openalex_cited_work` asks, for scripting answers in
/// tests.
pub fn openalex_cited_work_url(id: &str, api_key: Option<&str>) -> String {
    let mut url =
        format!("https://api.openalex.org/works/{id}?select=id,doi,title,publication_year");
    if let Some(key) = api_key.filter(|key| !key.is_empty()) {
        url.push_str(&format!("&api_key={key}"));
    }
    url
}

impl Resolver {
    /// Reads the id, DOI, title, and year of one OpenAlex work. `None` when
    /// OpenAlex does not know it.
    pub fn openalex_cited_work(
        &self,
        id: &str,
        api_key: Option<&str>,
        contact: &Contact,
    ) -> Result<Option<Value>, FetchError> {
        let url = openalex_cited_work_url(id, api_key);
        let response = self.get(&url, &[("User-Agent", &contact.user_agent())])?;
        if response.status == 404 {
            return Ok(None);
        }
        if !response.is_success() {
            return Err(FetchError::Status {
                status: response.status,
                url,
            });
        }
        response.json(crate::pdf::OPENALEX).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn referenced_ids_and_cited_works_are_read_from_openalex_records() {
        let work = json!({"referenced_works": [
            "https://openalex.org/W2963403868", "https://openalex.org/A123", "nonsense W1x"
        ]});
        assert_eq!(referenced_work_ids(&work), vec!["W2963403868".to_string()]);
        let record = json!({
            "id": "https://openalex.org/W2963403868",
            "doi": "https://doi.org/10.48550/ARXIV.1706.03762",
            "title": "Attention Is All You Need",
            "publication_year": 2017
        });
        assert_eq!(
            cited_work(&record),
            Some(CitedWork {
                doi: Some("10.48550/arxiv.1706.03762".to_string()),
                title: "Attention Is All You Need".to_string(),
                year: Some(2017),
            })
        );
        assert_eq!(cited_work(&json!({"title": null})), None);
    }
}

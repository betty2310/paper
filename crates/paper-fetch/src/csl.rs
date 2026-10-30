//! DOI content negotiation: `GET https://doi.org/{doi}` with
//! `Accept: application/vnd.citationstyles.csl+json`. Works for Crossref,
//! DataCite (arXiv, Zenodo), and mEDRA DOIs, and is the fallback when Crossref
//! does not know a DOI.

use paper_core::{CreatorRole, ItemFields, ItemKind};
use serde_json::Value;

use crate::crossref::{creators, date_parts, first_str, percent_encode_path, str_at};
use crate::text::{collapse_whitespace, strip_jats};

pub const SOURCE: &str = "doi_org";
pub const ACCEPT: &str = "application/vnd.citationstyles.csl+json";

pub fn url(doi: &str) -> String {
    format!("https://doi.org/{}", percent_encode_path(doi))
}

/// Maps a CSL JSON item onto our fields.
pub fn map_csl(item: &Value) -> ItemFields {
    let doi = str_at(item, "DOI").map(|doi| doi.to_lowercase());
    let (year, published_date) = ["issued", "published", "created"]
        .iter()
        .find_map(|key| date_parts(&item[key]))
        .unzip();
    let publisher = str_at(item, "publisher").map(str::to_string);
    let mut title = first_str(&item["title"])
        .map(collapse_whitespace)
        .unwrap_or_default();
    if title.is_empty() {
        title = doi.clone().unwrap_or_else(|| "Untitled".to_string());
    }
    let mut creators_list = creators(&item["author"], CreatorRole::Author);
    creators_list.extend(creators(&item["editor"], CreatorRole::Editor));
    let arxiv_id = doi
        .as_deref()
        .and_then(|doi| doi.strip_prefix("10.48550/arxiv."))
        .map(str::to_string);

    ItemFields {
        kind: Some(kind_from_csl(
            str_at(item, "type").unwrap_or(""),
            arxiv_id.is_some() || publisher.as_deref() == Some("arXiv"),
        )),
        arxiv_id,
        doi,
        title,
        creators: creators_list,
        container_title: first_str(&item["container-title"]).map(collapse_whitespace),
        container_short: first_str(&item["container-title-short"]).map(collapse_whitespace),
        publisher,
        year,
        published_date,
        volume: str_at(item, "volume").map(str::to_string),
        issue: str_at(item, "issue").map(str::to_string),
        pages: str_at(item, "page").map(str::to_string),
        abstract_text: str_at(item, "abstract")
            .map(strip_jats)
            .filter(|text| !text.is_empty()),
        url: str_at(item, "URL").map(str::to_string),
        issn: first_str(&item["ISSN"]).map(str::to_string),
        language: str_at(item, "language").map(str::to_string),
        extra: None,
    }
}

fn kind_from_csl(csl_type: &str, is_arxiv: bool) -> ItemKind {
    match csl_type {
        "article-journal" => ItemKind::JournalArticle,
        "paper-conference" => ItemKind::ConferencePaper,
        "article" if is_arxiv => ItemKind::Preprint,
        "posted-content" | "preprint" => ItemKind::Preprint,
        _ => ItemKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paper_core::Creator;

    #[test]
    fn arxiv_record_from_doi_org_maps_to_a_preprint() {
        let path = format!(
            "{}/tests/fixtures/csl_arxiv_1706_03762.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let item: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let fields = map_csl(&item);
        assert_eq!(fields.kind, Some(ItemKind::Preprint));
        assert_eq!(fields.doi.as_deref(), Some("10.48550/arxiv.1706.03762"));
        assert_eq!(fields.arxiv_id.as_deref(), Some("1706.03762"));
        assert_eq!(fields.title, "Attention Is All You Need");
        assert_eq!(fields.creators.len(), 8);
        assert_eq!(fields.creators[0], Creator::person("Ashish", "Vaswani"));
        assert_eq!(fields.publisher.as_deref(), Some("arXiv"));
        assert_eq!(fields.year, Some(2017));
        assert_eq!(fields.published_date.as_deref(), Some("2017"));
        assert_eq!(
            fields.url.as_deref(),
            Some("https://arxiv.org/abs/1706.03762")
        );
        assert!(
            fields
                .abstract_text
                .unwrap()
                .starts_with("The dominant sequence transduction models")
        );
        assert_eq!(fields.container_title, None);
    }

    #[test]
    fn csl_types_map_to_kinds() {
        assert_eq!(
            kind_from_csl("article-journal", false),
            ItemKind::JournalArticle
        );
        assert_eq!(
            kind_from_csl("paper-conference", false),
            ItemKind::ConferencePaper
        );
        assert_eq!(kind_from_csl("article", true), ItemKind::Preprint);
        assert_eq!(kind_from_csl("article", false), ItemKind::Other);
        assert_eq!(kind_from_csl("dataset", false), ItemKind::Other);
    }
}

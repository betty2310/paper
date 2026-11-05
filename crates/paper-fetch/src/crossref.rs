//! Crossref REST API: `GET https://api.crossref.org/works/{doi}`.

use paper_core::{Creator, CreatorRole, ItemFields, ItemKind};
use serde_json::Value;

use crate::text::{collapse_whitespace, strip_jats};

pub const SOURCE: &str = "crossref";

pub fn work_url(doi: &str, email: Option<&str>) -> String {
    let encoded = percent_encode_path(doi);
    match email {
        Some(email) => format!(
            "https://api.crossref.org/works/{encoded}?mailto={}",
            percent_encode_query_value(email)
        ),
        None => format!("https://api.crossref.org/works/{encoded}"),
    }
}

/// A bibliographic search for `query` (a title, optionally with an author),
/// asking for the five best matches.
pub fn search_url(query: &str, email: Option<&str>) -> String {
    let mut url = format!(
        "https://api.crossref.org/works?rows=5&select=DOI,title,type&query.bibliographic={}",
        percent_encode_query_value(query)
    );
    if let Some(email) = email {
        url.push_str("&mailto=");
        url.push_str(&percent_encode_query_value(email));
    }
    url
}

/// Encodes a query value; `+` in an address would otherwise arrive as a space.
fn percent_encode_query_value(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'@' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Maps the `message` object of a Crossref work onto our fields.
pub fn map_work(message: &Value) -> ItemFields {
    let doi = str_at(message, "DOI").map(|doi| doi.to_lowercase());
    let (year, published_date) = ["issued", "published-online", "published-print", "created"]
        .iter()
        .find_map(|key| date_parts(&message[key]))
        .unzip();
    let mut title = first_str(&message["title"])
        .map(collapse_whitespace)
        .unwrap_or_default();
    if let Some(subtitle) = first_str(&message["subtitle"]).map(collapse_whitespace)
        && !subtitle.is_empty()
        && !title.to_lowercase().contains(&subtitle.to_lowercase())
    {
        title = format!("{title}: {subtitle}");
    }
    if title.is_empty() {
        title = doi.clone().unwrap_or_else(|| "Untitled".to_string());
    }
    let mut creators = creators(&message["author"], CreatorRole::Author);
    creators.extend(creators_from(&message["editor"], CreatorRole::Editor));

    ItemFields {
        kind: Some(kind_from_type(str_at(message, "type").unwrap_or(""))),
        arxiv_id: doi
            .as_deref()
            .and_then(|doi| doi.strip_prefix("10.48550/arxiv."))
            .map(str::to_string),
        doi,
        title,
        creators,
        container_title: first_str(&message["container-title"]).map(collapse_whitespace),
        container_short: first_str(&message["short-container-title"]).map(collapse_whitespace),
        publisher: str_at(message, "publisher").map(str::to_string),
        year,
        published_date,
        volume: str_at(message, "volume").map(str::to_string),
        issue: str_at(message, "issue").map(str::to_string),
        pages: str_at(message, "page")
            .or_else(|| str_at(message, "article-number"))
            .map(str::to_string),
        abstract_text: str_at(message, "abstract")
            .map(strip_jats)
            .filter(|text| !text.is_empty()),
        url: message["resource"]["primary"]["URL"]
            .as_str()
            .or_else(|| str_at(message, "URL"))
            .map(str::to_string),
        issn: first_str(&message["ISSN"]).map(str::to_string),
        language: str_at(message, "language").map(str::to_string),
        extra: None,
    }
}

pub fn kind_from_type(crossref_type: &str) -> ItemKind {
    match crossref_type {
        "journal-article" => ItemKind::JournalArticle,
        "proceedings-article" => ItemKind::ConferencePaper,
        "posted-content" => ItemKind::Preprint,
        _ => ItemKind::Other,
    }
}

/// Crossref and CSL JSON share the `author[]` shape.
pub fn creators(list: &Value, role: CreatorRole) -> Vec<Creator> {
    creators_from(list, role).collect()
}

fn creators_from(list: &Value, role: CreatorRole) -> impl Iterator<Item = Creator> + '_ {
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(move |person| {
            let family = str_at(person, "family").map(collapse_whitespace);
            let given = str_at(person, "given").map(collapse_whitespace);
            let literal = str_at(person, "name")
                .or_else(|| str_at(person, "literal"))
                .map(collapse_whitespace);
            if family.is_none() && literal.is_none() && given.is_none() {
                return None;
            }
            Some(Creator {
                role: Some(role),
                family,
                given,
                literal,
                orcid: str_at(person, "ORCID").map(|orcid| {
                    orcid
                        .trim_start_matches("https://orcid.org/")
                        .trim_start_matches("http://orcid.org/")
                        .to_string()
                }),
            })
        })
}

/// `{"date-parts": [[2015, 5, 27]]}` to `(2015, "2015-05-27")`.
pub fn date_parts(date: &Value) -> Option<(i32, String)> {
    let parts = date["date-parts"][0].as_array()?;
    let year = parts.first()?.as_i64()? as i32;
    let month = parts.get(1).and_then(Value::as_i64);
    let day = parts.get(2).and_then(Value::as_i64);
    let text = match (month, day) {
        (Some(month), Some(day)) => format!("{year:04}-{month:02}-{day:02}"),
        (Some(month), None) => format!("{year:04}-{month:02}"),
        _ => format!("{year:04}"),
    };
    Some((year, text))
}

pub fn str_at<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value[key]
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

/// CSL JSON has plain strings where Crossref has one element arrays.
pub fn first_str(value: &Value) -> Option<&str> {
    match value {
        Value::String(text) => Some(text.trim()).filter(|text| !text.is_empty()),
        Value::Array(items) => items.iter().find_map(first_str),
        _ => None,
    }
}

/// Encodes a DOI for use in a URL path. Slashes stay, because Crossref and
/// doi.org both accept them literally.
pub fn percent_encode_path(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'.'
            | b'_'
            | b'~'
            | b'/'
            | b'('
            | b')'
            | b':'
            | b';' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("{path}")))
            .unwrap()
    }

    #[test]
    fn journal_article_maps_every_field() {
        let fields = map_work(&fixture("crossref_nature14539.json")["message"]);
        assert_eq!(fields.kind, Some(ItemKind::JournalArticle));
        assert_eq!(fields.doi.as_deref(), Some("10.1038/nature14539"));
        assert_eq!(fields.title, "Deep learning");
        assert_eq!(fields.creators.len(), 3);
        assert_eq!(fields.creators[0], Creator::person("Yann", "LeCun"));
        assert_eq!(fields.creators[2].family.as_deref(), Some("Hinton"));
        assert_eq!(fields.container_title.as_deref(), Some("Nature"));
        assert_eq!(fields.container_short.as_deref(), Some("Nature"));
        assert_eq!(
            fields.publisher.as_deref(),
            Some("Springer Science and Business Media LLC")
        );
        assert_eq!(fields.year, Some(2015));
        assert_eq!(fields.published_date.as_deref(), Some("2015-05-27"));
        assert_eq!(fields.volume.as_deref(), Some("521"));
        assert_eq!(fields.issue.as_deref(), Some("7553"));
        assert_eq!(fields.pages.as_deref(), Some("436-444"));
        assert_eq!(fields.issn.as_deref(), Some("0028-0836"));
        assert_eq!(
            fields.url.as_deref(),
            Some("https://www.nature.com/articles/nature14539")
        );
        assert_eq!(fields.language.as_deref(), Some("en"));
        assert_eq!(fields.abstract_text, None);
        assert_eq!(fields.arxiv_id, None);
    }

    #[test]
    fn proceedings_article_joins_title_and_subtitle() {
        let fields = map_work(&fixture("crossref_xgboost.json")["message"]);
        assert_eq!(fields.kind, Some(ItemKind::ConferencePaper));
        assert_eq!(fields.title, "XGBoost: A Scalable Tree Boosting System");
        assert_eq!(
            fields.container_title.as_deref(),
            Some(
                "Proceedings of the 22nd ACM SIGKDD International Conference on Knowledge Discovery and Data Mining"
            )
        );
        assert_eq!(fields.container_short, None);
        assert_eq!(fields.pages.as_deref(), Some("785-794"));
        assert_eq!(fields.year, Some(2016));
        assert_eq!(fields.creators.len(), 2);
    }

    #[test]
    fn article_number_orcid_and_short_journal_are_read() {
        let fields = map_work(&fixture("crossref_plos_pcbi.json")["message"]);
        assert_eq!(fields.pages.as_deref(), Some("e1005619"));
        assert_eq!(fields.container_short.as_deref(), Some("PLoS Comput Biol"));
        assert_eq!(
            fields.creators[0].orcid.as_deref(),
            Some("0000-0003-2288-054X")
        );
        assert_eq!(fields.volume.as_deref(), Some("13"));
        assert_eq!(fields.issue.as_deref(), Some("9"));
    }

    #[test]
    fn preprint_abstract_is_stripped_of_jats() {
        let fields = map_work(&fixture("crossref_biorxiv.json")["message"]);
        assert_eq!(fields.kind, Some(ItemKind::Preprint));
        assert_eq!(fields.creators.len(), 100);
        let abstract_text = fields.abstract_text.unwrap();
        assert!(
            abstract_text.starts_with("An outbreak of the novel coronavirus SARS-CoV-2"),
            "{abstract_text}"
        );
        assert!(!abstract_text.contains('<'), "{abstract_text}");
        assert!(!abstract_text.contains("  "), "{abstract_text}");
        assert_eq!(fields.container_title, None);
        assert_eq!(fields.published_date.as_deref(), Some("2020-03-22"));
    }

    #[test]
    fn missing_fields_fall_back_sensibly() {
        let fields = map_work(&serde_json::json!({
            "DOI": "10.1000/ABC",
            "type": "book-chapter",
            "published-print": {"date-parts": [[2001, 3]]},
            "author": [{"name": "The ATLAS Collaboration"}, {"given": "Only"}],
            "URL": "https://doi.org/10.1000/abc"
        }));
        assert_eq!(fields.kind, Some(ItemKind::Other));
        assert_eq!(
            fields.title, "10.1000/abc",
            "the DOI stands in for a missing title"
        );
        assert_eq!(fields.year, Some(2001));
        assert_eq!(fields.published_date.as_deref(), Some("2001-03"));
        assert_eq!(
            fields.creators[0].literal.as_deref(),
            Some("The ATLAS Collaboration")
        );
        assert_eq!(fields.creators[0].family, None);
        assert_eq!(fields.creators[1].given.as_deref(), Some("Only"));
        assert_eq!(fields.url.as_deref(), Some("https://doi.org/10.1000/abc"));
    }

    #[test]
    fn work_url_encodes_the_doi_and_adds_the_polite_email() {
        assert_eq!(
            work_url(
                "10.1002/(sici)1097-0258<1661::aid-sim968>3.0.co;2-2",
                Some("a@b.c")
            ),
            "https://api.crossref.org/works/10.1002/(sici)1097-0258%3C1661::aid-sim968%3E3.0.co;2-2?mailto=a@b.c"
        );
        assert_eq!(
            work_url("10.1038/nature14539", None),
            "https://api.crossref.org/works/10.1038/nature14539"
        );
        assert_eq!(
            work_url("10.1038/nature14539", Some("me+paper@gmail.com")),
            "https://api.crossref.org/works/10.1038/nature14539?mailto=me%2Bpaper@gmail.com",
            "a plus in the address must not become a space"
        );
    }
}

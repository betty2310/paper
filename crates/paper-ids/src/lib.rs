//! DOI and arXiv identifier parsing and normalization.
//!
//! The rules follow `docs/research/doi-and-pdf-sources.md`: a DOI is
//! `10.` + 4 to 9 digits + `/` + a suffix, accepted with or without `doi:` or
//! a doi.org URL in front and stored lowercase. An arXiv ID is either the new
//! form `2104.12345` or the old form `hep-th/9901001`, optionally with a
//! version, and is accepted with an `arXiv:` prefix or an arxiv.org URL.

use std::fmt;

/// A normalized identifier the user can add a paper by.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Identifier {
    /// A lowercase DOI without any prefix, for example `10.1038/nature14539`.
    Doi(String),
    /// An arXiv identifier without the `arXiv:` prefix, for example
    /// `1706.03762` or `hep-th/9901001`. The version suffix is kept when given.
    Arxiv(String),
}

impl Identifier {
    /// Parses user input: a DOI, a `doi:` string, a doi.org URL, an arXiv ID,
    /// an `arXiv:` string, or an arxiv.org URL. Returns `None` for anything
    /// else, including free text that should become a search instead.
    pub fn parse(text: &str) -> Option<Identifier> {
        let text = text.trim();
        if text.is_empty() || text.chars().any(char::is_whitespace) {
            return None;
        }
        if let Some(arxiv) = parse_arxiv(text) {
            return Some(Identifier::Arxiv(arxiv));
        }
        normalize_doi(text).map(Identifier::Doi)
    }

    /// The DOI this identifier resolves to. arXiv IDs have a DataCite DOI of the
    /// form `10.48550/arxiv.<id>` (without the version).
    pub fn doi(&self) -> String {
        match self {
            Identifier::Doi(doi) => doi.clone(),
            Identifier::Arxiv(id) => format!("10.48550/arxiv.{}", strip_arxiv_version(id)),
        }
    }

    /// The arXiv ID, if this identifier is one or is the DOI of one.
    pub fn arxiv_id(&self) -> Option<String> {
        match self {
            Identifier::Arxiv(id) => Some(id.clone()),
            Identifier::Doi(doi) => doi
                .strip_prefix("10.48550/arxiv.")
                .filter(|id| is_arxiv_id(id))
                .map(str::to_string),
        }
    }
}

impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Identifier::Doi(doi) => write!(f, "{doi}"),
            Identifier::Arxiv(id) => write!(f, "arXiv:{id}"),
        }
    }
}

/// Accepts `10.x/y`, `doi:10.x/y`, and doi.org URLs. Returns the lowercase DOI
/// with trailing punctuation removed, or `None` if the text is not a DOI.
pub fn normalize_doi(text: &str) -> Option<String> {
    let mut rest = text.trim();
    for scheme in ["https://", "http://"] {
        if let Some(stripped) = strip_prefix_ignore_case(rest, scheme) {
            rest = stripped;
            break;
        }
    }
    for host in ["dx.doi.org/", "doi.org/", "www.doi.org/"] {
        if let Some(stripped) = strip_prefix_ignore_case(rest, host) {
            rest = stripped;
            break;
        }
    }
    if let Some(stripped) = strip_prefix_ignore_case(rest, "doi:") {
        rest = stripped;
    }
    let decoded = percent_decode(rest.trim());
    let trimmed = decoded.trim_end_matches(['.', ',', ';', ')', ']']);

    let (prefix, suffix) = trimmed.split_once('/')?;
    let digits = prefix.strip_prefix("10.")?;
    let prefix_ok = (4..=9).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit());
    let suffix_ok = !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_graphic());
    (prefix_ok && suffix_ok).then(|| trimmed.to_lowercase())
}

/// Accepts a bare arXiv ID, `arXiv:<id>`, or an arxiv.org abs or pdf URL and
/// returns the bare ID with its version, or `None` if the text is not one.
pub fn normalize_arxiv_id(text: &str) -> Option<String> {
    let text = text.trim();
    if text.chars().any(char::is_whitespace) {
        return None;
    }
    parse_arxiv(text)
}

/// Accepts a bare arXiv ID, `arXiv:<id>`, or an arxiv.org abs or pdf URL.
fn parse_arxiv(text: &str) -> Option<String> {
    let mut rest = text;
    for scheme in ["https://", "http://"] {
        if let Some(stripped) = strip_prefix_ignore_case(rest, scheme) {
            rest = stripped;
            break;
        }
    }
    let from_url = [
        "arxiv.org/abs/",
        "arxiv.org/pdf/",
        "www.arxiv.org/abs/",
        "www.arxiv.org/pdf/",
    ]
    .iter()
    .find_map(|prefix| strip_prefix_ignore_case(rest, prefix));
    let candidate = match from_url {
        Some(id) => id.trim_end_matches(".pdf"),
        None => match strip_prefix_ignore_case(rest, "arxiv:") {
            Some(id) => id,
            // A bare new form ID like `1706.03762` cannot be anything else, but
            // a bare old form like `math.GT/0309136` is too close to a path.
            None if !rest.contains('/') => rest,
            None => return None,
        },
    };
    is_arxiv_id(candidate).then(|| candidate.to_string())
}

/// New form `YYMM.NNNNN(vN)` or old form `archive(.SC)/YYMMNNN(vN)`.
fn is_arxiv_id(id: &str) -> bool {
    let body = strip_arxiv_version(id);
    if let Some((year_month, number)) = body.split_once('.')
        && year_month.len() == 4
        && year_month.bytes().all(|b| b.is_ascii_digit())
    {
        return (4..=5).contains(&number.len()) && number.bytes().all(|b| b.is_ascii_digit());
    }
    if let Some((archive, number)) = body.split_once('/') {
        let (archive, subject) = archive.split_once('.').unwrap_or((archive, ""));
        let archive_ok =
            !archive.is_empty() && archive.bytes().all(|b| b.is_ascii_lowercase() || b == b'-');
        let subject_ok = subject.is_empty()
            || (subject.len() == 2 && subject.bytes().all(|b| b.is_ascii_uppercase()));
        let number_ok = number.len() == 7 && number.bytes().all(|b| b.is_ascii_digit());
        return archive_ok && subject_ok && number_ok;
    }
    false
}

fn strip_arxiv_version(id: &str) -> &str {
    match id.rfind('v') {
        Some(at)
            if at > 0 && id[at + 1..].bytes().all(|b| b.is_ascii_digit()) && at + 1 < id.len() =>
        {
            &id[..at]
        }
        _ => id,
    }
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

/// Decodes `%2F` and friends, which doi.org URLs copied from a browser contain.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(Ok(value)) = text
                .get(i + 1..i + 3)
                .map(|hex| u8::from_str_radix(hex, 16))
        {
            out.push(value);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dois_in_every_accepted_form_normalize_to_the_same_value() {
        for text in [
            "10.1038/nature14539",
            " 10.1038/NATURE14539 ",
            "doi:10.1038/nature14539",
            "DOI:10.1038/nature14539",
            "https://doi.org/10.1038/nature14539",
            "http://dx.doi.org/10.1038/nature14539",
            "HTTPS://DOI.ORG/10.1038/nature14539",
            "https://doi.org/10.1038%2Fnature14539",
            "10.1038/nature14539.",
            "10.1038/nature14539),",
        ] {
            assert_eq!(
                Identifier::parse(text),
                Some(Identifier::Doi("10.1038/nature14539".into())),
                "{text:?}"
            );
        }
    }

    #[test]
    fn dois_with_unusual_suffixes_are_accepted() {
        let wiley = "10.1002/(SICI)1097-0258(19980815/30)17:15/16<1661::AID-SIM968>3.0.CO;2-2";
        assert_eq!(
            normalize_doi(wiley).as_deref(),
            Some(wiley.to_lowercase().as_str())
        );
        assert_eq!(
            normalize_doi("10.1371/journal.pcbi.1005619").as_deref(),
            Some("10.1371/journal.pcbi.1005619")
        );
        assert_eq!(
            normalize_doi("10.48550/arXiv.1706.03762").as_deref(),
            Some("10.48550/arxiv.1706.03762")
        );
    }

    #[test]
    fn text_that_is_not_an_identifier_is_rejected() {
        for text in [
            "",
            "   ",
            "attention is all you need",
            "10.1038",
            "10.1038/",
            "10./x",
            "11.1000/x",
            "10.123/x",
            "10.1234567890/x",
            "10.1038/nature 14539",
            "doi.org/",
            "https://example.com/paper.pdf",
            "arXiv:",
            "arXiv:abc",
            "1706.037",
            "1706.0376212",
        ] {
            assert_eq!(Identifier::parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn arxiv_ids_in_every_accepted_form_are_recognized() {
        for text in [
            "1706.03762",
            "arXiv:1706.03762",
            "arxiv:1706.03762",
            "https://arxiv.org/abs/1706.03762",
            "https://arxiv.org/pdf/1706.03762",
            "https://arxiv.org/pdf/1706.03762.pdf",
            "http://www.arxiv.org/abs/1706.03762",
        ] {
            let id = Identifier::parse(text);
            assert_eq!(id, Some(Identifier::Arxiv("1706.03762".into())), "{text:?}");
            assert_eq!(id.unwrap().doi(), "10.48550/arxiv.1706.03762");
        }
        assert_eq!(
            Identifier::parse("arXiv:1706.03762v7"),
            Some(Identifier::Arxiv("1706.03762v7".into()))
        );
        assert_eq!(
            Identifier::parse("arXiv:1706.03762v7").unwrap().doi(),
            "10.48550/arxiv.1706.03762",
            "the DataCite DOI has no version"
        );
        assert_eq!(
            Identifier::parse("2104.12345"),
            Some(Identifier::Arxiv("2104.12345".into()))
        );
        assert_eq!(
            Identifier::parse("arXiv:hep-th/9901001v2"),
            Some(Identifier::Arxiv("hep-th/9901001v2".into()))
        );
        assert_eq!(
            Identifier::parse("math.GT/0309136"),
            None,
            "old form without a prefix or URL is ambiguous and not accepted"
        );
        assert_eq!(
            Identifier::parse("arXiv:math.GT/0309136"),
            Some(Identifier::Arxiv("math.GT/0309136".into()))
        );
    }

    #[test]
    fn arxiv_id_is_recovered_from_its_doi() {
        let doi = Identifier::Doi("10.48550/arxiv.1706.03762".into());
        assert_eq!(doi.arxiv_id().as_deref(), Some("1706.03762"));
        assert_eq!(
            Identifier::Doi("10.1038/nature14539".into()).arxiv_id(),
            None
        );
        assert_eq!(
            Identifier::Arxiv("1706.03762v2".into())
                .arxiv_id()
                .as_deref(),
            Some("1706.03762v2")
        );
    }

    #[test]
    fn edited_arxiv_ids_normalize_like_typed_ones() {
        assert_eq!(
            normalize_arxiv_id(" arXiv:1706.03762v7 ").as_deref(),
            Some("1706.03762v7")
        );
        assert_eq!(
            normalize_arxiv_id("https://arxiv.org/abs/hep-th/9901001").as_deref(),
            Some("hep-th/9901001")
        );
        assert_eq!(normalize_arxiv_id("1706.03762 v7"), None);
        assert_eq!(normalize_arxiv_id("10.1038/nature14539"), None);
    }

    #[test]
    fn display_is_what_a_user_would_type() {
        assert_eq!(
            Identifier::Doi("10.1038/nature14539".into()).to_string(),
            "10.1038/nature14539"
        );
        assert_eq!(
            Identifier::Arxiv("1706.03762".into()).to_string(),
            "arXiv:1706.03762"
        );
    }
}

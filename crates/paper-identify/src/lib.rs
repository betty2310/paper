//! Finds which paper a PDF is, from the file alone.
//!
//! `PdfFile::head` reads what identification needs from the first pages: the
//! Info dictionary, the XMP packet, link URIs, and the text lines with their
//! font sizes. `PdfHead::candidates` ranks the DOIs and arXiv IDs found there,
//! and `PdfHead::title_guess` takes the largest text on page 1. None of this
//! is trusted on its own: the engine confirms each answer with Crossref, using
//! `title_coverage` and `title_similarity` from here. `PdfFile::render_first_page`
//! draws page 1 for the model fallback.
//!
//! The approach and its measured accuracy are in
//! `docs/research/pdf-identification.md`.

use std::collections::HashMap;
use std::sync::LazyLock;

use paper_ids::Identifier;
use pdf_oxide::PdfDocument;
use pdf_oxide::object::Object;
use regex::Regex;

/// Pages read from the start of a file. A paper's own DOI is printed on its
/// first page or two; later pages mostly hold other papers' DOIs.
pub const HEAD_PAGES: usize = 3;
/// The longer side of the page 1 image in pixels. The model API scales larger
/// images down to this anyway.
pub const IMAGE_MAX_SIDE: u32 = 1568;
/// The share of a registered title's words that must be on page 1 for a
/// candidate found in the file to count as this paper.
pub const TITLE_ON_PAGE: f32 = 0.8;
/// How alike two titles must be to name the same paper.
pub const SAME_TITLE: f32 = 0.8;

/// One line of text on a page, in reading order.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    /// The largest font size on the line, in points.
    pub font_size: f32,
}

/// What identification reads from the start of a PDF.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PdfHead {
    /// Info dictionary entries as (key, value).
    pub info: Vec<(String, String)>,
    /// The raw XMP packet, if the file has one.
    pub xmp: Option<String>,
    /// Link annotation URIs as (0 based page, URI).
    pub links: Vec<(usize, String)>,
    /// The text lines of each page read, first page first.
    pub pages: Vec<Vec<Line>>,
}

/// An identifier found in the file, with how strongly the file points at it.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub identifier: Identifier,
    pub score: f32,
}

/// A parsed PDF.
pub struct PdfFile {
    doc: PdfDocument,
}

impl PdfFile {
    /// Parses the file. Fails when it is not a PDF `pdf_oxide` can read.
    pub fn open(bytes: Vec<u8>) -> Result<PdfFile, String> {
        PdfDocument::from_bytes(bytes)
            .map(|doc| PdfFile { doc })
            .map_err(|error| error.to_string())
    }

    /// Reads the metadata, links, and text of the first `pages` pages. Parts
    /// the file does not have, or that cannot be read, are left empty.
    pub fn head(&self, pages: usize) -> PdfHead {
        let doc = &self.doc;
        let page_count = doc.page_count().unwrap_or(0);
        let mut head = PdfHead {
            info: info_dict(doc),
            xmp: pdf_oxide::extractors::xmp::XmpExtractor::extract(doc)
                .ok()
                .flatten()
                .and_then(|xmp| xmp.raw_xml),
            ..PdfHead::default()
        };
        for index in 0..page_count.min(pages) {
            if let Ok(annotations) = doc.get_annotations(index) {
                for annotation in annotations {
                    if let Some(pdf_oxide::LinkAction::Uri(uri)) = annotation.action {
                        head.links.push((index, uri));
                    }
                }
            }
            head.pages.push(page_lines(doc, index));
        }
        head
    }

    /// Page 1 as a PNG whose longer side is at most `max_side` pixels, or
    /// `None` when it cannot be drawn.
    pub fn render_first_page(&self, max_side: u32) -> Option<Vec<u8>> {
        use pdf_oxide::rendering::{RenderOptions, render_page};
        let (x0, y0, x1, y1) = self.doc.get_page_media_box(0).ok()?;
        let longest = (x1 - x0).abs().max((y1 - y0).abs());
        if longest <= 0.0 {
            return None;
        }
        let dpi = ((max_side as f32 * 72.0 / longest).floor() as u32).clamp(36, 300);
        match render_page(&self.doc, 0, &RenderOptions::with_dpi(dpi)) {
            Ok(image) => Some(image.data),
            Err(error) => {
                tracing::info!(%error, "could not draw page 1");
                None
            }
        }
    }
}

impl PdfHead {
    /// Whether any page read has text. Scanned papers have none.
    pub fn has_text(&self) -> bool {
        self.pages.iter().flatten().next().is_some()
    }

    /// The text of page 1, one line per line.
    pub fn first_page_text(&self) -> String {
        self.pages
            .first()
            .map(|lines| join_lines(lines))
            .unwrap_or_default()
    }

    /// DOIs and arXiv IDs in the file, most likely first. The file's own
    /// metadata counts most, then links and text on page 1, then later pages.
    /// Anything after a references heading is left out, because those
    /// identifiers belong to the papers this one cites.
    pub fn candidates(&self) -> Vec<Candidate> {
        let mut found: HashMap<String, Candidate> = HashMap::new();
        let mut add = |identifier: Identifier, score: f32| {
            // An arXiv ID and its DataCite DOI are one candidate; the arXiv
            // form wins because it keeps the version.
            let entry = found.entry(identifier.doi()).or_insert(Candidate {
                identifier: identifier.clone(),
                score: 0.0,
            });
            if matches!(identifier, Identifier::Arxiv(_)) {
                entry.identifier = identifier;
            }
            entry.score += score;
        };

        for (key, value) in &self.info {
            let score = if key.to_lowercase().contains("doi") {
                6.0
            } else {
                4.0
            };
            for doi in dois_in(value) {
                add(Identifier::Doi(doi), score);
            }
        }
        if let Some(xmp) = &self.xmp {
            for value in XMP_IDENTIFIER
                .captures_iter(xmp)
                .filter_map(|c| c.get(2).or(c.get(3)))
            {
                for doi in dois_in(value.as_str()) {
                    add(Identifier::Doi(doi), 6.0);
                }
            }
        }
        for (page, uri) in &self.links {
            if let Some((_, rest)) = uri.split_once("doi.org/")
                && let Some(doi) = clean_doi(rest)
            {
                add(Identifier::Doi(doi), if *page == 0 { 4.0 } else { 1.0 });
            }
        }
        let mut in_references = false;
        for (index, lines) in self.pages.iter().enumerate() {
            if in_references {
                break;
            }
            let mut text = join_lines(lines);
            if let Some(heading) = REFERENCES_HEADING.find(&text) {
                text.truncate(heading.start());
                in_references = true;
            }
            let score = [3.0, 1.5, 0.75].get(index).copied().unwrap_or(0.5);
            for doi in dois_in(&text) {
                add(Identifier::Doi(doi), score);
            }
            // arXiv prints its stamp in the margin of page 1 only.
            if index == 0 {
                for id in arxiv_ids_in(&text) {
                    add(Identifier::Arxiv(id), 3.5);
                }
            }
        }

        let mut list: Vec<Candidate> = found.into_values().collect();
        list.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.identifier.doi().cmp(&b.identifier.doi()))
        });
        list
    }

    /// The title as the layout shows it: the first run of lines in the
    /// largest font near the top of page 1. Lines of one word (section labels
    /// such as "ARTICLE") and the arXiv stamp are skipped.
    pub fn title_guess(&self) -> Option<String> {
        let lines: Vec<&Line> = self
            .pages
            .first()?
            .iter()
            .take(40)
            .filter(|line| {
                line.text.split_whitespace().count() >= 2 && arxiv_ids_in(&line.text).is_empty()
            })
            .collect();
        let largest = lines.iter().map(|line| line.font_size).fold(0.0, f32::max);
        if largest <= 0.0 {
            return None;
        }
        let same_size = |line: &&&Line| (line.font_size - largest).abs() < 0.5;
        let title = lines
            .iter()
            .skip_while(|line| !same_size(line))
            .take_while(same_size)
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        (words(&title).len() >= 3 && title.len() <= 400).then_some(title)
    }
}

/// The share of `title`'s words that appear anywhere in `text`.
pub fn title_coverage(title: &str, text: &str) -> f32 {
    let title = words(title);
    if title.is_empty() {
        return 0.0;
    }
    let text: std::collections::HashSet<String> = words(text).into_iter().collect();
    title.iter().filter(|word| text.contains(*word)).count() as f32 / title.len() as f32
}

/// How alike two titles are, from 0 to 1: the smaller share of either title's
/// words found in the other, so a short title inside a long one is no match.
pub fn title_similarity(a: &str, b: &str) -> f32 {
    title_coverage(a, b).min(title_coverage(b, a))
}

/// Lowercase words of two or more letters or digits. Inline markup that
/// Crossref puts in titles is dropped first, and the text around it joined,
/// so `Fe<sub>3</sub>GeTe<sub>2</sub>` reads as `fe3gete2`.
pub fn words(text: &str) -> Vec<String> {
    MARKUP
        .replace_all(text, "")
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.chars().count() > 1)
        .map(str::to_string)
        .collect()
}

static DOI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\b10\.\d{4,9}/[^\s"<>]+"#).unwrap());
/// A DOI broken at the end of a line: after a slash, hyphen, or underscore,
/// or after a period when the next line goes on in lowercase or digits (a
/// new sentence starts with a capital).
static BROKEN_DOI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([/_-])\n(\S)|(\.)\n([a-z0-9])").unwrap());
static ARXIV: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\barxiv\s*:?\s*(\d{4}\.\d{4,5}(?:v\d+)?|[a-z][a-z.-]+/\d{7}(?:v\d+)?)\b")
        .unwrap()
});
static REFERENCES_HEADING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?im)^\s*(?:\d+\.?\s*)?(?:references|bibliography|literature cited|works cited)\s*$",
    )
    .unwrap()
});
/// The XMP fields that hold the document's own identifier, as elements or
/// attributes.
static XMP_IDENTIFIER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)<(prism:doi|dc:identifier|pdfx:doi|crossmark:DOI|prism:url)\b[^>]*>(.*?)</(?:prism:doi|dc:identifier|pdfx:doi|crossmark:DOI|prism:url)>|(?:prism:doi|pdfx:doi|crossmark:DOI)="([^"]*)""#)
        .unwrap()
});
static MARKUP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\s*</?(?:sub|sup|i|b|em|strong|scp|mml:[a-z]+)\b[^>]*>\s*").unwrap()
});

/// DOIs in text, with broken ones joined and also as written. A wrong join
/// is rejected later by the engine's Crossref check.
fn dois_in(text: &str) -> Vec<String> {
    let joined = BROKEN_DOI.replace_all(text, "$1$2$3$4");
    let mut dois: Vec<String> = Vec::new();
    for source in [joined.as_ref(), text] {
        for found in DOI.find_iter(source) {
            if let Some(doi) = clean_doi(found.as_str())
                && !dois.contains(&doi)
            {
                dois.push(doi);
            }
        }
    }
    dois
}

/// Trims what a match in prose drags along: trailing punctuation and closing
/// brackets that were never opened.
fn clean_doi(raw: &str) -> Option<String> {
    let mut doi = raw.to_string();
    loop {
        let before = doi.len();
        doi = doi
            .trim_end_matches(['.', ',', ';', ':', '\'', '’', '”'])
            .to_string();
        for (open, close) in [('(', ')'), ('[', ']'), ('{', '}')] {
            if doi.ends_with(close) && doi.matches(open).count() < doi.matches(close).count() {
                doi.pop();
            }
        }
        if doi.len() == before {
            break;
        }
    }
    paper_ids::normalize_doi(&doi)
}

fn arxiv_ids_in(text: &str) -> Vec<String> {
    ARXIV
        .captures_iter(text)
        .filter_map(|c| paper_ids::normalize_arxiv_id(&c[1].to_lowercase()))
        .collect()
}

fn join_lines(lines: &[Line]) -> String {
    lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The page's text spans grouped into lines by their vertical center, in
/// reading order.
fn page_lines(doc: &PdfDocument, index: usize) -> Vec<Line> {
    let mut spans = doc.extract_spans(index).unwrap_or_default();
    spans.sort_by_key(|span| span.sequence);
    struct Building {
        center: f32,
        font_size: f32,
        /// (x, text) of each span on the line.
        parts: Vec<(f32, String)>,
    }
    let mut lines: Vec<Building> = Vec::new();
    for span in spans {
        if span.text.trim().is_empty() {
            continue;
        }
        let center = span.bbox.y + span.bbox.height / 2.0;
        let tolerance = (span.bbox.height / 2.0).max(1.5);
        let part = (span.bbox.x, span.text.trim_end_matches('\n').to_string());
        // Superscripts and footnote marks join one of the last few lines.
        match lines
            .iter_mut()
            .rev()
            .take(3)
            .find(|line| (line.center - center).abs() <= tolerance)
        {
            Some(line) => {
                line.font_size = line.font_size.max(span.font_size);
                line.parts.push(part);
            }
            None => lines.push(Building {
                center,
                font_size: span.font_size,
                parts: vec![part],
            }),
        }
    }
    lines
        .into_iter()
        .map(|mut line| {
            line.parts.sort_by(|a, b| a.0.total_cmp(&b.0));
            let text = line
                .parts
                .iter()
                .flat_map(|(_, text)| text.split_whitespace())
                .collect::<Vec<_>>()
                .join(" ");
            Line {
                text,
                font_size: line.font_size,
            }
        })
        .collect()
}

fn info_dict(doc: &PdfDocument) -> Vec<(String, String)> {
    let resolve = |object: &Object| match object.as_reference() {
        Some(reference) => doc.load_object(reference).ok(),
        None => Some(object.clone()),
    };
    let Some(info) = doc
        .trailer()
        .as_dict()
        .and_then(|trailer| trailer.get("Info"))
        .and_then(resolve)
    else {
        return Vec::new();
    };
    let Some(dict) = info.as_dict() else {
        return Vec::new();
    };
    dict.iter()
        .filter_map(|(key, value)| {
            let value = resolve(value)?;
            Some((key.clone(), decode_pdf_string(value.as_string()?)))
        })
        .collect()
}

/// PDF text strings are UTF-16BE with a byte order mark, or PDFDocEncoding,
/// which matches Latin-1 for every character an identifier uses.
fn decode_pdf_string(bytes: &[u8]) -> String {
    match bytes.strip_prefix(&[0xFE, 0xFF]) {
        Some(utf16) => {
            let units: Vec<u16> = utf16
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_be_bytes(*pair))
                .collect();
            String::from_utf16_lossy(&units)
        }
        None => bytes.iter().map(|&byte| byte as char).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, font_size: f32) -> Line {
        Line {
            text: text.to_string(),
            font_size,
        }
    }

    fn page(text: &str) -> Vec<Line> {
        text.lines().map(|text| line(text, 10.0)).collect()
    }

    fn dois(head: &PdfHead) -> Vec<String> {
        head.candidates()
            .into_iter()
            .map(|c| c.identifier.to_string())
            .collect()
    }

    #[test]
    fn the_papers_own_doi_beats_the_ones_it_cites() {
        let head = PdfHead {
            pages: vec![
                page(
                    "Received 3 May 2015\n\
                     Cite this: Nature 521, 436 (2015). https://doi.org/10.1038/\n\
                     nature14539.\n\
                     As shown before in 10.1016/j.\n\
                     cell.2020.01.001, and (doi:10.1000/earlier.work), deep nets learn.",
                ),
                page(
                    "Background (10.1000/earlier.work)\nReferences\n1. Other, doi 10.1000/cited.2",
                ),
                page("2. Third, https://doi.org/10.1000/cited.3"),
            ],
            ..PdfHead::default()
        };
        let found = dois(&head);
        assert_eq!(
            found[0], "10.1000/earlier.work",
            "printed twice beats printed once"
        );
        for joined in ["10.1038/nature14539", "10.1016/j.cell.2020.01.001"] {
            assert!(
                found.contains(&joined.to_string()),
                "{joined} is joined across the line break: {found:?}"
            );
        }
        assert!(
            !found.iter().any(|doi| doi.ends_with(".as")),
            "a period at the end of a sentence does not glue on the next one: {found:?}"
        );
        assert!(
            !found.iter().any(|doi| doi.contains("cited")),
            "nothing after the references heading, including later pages: {found:?}"
        );
        assert!(
            found.iter().all(|doi| !doi.ends_with(['.', ')'])),
            "trailing punctuation is trimmed: {found:?}"
        );
    }

    #[test]
    fn metadata_outranks_text_and_arxiv_stamps_are_found() {
        let head = PdfHead {
            info: vec![("Subject".into(), "Phys. Rev. A 99, doi:10.1103/PhysRevA.99.063412".into())],
            xmp: Some(r#"<rdf:Description prism:doi="10.1103/physreva.99.063412"/><prism:doi>10.1103/PhysRevA.99.063412</prism:doi>"#.into()),
            pages: vec![page("arXiv:1903.03248v2 [quant-ph] 12 Jun 2019\nSee also 10.1000/x.1 and 10.1000/x.1")],
            ..PdfHead::default()
        };
        let candidates = head.candidates();
        assert_eq!(
            candidates[0].identifier,
            Identifier::Doi("10.1103/physreva.99.063412".into())
        );
        assert!(
            candidates
                .iter()
                .any(|c| c.identifier == Identifier::Arxiv("1903.03248v2".into())),
            "{candidates:?}"
        );
    }

    #[test]
    fn the_title_is_the_first_run_of_the_largest_lines() {
        let head = PdfHead {
            pages: vec![vec![
                line("arXiv:2306.16514v1 [math.NA] 28 Jun 2023", 20.0),
                line("RESEARCH", 22.0),
                line("Journal of Things 12 (2023)", 9.0),
                line("Attention Is All", 17.2),
                line("You Need", 17.0),
                line("Ashish Vaswani, Noam Shazeer", 11.0),
                line("Table 1 Results Of Something Long", 17.0),
            ]],
            ..PdfHead::default()
        };
        assert_eq!(
            head.title_guess().as_deref(),
            Some("Attention Is All You Need")
        );
    }

    #[test]
    fn titles_match_through_markup_and_case_but_not_inside_longer_titles() {
        let crossref = "Observation of Magnetic Skyrmion Bubbles in a van der Waals Ferromagnet Fe\n  <sub>3</sub>\n  GeTe\n  <sub>2</sub>";
        let printed =
            "Observation of magnetic skyrmion bubbles in a van der Waals ferromagnet Fe3GeTe2";
        assert_eq!(title_similarity(crossref, printed), 1.0);
        assert!(
            title_similarity("Deep learning", "Deep learning for symbolic mathematics")
                < SAME_TITLE
        );
        assert_eq!(
            title_coverage("Deep learning", "Abstract. Deep learning allows..."),
            1.0
        );
    }

    #[test]
    fn a_written_pdf_reads_back_with_its_metadata_text_and_title() {
        let mut pdf = pdf_oxide::api::PdfBuilder::new()
            .title("Deep learning")
            .subject("doi:10.1038/nature14539")
            .from_markdown(
                "# Deep learning\n\nYann LeCun, Yoshua Bengio, Geoffrey Hinton\n\n\
                 Deep learning allows computational models to learn.\n\n\
                 ## References\n\n1. Other work, doi 10.1000/other.1\n",
            )
            .unwrap();
        let file = PdfFile::open(pdf.to_bytes().unwrap()).unwrap();
        let head = file.head(HEAD_PAGES);

        assert!(
            head.info
                .contains(&("Subject".into(), "doi:10.1038/nature14539".into())),
            "{:?}",
            head.info
        );
        assert!(
            head.first_page_text()
                .contains("Yann LeCun, Yoshua Bengio, Geoffrey Hinton")
        );
        assert_eq!(dois(&head), vec!["10.1038/nature14539"]);
        assert_eq!(
            head.title_guess(),
            None,
            "two words are too few to search by"
        );
        let png = file.render_first_page(IMAGE_MAX_SIDE).unwrap();
        assert!(png.starts_with(b"\x89PNG"));
    }

    #[test]
    fn files_that_are_not_pdfs_do_not_open() {
        assert!(PdfFile::open(b"<html>Access denied</html>".to_vec()).is_err());
    }
}

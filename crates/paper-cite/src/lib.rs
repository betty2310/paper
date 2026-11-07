//! Citation keys, BibTeX entries, and formatted citations for items.
//!
//! Pure functions over `paper-core` types: nothing here touches the database,
//! the network, or the file system. `paper-engine` decides when a key is
//! stored and where an export is written.
//!
//! BibTeX is written by hand rather than through a BibTeX library so the
//! escaping, the case protection of titles, and the field order are ours and
//! stable. Formatted citations go through hayagriva's CSL processor with its
//! bundled styles.

use std::sync::OnceLock;

use hayagriva::citationberg::{FontStyle, FontWeight, IndependentStyle, Locale, Style};
use hayagriva::{
    BibliographyDriver, BibliographyRequest, BufWriteFormat, CitationItem, CitationRequest,
    ElemChild, ElemChildren, Formatted,
};
use paper_core::{Creator, CreatorRole, ItemFields, ItemKind};
use serde_json::{Map, Value, json};

/// What a citation key is made from, and the item to cite.
pub struct CiteItem<'a> {
    pub key: &'a str,
    pub fields: &'a ItemFields,
}

// ----- citation keys -----

/// Words that never make the title part of a key.
const STOP_WORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "by", "for", "from", "how", "in", "into", "is", "of",
    "on", "or", "the", "to", "toward", "towards", "via", "what", "when", "why", "with",
];

/// The key an item would get with no other item in the way: the first
/// author's family name, the year, and the first word of the title that is
/// not a stop word, folded to lowercase ASCII, as in `vaswani2017attention`.
/// A part that is missing is left out; an item with nothing usable is
/// `paper`. The library adds `a`, `b`, ... when the key is taken.
pub fn key_base(fields: &ItemFields) -> String {
    let author = fields
        .creators
        .iter()
        .find(|creator| creator.role != Some(CreatorRole::Editor))
        .or(fields.creators.first())
        .map(key_name)
        .unwrap_or_default();
    let year = fields.year.map(|year| year.to_string()).unwrap_or_default();
    let title = without_tags(&fields.title);
    let words: Vec<String> = ascii_words(&title).collect();
    let word = words
        .iter()
        .find(|word| !STOP_WORDS.contains(&word.as_str()))
        .or(words.first())
        .cloned()
        .unwrap_or_default();
    let key = if author.is_empty() {
        format!("{word}{year}")
    } else {
        format!("{author}{year}{word}")
    };
    if key.is_empty() {
        "paper".to_string()
    } else {
        key
    }
}

/// The family name squeezed to ASCII letters and digits; for an organization,
/// the first word of its name.
fn key_name(creator: &Creator) -> String {
    match (&creator.family, &creator.literal) {
        (Some(family), _) => ascii_words(family).collect(),
        (None, Some(literal)) => ascii_words(literal).next().unwrap_or_default(),
        (None, None) => creator
            .given
            .as_deref()
            .map(|given| ascii_words(given).collect())
            .unwrap_or_default(),
    }
}

/// Lowercase ASCII runs of letters and digits, with accents and other
/// scripts transliterated (`Müller` → `muller`, `Ångström` → `angstrom`).
fn ascii_words(text: &str) -> impl Iterator<Item = String> {
    deunicode::deunicode(text)
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>()
        .into_iter()
}

/// Whether `key` can be used in a `.bib` file and in `\cite{}`: not empty, no
/// whitespace, and none of the characters BibTeX or LaTeX treat specially.
pub fn is_valid_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|c| !c.is_whitespace() && !c.is_control() && !",{}()\"#%'=\\~".contains(c))
}

// ----- BibTeX -----

/// BibTeX entries for the items, separated by blank lines, in the order
/// given. Values are UTF-8 with LaTeX's special characters escaped, and
/// title words with inner capitals (`BERT`, `ImageNet`) are braced so styles
/// that change case leave them alone.
pub fn bibtex(items: &[CiteItem<'_>]) -> String {
    let mut out = String::new();
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        write_entry(&mut out, item);
    }
    out
}

fn write_entry(out: &mut String, item: &CiteItem<'_>) {
    let fields = item.fields;
    let arxiv = fields.arxiv_id.as_deref();
    let entry_type = match fields.kind.unwrap_or(ItemKind::Other) {
        ItemKind::JournalArticle => "article",
        ItemKind::ConferencePaper => "inproceedings",
        ItemKind::Preprint | ItemKind::Other => "misc",
    };
    let mut pairs: Vec<(&str, String)> = Vec::new();
    let names = |role: CreatorRole| -> Option<String> {
        let names: Vec<String> = fields
            .creators
            .iter()
            .filter(|creator| creator.role.unwrap_or(CreatorRole::Author) == role)
            .filter_map(bibtex_name)
            .collect();
        (!names.is_empty()).then(|| names.join(" and "))
    };
    if let Some(authors) = names(CreatorRole::Author) {
        pairs.push(("author", authors));
    }
    pairs.push(("title", protect_case(&escape(&without_tags(&fields.title)))));
    let container = fields
        .container_title
        .as_deref()
        .filter(|text| !text.is_empty());
    match entry_type {
        "article" => {
            if let Some(journal) = container {
                pairs.push(("journal", escape(journal)));
            }
        }
        "inproceedings" => {
            if let Some(booktitle) = container {
                pairs.push(("booktitle", escape(booktitle)));
            }
            if let Some(editors) = names(CreatorRole::Editor) {
                pairs.push(("editor", editors));
            }
        }
        // arXiv's own BibTeX names the archive in `eprint` and nothing else.
        _ if arxiv.is_none() => {
            if let Some(venue) = container {
                pairs.push(("howpublished", escape(venue)));
            }
        }
        _ => {}
    }
    if let Some(year) = fields.year {
        pairs.push(("year", year.to_string()));
    }
    if let Some(month) = month(fields.published_date.as_deref()) {
        pairs.push(("month", month.to_string()));
    }
    for (name, value) in [("volume", &fields.volume), ("number", &fields.issue)] {
        if let Some(value) = value.as_deref().filter(|value| !value.is_empty()) {
            pairs.push((name, escape(value)));
        }
    }
    if let Some(pages) = fields.pages.as_deref().filter(|pages| !pages.is_empty()) {
        pairs.push(("pages", page_range(&escape(pages))));
    }
    let publisher = fields.publisher.as_deref().filter(|text| !text.is_empty());
    if let Some(publisher) = publisher.filter(|_| entry_type != "misc" || arxiv.is_none()) {
        pairs.push(("publisher", escape(publisher)));
    }
    if let Some(doi) = &fields.doi {
        pairs.push(("doi", verbatim(doi)));
    }
    if let Some(arxiv) = arxiv {
        pairs.push(("eprint", verbatim(arxiv)));
        pairs.push(("archivePrefix", "arXiv".to_string()));
    }
    let url = fields
        .url
        .clone()
        .filter(|url| !url.is_empty())
        .or_else(|| arxiv.map(|id| format!("https://arxiv.org/abs/{id}")));
    if let Some(url) = url {
        pairs.push(("url", verbatim(&url)));
    }

    out.push_str(&format!("@{entry_type}{{{},\n", item.key));
    for (index, (name, value)) in pairs.iter().enumerate() {
        let comma = if index + 1 < pairs.len() { "," } else { "" };
        // Months are BibTeX's own macros, written bare.
        if *name == "month" {
            out.push_str(&format!("  {name} = {value}{comma}\n"));
        } else {
            out.push_str(&format!("  {name} = {{{value}}}{comma}\n"));
        }
    }
    out.push_str("}\n");
}

/// `Family, Given` with LaTeX escaped, or an organization braced whole so
/// BibTeX does not split it into names.
fn bibtex_name(creator: &Creator) -> Option<String> {
    match (&creator.family, &creator.given, &creator.literal) {
        (Some(family), Some(given), _) => Some(format!("{}, {}", escape(family), escape(given))),
        (Some(family), None, _) => Some(escape(family)),
        (None, _, Some(literal)) => Some(format!("{{{}}}", escape(literal))),
        (None, Some(given), None) => Some(escape(given)),
        (None, None, None) => None,
    }
}

/// Escapes the characters LaTeX treats specially. Braces become commands
/// because BibTeX counts every brace in a value, escaped or not.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' | '%' | '$' | '#' | '_' => {
                out.push('\\');
                out.push(c);
            }
            '{' => out.push_str("\\textbraceleft{}"),
            '}' => out.push_str("\\textbraceright{}"),
            '~' => out.push_str("\\textasciitilde{}"),
            '^' => out.push_str("\\textasciicircum{}"),
            '\\' => out.push_str("\\textbackslash{}"),
            '\n' | '\r' | '\t' => out.push(' '),
            _ => out.push(c),
        }
    }
    out
}

/// DOIs and URLs are printed through `\url` or `\doi`, which take their
/// characters literally; only braces would unbalance the entry.
fn verbatim(text: &str) -> String {
    text.replace('{', "%7B").replace('}', "%7D")
}

/// Braces each word with a capital after its first letter, such as `BERT`,
/// `ImageNet`, or `GPT-3`, so a style that lowercases titles keeps it.
fn protect_case(title: &str) -> String {
    title
        .split(' ')
        .map(|word| {
            let inner_capital = word.chars().skip(1).any(char::is_uppercase);
            if inner_capital && !word.contains('\\') {
                format!("{{{word}}}")
            } else {
                word.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `100-110` as BibTeX's `100--110`.
fn page_range(pages: &str) -> String {
    let pages = pages.replace(['–', '—'], "-");
    if pages.contains("--") {
        return pages;
    }
    pages.replace('-', "--")
}

/// The BibTeX month macro for a `YYYY-MM` or `YYYY-MM-DD` date.
fn month(date: Option<&str>) -> Option<&'static str> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let month: usize = date?.split('-').nth(1)?.parse().ok()?;
    MONTHS.get(month.checked_sub(1)?).copied()
}

/// Titles from some sources keep inline markup such as `<i>`; it has no
/// place in a citation.
fn without_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

// ----- formatted citations -----

/// A citation style the engine can format. Closed set; more CSL styles come
/// with a style picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CitationStyle {
    /// APA 7th edition.
    Apa,
}

/// A formatted reference list, as plain text and as HTML with the style's
/// italics, so a paste into a word processor keeps them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Citation {
    /// One reference per line.
    pub text: String,
    /// One `<p>` per reference.
    pub html: String,
}

/// The items' entries in the style's reference list, in the style's order.
pub fn citation(items: &[CiteItem<'_>], style: CitationStyle) -> Citation {
    let library: hayagriva::Library = items.iter().filter_map(hayagriva_entry).collect();
    let entries: Vec<&hayagriva::Entry> = library.iter().collect();
    let style = match style {
        CitationStyle::Apa => apa(),
    };
    let locales = locales();
    let mut driver = BibliographyDriver::new();
    driver.citation(CitationRequest::from_items(
        entries
            .iter()
            .map(|entry| CitationItem::with_entry(*entry))
            .collect(),
        style,
        locales,
    ));
    let rendered = driver.finish(BibliographyRequest::new(style, None, locales));
    let mut text = Vec::new();
    let mut html = String::new();
    for item in rendered.bibliography.map(|b| b.items).unwrap_or_default() {
        let mut plain = String::new();
        let _ = item.content.write_buf(&mut plain, BufWriteFormat::Plain);
        text.push(plain.trim().to_string());
        let mut marked = String::new();
        write_html(&mut marked, &item.content);
        html.push_str(&format!("<p>{}</p>", marked.trim()));
    }
    Citation {
        text: text.join("\n"),
        html,
    }
}

/// hayagriva's own HTML leaves `&` and `<` in the text unescaped, so the
/// tree is walked here: italics and bold as tags, links as links.
fn write_html(out: &mut String, children: &ElemChildren) {
    for child in &children.0 {
        match child {
            ElemChild::Text(text) => write_formatted(out, text),
            ElemChild::Elem(elem) => write_html(out, &elem.children),
            ElemChild::Markup(markup) => out.push_str(&escape_html(markup)),
            ElemChild::Link { text, url } => {
                out.push_str(&format!("<a href=\"{}\">", escape_html(url)));
                write_formatted(out, text);
                out.push_str("</a>");
            }
            ElemChild::Transparent { .. } => {}
        }
    }
}

fn write_formatted(out: &mut String, text: &Formatted) {
    let italic = text.formatting.font_style == FontStyle::Italic;
    let bold = text.formatting.font_weight == FontWeight::Bold;
    if italic {
        out.push_str("<i>");
    }
    if bold {
        out.push_str("<b>");
    }
    out.push_str(&escape_html(&text.text));
    if bold {
        out.push_str("</b>");
    }
    if italic {
        out.push_str("</i>");
    }
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn apa() -> &'static IndependentStyle {
    static STYLE: OnceLock<IndependentStyle> = OnceLock::new();
    STYLE.get_or_init(
        || match hayagriva::archive::ArchivedStyle::AmericanPsychologicalAssociation.get() {
            Style::Independent(style) => style,
            Style::Dependent(_) => unreachable!("APA is an independent style"),
        },
    )
}

/// English terms only: every bundled style falls back to `en-US`.
fn locales() -> &'static [Locale] {
    static LOCALES: OnceLock<Vec<Locale>> = OnceLock::new();
    LOCALES.get_or_init(|| {
        hayagriva::archive::locales()
            .into_iter()
            .filter(|locale| {
                locale
                    .lang
                    .as_ref()
                    .is_some_and(|lang| lang.to_string().starts_with("en"))
            })
            .collect()
    })
}

/// The item as a hayagriva entry, built as its YAML format (written as JSON,
/// which YAML reads) so the field names are hayagriva's documented ones.
fn hayagriva_entry(item: &CiteItem<'_>) -> Option<hayagriva::Entry> {
    let fields = item.fields;
    let mut entry = Map::new();
    let mut parent = Map::new();
    let container = fields
        .container_title
        .as_deref()
        .filter(|text| !text.is_empty());
    match fields.kind.unwrap_or(ItemKind::Other) {
        ItemKind::JournalArticle => {
            entry.insert("type".into(), json!("article"));
            parent.insert("type".into(), json!("periodical"));
        }
        ItemKind::ConferencePaper => {
            entry.insert("type".into(), json!("article"));
            parent.insert("type".into(), json!("proceedings"));
        }
        ItemKind::Preprint => {
            entry.insert("type".into(), json!("article"));
            parent.insert("type".into(), json!("repository"));
        }
        // hayagriva's `misc` prints APA dates as "(2015,)"; a plain article
        // with no journal reads as a standalone work.
        ItemKind::Other => {
            entry.insert("type".into(), json!("article"));
            parent.insert("type".into(), json!("misc"));
        }
    }
    entry.insert("title".into(), json!(without_tags(&fields.title)));
    let people = |role: CreatorRole| -> Vec<Value> {
        fields
            .creators
            .iter()
            .filter(|creator| creator.role.unwrap_or(CreatorRole::Author) == role)
            .filter_map(
                |creator| match (&creator.family, &creator.given, &creator.literal) {
                    (Some(family), Some(given), _) => {
                        Some(json!({ "name": family, "given-name": given }))
                    }
                    (Some(name), None, _) | (None, _, Some(name)) => Some(json!({ "name": name })),
                    (None, Some(given), None) => Some(json!({ "name": given })),
                    (None, None, None) => None,
                },
            )
            .collect()
    };
    let authors = people(CreatorRole::Author);
    if !authors.is_empty() {
        entry.insert("author".into(), Value::Array(authors));
    }
    let editors = people(CreatorRole::Editor);
    if !editors.is_empty() {
        parent.insert("editor".into(), Value::Array(editors));
    }
    let date = fields
        .published_date
        .clone()
        .filter(|date| {
            fields
                .year
                .is_none_or(|year| date.starts_with(&year.to_string()))
        })
        .or_else(|| fields.year.map(|year| year.to_string()));
    if let Some(date) = date {
        entry.insert("date".into(), json!(date));
    }
    let mut serials = Map::new();
    if let Some(doi) = &fields.doi {
        serials.insert("doi".into(), json!(doi));
    }
    if let Some(arxiv) = &fields.arxiv_id {
        serials.insert("arxiv".into(), json!(arxiv));
    }
    if !serials.is_empty() {
        entry.insert("serial-number".into(), Value::Object(serials));
    }
    // APA prints the DOI as a link and a URL only when there is no DOI.
    let url = fields
        .url
        .clone()
        .filter(|url| !url.is_empty())
        .or_else(|| {
            fields
                .arxiv_id
                .as_ref()
                .map(|id| format!("https://arxiv.org/abs/{id}"))
        });
    if let (None, Some(url)) = (&fields.doi, url) {
        entry.insert("url".into(), json!(url));
    }
    if let Some(pages) = fields.pages.as_deref().filter(|pages| !pages.is_empty()) {
        entry.insert("page-range".into(), json!(pages));
    }
    // Volume and issue belong to the journal in hayagriva's model.
    for (name, value) in [("volume", &fields.volume), ("issue", &fields.issue)] {
        if let Some(value) = value.as_deref().filter(|value| !value.is_empty()) {
            parent.insert(name.into(), json!(value));
        }
    }
    match container {
        // Braced so the style keeps the journal's own case (`eLife`, not `Elife`).
        Some(title) => {
            parent.insert(
                "title".into(),
                json!(format!("{{{}}}", title.replace(['{', '}'], ""))),
            );
        }
        // A preprint server is named as the publisher of a preprint.
        None if fields.kind == Some(ItemKind::Preprint) && fields.arxiv_id.is_some() => {
            parent.insert("title".into(), json!("arXiv"));
        }
        None => {}
    }
    if let Some(publisher) = fields.publisher.as_deref().filter(|text| !text.is_empty()) {
        if fields.kind == Some(ItemKind::Other) || container.is_none() {
            entry.insert("publisher".into(), json!(publisher));
        } else {
            parent.insert("publisher".into(), json!(publisher));
        }
    }
    if parent.len() > 1 {
        entry.insert("parent".into(), Value::Object(parent));
    }
    let mut library = Map::new();
    library.insert(item.key.to_string(), Value::Object(entry));
    let yaml = Value::Object(library).to_string();
    match hayagriva::io::from_yaml_str(&yaml) {
        Ok(library) => library.into_iter().next(),
        Err(error) => {
            tracing::warn!(%error, key = item.key, "could not build a citation entry");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paper_core::Creator;

    fn org(name: &str) -> Creator {
        Creator {
            role: Some(CreatorRole::Author),
            literal: Some(name.to_string()),
            ..Creator::default()
        }
    }

    fn deep_learning() -> ItemFields {
        ItemFields {
            kind: Some(ItemKind::JournalArticle),
            doi: Some("10.1038/nature14539".into()),
            title: "Deep learning".into(),
            creators: vec![
                Creator::person("Yann", "LeCun"),
                Creator::person("Yoshua", "Bengio"),
                Creator::person("Geoffrey", "Hinton"),
            ],
            container_title: Some("Nature".into()),
            year: Some(2015),
            published_date: Some("2015-05-27".into()),
            volume: Some("521".into()),
            issue: Some("7553".into()),
            pages: Some("436-444".into()),
            ..ItemFields::default()
        }
    }

    #[test]
    fn keys_fold_names_skip_stop_words_and_leave_out_what_is_missing() {
        let mut fields = ItemFields {
            title: "The Ångström effect on <i>E. coli</i>".into(),
            creators: vec![
                Creator {
                    role: Some(CreatorRole::Editor),
                    ..Creator::person("Eve", "Editor")
                },
                Creator::person("Ana", "Müller-Lüdenscheidt"),
            ],
            year: Some(2020),
            ..ItemFields::default()
        };
        assert_eq!(key_base(&fields), "mullerludenscheidt2020angstrom");

        fields.year = None;
        fields.creators = vec![org("World Health Organization")];
        assert_eq!(key_base(&fields), "worldangstrom");

        fields.creators.clear();
        fields.year = Some(1999);
        assert_eq!(key_base(&fields), "angstrom1999");

        // A title of stop words alone still names the paper.
        fields.title = "On the".into();
        assert_eq!(key_base(&fields), "on1999");

        assert_eq!(key_base(&ItemFields::default()), "paper");
    }

    #[test]
    fn user_keys_reject_what_breaks_bibtex() {
        for good in ["lecun2015", "LeCun:2015.deep", "a-b_c/d"] {
            assert!(is_valid_key(good), "{good}");
        }
        for bad in [
            "",
            "two words",
            "a,b",
            "a{b",
            "x%",
            "q\"",
            "tab\tkey",
            "a=b",
            "a\\b",
        ] {
            assert!(!is_valid_key(bad), "{bad:?}");
        }
    }

    #[test]
    fn bibtex_escapes_latex_and_protects_inner_capitals() {
        let fields = ItemFields {
            kind: Some(ItemKind::ConferencePaper),
            title: "BERT beats 50% of R&D_costs {really} in mRNA ~ ^ \\ tasks".into(),
            creators: vec![Creator::person("Jacob", "Devlin"), org("Google AI & Co")],
            container_title: Some("Proceedings of NAACL".into()),
            pages: Some("4171–4186".into()),
            published_date: Some("2019-06".into()),
            year: Some(2019),
            doi: Some("10.18653/v1/n19-1423".into()),
            ..ItemFields::default()
        };
        let bib = bibtex(&[CiteItem {
            key: "devlin2019bert",
            fields: &fields,
        }]);
        assert_eq!(
            bib,
            "@inproceedings{devlin2019bert,\n\
             \x20 author = {Devlin, Jacob and {Google AI \\& Co}},\n\
             \x20 title = {{BERT} beats 50\\% of R\\&D\\_costs \\textbraceleft{}really\\textbraceright{} in {mRNA} \\textasciitilde{} \\textasciicircum{} \\textbackslash{} tasks},\n\
             \x20 booktitle = {Proceedings of NAACL},\n\
             \x20 year = {2019},\n\
             \x20 month = jun,\n\
             \x20 pages = {4171--4186},\n\
             \x20 doi = {10.18653/v1/n19-1423}\n\
             }\n"
        );
        // BibTeX counts every brace, escaped or not.
        assert_eq!(bib.matches('{').count(), bib.matches('}').count());
    }

    #[test]
    fn arxiv_preprints_cite_like_arxiv_and_entries_are_separated() {
        let preprint = ItemFields {
            kind: Some(ItemKind::Preprint),
            title: "Attention Is All You Need".into(),
            creators: vec![Creator::person("Ashish", "Vaswani")],
            container_title: Some("arXiv".into()),
            publisher: Some("arXiv".into()),
            arxiv_id: Some("1706.03762".into()),
            year: Some(2017),
            ..ItemFields::default()
        };
        let journal = deep_learning();
        let bib = bibtex(&[
            CiteItem {
                key: "vaswani2017attention",
                fields: &preprint,
            },
            CiteItem {
                key: "lecun2015deep",
                fields: &journal,
            },
        ]);
        let (first, second) = bib
            .split_once("}\n\n@")
            .expect("a blank line between entries");
        assert!(first.starts_with("@misc{vaswani2017attention,\n"));
        assert!(first.contains("  eprint = {1706.03762},\n  archivePrefix = {arXiv},\n"));
        assert!(first.contains("  url = {https://arxiv.org/abs/1706.03762}"));
        assert!(!first.contains("howpublished") && !first.contains("publisher"));
        assert!(second.starts_with("article{lecun2015deep,\n"));
        assert!(second.contains("  journal = {Nature},\n"));
        assert!(second.contains("  number = {7553},\n  pages = {436--444},\n"));
    }

    #[test]
    fn apa_lists_references_in_author_order_with_italics_in_html() {
        let journal = deep_learning();
        let other = ItemFields {
            kind: Some(ItemKind::Other),
            title: "Notes on R&D <groups>".into(),
            creators: vec![Creator::person("Ana", "Abbott")],
            ..ItemFields::default()
        };
        let elife = ItemFields {
            kind: Some(ItemKind::JournalArticle),
            title: "Dopaminergic neurons".into(),
            creators: vec![Creator::person("Yoshinori", "Aso")],
            container_title: Some("eLife".into()),
            year: Some(2016),
            volume: Some("5".into()),
            ..ItemFields::default()
        };
        let dated_other = ItemFields {
            year: Some(2015),
            creators: vec![Creator::person("Bea", "Baker")],
            ..other.clone()
        };
        let extra = citation(
            &[
                CiteItem {
                    key: "aso2016",
                    fields: &elife,
                },
                CiteItem {
                    key: "baker2015",
                    fields: &dated_other,
                },
            ],
            CitationStyle::Apa,
        );
        assert_eq!(
            extra.text,
            "Aso, Y. (2016). Dopaminergic neurons. eLife, 5.\nBaker, B. (2015). Notes on R&D."
        );
        let citation = citation(
            &[
                CiteItem {
                    key: "lecun2015deep",
                    fields: &journal,
                },
                CiteItem {
                    key: "abbottnotes",
                    fields: &other,
                },
            ],
            CitationStyle::Apa,
        );
        let lines: Vec<&str> = citation.text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(
            lines[0].starts_with("Abbott, A. (n.d.). Notes on R&D"),
            "{}",
            lines[0]
        );
        assert_eq!(
            lines[1],
            "LeCun, Y., Bengio, Y., & Hinton, G. (2015). Deep learning. Nature, 521(7553), 436–444. https://doi.org/10.1038/nature14539"
        );
        assert!(
            citation
                .html
                .starts_with("<p>Abbott, A. (n.d.). <i>Notes on R&amp;D</i>")
        );
        assert!(citation.html.contains(
            "LeCun, Y., Bengio, Y., &amp; Hinton, G. (2015). Deep learning. <i>Nature</i>, <i>521</i>(7553), 436–444. <a href=\"https://doi.org/10.1038/nature14539\">"
        ));
        assert_eq!(citation.html.matches("<p>").count(), 2);
    }
}

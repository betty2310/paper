//! The reference list of a PDF attachment, each entry given the work it
//! cites. The order of the sources, cheapest and surest first, is from
//! `docs/research/references.md`:
//!
//! 1. the reference list the publisher deposited with Crossref, which the
//!    lookup already stored (no request);
//! 2. a DOI, arXiv ID, or web address the entry prints or links to;
//! 3. the works OpenAlex lists as cited by the paper, read one free record
//!    at a time and matched to entries by title;
//! 4. a Crossref search of one entry, only when the user asks for it.
//!
//! The list itself is read from the file each time; only the answers from
//! the network are kept, in `source_records`, so the reader never asks twice.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use paper_core::{
    CitationLink, Library, PageRect, PaperError, Reference, ReferencePart, ReferenceSource,
    References,
};
use paper_fetch::{CROSSREF_SEARCH, CitedWork, Contact, OPENALEX_REFERENCES, Resolver};
use paper_identify::{ReferenceList, title_coverage, words};
use serde_json::{Map, Value, json};

type Result<T> = std::result::Result<T, PaperError>;

/// The source name, in `source_records`, of the Crossref searches for
/// single entries: an object from entry text to the search's answer.
pub(crate) const REFERENCE_SEARCHES: &str = "reference_searches";
/// A search hit counts when this share of its title's words is in the entry.
const MIN_TITLE_COVERAGE: f32 = 0.8;
/// Titles shorter than this many words are too common to match on.
const MIN_TITLE_WORDS: usize = 3;
/// Crossref searches the start of a long entry; the rest is noise.
const MAX_QUERY_CHARS: usize = 400;

/// The part of the work that runs on an engine thread.
#[derive(Clone)]
pub(crate) struct ReferenceJob {
    pub library: Arc<Library>,
    pub resolver: Arc<Resolver>,
    pub contact: Contact,
    pub openalex_api_key: Option<String>,
}

impl ReferenceJob {
    /// The attachment's references, with what is known so far and no
    /// request. `None` when the file has no references section.
    pub fn read(&self, attachment_id: &str) -> Result<Option<References>> {
        let row = self.library.attachment(attachment_id)?;
        let bytes = std::fs::read(self.library.attachment_path(&row))?;
        let Ok(file) = paper_identify::PdfFile::open(bytes) else {
            return Ok(None);
        };
        let Some(list) = file.references() else {
            return Ok(None);
        };
        let own_doi = self.library.item(&row.item_id)?.fields.doi;
        let records: HashMap<String, Value> = self
            .library
            .source_records(&row.item_id)?
            .into_iter()
            .filter_map(|record| Some((record.source, serde_json::from_str(&record.json).ok()?)))
            .collect();
        let known = Known::from_records(&records);
        let mut references = resolve(&list, own_doi.as_deref(), &known);
        for reference in &mut references.references {
            reference.item_id = self.library_item(reference, &row.item_id)?;
        }
        Ok(Some(references))
    }

    /// Reads the works OpenAlex lists as cited by the attachment's paper,
    /// once per paper, and keeps their records. One request per cited work.
    pub fn fetch_cited_works(&self, attachment_id: &str) -> Result<()> {
        let item_id = self.library.attachment(attachment_id)?.item_id;
        if self
            .library
            .has_source_record(&item_id, OPENALEX_REFERENCES)?
        {
            return Ok(());
        }
        let Some(doi) = self.library.item(&item_id)?.fields.doi else {
            return Ok(());
        };
        let key = self.openalex_api_key.as_deref();
        let stored = self
            .library
            .source_records(&item_id)?
            .into_iter()
            .find(|record| record.source == paper_fetch::OPENALEX)
            .and_then(|record| serde_json::from_str::<Value>(&record.json).ok());
        let work = match stored {
            Some(work) => work,
            None => match self
                .resolver
                .openalex_work(&doi, key, &self.contact)
                .map_err(network)?
            {
                Some(work) => {
                    self.library.put_source_record(
                        &item_id,
                        &work.record.source,
                        &work.record.json,
                    )?;
                    serde_json::from_str(&work.record.json).unwrap_or_default()
                }
                None => Value::Null,
            },
        };
        let mut cited = Vec::new();
        for id in paper_fetch::referenced_work_ids(&work) {
            if let Some(record) = self
                .resolver
                .openalex_cited_work(&id, key, &self.contact)
                .map_err(network)?
            {
                cited.push(record);
            }
        }
        // Kept even when empty, so the paper is not asked about again.
        self.library.put_source_record(
            &item_id,
            OPENALEX_REFERENCES,
            &Value::Array(cited).to_string(),
        )?;
        Ok(())
    }

    /// Searches Crossref for one entry that has no identifier yet and keeps
    /// the answer, found or not. Returns the entry as it is now.
    pub fn search(&self, attachment_id: &str, index: u32) -> Result<Reference> {
        let not_found = || PaperError::NotFound {
            id: format!("{attachment_id} reference {index}"),
        };
        let references = self.read(attachment_id)?.ok_or_else(not_found)?;
        let reference = references
            .references
            .into_iter()
            .nth(index as usize)
            .ok_or_else(not_found)?;
        if reference.found_by != ReferenceSource::Unresolved || reference.text.trim().is_empty() {
            return Ok(reference);
        }
        let query: String = reference.text.chars().take(MAX_QUERY_CHARS).collect();
        let search = self
            .resolver
            .search_title(&query, None, &self.contact)
            .map_err(network)?;
        let hit = search.matches.iter().find(|hit| {
            words(&hit.title).len() >= MIN_TITLE_WORDS
                && title_coverage(
                    &fold_ligatures(&hit.title),
                    &fold_ligatures(&reference.text),
                ) >= MIN_TITLE_COVERAGE
        });
        let answer = match hit {
            Some(hit) => json!({"doi": hit.doi, "title": hit.title}),
            None => Value::Null,
        };
        let item_id = self.library.attachment(attachment_id)?.item_id;
        let mut searches = self
            .library
            .source_records(&item_id)?
            .into_iter()
            .find(|record| record.source == REFERENCE_SEARCHES)
            .and_then(|record| serde_json::from_str::<Map<String, Value>>(&record.json).ok())
            .unwrap_or_default();
        let raw: Value = serde_json::from_str(&search.record.json).unwrap_or_default();
        searches.insert(
            reference.text.clone(),
            json!({"answer": answer, CROSSREF_SEARCH: raw}),
        );
        self.library.put_source_record(
            &item_id,
            REFERENCE_SEARCHES,
            &Value::Object(searches).to_string(),
        )?;
        let references = self.read(attachment_id)?.ok_or_else(not_found)?;
        references
            .references
            .into_iter()
            .nth(index as usize)
            .ok_or_else(not_found)
    }

    /// The live item, other than the paper itself, that the entry cites.
    fn library_item(&self, reference: &Reference, own_item: &str) -> Result<Option<String>> {
        let doi = reference.doi.clone().or_else(|| {
            let arxiv_id = reference.arxiv_id.as_deref()?;
            Some(paper_ids::Identifier::parse(arxiv_id)?.doi())
        });
        let Some(doi) = doi else { return Ok(None) };
        Ok(self
            .library
            .find_item_by_doi(&doi)?
            .filter(|found| !found.deleted && found.item_id != own_item)
            .map(|found| found.item_id))
    }
}

fn network(error: paper_fetch::FetchError) -> PaperError {
    PaperError::Network {
        message: error.to_string(),
    }
}

/// What the stored records say about the paper's references.
#[derive(Debug, Default)]
pub(crate) struct Known {
    /// Crossref's `reference` list, in the order the publisher deposited it.
    pub deposited: Vec<Value>,
    /// The works OpenAlex lists as cited.
    pub cited: Vec<CitedWork>,
    /// Entry text to the work a search found for it; `None` for none.
    pub searched: HashMap<String, Option<(String, String)>>,
}

impl Known {
    fn from_records(records: &HashMap<String, Value>) -> Known {
        let deposited = records
            .get(paper_fetch::CROSSREF)
            .and_then(|work| work["message"]["reference"].as_array())
            .cloned()
            .unwrap_or_default();
        let cited = records
            .get(OPENALEX_REFERENCES)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(paper_fetch::cited_work)
            .collect();
        let searched = records
            .get(REFERENCE_SEARCHES)
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .map(|(text, search)| {
                let answer = &search["answer"];
                let found = answer["doi"]
                    .as_str()
                    .zip(answer["title"].as_str())
                    .map(|(doi, title)| (doi.to_string(), title.to_string()));
                (text.clone(), found)
            })
            .collect();
        Known {
            deposited,
            cited,
            searched,
        }
    }
}

/// Gives each entry the first answer the sources have, in the order the
/// module comment lists them.
pub(crate) fn resolve(list: &ReferenceList, own_doi: Option<&str>, known: &Known) -> References {
    let own_doi = own_doi.map(str::to_lowercase);
    let deposits = deposits_by_entry(list, &known.deposited);
    let mut references: Vec<Reference> = list
        .entries
        .iter()
        .zip(&deposits)
        .enumerate()
        .map(|(index, (entry, deposit))| {
            let deposited_doi = deposit
                .and_then(|d| d["DOI"].as_str())
                .map(str::to_lowercase);
            let printed_doi = entry
                .dois
                .iter()
                .map(|doi| doi.to_lowercase())
                .find(|doi| Some(doi) != own_doi.as_ref());
            let found_by = if deposited_doi.is_some() {
                ReferenceSource::Deposited
            } else if printed_doi.is_some() || entry.arxiv_id.is_some() {
                ReferenceSource::Printed
            } else {
                ReferenceSource::Unresolved
            };
            Reference {
                index: index as u32,
                number: entry.number,
                text: entry.text.clone(),
                parts: entry
                    .parts
                    .iter()
                    .map(|part| ReferencePart {
                        page_index: part.page_index as i32,
                        rect: page_rect(&part.rect),
                    })
                    .collect(),
                doi: deposited_doi.or(printed_doi),
                arxiv_id: entry.arxiv_id.clone(),
                url: entry.url.clone(),
                title: deposit
                    .and_then(|d| d["article-title"].as_str().or(d["volume-title"].as_str()))
                    .map(str::to_string),
                year: deposit
                    .and_then(|d| d["year"].as_str())
                    .and_then(|year| year.get(..4)?.parse().ok()),
                found_by,
                item_id: None,
            }
        })
        .collect();

    match_cited_works(&mut references, &known.cited);

    for reference in &mut references {
        if reference.found_by != ReferenceSource::Unresolved {
            continue;
        }
        if let Some(Some((doi, title))) = known.searched.get(&reference.text) {
            reference.doi = Some(doi.clone());
            reference.title = Some(title.clone());
            reference.found_by = ReferenceSource::Search;
        } else if reference.url.is_some() {
            reference.found_by = ReferenceSource::Printed;
        }
    }
    for reference in &mut references {
        if reference.arxiv_id.is_none()
            && let Some(id) = reference
                .doi
                .as_deref()
                .and_then(|doi| doi.strip_prefix("10.48550/arxiv."))
        {
            reference.arxiv_id = Some(id.to_string());
        }
    }

    let citations = list
        .citations
        .iter()
        .map(|mark| CitationLink {
            page_index: mark.page_index as i32,
            rect: page_rect(&mark.rect),
            reference: mark.entry as u32,
        })
        .collect();
    References {
        references,
        citations,
    }
}

/// The deposited reference of each entry. Publishers deposit the list in
/// print order, so when the counts agree the positions do. Otherwise a
/// numbered list goes by the number at the end of each deposit's key, such
/// as 12 in `1977_CR12` or `pone.0316678.ref012`, when those numbers are
/// the deposits' positions.
fn deposits_by_entry<'a>(list: &ReferenceList, deposited: &'a [Value]) -> Vec<Option<&'a Value>> {
    if deposited.len() == list.entries.len() {
        return deposited.iter().map(Some).collect();
    }
    let key_number = |deposit: &Value| -> Option<u32> {
        let key = deposit["key"].as_str()?;
        let digits = key.len() - key.trim_end_matches(|c: char| c.is_ascii_digit()).len();
        key[key.len() - digits..].parse().ok()
    };
    let by_position = deposited
        .iter()
        .enumerate()
        .filter(|(i, deposit)| key_number(deposit) == Some(*i as u32 + 1))
        .count();
    let by_number: HashMap<u32, &Value> = if by_position * 10 >= deposited.len() * 9 {
        deposited
            .iter()
            .filter_map(|deposit| Some((key_number(deposit)?, deposit)))
            .collect()
    } else {
        HashMap::new()
    };
    list.entries
        .iter()
        .map(|entry| by_number.get(&entry.number?).copied())
        .collect()
}

/// Gives entries without an identifier the cited work whose whole title
/// they contain as one run of words. Longer titles go first and each work
/// goes to one entry, because a short title such as "Quantum Random Access
/// Memory" is part of many entries.
fn match_cited_works(references: &mut [Reference], cited: &[CitedWork]) {
    let taken: HashSet<&str> = references
        .iter()
        .filter_map(|reference| reference.doi.as_deref())
        .collect();
    let works: Vec<(usize, Vec<String>)> = cited
        .iter()
        .enumerate()
        .filter(|(_, work)| work.doi.as_deref().is_none_or(|doi| !taken.contains(doi)))
        .map(|(index, work)| (index, words(&fold_ligatures(&work.title))))
        .filter(|(_, title)| title.len() >= MIN_TITLE_WORDS)
        .collect();
    let mut pairs: Vec<(usize, usize, usize)> = Vec::new();
    for (entry, reference) in references.iter().enumerate() {
        if reference.found_by != ReferenceSource::Unresolved {
            continue;
        }
        let text = words(&fold_ligatures(&reference.text));
        for (work, title) in &works {
            if text.windows(title.len()).any(|run| run == title.as_slice()) {
                pairs.push((title.len(), entry, *work));
            }
        }
    }
    pairs.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut used_entries = HashSet::new();
    let mut used_works = HashSet::new();
    for (_, entry, work) in pairs {
        if used_entries.contains(&entry) || used_works.contains(&work) {
            continue;
        }
        used_entries.insert(entry);
        used_works.insert(work);
        let reference = &mut references[entry];
        let work = &cited[work];
        reference.doi = work.doi.clone();
        reference.title = Some(work.title.clone());
        reference.year = reference.year.or(work.year);
        reference.found_by = ReferenceSource::OpenAlex;
    }
}

/// PDF text keeps ligatures such as "ﬁ"; titles spell the letters out.
fn fold_ligatures(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            'ﬀ' => out.push_str("ff"),
            'ﬁ' => out.push_str("fi"),
            'ﬂ' => out.push_str("fl"),
            'ﬃ' => out.push_str("ffi"),
            'ﬄ' => out.push_str("ffl"),
            'ﬅ' | 'ﬆ' => out.push_str("st"),
            c => out.push(c),
        }
    }
    out
}

fn page_rect(rect: &paper_identify::Rect) -> PageRect {
    PageRect {
        x0: rect.x0.into(),
        y0: rect.y0.into(),
        x1: rect.x1.into(),
        y1: rect.y1.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paper_identify::{CitationMark, EntryPart, Rect, ReferenceEntry};

    fn entry(number: Option<u32>, text: &str, dois: &[&str]) -> ReferenceEntry {
        ReferenceEntry {
            number,
            text: text.to_string(),
            parts: vec![EntryPart {
                page_index: 9,
                rect: Rect {
                    x0: 50.0,
                    y0: 600.0,
                    x1: 280.0,
                    y1: 620.0,
                },
            }],
            dois: dois.iter().map(|doi| doi.to_string()).collect(),
            arxiv_id: None,
            url: None,
        }
    }

    fn work(doi: &str, title: &str) -> CitedWork {
        CitedWork {
            doi: Some(doi.to_string()),
            title: title.to_string(),
            year: Some(2020),
        }
    }

    #[test]
    fn deposits_go_by_key_number_when_the_counts_differ() {
        // The PDF misses entry 2, so positions would give entry 3 the
        // deposit of entry 2.
        let list = ReferenceList {
            entries: vec![
                entry(Some(1), "A. First.", &[]),
                entry(Some(3), "C. Third.", &[]),
            ],
            citations: vec![CitationMark {
                page_index: 2,
                rect: Rect {
                    x0: 1.0,
                    y0: 2.0,
                    x1: 3.0,
                    y1: 4.0,
                },
                entry: 1,
            }],
        };
        let known = Known {
            deposited: vec![
                json!({"key": "1977_CR1", "DOI": "10.1/ONE", "article-title": "First", "year": "2001"}),
                json!({"key": "1977_CR2", "DOI": "10.1/two"}),
                json!({"key": "1977_CR3", "DOI": "10.1/three"}),
            ],
            ..Known::default()
        };
        let resolved = resolve(&list, None, &known);
        let dois: Vec<_> = resolved
            .references
            .iter()
            .map(|r| r.doi.as_deref())
            .collect();
        assert_eq!(dois, vec![Some("10.1/one"), Some("10.1/three")]);
        assert_eq!(resolved.references[0].title.as_deref(), Some("First"));
        assert_eq!(resolved.references[0].year, Some(2001));
        assert_eq!(resolved.references[0].found_by, ReferenceSource::Deposited);
        assert_eq!(resolved.citations[0].reference, 1);
        assert_eq!(resolved.citations[0].page_index, 2);
    }

    #[test]
    fn the_papers_own_doi_printed_in_a_footer_is_not_an_entrys() {
        let list = ReferenceList {
            entries: vec![entry(
                Some(1),
                "Smith J. A study. PLOS ONE 2020.",
                &["10.1371/journal.pone.0316678", "10.1/real"],
            )],
            citations: vec![],
        };
        let resolved = resolve(
            &list,
            Some("10.1371/JOURNAL.PONE.0316678"),
            &Known::default(),
        );
        assert_eq!(resolved.references[0].doi.as_deref(), Some("10.1/real"));
        assert_eq!(resolved.references[0].found_by, ReferenceSource::Printed);
    }

    #[test]
    fn cited_works_match_whole_titles_longest_first_one_entry_each() {
        let list = ReferenceList {
            entries: vec![
                // Contains the short title too, but the long one fits better.
                entry(
                    None,
                    "Doe J. Projection-based constrained policy optimization. In ICLR, 2020.",
                    &[],
                ),
                entry(
                    None,
                    "Achiam J. Constrained policy optimization. In ICML, 2017.",
                    &[],
                ),
                // Every title word is here, but not as one run.
                entry(
                    None,
                    "Roe K. Policy search, made constrained by optimization. 2019.",
                    &[],
                ),
                entry(None, "Lee A. Efﬁcient memory for agents. 2021.", &[]),
            ],
            citations: vec![],
        };
        let known = Known {
            cited: vec![
                work("10.1/cpo", "Constrained Policy Optimization"),
                work(
                    "10.1/pcpo",
                    "Projection-Based Constrained Policy Optimization",
                ),
                work("10.1/mem", "Efficient Memory for Agents"),
            ],
            ..Known::default()
        };
        let resolved = resolve(&list, None, &known);
        let dois: Vec<_> = resolved
            .references
            .iter()
            .map(|r| r.doi.as_deref())
            .collect();
        assert_eq!(
            dois,
            vec![Some("10.1/pcpo"), Some("10.1/cpo"), None, Some("10.1/mem")]
        );
        assert_eq!(resolved.references[1].found_by, ReferenceSource::OpenAlex);
        assert_eq!(resolved.references[2].found_by, ReferenceSource::Unresolved);
    }
}

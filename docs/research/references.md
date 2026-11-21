# Research: links from citations to the cited papers

Measured 2026-11-21. Decides how the reader finds the reference list of a PDF, gives each reference a link to the cited paper on the web, and makes the in-text citations open that link.

## Goal

When the user reads a paper in Paper, a citation such as "[12]" or "(Vaswani et al., 2017)" should show the cited paper and open it on the web in one click. Not every cited work has a public copy, but most have a DOI or an arXiv ID, and a DOI or arXiv page always leads to the paper. When no identifier is found, a search link for the title is still better than nothing.

The work has three parts:

1. Find the references section and split it into entries.
2. Give each entry an identifier: a DOI, an arXiv ID, or a URL.
3. Connect each in-text citation to its entry, so a hover or click on the citation can show and open it.

## How others do it

| Product | What it does | How |
|---|---|---|
| Zotero 7 reader | A hover on a citation or internal link shows the entry text from the references section in a popup. A newer prototype (zotero/zotero#5147) recognizes the entry on click, shows its abstract, and offers "Add to Library", with a Google Scholar search when recognition fails. | Follows the PDF's own internal links. For author-year citations without links, it shows the entries with the hovered year and the nearby author names, and lets the user pick. |
| Google Scholar PDF Reader (Chrome) | A click on a citation shows a summary of the cited paper and a link to its PDF. | Google's own parsing and its Scholar index. Closed. |
| Semantic Reader | Citation cards with the cited paper's details and TLDR. | Only for papers Semantic Scholar has processed, mostly arXiv. |
| GROBID | Open source. Extracts and parses references (about 0.87 to 0.90 F1) and connects citation markers to entries (0.76 to 0.91 F1), with PDF coordinates. Can resolve entries to DOIs through Crossref. | A Java service with machine learning models. Too heavy to ship inside a Mac app, but its numbers set the bar. |
| Crossref's own matching | Searching the whole reference string with `query.bibliographic` and then checking the best hits ("search-based matching with validation") reached 0.98 precision and 0.95 recall on 2,000 real reference strings. | Published in Crossref's blog, "Reference matching: for real this time". |

## Test set

26 open access PDFs with known DOIs. 17 are publisher typeset versions sampled from OpenAlex (Springer Nature 3, IEEE 3, PLOS 3, Frontiers 3, MIT Press Computational Linguistics 3, Tech Science Press 1, Taylor & Francis 1) and 9 are arXiv computer science preprints made with LaTeX. Elsevier, Wiley, MDPI, Oxford, and most Taylor & Francis files could not be downloaded by a script (HTTP 403), so those publishers are missing. Elsevier and Wiley typeset files usually have citation links, so the gap should not change the conclusions, but it is not measured. The scripts are in `.amp/in/orb-backup/refs/` of the research thread.

## What the PDF file gives

### Citation links

| | Files |
|---|---|
| In-text citations are internal links | 21 of 26 |
| No citation links at all | 5 (all 3 IEEE Access and TMM files, Taylor & Francis, one Computational Linguistics file) |

Every citation link in the set points to a named destination, and the name tells which entry it is:

| Name form | Example | Files | Entry number in the name |
|---|---|---|---|
| Springer | `12672_2025_1977_Article.indd:CR12:99` | 3 | yes, `CR12` |
| PLOS | `Rpone.0316678.ref012` | 3 | yes, `ref012` |
| Tech Science Press | `cite.ref-12` | 1 | yes |
| LaTeX `hyperref` | `cite.vaswani2017attention` | 11 | no, the BibTeX key |
| Frontiers (InDesign) | `fneur-15-1452194.indd:Anchor 14:1254` | 3 | no, an anchor counter |

In two files the unique destinations equal the deposited references (42 for 42, 49 for 49). Most files have fewer, such as 29 destinations for 50 entries, because a range such as "[3–6]" links only its ends and some entries are cited only in tables. So destinations mark where many entries start, which helps to split author-year lists, but they cannot split a list on their own.

`pdf_oxide` 0.3.78, which `paper-identify` already uses, returns each link's rectangle and its `LinkDestination::Named(name)`, so the engine can read the citation links without PDFKit. PyMuPDF placed the destination points of named destinations wrongly in about half the cases, so the engine should use the name and the entry order, not the point. PDFKit resolves destinations itself; a spike must check `PDFDestination.point` on these files before the app trusts it.

### The references section and its entries

The references heading was found in 25 of 26 files with the same pattern `paper-identify` uses to stop looking for a paper's own DOI. Numbered lists (`[12]`, `12.`) split cleanly by their numbers: 50 entries for 50 deposited references, 43 for 42, 33 for 32. Author-year lists split by the left edge of each line gave two to four times too many entries, because continuation lines are not always indented in the text layer. For those lists the link destinations, or the count and order of the deposited references, are the way to split.

### Identifiers printed in the entries

| Group | Entries | With a printed DOI or arXiv ID |
|---|---|---|
| Publisher, numbered (6 papers) | 268 | 54 (20%) |
| arXiv, numbered (5 papers) | 212 | 93 (44%) |

PLOS and Frontiers print a DOI for most entries; Springer, IEEE, and the Computational Linguistics journal print almost none. Printed DOIs need care: the paper's own DOI in the page footer was glued to the last entry of a page 4 times, a DOI broken at a line break was cut short 4 times, and one Springer DOI had zero width spaces inside it.

## What the metadata APIs give

| Source | Papers it covers | Notes |
|---|---|---|
| Crossref deposited references (`reference` in `/works/{doi}`) | all 17 publisher papers, 0 of 9 arXiv papers | The full list in all 17. 650 of 815 entries (80%) carry a DOI; the rest carry the citation as a string or as fields. Each entry has a `key` such as `1977_CR12`, `ref12`, `pone.0316678.ref012`, `bib12`, or `CIT0012`, which carries the entry number. arXiv DOIs are registered with DataCite, so Crossref answers 404 for them. |
| OpenAlex `referenced_works` | 26 of 26 | OpenAlex IDs only, in no particular order and with no link to the printed entry. Each cited work can be read by ID for its title, DOI, and arXiv ID. Works OpenAlex does not know are left out (20 works for a paper with 29 entries). |
| Semantic Scholar `/paper/{id}/references` | not measured fairly | Without a key it answered HTTP 429 to most calls. The 6 calls that worked returned 23 to 52 references, most with a DOI or arXiv ID. |

The engine already stores Crossref's full `/works/{doi}` response in `source_records` after a lookup, so every paper added by DOI already has its deposited references on disk.

## Giving each entry an identifier

### Publisher papers: Crossref search checked against the deposits

For the 6 numbered publisher papers, the deposited DOI of the entry with the same number is the answer key. A printed DOI or arXiv ID was used when there was one; otherwise the whole entry was searched with Crossref's `query.bibliographic`, and a hit counted when at least 80% of its title's words are in the entry (the same test `paper-identify` uses for page 1).

| Entries with a deposited DOI | Right | Wrong | Nothing |
|---|---|---|---|
| 246 | 230 (93%) | 4 | 12 |

These numbers count after the fixes for printed DOIs above. Of the 4 wrong answers, one is the arXiv version of the same work, one is an entry that was really a table row, and two are papers by the same author with nearly the same title, which a year check would have caught.

For publisher papers this search is not needed at all: the deposited references give the DOI of 80% of the entries with no request and no guess. The search matters for the remaining 20% and for papers whose publisher deposits no references.

### arXiv papers: OpenAlex referenced_works matched on the machine

arXiv papers have no deposited references, so for the 5 numbered arXiv papers each answer was checked by hand.

| Method | Entries with an answer | Right |
|---|---|---|
| DOI or arXiv ID printed in the entry | 93 of 212 | all, except DOIs cut at a line break |
| Crossref search of the entry, title check | 64 | 58 |
| OpenAlex `referenced_works`, each work read by ID, then each entry given the work whose title it contains, one work per entry, best pairs first | 178 | 176 |
| Any of the three | 196 (92%) | |

Matching against `referenced_works` is the best method for papers without deposits. It is also cheap: reading one OpenAlex record is free and has no daily limit, so it costs one request for the paper and one per cited work, and no search. It needs four details to work: titles and entries must be normalized with NFKC, because PDF text keeps ligatures such as "ﬁ"; each work may go to one entry only, because a short title such as "Quantum Random Access Memory" is contained in many entries; the title must appear as one run of words in the entry, not scattered; and the full entry text must be used, because long author lists push the title far from the start. The 2 wrong matches are a longer title losing to a shorter one ("Projection-based constrained policy optimization" given "Constrained Policy Optimization") and a PhD thesis given the journal paper with a similar title.

The 16 entries no method found are mostly NeurIPS, ICML, and ICLR papers, books, and technical reports such as GPT-2. OpenAlex knows most of them, but searching it with the whole entry string found 4 of 55 entries, 2 of them wrong. Searching it by a title cut from the entry found 34 of 42 before the daily budget ran out. For these, a search link is the practical answer.

### Rate limits and costs

| API | Limit measured | Consequence |
|---|---|---|
| Crossref `/works/{doi}` | 10 per second, 3 at a time (`x-rate-limit-limit`, `x-concurrency-limit`) | The engine's limiter already allows one request per 100 ms for `api.crossref.org`. |
| Crossref search (`/works?query...`) | 3 per second, 3 at a time | The engine's limiter uses one 100 ms gap for the whole host, which is too fast for searches: about 2.3 searches per second already got HTTP 429. Searches need their own budget. A search takes 336 ms at the median and 483 ms at the 90th percentile. |
| OpenAlex record by ID or DOI | free, no daily limit | Reading all cited works of a paper is free. |
| OpenAlex search | $0.001 per call; $0.10 per day without a key, $1 with the free key | About 100 searches a day without a key, so searches must be rare. |

## Decision

Status: implemented. `paper-identify` reads the list and the citation links (`PdfFile::references`), `paper-engine` resolves the entries (`references`, `resolve_references`, `search_reference`), and the reader shows a card on citations and entries (`ReaderReferences.swift`, `ReferenceCard.swift`). On the test PDFs opened in the app, the engine linked 38 of 39 entries of a PLOS paper, 54 of 54 of a Frontiers paper, and 61 of 68 of an arXiv paper before any search.


1. Read the citations and entries from the file in `paper-identify`, with no network: the references section, its entries, the citation links with their rectangles and destination names, and the entry number each name gives. Number the entries from the numbered list when there is one, else from the destination names when they carry numbers, else by the order of the entries and destinations.
2. Resolve entries in the engine, cheapest and surest first, and stop at the first answer:
   1. the deposited Crossref reference with the same number, read from the stored `crossref` record (no request);
   2. a DOI, arXiv ID, or URL printed in the entry, after removing the paper's own DOI, joining line breaks, and dropping zero width characters;
   3. OpenAlex `referenced_works`, read by ID and matched on the machine as described above (free requests, kept in `source_records` as `openalex_references`);
   4. a Crossref bibliographic search of the entry, only for an entry the user hovers or clicks, with a title, year, and first author check, at most 3 per second;
   5. otherwise a Google Scholar search link for the entry's title, or for its text when no title can be cut.
3. Store the result per attachment in SQLite: the entry number, raw text, rectangle of the entry, identifier, and how it was found, so the reader does not ask again. Raw responses stay in `source_records`.
4. In the reader, a hover on a citation link shows a popover with the entry text and, when known, the cited paper's title and year, with buttons to open the DOI or arXiv page, to open the paper in Paper when it is already in the library, and to add it by DOI. A click keeps PDFKit's jump to the references and the Back stack the reader already has. Each entry in the references section also gets the same popover, which covers the 5 files without citation links. Nothing is written into the PDF file.

For files without citation links (all IEEE files in the set), numbered markers such as "[12]" and "[3]–[5]" can be found in the page text and connected to entries by number. Author-year citations without links would need Zotero's approach of offering the entries with the hovered year; that is left for later.

## Open questions

1. The spike above: does `PDFDestination.point` land on the right entry for named destinations in PDFKit, including two column pages?
2. Splitting author-year lists that have no links and no deposited references.
3. When to resolve. The proposal resolves steps 1 to 3 when the reader opens a paper, which costs at most one request per cited work, and leaves step 4 to hover. Resolving when the paper is added instead would make the popover instant, at the cost of requests for papers the user never reads.
4. Whether to ask Semantic Scholar too. It would need an API key in settings to be usable at all.

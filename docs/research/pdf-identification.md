# Research: identifying a paper from its PDF

Measured 2026-11-05. Decides how `import_pdf` finds the DOI of a dropped PDF.

## How others do it

Zotero's `pdf-worker` reads the first five pages with pdf.js and keeps every word with its box, font size, and style, plus the Info dictionary. It sends that JSON to Zotero's recognizer web service, which is not open source and returns an arXiv ID, DOI, ISBN, or a guessed title, tried in that order. With no text at all Zotero reports that the file needs OCR.

pdf2doi checks the metadata (keys containing "doi" first), then the file name, then the page text with a regex, then a guessed title, and validates every candidate online. Its README names the usual failure: a cited paper's DOI printed before the paper's own.

## Test set

57 open access PDFs with known DOIs, three per publisher, sampled from OpenAlex across Elsevier, Springer Nature, Wiley, IEEE, ACM, Taylor & Francis, MDPI, Frontiers, PLOS, Oxford, ACS, IOP, eLife, Copernicus, BioMed Central, SAGE, Cambridge, APS, and RSC. Most are the copies OpenAlex points to, so 20 are preprints or accepted manuscripts (arXiv, bioRxiv, Research Square) rather than the journal's typeset version.

An answer is right when it is the expected DOI or the DOI of another version of the same work (for example the arXiv DOI of the preprint the file really is). Three preprints changed their title in the journal; their preprint DOI also counts as right.

## Results

| Method | Right | Wrong | Nothing |
|---|---|---|---|
| File text: metadata, links, and text candidates, accepted when Crossref's title for them is on page 1 | 49 | 0 | 8 |
| Title search: the largest text on page 1 searched in Crossref | 38 | 1 | 18 |
| Claude Haiku 5.5 reading an image of page 1 | 55 | 0 | 2 |
| Claude Haiku 5.5 reading the extracted page 1 text | 54 | 1 | 2 |
| File text, then title search, then the image model | 57 | 0 | 0 |

The engine's `import_pdf`, run over the same 57 files against the live APIs with a key set, got all 57 right (36 exact, 21 another version of the work): 49 by file text, 6 by title search, 1 by the model, and 1 correctly reported as a duplicate (the set holds one paper twice). It took 0.2 s per file at the median and 1.8 s at most.

The last row of the table is after one fix: Crossref titles carry markup such as `Fe<sub>3</sub>GeTe<sub>2</sub>`, which has to be removed before titles are compared. Before the fix two papers were missed.

The file text step missed only papers that print no DOI at all (ACS and IOP accepted manuscripts, some Cambridge, SAGE, and MDPI files). In none of those did the model read a DOI either; it helped by reading the title, which is then searched in Crossref. Title search alone misses when the largest text on page 1 is not the title, such as a journal banner.

## Cost and speed

| Step | Median | 90th percentile |
|---|---|---|
| Reading three pages with `pdf_oxide` (release build) | 69 ms | 119 ms |
| Rendering page 1 to a PNG of at most 1568 px | 90 ms | 763 ms |
| Crossref check of one candidate | 81 ms | |
| Haiku with the image | 1.2 s | 2.3 s |

The image call takes about 2,600 input and 100 output tokens, about $0.0003 per paper at Haiku 5.5 prices ($0.10 and $0.50 per million tokens), so about $0.31 per 1,000 papers. The text call is a little cheaper (about 1,500 input tokens) but fails on scanned pages and garbled text layers, so the engine sends the image.

Haiku 5.5 sometimes answers with a thinking block before the text block, so the answer must be read from the first block of type `text`, and `max_tokens` must leave room for the thinking.

## Decision

1. File text, checked with Crossref. Free, local, and never wrong in the test.
2. Title search in Crossref. Free; marked `TitleSearch` so the app can ask the user to check.
3. Haiku with the page 1 image, only when the user has set an Anthropic key. A DOI it reads is accepted only when its registered title matches the title it reads.

Not done yet: following a preprint's DOI to the journal version (Crossref's `is-preprint-of` relation), the ISBN path for books, and OCR for scanned files without a model key.

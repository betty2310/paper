# Research: DOI metadata, open access PDFs, and citations

Verified 2026-10-27 against the linked primary sources. Items marked unverified were not confirmed from an official page.

## Sources

| Source | Endpoint | Auth | Rate limit | Gives us | Notes |
|---|---|---|---|---|---|
| Crossref REST | `GET https://api.crossref.org/works/{doi}`, agency `GET /works/{doi}/agency` | none; polite pool with `?mailto=` or `mailto:` in User-Agent | since 2025-12-01: public 5/s, polite 10/s with concurrency 3; 429 on limit | canonical metadata, JATS abstract, ISSN, `license[]`, `link[]`, `reference[]`, `is-referenced-by-count` | Crossref DOIs only; DataCite DOIs (arXiv `10.48550`, Zenodo) return 404 |
| DOI content negotiation | `GET https://doi.org/{doi}` with `Accept: application/vnd.citationstyles.csl+json` (also `application/x-bibtex`, RIS, `text/x-bibliography; style=apa`) | none | none documented | CSL JSON for Crossref, DataCite, and mEDRA DOIs | thinner than REST, follows redirects |
| OpenAlex | `GET https://api.openalex.org/works/https://doi.org/{doi}` | free API key required since 2026-02-13 (`?api_key=` or Bearer); `mailto` pool removed | credit metered, single entity lookup by DOI costs 0, hard cap 100/s | `open_access.oa_url`, `best_oa_location.pdf_url`, `locations[]`, `authorships`, `abstract_inverted_index`, `topics`, `cited_by_count`, `ids.{pmid,pmcid}`, `is_retracted` | data CC0; covers Crossref, DataCite, arXiv, PubMed |
| Semantic Scholar | `GET https://api.semanticscholar.org/graph/v1/paper/DOI:{doi}?fields=...` | optional `x-api-key` | shared pool without key, 1/s with key | `externalIds`, `openAccessPdf`, `tldr`, `citationCount` | license is non commercial and requires attribution with `utm_source=api`; keep optional |
| Unpaywall | `GET https://api.unpaywall.org/v2/{doi}?email=` | email param | 100k/day | `is_oa`, `oa_status`, `best_oa_location.url_for_pdf`, `oa_locations[]` | still operating in 2026, served from OpenAlex data; search endpoint retired 2026-09-18 |
| DataCite | `GET https://api.datacite.org/dois/{doi}` | none | not documented (unverified) | `data.attributes.{titles,creators,publisher,publicationYear,types,url,descriptions}` | datasets, Zenodo, arXiv DOIs |
| arXiv | PDF `https://arxiv.org/pdf/{id}`, API `http://export.arxiv.org/api/query?id_list={id}` | none | one request per 3 s, single connection | guaranteed OA PDF | acknowledgement text required: "Thank you to arXiv for use of its open access interoperability." |
| NCBI ID converter | `GET https://pmc.ncbi.nlm.nih.gov/tools/idconv/api/v1/articles/?ids={doi}&format=json&tool=&email=` | none | 200 ids/request | `records[]{pmcid,pmid,live,release-date}` | |
| PMC | `https://pmc.ncbi.nlm.nih.gov/articles/{PMCID}/` | none | "systematic retrieval ... is prohibited" | OA PDFs for about a quarter of PMC | `oa.fcgi` service retired 2026-08-24 |
| Europe PMC | `GET https://www.ebi.ac.uk/europepmc/webservices/rest/search?query=DOI:"{doi}"&resultType=core&format=json`, PDF `https://europepmc.org/articles/PMC{id}?pdf=render` | none | 10/s per IP (unverified) | `fullTextUrlList`, `isOpenAccess` | |
| Landing page | follow `https://doi.org/{doi}`, read `citation_pdf_url` or `<link rel="alternate" type="application/pdf">` | none | per site | direct PDF URL on repositories and OA journals | large publishers block non browser clients with Cloudflare or Akamai; treat 403 as unavailable |

## Metadata pipeline

1. Normalize the identifier (see below).
2. Crossref polite `GET /works/{doi}?mailto=...` with `User-Agent: Paper/<version> (<url>; mailto:<email>)`. On 200 this is the primary record.
3. On 404 call `/works/{doi}/agency`. If `datacite`, use the DataCite API or content negotiation. Otherwise use content negotiation for CSL JSON.
4. In the background, enrich from OpenAlex (OA status, PDF links, topics, citation count, PMCID, retraction flag).
5. Cache every raw response locally.

## PDF pipeline (open access only)

1. arXiv if the DOI is `10.48550/arXiv.{id}` or an arXiv ID is known from OpenAlex or Crossref `relation.has-preprint`.
2. OpenAlex `best_oa_location.pdf_url`, then `locations[]` with `is_oa && pdf_url`, preferring `publishedVersion` over `acceptedVersion` over `submittedVersion`.
3. Unpaywall `best_oa_location.url_for_pdf`, then `oa_locations[].url_for_pdf`.
4. Europe PMC or PMC landing page when a PMCID is known.
5. Landing page meta tag as a best effort.
6. Otherwise show "No open access PDF found" with buttons to open the publisher page and to attach a file. Offer Zotero style custom resolvers in settings for institutional users.

Download hygiene copied from Zotero's `attachments.js`: at most 6 URLs per source, dedupe normalized URLs, force HTTPS, three attempts with backoff, at least 1 s between requests to the same host (none for doi.org), stop after 5 consecutive failures on a host, accept only `Content-Type` starting with `application/pdf` or a body starting with `%PDF`.

## Field mapping

| Our field | Crossref `message.` | CSL JSON | OpenAlex | Unpaywall | DataCite `data.attributes.` |
|---|---|---|---|---|---|
| doi | `DOI` (lowercase) | `DOI` | `doi` (URL form) | `doi` | `doi` |
| title | `title[0]` plus `subtitle[0]` | `title` | `title` | `title` | `titles[0].title` |
| authors | `author[]{given,family,ORCID,sequence}` | `author[]` | `authorships[]{author{display_name,orcid},raw_author_name}` | `z_authors[]` | `creators[]{givenName,familyName,name}` |
| container | `container-title[0]`, `short-container-title[0]` | `container-title` | `primary_location.source.display_name` | `journal_name` | `publisher` |
| date | `issued.date-parts[0]`, `published-online`, `published-print` | `issued.date-parts` | `publication_date` | `published_date` | `publicationYear`, `dates[]` |
| volume, issue, pages | `volume`, `issue`, `page` or `article-number` | same | `biblio.{volume,issue,first_page,last_page}` | | |
| abstract | `abstract` (JATS XML, strip tags) | `abstract` | `abstract_inverted_index` (rebuild) | | `descriptions[]` with `descriptionType == "Abstract"` |
| publisher | `publisher` | `publisher` | `primary_location.source.host_organization_name` | `publisher` | `publisher` |
| url | `resource.primary.URL` | `URL` | `primary_location.landing_page_url` | `doi_url` | `url` |
| type | `type` (`journal-article`, `proceedings-article`, `posted-content`, `book-chapter`) | `type` | `type` | `genre` | `types.resourceTypeGeneral` |
| issn | `ISSN[]` | `ISSN` | `primary_location.source.issn[]` | `journal_issns` | |
| OA | `link[]`, `license[]` | | `open_access.*`, `best_oa_location.*` | `is_oa`, `oa_status`, `best_oa_location.*` | |
| external ids | `relation` | `PMID`, `PMCID` sometimes | `ids.{pmid,pmcid,mag}` | | `identifiers[]` |

## How Zotero finds PDFs

`getFileResolvers` order: DOI landing page through translators, item URL, open access lookup through Zotero's own server (Unpaywall derived data) trying published, accepted, then submitted versions, PMC landing page when a PMCID exists, then custom resolvers from `extensions.zotero.findPDFs.resolvers`. The desktop app has no browser cookies, so paywalled PDFs only work on campus networks or VPN; ScienceDirect shows a CAPTCHA that Zotero pops up in a window.

## Legal and terms constraints

1. Only auto download when a source asserts open access. Otherwise open the browser or let the user attach a file. No paywall circumvention.
2. PMC prohibits systematic retrieval outside its approved services; single user initiated fetches are fine.
3. arXiv: one request per 3 s, show the acknowledgement.
4. Semantic Scholar: non commercial, attribution required. Off by default.
5. OpenAlex: CC0 data, API key required, credit budget, terms revised 2026-10-02.
6. Crossref: metadata is free to use, respect pools and 429, identify with `mailto`.
7. Unpaywall: email required, 100k per day.
8. Respect 403 and 429 from publishers, never share browser cookies without explicit user action.

## Identifier parsing

DOI regex (Crossref blog, matches 74.4M of 74.9M DOIs): `^10\.\d{4,9}/[-._;()/:A-Z0-9]+$` case insensitive. Strip `https://doi.org/`, `http://dx.doi.org/`, `doi:`, URL decode `%2F`, trim trailing `.`, `,`, `)`, `;`. Store lowercase.

arXiv: new form `\d{4}\.\d{4,5}(v\d+)?`, old form `[a-z\-]+(\.[A-Z]{2})?/\d{7}(v\d+)?`. Accept `arXiv:` prefix and `arxiv.org/abs|pdf/` URLs. arXiv DOI is `10.48550/arXiv.{id}`.

PDF to DOI on import: run the DOI regex over the text of the first two or three pages, preferring matches near `doi` or `https://doi.org/` and ignoring reference list DOIs; read XMP `prism:doi`, `pdf:doi`, `dc:identifier`; fall back to `GET https://api.crossref.org/works?query.bibliographic={title}&rows=3` and accept only with high title similarity plus an author or year match.

## Citations in Rust

`hayagriva 0.10.1` (Typst) is a full CSL 1.0.2 processor with about 100 bundled styles (`ArchivedStyle::Apa`) and reads BibLaTeX and its own YAML; it has no BibTeX writer. `biblatex 0.12.0` parses and writes BibTeX and BibLaTeX (`Entry::to_bibtex_string()`). `citeproc-rs` is abandoned.

MVP path: map the item to `biblatex::Entry` for "copy BibTeX" with a citation key `{family}{year}{firstword}`; map to `hayagriva::Entry` (`EntryType::Article` with a `Periodical` parent) and render with `BibliographyDriver` and the APA style for "copy citation".

## HTTP client

`reqwest 0.13` needs tokio. `ureq 3` is blocking with rustls by default and fits GPUI's background executor. Use rustls with `rustls-native-certs` so campus and corporate certificates work. Honor `Retry-After` and keep a per host token bucket.

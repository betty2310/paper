# Paper: product requirements (MVP)

Status: draft for discussion, 2026-10-27. The app is called Paper, bundle identifier `com.betty2310.paper`, MIT licensed. It is built for the author's own use first.

## Summary

Paper is a native desktop application for managing scientific papers. The user enters a DOI, and the app fetches the metadata, tries to download the open access PDF, and stores both in a local library. The user organizes papers into collections, reads the PDFs in a built-in viewer, highlights text, adds sticky notes, writes notes about each paper, and copies citations. Everything is stored on the user's machine. The logic is a Rust engine, and each platform gets a native app on top of it. Version 1.0 is the macOS app, built with AppKit, SwiftUI, and PDFKit. Windows and Linux apps follow after 1.0 on the same engine.

## Problem

Zotero is the standard free reference manager, but it is a Firefox based application. Opening two or three PDFs in its reader uses 2 to 3 GB of memory on the author's machine, startup is slow, and scrolling and searching inside PDFs feel sluggish. The alternatives are either web based (Paperpile, ReadCube) or macOS only (Skim, which does not manage references). There is no fast native reference manager that runs on all three desktop platforms.

## Goals

1. Managing papers must be fast. The app starts in under one second, a PDF shows its first page in under 300 ms, and scrolling stays at the display refresh rate.
2. Memory stays low. Three open PDFs should use under 400 MB of resident memory in total.
3. Adding a paper by DOI takes one action and no manual typing of metadata.
4. Reading and annotating happen in the app, and the user never needs another PDF viewer.
5. The library is a plain SQLite file plus a folder of PDFs that the user owns and can back up or sync with any file sync tool.

## Non goals for the MVP

The MVP does not include cloud sync or accounts, a browser extension, word processor plugins, group libraries, OCR, pen or tablet ink input, or import from Mendeley. It also does not try to download paywalled PDFs. Import from Zotero is part of version 1.0 but is the last milestone, after the reading and annotation features work. Books, theses, and web pages can be added later, but the MVP only has to handle items that have a DOI (journal articles, conference papers, and preprints).

## Users

The primary user is a graduate student or researcher who reads 5 to 50 papers a month, mostly as PDFs, and who needs to find a paper again later, remember what it said, and cite it. The user is comfortable installing a desktop app and already knows what a DOI is.

## Core flows

### First launch and library folders

On first launch the app asks the user to create a new library folder or open an existing one, the same way Obsidian asks for a vault. A library is a folder that contains `library.db` and a `files/` directory. If the chosen folder already contains a library, the app opens it and nothing is imported or converted. The app remembers recent libraries, and the user can switch between them from the File menu. Because the library is a plain folder, the user can place it in a synced folder such as Dropbox or iCloud Drive, with the caveat that the MVP does not merge concurrent edits from two machines.

### Add a paper by DOI

1. The user pastes a DOI, a doi.org URL, or an arXiv ID into the add box and presses Enter.
2. The app normalizes the identifier and fetches metadata from Crossref, with a fallback to DOI content negotiation for DataCite DOIs (arXiv, Zenodo).
3. The app shows the fetched title, authors, journal, and year immediately, then looks for an open access PDF through OpenAlex, Unpaywall, arXiv, and PubMed Central.
4. If a PDF is found, the app downloads it, verifies it is a PDF, stores it under the library folder, and attaches it to the item. If no PDF is found, the item shows a "no PDF" state with buttons to open the publisher page in the browser or to attach a file.
5. If the DOI already exists in the library, the app selects the existing item instead of creating a duplicate.

### Add a paper from a PDF file

The user drags a PDF into the window or uses File > Import. The app extracts the text of the first pages, looks for a DOI, and runs the DOI flow above. If no DOI is found, it searches Crossref by the title from the PDF metadata and asks the user to confirm the match. If nothing matches, it creates an item with the PDF title and lets the user fill in a DOI later.

### Add a paper without a DOI

A manuscript the user is writing has no DOI yet. File > New Paper Without DOI (⌥⌘N, or "No DOI yet…" in the Add Paper popover) opens a sheet where only the title is required; abstract, authors, venue, and identifiers may stay blank, and a PDF can be picked or dropped on the sheet. Nothing is looked up and no PDF is searched for. The DOI can be added later in Edit.

### Organize

The sidebar lists collections. Collections can be nested, and one paper can be in several collections. The main list shows the papers of the selected collection with sortable columns (title, first author, journal, year, date added). Items can have tags. A search box filters the list by title, author, journal, tag, and note text as the user types.

### Read

Double clicking a paper opens its PDF in a tab in the same window. The viewer scrolls continuously through all pages, supports zoom (fit width, fit page, and percentages), page navigation, the document outline, an in-document text search with highlighted results, and clickable links. Several PDFs can be open in tabs at the same time.

### Annotate

The user selects text and chooses highlight, underline, or strikethrough in one of a few colors. The user can also click anywhere on a page to add a sticky note with text, and draw a rectangle around a figure. A sidebar lists all annotations of the current PDF in page order with their selected text and comment, and clicking one scrolls to it. Annotations are stored in the library database and drawn on top of the page. The PDF file on disk is never modified. The user can export a copy of the PDF with the annotations embedded as standard PDF annotations.

### Take notes

Each paper has a free-form note in Markdown with a plain editor and a rendered preview. The note can be edited next to the PDF.

### Cite

From the item list or the viewer, the user can copy a BibTeX entry or a formatted citation (APA to start, more CSL styles later) to the clipboard, and can export a selection of items or a whole collection to a .bib file. Each paper has a citation key such as `vaswani2017attention`, made once from its first author, year, and title and kept from then on, so documents that cite it keep working. The user can copy the key for `\cite{}` or set their own.

## Requirements by area

| Area | MVP requirement |
|---|---|
| Identifiers | Accept DOIs (with or without prefixes), doi.org URLs, and arXiv IDs. Store DOIs in lowercase. |
| Metadata | Title, authors with given and family names and ORCID, container title, year and full date, volume, issue, pages, abstract, publisher, ISSN, URL, type. Keep the raw JSON from each source. |
| PDF fetch | Only download when a source states the file is open access. Verify the response is a PDF. Follow polite usage rules of each API (contact email in the user agent, rate limits). |
| Storage | One SQLite database and one files folder in a user-chosen library directory. Every row has a UUID and modification timestamp so sync can be added later. |
| Library chooser | Create or open a library folder on first launch, list recent libraries, switch libraries without restarting. |
| Zotero import (last milestone) | Read `zotero.sqlite` and the `storage/` folder: items with DOIs, creators, collections, tags, notes, PDF attachments, and reader annotations. Report what was skipped. |
| Viewer | Continuous scrolling, zoom from 50% to 400%, page thumbnails optional, text search, outline, links, remember last page per PDF. |
| Annotations | Highlight, underline, strikethrough, sticky note, rectangle. Colors, comment text, created and modified times. Export to PDF copy. |
| Notes | One Markdown note per item, autosaved. |
| Citation | Copy BibTeX, copy APA (rich and plain text), copy citation keys, export a selection or a collection to .bib. Stable, unique, editable citation keys. |
| Platforms | Version 1.0: macOS 13 and later on Apple silicon and Intel, signed, notarized, sandboxed, with Sparkle updates. After 1.0: Windows 10 and later, Linux. The engine builds and passes its tests on all three from the start. |
| Offline | Everything except adding by DOI works offline. Adding a DOI offline queues the lookup. |

## Performance targets

| Measure | Target |
|---|---|
| Cold start to interactive library | under 1 s |
| Open a PDF to first page visible | under 300 ms |
| Memory with the library and three open PDFs of 30 pages | under 400 MB resident |
| Scrolling and zooming | no dropped frames at 60 Hz on a 2020 laptop |
| DOI to metadata shown | under 2 s on a normal connection |
| Library of 5,000 items | list filtering under 50 ms |

## Success measures for the MVP

The MVP is done when the author can move their own reading from Zotero to Paper for one month without going back for daily reading, organizing, and citing. The measurable checks are the performance targets above and the ability to add 20 recent DOIs with PDFs found for at least the open access ones.

## Decisions made on 2026-10-27

1. Name: Paper. Bundle identifier `com.betty2310.paper`. License: MIT.
2. Library location: the user chooses a folder on first launch, and an existing library folder can be opened directly.
3. Zotero import ships in version 1.0 as the last milestone.
4. UI: a three pane layout with the library as the main view and each PDF in a tab next to it, serif titles, one accent color, light theme first. The full specification and mockups are in `docs/UI.md` and `docs/design/`.

## Decisions made on 2026-10-28

1. Shape: a Rust engine with a native app per platform, called in process through UniFFI, instead of one GPUI app. The research is in `docs/research/native-ui-rust-core.md`.
2. macOS first. Version 1.0 is the macOS app only, built with AppKit, SwiftUI for leaf content, and PDFKit for the reader. The goal is an app that is as good as a Mac app can be.
3. Windows and Linux apps come after 1.0, reuse the engine unchanged, and use PDFium for rendering and text because Windows has no system text or annotation API.

## Open questions

1. Dark Mode. `docs/UI.md` schedules the dark theme after 1.0, but Mac users expect a native app to follow the system appearance from day one. Decide before milestone 1 whether the dark tokens are part of 1.0.
2. Distribution. Direct download with Sparkle is planned. If the Mac App Store is wanted too, the sandbox entitlements and the update path have to be settled before milestone 4.
3. Minimum macOS version. 13 is the plan. Raising it to 14 would allow more SwiftUI and the newer `PDFView` behavior, at the cost of one year of Macs.

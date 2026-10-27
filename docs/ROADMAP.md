# Roadmap

Each milestone ends with something the author can run. Risky parts are proven first with small spikes, so the order below is by risk, not by user value.

## Milestone 0: spikes (about one week)

1. GPUI hello world with gpui-kit 0.8.1 builds and opens a window on the author's machine, and CI builds it on macOS, Windows, and Linux. Measure clean and incremental build times. Done so far: the README example compiles unchanged on Linux x64 in 5 minutes 50 seconds on 4 cores (2026-10-27). Opening a window has not been tested yet because the build machine has no display or GPU.
2. PDFium page to screen. Load a 30 page paper through pdfium-render on a worker thread, render pages as BGRA, show them in a scrolling `list` of `canvas` elements, and measure memory and frame times at 100% and 200% zoom. The result decides tile size and cache limits.
3. Selection to highlight. Get character boxes from PDFium, implement drag selection and line grouping, draw highlight quads over the page, and export one highlight to a PDF copy that opens correctly in Preview, Acrobat, and Chrome.
4. DOI lookup. A command line tool in `paper-fetch` that takes a DOI and prints the mapped metadata and the PDF URL it would download, tested on 20 DOIs including an arXiv DOI and a paywalled Elsevier paper.

## Milestone 1: library

Workspace skeleton, SQLite schema with migrations, `Library` service, collections tree, item table with sorting and filtering, add by DOI with metadata only, item detail panel, delete and undo. No PDFs yet.

## Milestone 2: PDFs

Open access PDF fetching with progress, drag and drop import with DOI detection, attachment storage, reader tab with continuous scrolling, zoom, outline, links, in-document search, remembering the last page.

## Milestone 3: annotations and notes

Highlight, underline, strikeout, sticky note, rectangle, colors, annotation list panel, Markdown note panel, export PDF with annotations, import of embedded annotations.

## Milestone 4: citations and packaging

Copy BibTeX, copy APA, export .bib, citation keys, settings screen (contact email, OpenAlex key, theme), library chooser and recent libraries, signed macOS build, Windows installer, Linux tarball, crash and error reporting to a local log.

## Milestone 5: Zotero import and 1.0

Import from `zotero.sqlite` and the `storage/` folder: items with DOIs, creators, collections, tags, notes, PDF attachments, and reader annotations (Zotero stores them in `itemAnnotations` with page index and position JSON, which maps onto our quads). Items without a DOI are imported with their fields as given. The import runs from the File menu, shows progress, and reports what was skipped.

## After 1.0

More CSL styles and a style picker, full text search inside PDFs across the library, sync through a user chosen folder, browser extension, group libraries, and ink annotations for tablets.

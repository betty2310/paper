# Research: PDF engines and annotation storage

Verified 2026-10-27 against crates.io, GitHub, crate sources, and linked docs.

## Versions and licenses

| Crate | Latest | Released | License | Notes |
|---|---|---|---|---|
| `pdfium-render` | 0.9.4 | 2026-09-06 | MIT or Apache-2.0 (PDFium is BSD-3) | 89 releases, active, single maintainer (ajrcarey) |
| `mupdf` / `mupdf-sys` | 0.8.0 | 2026-06-22 | AGPL-3.0 | vendors MuPDF 1.27.2 |
| `hayro` | 0.8.0 | 2026-10-04 | Apache-2.0 or MIT | pure Rust, `forbid(unsafe_code)`, read only |
| `pdf` (pdf-rs) | 0.10.0 | 2026-03-02 | MIT | parser only |
| `lopdf` | 0.46.0 | 2026-10-21 | MIT | object level editing, `IncrementalDocument`, no rendering |
| `krilla` | 0.8.2 | 2026-06-04 | MIT or Apache-2.0 | writer only |
| `poppler-rs` | 0.26.0 | 2026-02-21 | MIT bindings, poppler GPL | needs system poppler-glib, cairo, GLib |
| `pdfium` (newinnovations) | 0.10.4 | 2026-05-17 | GPL-3.0 | do not confuse with pdfium-render |
| pdfium-binaries | chromium/8125 | 2026-10-26 | MIT scripts, PDFium BSD-3 | built every Monday since 2017, dynamic libraries only |

## Capability comparison

| Capability | pdfium-render | mupdf-rs | hayro | lopdf |
|---|---|---|---|---|
| Render quality | Chrome's engine | reference class | renders nearly everything Chrome and pdf.js do, slower on heavy glyph pages | none |
| Character boxes | `PdfPageTextChar::tight_bounds()`, `loose_bounds()`, `origin()`, `get_char_at_point()`, line rects via `PdfPageTextSegments` | `TextPage` blocks, lines, chars with `quad()` | only by implementing `Device::draw_glyph_run` | no |
| Text search | `PdfPageText::search()` returning segments with `bounds()` | `Page::search()` returning quads | no | no |
| Links and outline | `PdfPage::links()`, `PdfDocument::bookmarks()` | yes | no | outline structs only |
| Read annotations | typed (highlight, underline, strikeout, squiggly, ink, square, circle, text, free text, popup, link, widget) | yes | renders appearance streams only | raw dicts |
| Write annotations | `create_highlight_annotation()` and friends, `set_bounds`, `set_contents`, `set_stroke_color`, `attachment_points_mut().create_attachment_point_at_end(PdfQuadPoints)`; no ink stroke or opacity setter in the high level API | complete, including `set_opacity`, `set_ink_list`, `update()` | no | hand built dicts, no appearance streams |
| Incremental save | no (`save_to_*` uses flags 0, TODO in source) | yes | no | yes |
| Thread model | not thread safe, global mutex with `thread_safe` feature | per thread cloned contexts, `DisplayList` is Send + Sync | no shared state | n/a |
| Build | download the dynamic library, no C toolchain | compiles MuPDF C source with bindgen, needs libclang | `cargo build` | `cargo build` |
| Binary cost | `libpdfium.so` x64 8.05 MB, needs only glibc 2.16, no libstdc++ | large, fonts included | small | small |

### pdfium-render details

It binds PDFium at run time through `libloading`. `Pdfium::default()` looks for `./libpdfium.*` then the system library, and `Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(dir))` binds a specific path. Static linking exists (`static` feature plus `PDFIUM_STATIC_LIB_PATH`) but you must build PDFium yourself with depot_tools, and pdfium-binaries states "we don't plan to add static builds". The crate's `pdfium_XXXX` feature must not be newer than the library (issue #151); `pdfium_latest` is `pdfium_7881` in 0.9.3+. Avoid pdfium build 6996 on macOS (issue #192). PDFium's `fpdfview.h`: "None of the PDFium APIs are thread-safe." Issue #262 (open) showed unsoundness when the mutex was dropped in 0.9.0; 0.9.4 reinstated `ThreadSafePdfiumBindings`. Design for a single owning thread regardless.

Render API: `PdfRenderConfig::new().scale_page_by_factor(f)`, `set_target_width/height`, `set_origin()` for tiles (0.9.3), `render_annotations(bool)`, `render_form_data`, `set_reverse_byte_order` (BGRA/RGBA order). `PdfPage::render_with_config()` gives `PdfBitmap` with `as_rgba_bytes()` or `as_raw_bytes()`. `PdfPage::page_size()`, `rotation()`, `boundaries()` (CropBox and MediaBox). `FPDF_PageToDevice` and `FPDF_DeviceToPage` are wrapped on `PdfPage`.

Save: `PdfDocument::save_to_file/bytes/writer`. Raw escape hatches: `bindings().FPDFAnnot_AddInkStroke`, `FPDF_SaveWithVersion(.., FPDF_INCREMENTAL, ..)`.

### MuPDF

Best write path (quads, ink, opacity, incremental save) and `DisplayList` for tiling in parallel. License is AGPL unless a commercial license is bought from Artifex. Build compiles MuPDF plus freetype, harfbuzz, jbig2dec, openjpeg, lcms2; disable defaults (`default-features = false, features = ["base14-fonts"]`).

### hayro

Pure Rust, Linebender ecosystem (`vello_cpu`, `kurbo`), fast on image heavy pages, slower on glyph heavy pages (issue #1358). No text page, search, link, or annotation object API. Author warns about timeouts and OOM on hostile files and API churn. Worth watching for 2027; could serve as a thumbnail renderer.

## How other apps store annotations

Zotero 7 stores annotations in its database, not the PDF ("allows for fast, conflict-free syncing"); embedded annotations are read only until imported; "Store Annotations in File" was removed; export embeds them. Skim stores notes in extended attributes with `.skim` sidecar and embedded export. sioyek stores in `shared.db` with an `embed_annotations` command. Okular historically used XML docdata and now can save into the PDF.

| | Database rows drawn as overlay | Write into the PDF |
|---|---|---|
| Sync | small mergeable deltas | whole file per edit, conflicts |
| Portability | needs export | visible everywhere |
| Integrity | file never modified | risk of corrupting the file |
| Features | tag, search, link annotations across the library | PDF model only |
| Engine needs | render plus text geometry | write, appearance streams, incremental save |

Recommendation: database rows with export and import. Keep the schema PDF shaped (page index, kind, quads in user space, rect, color, opacity, contents, author, dates) plus a text anchor (selected text with before and after context) for re-anchoring when the file is replaced.

## Highlight geometry

PDF user space has its origin at the bottom left, y up, 1 pt = 1/72 in, inside the CropBox. `/Rotate` is applied at display time; character boxes and annotation rects are stored unrotated. Screen px = pt × zoom × window scale factor. Keep points in the model and convert only when painting or hit testing.

Selection: mouse down and up map to character indexes through `get_char_at_point`. Line rectangles come either from `PdfPageTextSegments` (PDFium groups runs per line) or by grouping `loose_bounds()` of the characters by baseline. Each line rect becomes a `PdfQuadPoints` in the ISO 32000 order upper left, upper right, lower left, lower right. For export, `create_highlight_annotation()`, `set_bounds(union)`, one attachment point per line, `set_stroke_color`, `set_contents`, dates; PDFium generates the appearance stream. Sticky notes use a 20 × 20 pt rect.

## Rendering numbers

RGBA8 at 2× device scale and 100% zoom: Letter 1224 × 1584 px = 7.76 MB, A4 1190 × 1684 px = 8.02 MB. At pdf.js's 96 dpi convention the same pages are 13.8 and 14.3 MB. A 256 MB cache holds about 30 Letter pages at 2×. Render whole pages up to about 200% zoom, tiles of 1024 px above that. Render a quarter scale preview first. LRU keyed by (document, page, zoom bucket, rotation, tile); prefetch two pages in the scroll direction; cache the per page text layer.

## Platform pitfalls for pdfium-render

Dynamic libraries exist for mac-arm64, mac-x64, mac-univ, win-x64, win-arm64, linux-x64, linux-arm64, and musl. Place the library next to the executable or in `Contents/Frameworks` and code sign it on macOS. Pin the pdfium-binaries tag and the matching crate feature. Linux library needs only glibc 2.16.

## Recommendation

pdfium-render 0.9.x with the pinned dynamic pdfium-binaries library, one PDF worker thread, annotations in SQLite with export and import. Keep the annotation schema engine agnostic so MuPDF could be used for export later under a commercial license.

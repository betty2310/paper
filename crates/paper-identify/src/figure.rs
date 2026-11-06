//! Finds the region of a page that holds a paper's first figure, for the
//! preview picture in the app's paper list.
//!
//! Many figures are vector drawings rather than embedded images, so the
//! region comes from the layout: the "Figure 1" caption in the first pages,
//! and the drawings, images, and labels stacked directly above it. When no
//! caption leads to a figure, the largest embedded image stands in.

use std::sync::LazyLock;

use pdf_oxide::PdfDocument;
use regex::Regex;

use crate::PdfFile;

/// Pages searched for the first figure.
pub const FIGURE_PAGES: usize = 4;
/// The largest vertical gap, in points, between two parts of one figure.
const PART_GAP: f32 = 20.0;
/// Room left around the figure in the crop, in points.
const PADDING: f32 = 4.0;
/// A figure region is at least this wide and tall, in points.
const MIN_FIGURE_SIDE: f32 = 48.0;
/// An embedded image stands in for a figure only when both sides are at
/// least this long in points; smaller ones are logos and icons.
const MIN_IMAGE_SIDE: f32 = 96.0;
/// Drawings and text that lie wholly in this band at the top of the page,
/// in points, are the running header, not the figure.
const HEADER_BAND: f32 = 48.0;
/// A drawing covering this share of the page is a page background.
const BACKGROUND_SHARE: f32 = 0.8;
/// Text with this many words at the caption's size is body text, which a
/// figure never reaches past. Numbers, such as axis ticks, are not words.
const BODY_WORDS: usize = 7;

/// A rectangle in PDF user space: points, origin at the bottom left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Rect {
    fn from_oxide(rect: &pdf_oxide::geometry::Rect) -> Rect {
        let (xa, xb) = (rect.x, rect.x + rect.width);
        let (ya, yb) = (rect.y, rect.y + rect.height);
        Rect {
            x0: xa.min(xb),
            y0: ya.min(yb),
            x1: xa.max(xb),
            y1: ya.max(yb),
        }
    }

    pub fn width(&self) -> f32 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> f32 {
        self.y1 - self.y0
    }

    fn union(&self, other: &Rect) -> Rect {
        Rect {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    fn intersection(&self, other: &Rect) -> Rect {
        Rect {
            x0: self.x0.max(other.x0),
            y0: self.y0.max(other.y0),
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
        }
    }

    fn overlaps_x(&self, x0: f32, x1: f32) -> bool {
        self.x0 < x1 && self.x1 > x0
    }
}

/// How the figure region was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FigureSource {
    /// The drawings and images above the "Figure 1" caption.
    Caption,
    /// The largest embedded image in the first pages.
    LargestImage,
}

/// Where a paper's first figure is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FigureRegion {
    /// 0 based.
    pub page_index: usize,
    /// In PDF user space, inside the page's media box.
    pub rect: Rect,
    pub source: FigureSource,
}

impl PdfFile {
    /// The region of the first figure in the first `FIGURE_PAGES` pages, or
    /// `None` when there is neither a captioned figure nor a large image.
    pub fn first_figure(&self) -> Option<FigureRegion> {
        let doc = &self.doc;
        let pages = doc.page_count().unwrap_or(0).min(FIGURE_PAGES);
        for index in 0..pages {
            let segments = segments(doc, index);
            let Some(caption) = segments.iter().find(|s| CAPTION.is_match(&s.text)) else {
                continue;
            };
            if let Some(rect) = region_above(doc, index, caption, &segments) {
                return Some(FigureRegion {
                    page_index: index,
                    rect,
                    source: FigureSource::Caption,
                });
            }
            // The first figure's caption is found; a later one is not Figure 1.
            break;
        }
        largest_image(doc, pages)
    }
}

/// "Figure 1:", "Fig. 1.", "FIGURE 1 Overview", but not "Figure 1 shows",
/// "Fig. 1 (right)", or "Figure 10".
static CAPTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?i:fig(?:ure)?)\.?\s*1(?:\s*[:.|]|\s+\p{Lu})").unwrap());
/// A caption of another figure or table, which ends a figure above it.
static OTHER_CAPTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?i:fig(?:ure)?|table)\.?\s*\d+\s*[:.|]").unwrap());

/// A run of text spans on one baseline with no wide gap between them. Unlike
/// a whole line, it never joins text across columns.
#[derive(Debug, Clone)]
struct Segment {
    text: String,
    font_size: f32,
    rect: Rect,
}

fn segments(doc: &PdfDocument, index: usize) -> Vec<Segment> {
    let mut spans = doc.extract_spans(index).unwrap_or_default();
    spans.sort_by_key(|span| span.sequence);
    let mut segments: Vec<Segment> = Vec::new();
    for span in spans {
        let text = span.text.trim();
        if text.is_empty() {
            continue;
        }
        let rect = Rect::from_oxide(&span.bbox);
        let center = (rect.y0 + rect.y1) / 2.0;
        let joins = segments.last().is_some_and(|last| {
            let last_center = (last.rect.y0 + last.rect.y1) / 2.0;
            (last_center - center).abs() <= (rect.height() / 2.0).max(1.5)
                && rect.x0 >= last.rect.x0 - 1.0
                && rect.x0 - last.rect.x1 <= span.font_size * 1.5
        });
        if joins {
            let last = segments.last_mut().unwrap();
            last.text.push(' ');
            last.text.push_str(text);
            last.rect = last.rect.union(&rect);
            last.font_size = last.font_size.max(span.font_size);
        } else {
            segments.push(Segment {
                text: text.to_string(),
                font_size: span.font_size,
                rect,
            });
        }
    }
    segments
}

/// The drawings, images, and labels stacked directly above the caption, as
/// one padded rectangle. Body text or another caption above ends the figure.
fn region_above(
    doc: &PdfDocument,
    index: usize,
    caption: &Segment,
    segments: &[Segment],
) -> Option<Rect> {
    let page = Rect::from_media_box(doc.get_page_media_box(index).ok()?);
    let page_area = page.width() * page.height();
    let top_of_caption = caption.rect.y1;

    // (rect, is a drawing or image)
    let mut parts: Vec<(Rect, bool)> = Vec::new();
    for path in doc.extract_paths(index).unwrap_or_default() {
        // White fills without a stroke draw nothing on a white page; tools
        // put them behind whole figures and pages.
        let white = |c: &pdf_oxide::layout::Color| c.r.min(c.g).min(c.b) > 0.98;
        let visible = path.has_stroke() || path.fill_color.as_ref().is_some_and(|c| !white(c));
        let rect = Rect::from_oxide(&path.bbox);
        if visible && rect.width() * rect.height() < BACKGROUND_SHARE * page_area {
            parts.push((rect, true));
        }
    }
    for image in doc.page_image_handles(index).unwrap_or_default() {
        parts.push((Rect::from_oxide(&image.bbox), true));
    }
    let mut stops: Vec<Rect> = Vec::new();
    for segment in segments {
        let words = segment
            .text
            .split_whitespace()
            .filter(|word| word.chars().any(char::is_alphabetic))
            .count();
        let body = words >= BODY_WORDS && segment.font_size >= caption.font_size * 0.9;
        if body || OTHER_CAPTION.is_match(&segment.text) {
            stops.push(segment.rect);
        } else if segment.rect.height() <= segment.font_size * 2.0 {
            // Taller text is turned sideways, such as the arXiv stamp.
            parts.push((segment.rect, false));
        }
    }
    parts.retain(|(rect, _)| rect.y0 >= top_of_caption - 2.0 && rect.y0 < page.y1 - HEADER_BAND);
    parts.sort_by(|a, b| a.0.y0.total_cmp(&b.0.y0));

    // Grow upward from the caption. The figure's width starts as the
    // caption's and widens with each part taken, so the parts are walked
    // again until the width settles.
    let (mut x0, mut x1) = (caption.rect.x0, caption.rect.x1);
    loop {
        let ceiling = stops
            .iter()
            .filter(|stop| stop.y0 > top_of_caption && stop.overlaps_x(x0, x1))
            .map(|stop| stop.y0)
            .fold(page.y1, f32::min);
        let mut region = Rect {
            x0,
            y0: top_of_caption,
            x1,
            y1: top_of_caption,
        };
        let mut drawn = false;
        for (rect, graphic) in &parts {
            if rect.y0 > region.y1 + PART_GAP {
                break;
            }
            if rect.y1 > ceiling + 1.0 || !rect.overlaps_x(region.x0, region.x1) {
                continue;
            }
            // Labels make the figure taller but never wider, so text in the
            // margin, such as the arXiv stamp, stays out.
            region.y1 = region.y1.max(rect.y1);
            if *graphic {
                region.x0 = region.x0.min(rect.x0);
                region.x1 = region.x1.max(rect.x1);
                drawn = true;
            }
        }
        if (region.x0, region.x1) == (x0, x1) {
            let height = region.y1 - top_of_caption;
            if !drawn || height < MIN_FIGURE_SIDE || region.width() < MIN_FIGURE_SIDE {
                return None;
            }
            let padded = Rect {
                x0: region.x0 - PADDING,
                y0: top_of_caption,
                x1: region.x1 + PADDING,
                y1: region.y1 + PADDING,
            };
            return Some(padded.intersection(&page));
        }
        (x0, x1) = (region.x0, region.x1);
    }
}

/// The largest embedded image on the first `pages` pages that is not a logo.
fn largest_image(doc: &PdfDocument, pages: usize) -> Option<FigureRegion> {
    let mut best: Option<(f32, FigureRegion)> = None;
    for index in 0..pages {
        let Ok((x0, y0, x1, y1)) = doc.get_page_media_box(index) else {
            continue;
        };
        let page = Rect::from_media_box((x0, y0, x1, y1));
        for image in doc.page_image_handles(index).unwrap_or_default() {
            let rect = Rect::from_oxide(&image.bbox).intersection(&page);
            if rect.width() < MIN_IMAGE_SIDE || rect.height() < MIN_IMAGE_SIDE {
                continue;
            }
            let area = rect.width() * rect.height();
            if best.as_ref().is_none_or(|(largest, _)| area > *largest) {
                best = Some((
                    area,
                    FigureRegion {
                        page_index: index,
                        rect,
                        source: FigureSource::LargestImage,
                    },
                ));
            }
        }
    }
    best.map(|(_, region)| region)
}

impl Rect {
    fn from_media_box((x0, y0, x1, y1): (f32, f32, f32, f32)) -> Rect {
        Rect {
            x0: x0.min(x1),
            y0: y0.min(y1),
            x1: x0.max(x1),
            y1: y0.max(y1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_figure_is_the_drawing_stacked_on_its_caption() {
        let mut builder = pdf_oxide::writer::DocumentBuilder::new();
        builder
            .letter_page()
            .font("Helvetica", 10.0)
            // A sentence that starts with "Figure 1" is not its caption.
            .at(100.0, 300.0)
            .text("Figure 1 shows the model, which we now describe in more detail.")
            // Body text close above the figure, and a drawing above that.
            .filled_rect(100.0, 635.0, 400.0, 40.0, 0.2, 0.3, 0.5)
            .at(100.0, 615.0)
            .text("The previous section ends here with a sentence of body text.")
            // The figure: a box, a wider bar 10 pt above it, and a label.
            .filled_rect(220.0, 460.0, 160.0, 100.0, 0.8, 0.5, 0.2)
            .filled_rect(150.0, 570.0, 300.0, 30.0, 0.2, 0.6, 0.4)
            .at(270.0, 505.0)
            .text("Encoder")
            .at(240.0, 440.0)
            .text("Figure 1: The model.")
            .done();
        let file = PdfFile::open(builder.build().unwrap()).unwrap();
        let figure = file.first_figure().expect("a figure");

        assert_eq!(figure.page_index, 0);
        assert_eq!(figure.source, FigureSource::Caption);
        let rect = figure.rect;
        // The bar is wider than the caption and the box, and the figure
        // takes its width.
        assert!((rect.x0 - (150.0 - PADDING)).abs() < 1.0, "{rect:?}");
        assert!((rect.x1 - (450.0 + PADDING)).abs() < 1.0, "{rect:?}");
        // It starts at the caption and ends at the bar, below the body text.
        assert!(rect.y0 > 440.0 && rect.y0 < 460.0, "{rect:?}");
        assert!((rect.y1 - (600.0 + PADDING)).abs() < 1.0, "{rect:?}");
    }
}

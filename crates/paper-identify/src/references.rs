//! Reads a paper's reference list and its in-text citations from the file.
//!
//! `PdfFile::references` finds the last references heading, splits the
//! lines after it into entries, and collects what each entry prints or links
//! to: DOIs, an arXiv ID, a URL. It also follows every internal link of the
//! file, such as a citation "[12]" that jumps to its entry, and tells which
//! entry each one lands on. Nothing here touches the network; the engine
//! resolves the entries that print no identifier.
//!
//! How each piece was chosen, and how well it works on real papers, is in
//! `docs/research/references.md`.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::LazyLock;

use pdf_oxide::PdfDocument;
use pdf_oxide::object::{Object, ObjectRef};
use regex::Regex;

use crate::{PdfFile, Rect, arxiv_ids_in, clean_doi, dois_in, placed_lines};

/// Running heads and page numbers sit in these bands at the top and the
/// bottom of a page, in points. A line wholly inside one with few words is
/// not part of an entry; PLOS prints the paper's own DOI there.
const TOP_BAND: f32 = 50.0;
const BOTTOM_BAND: f32 = 45.0;
const BAND_WORDS: usize = 12;
/// A link target farther than this, in points, from every line of the
/// references names none of them.
const MAX_TARGET_DISTANCE: f32 = 30.0;
/// The largest offset `target_offset` considers, in points.
const MAX_OFFSET: i32 = 300;
/// A heading after the references is set at least this much larger than
/// the entries.
const HEADING_SCALE: f32 = 1.12;
/// Lines of one entry are at most this many font sizes apart.
const FLOW_GAP: f32 = 3.0;

/// A paper's references and the citations that point at them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReferenceList {
    /// In the order the paper prints them.
    pub entries: Vec<ReferenceEntry>,
    /// Internal links in the body that land on an entry.
    pub citations: Vec<CitationMark>,
}

/// One entry of the reference list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReferenceEntry {
    /// The printed number, such as 12 for "[12]" or "12.". `None` in
    /// author-year lists.
    pub number: Option<u32>,
    /// The entry as one paragraph, without its number, with words broken
    /// at line ends joined again.
    pub text: String,
    /// Where the entry is printed: one rectangle per page and column it
    /// covers.
    pub parts: Vec<EntryPart>,
    /// DOIs the entry links to or prints, links first.
    pub dois: Vec<String>,
    pub arxiv_id: Option<String>,
    /// A web address the entry links to or prints, other than a DOI or
    /// arXiv link.
    pub url: Option<String>,
}

/// A rectangle of an entry on one page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntryPart {
    /// 0 based.
    pub page_index: usize,
    /// In PDF user space.
    pub rect: Rect,
}

/// An in-text citation: a link in the body whose target is an entry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CitationMark {
    /// 0 based.
    pub page_index: usize,
    /// The link's rectangle in PDF user space.
    pub rect: Rect,
    /// Index into `ReferenceList::entries`.
    pub entry: usize,
}

impl PdfFile {
    /// The paper's reference list and the citations that link to it, or
    /// `None` when no references heading is found.
    pub fn references(&self) -> Option<ReferenceList> {
        let doc = &self.doc;
        let pages = page_objects(doc);
        let mut lines_of: HashMap<usize, Vec<crate::PlacedLine>> = HashMap::new();
        let mut heading = None;
        for index in (0..pages.len()).rev() {
            let lines = placed_lines(doc, index);
            let found = lines
                .iter()
                .rposition(|line| HEADING.is_match(line.text.trim()));
            lines_of.insert(index, lines);
            if let Some(line) = found {
                heading = Some((index, line));
                break;
            }
        }
        let (heading_page, heading_line) = heading?;

        let mut lines = Vec::new();
        for index in heading_page..pages.len() {
            let page_lines = lines_of
                .remove(&index)
                .unwrap_or_else(|| placed_lines(doc, index));
            let skip = if index == heading_page {
                heading_line + 1
            } else {
                0
            };
            let media = doc
                .get_page_media_box(index)
                .map(Rect::from_media_box)
                .unwrap_or(Rect {
                    x0: 0.0,
                    y0: 0.0,
                    x1: 612.0,
                    y1: 792.0,
                });
            let mut page_lines: Vec<_> = page_lines
                .into_iter()
                .skip(skip)
                .filter(|line| !in_margin_band(line, &media))
                .collect();
            if index > heading_page {
                drop_running_head(&mut page_lines, &media);
            }
            drop_trailing_head(&mut page_lines, &media);
            let mut page_lines: Vec<RefLine> = page_lines
                .into_iter()
                .map(|line| RefLine::new(index, line, &media))
                .collect();
            // On a page with one column, a short line is not a left column.
            if !page_lines.iter().any(|line| line.column == 1) {
                page_lines.iter_mut().for_each(|line| line.column = 2);
            }
            lines.extend(page_lines);
        }
        truncate_at_end(&mut lines);
        if lines.is_empty() {
            return None;
        }

        let links = read_links(doc, &pages);
        let targets: Vec<&Target> = links
            .iter()
            .filter_map(|link| match &link.target {
                LinkTarget::Internal(target) => Some(target),
                LinkTarget::Uri(_) => None,
            })
            .collect();
        let layout_starts: Vec<usize> = split(&lines, &HashSet::new())
            .iter()
            .map(|s| s.lines.start)
            .collect();
        let offset = target_offset(&lines, &layout_starts, &targets);
        let anchors: Vec<Option<usize>> = links
            .iter()
            .map(|link| match &link.target {
                LinkTarget::Internal(target) => anchor_line(&lines, target, offset),
                LinkTarget::Uri(_) => None,
            })
            .collect();
        let forced: HashSet<usize> = anchors.iter().flatten().copied().collect();
        let numbered_anchors: Vec<(usize, u32)> = links
            .iter()
            .zip(&anchors)
            .filter_map(|(link, anchor)| match &link.target {
                LinkTarget::Internal(target) => Some(((*anchor)?, target.number?)),
                LinkTarget::Uri(_) => None,
            })
            .collect();
        let mut segments = split(&lines, &forced);
        if !segments.iter().any(|s| s.number.is_some())
            && let Some(by_destination) = split_by_destinations(&lines, &numbered_anchors)
        {
            segments = by_destination;
        }

        let mut entries: Vec<ReferenceEntry> = segments
            .iter()
            .map(|s| entry_from(&lines[s.lines.clone()], s.number))
            .collect();
        let entry_of_line = |line: usize| segments.iter().position(|s| s.lines.contains(&line));
        let numbered = segments.iter().any(|s| s.number.is_some());

        // Links inside an entry: a DOI or arXiv link names the work cited.
        for link in &links {
            let LinkTarget::Uri(uri) = &link.target else {
                continue;
            };
            let Some(entry) = entries.iter_mut().find(|entry| {
                entry
                    .parts
                    .iter()
                    .any(|part| part.page_index == link.page && overlaps(&part.rect, &link.rect))
            }) else {
                continue;
            };
            add_uri(entry, uri);
        }

        let mut citations = Vec::new();
        for (link, anchor) in links.iter().zip(&anchors) {
            let LinkTarget::Internal(target) = &link.target else {
                continue;
            };
            // Links inside the list point elsewhere, such as back to the body.
            if lines
                .iter()
                .any(|line| line.page == link.page && overlaps(&line.rect, &link.rect))
            {
                continue;
            }
            let by_number = target
                .number
                .filter(|_| numbered)
                .and_then(|n| segments.iter().position(|s| s.number == Some(n)));
            let entry = by_number.or_else(|| anchor.and_then(entry_of_line));
            if let Some(entry) = entry {
                citations.push(CitationMark {
                    page_index: link.page,
                    rect: link.rect,
                    entry,
                });
            }
        }
        Some(ReferenceList { entries, citations })
    }
}

/// A line of the references with its page and column.
#[derive(Debug, Clone, PartialEq)]
struct RefLine {
    page: usize,
    text: String,
    font_size: f32,
    rect: Rect,
    /// 0 left, 1 right, 2 the whole width.
    column: u8,
}

impl RefLine {
    fn new(page: usize, line: crate::PlacedLine, media: &Rect) -> RefLine {
        let middle = (media.x0 + media.x1) / 2.0;
        let column = if line.rect.x1 <= middle + 5.0 {
            0
        } else if line.rect.x0 >= middle - 5.0 {
            1
        } else {
            2
        };
        RefLine {
            page,
            text: line.text.replace(['\u{200b}', '\u{00ad}', '\u{feff}'], ""),
            font_size: line.font_size,
            rect: line.rect,
            column,
        }
    }
}

/// Drops a running head that sits lower than the margin band, as ACM's
/// does: the topmost line of the page, in its top eighth, carrying a page
/// number.
fn drop_running_head(lines: &mut Vec<crate::PlacedLine>, media: &Rect) {
    let Some(top) = lines
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.rect.y1.total_cmp(&b.1.rect.y1))
        .map(|(index, _)| index)
    else {
        return;
    };
    let line = &lines[top];
    let words: Vec<&str> = line.text.split_whitespace().collect();
    let high = line.rect.y0 > media.y1 - media.height() / 8.0;
    let page_number = words
        .iter()
        .any(|word| word.len() <= 4 && word.chars().all(|c| c.is_ascii_digit()));
    if high && page_number && words.len() <= 15 && !END.is_match(&line.text) {
        lines.remove(top);
    }
}

/// Drops a running head that the file draws last, as PLOS does: a short
/// last line in the top eighth of the page, above the line before it. It
/// would otherwise join the page's last entry.
fn drop_trailing_head(lines: &mut Vec<crate::PlacedLine>, media: &Rect) {
    let [.., before, last] = lines.as_slice() else {
        return;
    };
    let high = last.rect.y0 > media.y1 - media.height() / 8.0;
    let short = last.text.split_whitespace().count() <= 15;
    if high && short && last.rect.y0 > before.rect.y1 && !END.is_match(&last.text) {
        lines.pop();
    }
}

fn in_margin_band(line: &crate::PlacedLine, media: &Rect) -> bool {
    let few_words = line.text.split_whitespace().count() <= BAND_WORDS;
    if few_words && (line.rect.y0 > media.y1 - TOP_BAND || line.rect.y1 < media.y0 + BOTTOM_BAND) {
        return true;
    }
    // A page number alone, set lower or higher than the bands.
    let text = line.text.trim();
    let page_number =
        !text.is_empty() && text.len() <= 4 && text.chars().all(|c| c.is_ascii_digit());
    let eighth = media.height() / 8.0;
    page_number && (line.rect.y0 > media.y1 - eighth || line.rect.y1 < media.y0 + eighth)
}

/// Drops what follows the list: an appendix, acknowledgments, a publisher's
/// note, or any heading set larger than the entries.
fn truncate_at_end(lines: &mut Vec<RefLine>) {
    let mut sizes: Vec<f32> = lines.iter().take(12).map(|line| line.font_size).collect();
    sizes.sort_by(f32::total_cmp);
    let typical = sizes.get(sizes.len() / 2).copied().unwrap_or(0.0);
    let end = lines.iter().position(|line| {
        let text = line.text.trim();
        let larger = typical > 0.0
            && line.font_size >= typical * HEADING_SCALE
            && text.split_whitespace().count() <= 10;
        larger || END.is_match(text)
    });
    if let Some(end) = end {
        lines.truncate(end);
    }
}

/// An entry and the lines it spans.
#[derive(Debug, Clone, PartialEq)]
struct Segment {
    number: Option<u32>,
    lines: Range<usize>,
}

/// Splits the lines into entries. A numbered list splits at its numbers,
/// which must count up. Otherwise an entry starts where a link lands, at a
/// hanging indent's left edge, or after a wider gap than between the lines
/// of one entry.
fn split(lines: &[RefLine], forced: &HashSet<usize>) -> Vec<Segment> {
    let mut starts: Vec<(usize, u32)> = Vec::new();
    let mut marked = 0;
    for (index, line) in lines.iter().enumerate() {
        let Some(number) = entry_number(&line.text).filter(|n| *n < 1000) else {
            continue;
        };
        marked += 1;
        let follows = match starts.last() {
            None => number <= 5,
            Some(&(_, last)) => number > last && number <= last + 2,
        };
        if follows {
            starts.push((index, number));
        }
    }
    // A font the text layer decodes wrongly turns numbers into other digits,
    // so most marks stop counting up; the layout splits that list instead.
    if starts.len() >= 3 && starts.len() * 10 >= marked * 7 {
        return starts
            .iter()
            .enumerate()
            .map(|(i, &(start, number))| Segment {
                number: Some(number),
                lines: start..starts.get(i + 1).map_or(lines.len(), |next| next.0),
            })
            .collect();
    }

    let groups = column_groups(lines);
    let first_line_out = first_lines_stand_out(lines, &groups, forced);
    let mut segments: Vec<Segment> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let start = index == 0 || forced.contains(&index) || {
            let previous = &lines[index - 1];
            let group = &groups[&(line.page, line.column)];
            let same_group = previous.page == line.page && previous.column == line.column;
            let ends_sentence = previous.text.trim_end().ends_with(['.', ')']);
            if group.hanging {
                group.at_left(line) == first_line_out
            } else if same_group {
                let pitch = previous.rect.y0 - line.rect.y0;
                let short = previous.rect.width() < group.width * 0.8;
                pitch > group.pitch * 1.3 || (short && ends_sentence)
            } else {
                ends_sentence && line.text.starts_with(char::is_uppercase)
            }
        };
        match segments.last_mut() {
            Some(segment) if !start => segment.lines.end = index + 1,
            _ => segments.push(Segment {
                number: None,
                lines: index..index + 1,
            }),
        }
    }
    segments
}

/// Splits a numbered list whose digits the text layer garbles, as in some
/// PLOS files where "19." reads "3;.". The destination names still carry
/// the numbers, so an entry starts where a numbered link lands, and also
/// at each line that starts with a mark shaped like a number. A mark
/// takes the number after the one before it, unless that number belongs
/// to a later anchor, in which case the line joins the entry above.
/// `None` when the anchors do not show such a list.
fn split_by_destinations(lines: &[RefLine], anchors: &[(usize, u32)]) -> Option<Vec<Segment>> {
    // A target clamped to the page top lands several numbers on one line;
    // such a line names none of them.
    let mut numbers_of: HashMap<usize, HashSet<u32>> = HashMap::new();
    let mut line_of: HashMap<u32, usize> = HashMap::new();
    for &(line, number) in anchors {
        numbers_of.entry(line).or_default().insert(number);
        line_of.entry(number).or_insert(line);
    }
    let mut known: Vec<(u32, usize)> = line_of
        .into_iter()
        .filter(|(_, line)| numbers_of[line].len() == 1)
        .collect();
    known.sort();
    let known = increasing_lines(&known);
    let shaped = |line: &RefLine| NUMBER_SHAPE.is_match(&line.text);
    let shaped_known = known
        .iter()
        .filter(|(_, line)| shaped(&lines[*line]))
        .count();
    if known.len() < 3 || shaped_known * 10 < known.len() * 8 {
        return None;
    }

    let number_at: HashMap<usize, u32> = known.iter().map(|&(n, line)| (line, n)).collect();
    let mut segments: Vec<Segment> = Vec::new();
    let mut previous = 0;
    for (index, line) in lines.iter().enumerate() {
        let number = match number_at.get(&index) {
            Some(&number) => Some(number),
            None if shaped(line) => {
                let next_known = known.iter().find(|(_, at)| *at > index).map(|(n, _)| *n);
                Some(previous + 1).filter(|n| next_known.is_none_or(|next| *n < next))
            }
            None => None,
        };
        match (number, segments.last_mut()) {
            (Some(number), _) => {
                previous = number;
                segments.push(Segment {
                    number: Some(number),
                    lines: index..index + 1,
                });
            }
            (None, Some(segment)) => segment.lines.end = index + 1,
            // Lines before the first mark belong to the first entry.
            (None, None) => {}
        }
    }
    if let Some(first) = segments.first_mut() {
        first.lines.start = 0;
    }
    Some(segments)
}

/// The longest run of `(number, line)` pairs, sorted by number, whose lines
/// also go down the list. An anchor out of order landed on a wrong line.
fn increasing_lines(pairs: &[(u32, usize)]) -> Vec<(u32, usize)> {
    let mut best: Vec<(usize, Option<usize>)> = Vec::with_capacity(pairs.len());
    for (i, &(_, line)) in pairs.iter().enumerate() {
        let before = (0..i)
            .filter(|&j| pairs[j].1 < line)
            .max_by_key(|&j| best[j].0);
        best.push(match before {
            Some(j) => (best[j].0 + 1, Some(j)),
            None => (1, None),
        });
    }
    let mut at = (0..pairs.len()).max_by_key(|&i| best[i].0);
    let mut run = Vec::new();
    while let Some(i) = at {
        run.push(pairs[i]);
        at = best[i].1;
    }
    run.reverse();
    run
}

/// Whether entries start at the left edge and go on indented (a hanging
/// indent), rather than the other way round as Frontiers sets them. Lines
/// that links land on decide; without links, entries start with a capital
/// or their number more often than their other lines do.
fn first_lines_stand_out(
    lines: &[RefLine],
    groups: &HashMap<(usize, u8), ColumnGroup>,
    forced: &HashSet<usize>,
) -> bool {
    let hanging: Vec<(usize, &RefLine, bool)> = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            let group = &groups[&(line.page, line.column)];
            group.hanging.then(|| (index, line, group.at_left(line)))
        })
        .collect();
    let forced_left = hanging
        .iter()
        .filter(|(i, _, left)| forced.contains(i) && *left)
        .count();
    let forced_indented = hanging
        .iter()
        .filter(|(i, _, left)| forced.contains(i) && !*left)
        .count();
    if forced_left + forced_indented > 0 {
        return forced_left >= forced_indented;
    }
    let capital_share = |left: bool| {
        let level: Vec<_> = hanging.iter().filter(|(_, _, l)| *l == left).collect();
        let capitals = level
            .iter()
            .filter(|(_, line, _)| {
                line.text.starts_with(char::is_uppercase) || NUMBER_MARK.is_match(&line.text)
            })
            .count();
        capitals as f32 / level.len().max(1) as f32
    };
    capital_share(true) >= capital_share(false)
}

/// How the lines of one column of one page are laid out.
struct ColumnGroup {
    /// Its widest line.
    width: f32,
    /// The usual distance from one baseline to the next.
    pitch: f32,
    /// Some lines start at `left` and others indented.
    hanging: bool,
    /// Lines left of this x are on the left level.
    split_at: f32,
}

impl ColumnGroup {
    fn at_left(&self, line: &RefLine) -> bool {
        line.rect.x0 < self.split_at
    }
}

fn column_groups(lines: &[RefLine]) -> HashMap<(usize, u8), ColumnGroup> {
    let mut by_group: HashMap<(usize, u8), Vec<&RefLine>> = HashMap::new();
    for line in lines {
        by_group
            .entry((line.page, line.column))
            .or_default()
            .push(line);
    }
    by_group
        .into_iter()
        .map(|(key, group)| {
            let left = group
                .iter()
                .map(|l| l.rect.x0)
                .fold(f32::INFINITY, f32::min);
            let width = group.iter().map(|l| l.rect.width()).fold(0.0, f32::max);
            let mut pitches: Vec<f32> = group
                .windows(2)
                .map(|pair| pair[0].rect.y0 - pair[1].rect.y0)
                .filter(|pitch| *pitch > 0.0 && *pitch < 40.0)
                .collect();
            pitches.sort_by(f32::total_cmp);
            let pitch = pitches
                .get(pitches.len() / 2)
                .copied()
                .unwrap_or(f32::INFINITY);
            // The indented level is the most common x right of the edge. Numbers
            // set flush right, as PLOS does, start "10." left of "9.", so the
            // levels split halfway rather than at the edge.
            let mut indents: HashMap<i32, usize> = HashMap::new();
            for line in &group {
                if line.rect.x0 > left + 2.0 && line.rect.x0 < left + 40.0 {
                    *indents.entry(line.rect.x0.round() as i32).or_default() += 1;
                }
            }
            let indent = indents
                .into_iter()
                .max_by_key(|&(x, count)| (count, -x))
                .map(|(x, _)| x as f32);
            let split_at = indent.map_or(left + 1.5, |indent| (left + indent) / 2.0);
            (
                key,
                ColumnGroup {
                    width,
                    pitch,
                    hanging: indent.is_some(),
                    split_at,
                },
            )
        })
        .collect()
}

/// The number an entry starts with: "[12]" or "12.".
fn entry_number(text: &str) -> Option<u32> {
    let captures = NUMBER_MARK.captures(text)?;
    captures.get(1).or(captures.get(2))?.as_str().parse().ok()
}

fn entry_from(lines: &[RefLine], number: Option<u32>) -> ReferenceEntry {
    let mut text = String::new();
    for line in lines {
        let piece = line.text.trim();
        let in_url = text
            .rsplit(' ')
            .next()
            .is_some_and(|word| word.contains('/'));
        if text.is_empty() {
            text.push_str(piece);
        } else if text.ends_with('-') && piece.starts_with(char::is_lowercase) && !in_url {
            // A word broken at the line end.
            text.pop();
            text.push_str(piece);
        } else if text.ends_with(['/', '-']) {
            text.push_str(piece);
        } else {
            text.push(' ');
            text.push_str(piece);
        }
    }
    if number.is_some() {
        let garbled = NUMBER_SHAPE.captures(&text).and_then(|c| c.get(1));
        text = match garbled {
            _ if NUMBER_MARK.is_match(&text) => NUMBER_MARK.replace(&text, "").into_owned(),
            Some(mark) => text[mark.end()..].to_string(),
            None => text,
        }
        .trim()
        .to_string();
    }
    let raw: String = lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    // Rectangles grow only with lines that go on down the column from the
    // last one kept. A footer, side note, or running head the text layer
    // put after the entry would otherwise stretch them across the page.
    let mut parts: Vec<EntryPart> = Vec::new();
    let mut last: Option<&RefLine> = None;
    for line in lines {
        let in_flow = last.is_none_or(|last| {
            last.page != line.page
                || !same_column(&last.rect, &line.rect)
                || (line.rect.y1 <= last.rect.y1 + 1.0
                    && last.rect.y0 - line.rect.y1 <= line.font_size.max(1.0) * FLOW_GAP)
        });
        if !in_flow {
            continue;
        }
        last = Some(line);
        match parts
            .iter_mut()
            .find(|part| part.page_index == line.page && same_column(&part.rect, &line.rect))
        {
            Some(part) => part.rect = part.rect.union(&line.rect),
            None => parts.push(EntryPart {
                page_index: line.page,
                rect: line.rect,
            }),
        }
    }

    let arxiv_id = arxiv_ids_in(&raw)
        .into_iter()
        .chain(
            ARXIV_FORMS
                .captures_iter(&text)
                .filter_map(|c| paper_ids::normalize_arxiv_id(&c[1])),
        )
        .next();
    let url = URL
        .find_iter(&text)
        .map(|found| {
            found
                .as_str()
                .trim_end_matches(['.', ',', ';', ')', ']'])
                .to_string()
        })
        .find(|url| !url.contains("doi.org/") && !url.contains("arxiv.org/"));
    ReferenceEntry {
        number,
        text,
        parts,
        dois: dois_in(&raw),
        arxiv_id,
        url,
    }
}

fn same_column(a: &Rect, b: &Rect) -> bool {
    a.x0 < b.x1 && b.x0 < a.x1
}

fn overlaps(a: &Rect, b: &Rect) -> bool {
    a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1
}

/// Takes what a link in an entry points at. A link counts before text, so
/// a DOI the text breaks badly is still right.
fn add_uri(entry: &mut ReferenceEntry, uri: &str) {
    if let Some((_, rest)) = uri.split_once("doi.org/") {
        let rest = percent_decode(rest);
        if let Some(doi) = clean_doi(&rest) {
            entry.dois.retain(|known| *known != doi);
            entry.dois.insert(0, doi);
        }
    } else if let Some(captures) = ARXIV_FORMS.captures(uri) {
        if let Some(id) = paper_ids::normalize_arxiv_id(&captures[1]) {
            entry.arxiv_id = Some(id);
        }
    } else if uri.starts_with("http") && entry.url.is_none() {
        entry.url = Some(uri.to_string());
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(value) = bytes
                .get(i + 1..i + 3)
                .and_then(|hex| std::str::from_utf8(hex).ok())
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(value);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Where an internal link lands.
#[derive(Debug, Clone, PartialEq)]
struct Target {
    page: usize,
    /// The left and top of the view the link asks for, when it says.
    x: Option<f32>,
    y: Option<f32>,
    /// The entry number in the destination's name, such as 12 in
    /// Springer's `CR12` or PLOS's `ref012`.
    number: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
enum LinkTarget {
    Internal(Target),
    Uri(String),
}

#[derive(Debug, Clone, PartialEq)]
struct Link {
    page: usize,
    rect: Rect,
    target: LinkTarget,
}

/// The line a link target lands on: the closest line top at or just below
/// the target, less the file's `offset`, in the column the target's left
/// edge is in. A left edge outside every line, such as PLOS's 0, says
/// nothing about the column.
fn anchor_line(lines: &[RefLine], target: &Target, offset: f32) -> Option<usize> {
    let y = target.y? - offset;
    let in_column = |x: f32, line: &RefLine| x >= line.rect.x0 - 60.0 && x <= line.rect.x1 + 5.0;
    let on_page = || {
        lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.page == target.page)
    };
    // Only a page with two columns needs the left edge to pick one.
    let two_columns = on_page().any(|(_, line)| line.column != 2);
    let x = target
        .x
        .filter(|&x| two_columns && on_page().any(|(_, line)| in_column(x, line)));
    on_page()
        .filter(|(_, line)| x.is_none_or(|x| in_column(x, line)))
        .map(|(index, line)| {
            let top = line.rect.y1;
            // Anchors usually sit a little above the entry's first line.
            let distance = if top <= y + 4.0 {
                y - top
            } else {
                (top - y) * 3.0
            };
            (index, distance)
        })
        .filter(|(_, distance)| *distance <= MAX_TARGET_DISTANCE)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

/// How far above the entries a file's link targets sit. Most land on the
/// entry or a few points above it, but PLOS puts every target 172 points
/// higher. The answer is the distance from targets to the entry starts the
/// layout finds that most targets share, the smallest one on a near tie.
fn target_offset(lines: &[RefLine], starts: &[usize], targets: &[&Target]) -> f32 {
    let mut support: HashMap<i32, HashSet<usize>> = HashMap::new();
    for (t, target) in targets.iter().enumerate() {
        let Some(y) = target.y else { continue };
        for &start in starts {
            let line = &lines[start];
            if line.page != target.page {
                continue;
            }
            let distance = (y - line.rect.y1).round() as i32;
            if (-4..=MAX_OFFSET).contains(&distance) {
                // A target counts for offsets within a few points of its own.
                for near in distance - 4..=distance + 4 {
                    support.entry(near).or_default().insert(t);
                }
            }
        }
    }
    let best = support.values().map(HashSet::len).max().unwrap_or(0);
    if best == 0 {
        return 0.0;
    }
    let near_best = |offset: &i32| support[offset].len() * 10 >= best * 9;
    let small = support
        .keys()
        .filter(|offset| near_best(offset))
        .min_by_key(|offset| offset.abs());
    // An offset within a line of the entry is the usual anchor; keep 0.
    match small {
        Some(&offset) if offset > MAX_TARGET_DISTANCE as i32 => offset as f32,
        _ => 0.0,
    }
}

/// Every page dictionary, in page order, with its object number.
fn page_objects(doc: &PdfDocument) -> Vec<(Option<ObjectRef>, HashMap<String, Object>)> {
    fn walk(
        doc: &PdfDocument,
        node: &Object,
        reference: Option<ObjectRef>,
        pages: &mut Vec<(Option<ObjectRef>, HashMap<String, Object>)>,
        depth: u8,
    ) {
        let Some(dict) = node.as_dict() else { return };
        if depth > 32 {
            return;
        }
        match dict.get("Kids").map(|kids| resolve(doc, kids)) {
            Some(Object::Array(kids)) => {
                for kid in kids {
                    let reference = kid.as_reference();
                    walk(doc, &resolve(doc, &kid), reference, pages, depth + 1);
                }
            }
            _ => pages.push((reference, dict.clone())),
        }
    }
    let mut pages = Vec::new();
    let Ok(catalog) = doc.catalog() else {
        return pages;
    };
    if let Some(root) = catalog.as_dict().and_then(|c| c.get("Pages")) {
        walk(doc, &resolve(doc, root), root.as_reference(), &mut pages, 0);
    }
    // A page tree this walk misread must not place links on wrong pages.
    if pages.len() != doc.page_count().unwrap_or(0) {
        pages.clear();
    }
    pages
}

fn resolve(doc: &PdfDocument, object: &Object) -> Object {
    match object {
        Object::Reference(reference) => doc.load_object(*reference).unwrap_or(Object::Null),
        other => other.clone(),
    }
}

fn number(object: &Object) -> Option<f32> {
    match object {
        Object::Integer(value) => Some(*value as f32),
        Object::Real(value) => Some(*value as f32),
        _ => None,
    }
}

/// The link annotations of every page.
fn read_links(
    doc: &PdfDocument,
    pages: &[(Option<ObjectRef>, HashMap<String, Object>)],
) -> Vec<Link> {
    let page_of: HashMap<u32, usize> = pages
        .iter()
        .enumerate()
        .filter_map(|(index, (reference, _))| Some((reference.as_ref()?.id, index)))
        .collect();
    let mut names: Option<HashMap<Vec<u8>, Object>> = None;
    let mut links = Vec::new();
    for (index, (_, page)) in pages.iter().enumerate() {
        let Some(Object::Array(annotations)) = page.get("Annots").map(|a| resolve(doc, a)) else {
            continue;
        };
        for annotation in annotations {
            let annotation = resolve(doc, &annotation);
            let Some(dict) = annotation.as_dict() else {
                continue;
            };
            if dict.get("Subtype").and_then(Object::as_name) != Some("Link") {
                continue;
            }
            let Some(rect) = dict
                .get("Rect")
                .map(|r| resolve(doc, r))
                .and_then(|r| rect_of(&r))
            else {
                continue;
            };
            let destination = match dict.get("A").map(|a| resolve(doc, a)) {
                Some(action) => {
                    let action = action.as_dict().cloned().unwrap_or_default();
                    match action.get("S").and_then(Object::as_name) {
                        Some("URI") => {
                            if let Some(uri) = action.get("URI").map(|u| resolve(doc, u))
                                && let Some(bytes) = uri.as_string()
                            {
                                links.push(Link {
                                    page: index,
                                    rect,
                                    target: LinkTarget::Uri(
                                        String::from_utf8_lossy(bytes).into_owned(),
                                    ),
                                });
                            }
                            continue;
                        }
                        Some("GoTo") => action.get("D").cloned(),
                        _ => None,
                    }
                }
                None => dict.get("Dest").cloned(),
            };
            let Some(destination) = destination else {
                continue;
            };
            if let Some(target) = target_of(doc, &destination, &page_of, &mut names) {
                links.push(Link {
                    page: index,
                    rect,
                    target: LinkTarget::Internal(target),
                });
            }
        }
    }
    links
}

fn rect_of(object: &Object) -> Option<Rect> {
    let values: Vec<f32> = object.as_array()?.iter().filter_map(number).collect();
    let [a, b, c, d] = values.as_slice() else {
        return None;
    };
    Some(Rect {
        x0: a.min(*c),
        y0: b.min(*d),
        x1: a.max(*c),
        y1: b.max(*d),
    })
}

fn target_of(
    doc: &PdfDocument,
    destination: &Object,
    page_of: &HashMap<u32, usize>,
    names: &mut Option<HashMap<Vec<u8>, Object>>,
) -> Option<Target> {
    let destination = resolve(doc, destination);
    let (name, array) = match &destination {
        Object::String(name) => (Some(name.clone()), lookup_name(doc, names, name)?),
        Object::Name(name) => (
            Some(name.as_bytes().to_vec()),
            lookup_name(doc, names, name.as_bytes())?,
        ),
        Object::Array(_) => (None, destination.clone()),
        Object::Dictionary(dict) => (None, resolve(doc, dict.get("D")?)),
        _ => return None,
    };
    let array = array.as_array()?;
    let page = match array.first()? {
        Object::Reference(reference) => *page_of.get(&reference.id)?,
        Object::Integer(index) => usize::try_from(*index).ok()?,
        _ => return None,
    };
    let at = |i: usize| array.get(i).and_then(number);
    let (x, y) = match array.get(1).and_then(Object::as_name) {
        Some("XYZ") => (at(2), at(3)),
        Some("FitH") | Some("FitBH") => (None, at(2)),
        Some("FitV") | Some("FitBV") => (at(2), None),
        Some("FitR") => (at(2), at(5)),
        _ => (None, None),
    };
    let number = name
        .map(|name| String::from_utf8_lossy(&name).into_owned())
        .and_then(|name| DESTINATION_NUMBER.captures(&name)?[1].parse().ok());
    Some(Target { page, x, y, number })
}

/// A named destination's value, from the catalog's `Dests` dictionary or
/// its `Names` tree. The tree is read once per file.
fn lookup_name(
    doc: &PdfDocument,
    names: &mut Option<HashMap<Vec<u8>, Object>>,
    name: &[u8],
) -> Option<Object> {
    let names = names.get_or_insert_with(|| named_destinations(doc));
    let value = resolve(doc, names.get(name)?);
    match &value {
        Object::Dictionary(dict) => dict.get("D").map(|d| resolve(doc, d)),
        _ => Some(value),
    }
}

fn named_destinations(doc: &PdfDocument) -> HashMap<Vec<u8>, Object> {
    fn walk(doc: &PdfDocument, node: &Object, out: &mut HashMap<Vec<u8>, Object>, depth: u8) {
        let Some(dict) = node.as_dict() else { return };
        if depth > 32 {
            return;
        }
        if let Some(Object::Array(pairs)) = dict.get("Names").map(|n| resolve(doc, n)) {
            for pair in pairs.chunks(2) {
                if let [key, value] = pair
                    && let Some(key) = resolve(doc, key).as_string()
                {
                    out.insert(key.to_vec(), value.clone());
                }
            }
        }
        if let Some(Object::Array(kids)) = dict.get("Kids").map(|k| resolve(doc, k)) {
            for kid in kids {
                walk(doc, &resolve(doc, &kid), out, depth + 1);
            }
        }
    }
    let mut out = HashMap::new();
    let Ok(catalog) = doc.catalog() else {
        return out;
    };
    let Some(catalog) = catalog.as_dict() else {
        return out;
    };
    if let Some(Object::Dictionary(dests)) = catalog.get("Dests").map(|d| resolve(doc, d)) {
        for (key, value) in dests {
            out.insert(key.as_bytes().to_vec(), value);
        }
    }
    if let Some(tree) = catalog
        .get("Names")
        .map(|n| resolve(doc, n))
        .and_then(|names| names.as_dict()?.get("Dests").cloned())
    {
        walk(doc, &resolve(doc, &tree), &mut out, 0);
    }
    out
}

static HEADING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(?:\d+\.?|[IVX]+\.)?\s*(?:references|bibliography|literature cited|works cited|reference list)$",
    )
    .unwrap()
});
/// What may follow a reference list.
static END: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(?:[A-Z]\.?\s+)?(?:appendix|appendices|supplementary (?:material|information)|acknowledge?ments?|publisher[’']s note|author contributions?|competing interests|conflicts? of interest|data availability|checklist)\b",
    )
    .unwrap()
});
static NUMBER_MARK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(?:\[(\d{1,4})\]|(\d{1,4})\.(?:\s|$))\s*").unwrap());
/// A number mark whose digits may be garbled: one to three characters, a
/// period, and a capital, such as "3;. Garcka". The mark is group 1.
static NUMBER_SHAPE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\s*[^\s.]{1,3}\.\s+)\p{Lu}").unwrap());
/// arXiv IDs as links and CoRR print them.
static ARXIV_FORMS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:arxiv\.org/(?:abs|pdf)/|\bCoRR,?\s*abs/)(\d{4}\.\d{4,5}(?:v\d+)?)").unwrap()
});
static URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"https?://[^\s<>"]+"#).unwrap());
/// Entry numbers in publishers' destination names: `…:CR12:99`, `…ref012`,
/// `cite.ref-12`, `bib12`, `CIT0012`.
static DESTINATION_NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[^A-Za-z])(?:CR|ref|bib|CIT)[-_]?0*(\d{1,4})(?::\d+)?$").unwrap()
});

#[cfg(test)]
mod tests {
    use super::*;

    fn line(page: usize, x0: f32, top: f32, text: &str) -> RefLine {
        RefLine {
            page,
            text: text.to_string(),
            font_size: 9.0,
            rect: Rect {
                x0,
                y0: top - 9.0,
                x1: 280.0,
                y1: top,
            },
            column: 0,
        }
    }

    #[test]
    fn a_numbered_list_splits_only_where_the_numbers_count_up() {
        let lines = vec![
            line(
                0,
                50.0,
                700.0,
                "[1] A. Beck. First-order methods in optimization. SIAM,",
            ),
            line(0, 60.0, 689.0, "2017."),
            line(
                0,
                50.0,
                678.0,
                "[2] H. Le and Y. Yue. Batch policy learning. In ICML,",
            ),
            // A year at a line start looks like a number but does not count up.
            line(0, 60.0, 667.0, "2019. URL https://example.org/a-"),
            line(0, 60.0, 656.0, "long/path."),
            line(0, 50.0, 645.0, "[3] C. Jin. Is Q-learning provably efﬁ-"),
            line(0, 60.0, 634.0, "cient? In NeurIPS, 2018. arXiv:1807.03765."),
        ];
        let segments = split(&lines, &HashSet::new());
        let numbers: Vec<_> = segments.iter().map(|s| s.number).collect();
        assert_eq!(numbers, vec![Some(1), Some(2), Some(3)]);
        let second = entry_from(&lines[segments[1].lines.clone()], Some(2));
        assert_eq!(
            second.text,
            "H. Le and Y. Yue. Batch policy learning. In ICML, 2019. URL https://example.org/a-long/path."
        );
        assert_eq!(
            second.url.as_deref(),
            Some("https://example.org/a-long/path")
        );
        let third = entry_from(&lines[segments[2].lines.clone()], Some(3));
        assert!(third.text.contains("provably efﬁcient?"), "{}", third.text);
        assert_eq!(third.arxiv_id.as_deref(), Some("1807.03765"));
    }

    #[test]
    fn a_list_with_garbled_numbers_splits_where_numbered_links_land() {
        // The text layer reads "1." as "3." and "9." as ";.", so the numbers
        // do not count up; the destination names still say 1, 2, and 3.
        let lines = vec![
            line(
                0,
                55.0,
                700.0,
                "3. Cloutker L. Sktuatkon rersonnelle. Tjesks; 2223.",
            ),
            line(
                0,
                55.0,
                689.0,
                "2. Berkman LF. Workkng longer kn tje US. RMKD: 3",
            ),
            line(0, 65.0, 678.0, "jttrs://dok.org/32.3232/abc"),
            line(
                0,
                55.0,
                667.0,
                ";. Cndor L. Emrlo{ment Trends. Eurorean Foundatkon;",
            ),
            line(0, 65.0, 656.0, "2223. Clso a lkne. Tjat looks lkke a mark."),
            line(
                0,
                50.0,
                645.0,
                "32. Larsson L. Sarcorenka. Rj{skol Rev. 2236.",
            ),
        ];
        assert!(
            split(&lines, &HashSet::new())
                .iter()
                .all(|s| s.number.is_none())
        );
        // Number 4 lands at the page top with 5, so that line names neither.
        let anchors = [(0, 1), (1, 2), (3, 3), (5, 4), (5, 5)];
        let segments = split_by_destinations(&lines, &anchors).unwrap();
        let found: Vec<_> = segments
            .iter()
            .map(|s| (s.number, s.lines.clone()))
            .collect();
        assert_eq!(
            found,
            vec![
                (Some(1), 0..1),
                (Some(2), 1..3),
                (Some(3), 3..5),
                (Some(4), 5..6)
            ]
        );
        let third = entry_from(&lines[3..5], Some(3));
        assert!(third.text.starts_with("Cndor L."), "{}", third.text);
    }

    #[test]
    fn a_running_head_drawn_after_the_last_entry_does_not_stretch_its_rectangle() {
        let lines = vec![
            line(
                0,
                50.0,
                120.0,
                "Mwanza JC, Oakley JD. Macular ganglion cell-inner plexiform",
            ),
            line(0, 60.0, 109.0, "layer. Invest Ophthalmol Vis Sci. 2011."),
            // PLOS draws the page's running head last, at the top.
            line(
                0,
                50.0,
                750.0,
                "Analysis of retinal layers in glaucoma patients",
            ),
        ];
        let entry = entry_from(&lines, None);
        let tops: Vec<f32> = entry.parts.iter().map(|part| part.rect.y1).collect();
        assert_eq!(tops, vec![120.0]);
    }

    #[test]
    fn an_author_year_list_splits_at_its_hanging_indent() {
        let lines = vec![
            line(
                0,
                50.0,
                700.0,
                "Ganin, Y. and Lempitsky, V. Unsupervised domain adaptation",
            ),
            line(0, 58.0, 689.0, "by backpropagation. In ICML, 2015."),
            line(
                0,
                50.0,
                678.0,
                "He, K., Zhang, X., Ren, S., and Sun, J. Deep residual",
            ),
            line(
                0,
                58.0,
                667.0,
                "learning for image recognition. In CVPR, 2016. doi:10.1109/",
            ),
            line(0, 58.0, 656.0, "cvpr.2016.90."),
            line(
                0,
                50.0,
                645.0,
                "Hull, J. J. A database for handwritten text recognition. PAMI, 1994.",
            ),
        ];
        let segments = split(&lines, &HashSet::new());
        let ranges: Vec<_> = segments.iter().map(|s| s.lines.clone()).collect();
        assert_eq!(ranges, vec![0..2, 2..5, 5..6]);
        let he = entry_from(&lines[2..5], None);
        assert_eq!(he.dois, vec!["10.1109/cvpr.2016.90".to_string()]);
    }

    #[test]
    fn a_list_without_indents_splits_at_wider_gaps_and_link_targets() {
        let lines = vec![
            line(
                0,
                50.0,
                700.0,
                "Yaroslav Ganin and Victor Lempitsky. Unsupervised domain",
            ),
            line(
                0,
                50.0,
                689.0,
                "adaptation by backpropagation. In ICML, 2015",
            ),
            line(
                0,
                50.0,
                674.0,
                "Kaiming He, Xiangyu Zhang, Shaoqing Ren, and Jian Sun. Deep",
            ),
            line(
                0,
                50.0,
                663.0,
                "residual learning for image recognition. In CVPR, 2016",
            ),
            line(
                0,
                50.0,
                652.0,
                "Jonathan J. Hull. A database for handwritten text recognition",
            ),
            line(0, 50.0, 641.0, "research. PAMI, 1994"),
        ];
        // The third entry follows without a gap; only its link target marks it.
        let forced = HashSet::from([4]);
        let segments = split(&lines, &forced);
        let ranges: Vec<_> = segments.iter().map(|s| s.lines.clone()).collect();
        assert_eq!(ranges, vec![0..2, 2..4, 4..6]);
    }

    #[test]
    fn a_link_target_lands_on_the_line_just_below_it_in_its_column() {
        let mut lines = vec![
            line(3, 50.0, 700.0, "[1] First entry"),
            line(3, 50.0, 689.0, "[2] Second entry"),
            line(3, 320.0, 700.0, "[30] Right column entry"),
        ];
        lines[2].rect.x1 = 560.0;
        let target = |x, y| Target {
            page: 3,
            x: Some(x),
            y: Some(y),
            number: None,
        };
        assert_eq!(anchor_line(&lines, &target(50.0, 691.0), 0.0), Some(1));
        assert_eq!(anchor_line(&lines, &target(318.0, 702.0), 0.0), Some(2));
        assert_eq!(
            anchor_line(&lines, &target(50.0, 400.0), 0.0),
            None,
            "nowhere near a line"
        );
        assert_eq!(
            anchor_line(
                &lines,
                &Target {
                    page: 2,
                    ..target(50.0, 691.0)
                },
                0.0
            ),
            None,
            "another page"
        );
    }

    #[test]
    fn destination_names_give_the_entry_number_publishers_put_in_them() {
        let number = |name: &str| {
            DESTINATION_NUMBER
                .captures(name)
                .map(|c| c[1].parse::<u32>().unwrap())
        };
        assert_eq!(number("12672_2025_1977_Article.indd:CR12:99"), Some(12));
        assert_eq!(number("Rpone.0316678.ref012"), Some(12));
        assert_eq!(number("cite.ref-7"), Some(7));
        assert_eq!(number("bib31"), Some(31));
        assert_eq!(number("CIT0004"), Some(4));
        assert_eq!(number("cite.vaswani2017attention"), None);
        assert_eq!(number("fneur-15-1452194.indd:Anchor 14:1254"), None);
    }

    #[test]
    fn a_doi_link_in_an_entry_wins_over_a_badly_broken_printed_doi() {
        let mut entry = ReferenceEntry {
            dois: vec!["10.1016/j".to_string()],
            ..ReferenceEntry::default()
        };
        add_uri(&mut entry, "https://doi.org/10.1016%2Fj.accpm.2020.02.003");
        assert_eq!(entry.dois[0], "10.1016/j.accpm.2020.02.003");
        add_uri(&mut entry, "https://arxiv.org/abs/2003.04297v2");
        assert_eq!(entry.arxiv_id.as_deref(), Some("2003.04297v2"));
    }

    #[test]
    fn the_list_ends_at_the_next_heading() {
        let mut lines = vec![
            line(0, 50.0, 700.0, "[1] First entry"),
            line(0, 50.0, 689.0, "[2] Second entry"),
            line(
                0,
                50.0,
                678.0,
                "Publisher’s Note Springer Nature remains neutral with regard to",
            ),
            line(0, 50.0, 667.0, "jurisdictional claims."),
        ];
        truncate_at_end(&mut lines);
        assert_eq!(lines.len(), 2);
        let mut lines = vec![
            line(0, 50.0, 700.0, "[1] First entry"),
            line(0, 50.0, 689.0, "[2] Second entry"),
            RefLine {
                font_size: 12.0,
                ..line(0, 50.0, 660.0, "A Proof of Theorem 1")
            },
            line(0, 50.0, 640.0, "We show that"),
        ];
        truncate_at_end(&mut lines);
        assert_eq!(lines.len(), 2);
    }
}

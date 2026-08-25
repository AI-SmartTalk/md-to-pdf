//! Turning glyphs at coordinates back into paragraphs.
//!
//! LibreOffice's PDF import is the obvious way to answer "PDF to Word", and it is the wrong
//! one. It is faithful to the *picture*: every line of the original becomes its own text
//! frame, pinned at the coordinates it had. Measured on this project's own README — ten
//! pages, no images — the result is 1.3 MB of `document.xml` holding 810 text boxes, 1140
//! absolute positions and not a single working hyperlink. It opens. It cannot be edited: put
//! the cursor in a sentence, type a word, and nothing reflows because nothing is a paragraph.
//!
//! So this module does the part LibreOffice skips. `pdftohtml -xml` reports every text run
//! with its position, its *width* and its font, and width is what makes the guess possible:
//! a line that stops short of the column's right edge ended a paragraph, one that reaches it
//! was wrapped by the layout engine and belongs to the same one. From there, font size gives
//! headings, a leading bullet gives lists, and what comes out is 46 KB of ordinary
//! paragraphs — the same text, in a document Word can actually edit.
//!
//! Everything here is a guess, and the module is written so the guesses are visible and
//! testable rather than buried in a converter. What it cannot recover — a table's grid, a
//! multi-column layout read in the wrong order — it does not pretend to: `/api/pdf-to-office`
//! measures the result against the source and says so in its verdict.

use std::collections::HashMap;

/// Two fragments belong to the same line when their vertical spans overlap by more than
/// this share of the shorter one.
///
/// Overlap rather than distance between centres: a superscript, a footnote marker or a
/// heading with inline code is set smaller and sits higher than the text it belongs to, so
/// its centre can be half a line away while it plainly shares the line. Two genuinely
/// consecutive lines do not overlap at all, so the rule has room to be generous.
const SAME_LINE_OVERLAP: f64 = 0.35;

/// A horizontal gap wider than this share of the line's character width is a space that the
/// PDF expressed by moving the cursor instead of by emitting a space glyph.
const SPACE_GAP_RATIO: f64 = 0.4;

/// Left edges within this fraction of a character are the same left edge. Justified text
/// wobbles by a hair and must not read as an indent.
const INDENT_SLACK_CHARS: f64 = 0.6;

/// How far a block's first line may be indented past the rest of it before the two stop
/// being the same block. Covers the book convention of an indented opening line, which would
/// otherwise split every single paragraph in two.
const FIRST_LINE_INDENT_CHARS: f64 = 6.0;

/// How much wider than the page's usual leading a gap has to be before it reads as a
/// paragraph break rather than as the next line of the same paragraph.
const PARAGRAPH_GAP_RATIO: f64 = 1.6;

/// Text this much larger than the body's own size is a heading.
const HEADING_RATIO: f64 = 1.15;

/// Below this share of a page's lines, a font size is an accident (a footnote, a caption)
/// rather than the body text.
const BODY_SIZE_QUORUM: f64 = 0.0;

/// Characters a PDF uses to mark a list item
const BULLETS: [char; 7] = ['•', '‣', '▪', '◦', '·', '–', '—'];

// ------------ The document pdftohtml describes ------------

/// One run of text, exactly as `pdftohtml -xml` places it
#[derive(Debug, Clone, PartialEq)]
pub struct Fragment {
    pub top: f64,
    pub left: f64,
    pub width: f64,
    pub height: f64,
    /// Font size in the same units as the coordinates, resolved from the page's `fontspec`
    pub size: f64,
    /// Set in a fixed-width face, which in a document means a code listing
    pub monospace: bool,
    /// Inline markup as poppler wrote it: text, `<b>`, `<i>`, `<a href>`
    pub html: String,
}

impl Fragment {
    fn right(&self) -> f64 {
        self.left + self.width
    }

    fn bottom(&self) -> f64 {
        self.top + self.height
    }

    /// Does this run share a line with `other`? See `SAME_LINE_OVERLAP`.
    fn shares_line_with(&self, other: &Fragment) -> bool {
        let overlap = self.bottom().min(other.bottom()) - self.top.max(other.top);
        overlap > self.height.min(other.height) * SAME_LINE_OVERLAP
    }

    /// Mean advance of one character, the unit every tolerance in this module is expressed
    /// in: a slack in pixels would mean something different at 8pt and at 40pt.
    fn char_width(&self) -> f64 {
        let chars = text_of(&self.html).chars().count();
        if chars == 0 {
            // Nothing to measure: the font size is the only estimate available, and it
            // over-states the advance of a proportional font by about half.
            return self.size * 0.5;
        }
        self.width / chars as f64
    }
}

/// An image poppler extracted next to the XML
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub top: f64,
    pub left: f64,
    pub width: f64,
    pub height: f64,
    /// File name, relative to the directory the XML was written in
    pub src: String,
}

#[derive(Debug, Default, PartialEq)]
pub struct Page {
    pub fragments: Vec<Fragment>,
    pub images: Vec<Image>,
}

// ------------ Parsing ------------

/// Read the `-xml` output of pdftohtml.
///
/// Hand-written rather than pulled from a crate: the input is not arbitrary XML but the
/// output of one program in one mode — `<page>`, `<fontspec>`, `<text>` and `<image>`, never
/// nested, never with a namespace. An XML crate would be one more dependency parsing
/// attacker-influenced bytes in the production image, to read four tag names.
pub fn parse(xml: &str) -> Vec<Page> {
    let mut pages = Vec::new();
    let mut current = Page::default();
    let mut started = false;
    let mut fonts: HashMap<String, (f64, bool)> = HashMap::new();

    let mut rest = xml;
    while let Some(open) = rest.find('<') {
        rest = &rest[open..];
        let Some(close) = rest.find('>') else { break };
        let tag = &rest[1..close];
        let name = tag_name(tag);

        // A closing tag carries no attributes and opens nothing. Without this, `</text>`
        // re-enters the arm below, which then swallows the rest of the document looking for
        // a `</text>` that has already gone by.
        if tag.starts_with('/') {
            rest = &rest[close + 1..];
            continue;
        }

        match name {
            "page" => {
                if started {
                    pages.push(std::mem::take(&mut current));
                }
                started = true;
                // Font ids restart at every page in some poppler builds and continue across
                // pages in others. Keeping one table for the whole document is right either
                // way, as long as a redefinition wins.
            }
            "fontspec" => {
                if let (Some(id), Some(size)) = (attr(tag, "id"), number(tag, "size")) {
                    let family = attr(tag, "family").unwrap_or_default();
                    fonts.insert(id.to_string(), (size, is_monospace(family)));
                }
            }
            "text" => {
                let body_start = close + 1;
                let end = rest[body_start..]
                    .find("</text>")
                    .map(|at| body_start + at)
                    .unwrap_or(rest.len());
                let html = &rest[body_start..end];

                let spec = attr(tag, "font").and_then(|id| fonts.get(id).copied());
                let size = spec
                    .map(|(size, _)| size)
                    // A run whose fontspec is missing still has a height, which is the size
                    // plus the leading: close enough to rank it against its neighbours.
                    .or_else(|| number(tag, "height"))
                    .unwrap_or(0.0);
                let monospace = spec.is_some_and(|(_, monospace)| monospace);

                if let (Some(top), Some(left), Some(width), Some(height)) = (
                    number(tag, "top"),
                    number(tag, "left"),
                    number(tag, "width"),
                    number(tag, "height"),
                ) {
                    if !text_of(html).trim().is_empty() {
                        current.fragments.push(Fragment {
                            top,
                            left,
                            width,
                            height,
                            size,
                            monospace,
                            html: html.to_string(),
                        });
                    }
                }

                rest = &rest[end.min(rest.len())..];
                continue;
            }
            "image" => {
                if let (Some(top), Some(left), Some(width), Some(height), Some(src)) = (
                    number(tag, "top"),
                    number(tag, "left"),
                    number(tag, "width"),
                    number(tag, "height"),
                    attr(tag, "src"),
                ) {
                    current.images.push(Image {
                        top,
                        left,
                        width,
                        height,
                        src: src.to_string(),
                    });
                }
            }
            _ => {}
        }

        rest = &rest[close + 1..];
    }

    if started {
        pages.push(current);
    }

    pages
}

/// Is this font family a fixed-width one?
///
/// Read from the name because that is all poppler reports, and every fixed-width face in
/// common use says so in it — `Courier`, `Consolas`, `DejaVu Sans Mono`, `Noto Sans Mono`,
/// `Menlo`. Subset prefixes (`PXIPUI+Noto-Sans-Mono`) and separators vary, so the test is on
/// the lowercased name with punctuation removed.
fn is_monospace(family: &str) -> bool {
    let normalised: String = family
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();

    ["mono", "courier", "consolas", "menlo", "inconsolata"]
        .iter()
        .any(|needle| normalised.contains(needle))
}

fn tag_name(tag: &str) -> &str {
    let tag = tag.trim_start_matches('/');
    let end = tag
        .find(|c: char| c.is_whitespace() || c == '/' || c == '>')
        .unwrap_or(tag.len());
    &tag[..end]
}

/// Value of `name="…"` in a tag, without unescaping: every value this module reads is a
/// number or a file name poppler generated.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = tag;
    loop {
        let at = rest.find(name)?;
        let after = &rest[at + name.len()..];
        // `top` must not match the `top` inside `stop`, nor the name of another attribute
        // that merely starts with it.
        let boundary_before = at == 0
            || rest[..at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_whitespace());
        if boundary_before && after.starts_with("=\"") {
            let value = &after[2..];
            let end = value.find('"')?;
            return Some(&value[..end]);
        }
        rest = &rest[at + name.len()..];
    }
}

fn number(tag: &str, name: &str) -> Option<f64> {
    attr(tag, name)?.parse().ok()
}

/// Visible characters of a fragment: its markup stripped and its entities resolved
fn text_of(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;

    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        match rest[open..].find('>') {
            Some(close) => rest = &rest[open + close + 1..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);

    unescape(&out)
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at..];
        let Some(end) = after.find(';').filter(|end| *end <= 10) else {
            out.push('&');
            rest = &after[1..];
            continue;
        };

        let entity = &after[1..end];
        let resolved = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            numeric if numeric.starts_with("#x") || numeric.starts_with("#X") => {
                u32::from_str_radix(&numeric[2..], 16)
                    .ok()
                    .and_then(char::from_u32)
            }
            numeric if numeric.starts_with('#') => {
                numeric[1..].parse().ok().and_then(char::from_u32)
            }
            _ => None,
        };

        match resolved {
            Some(c) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = &after[1..];
            }
        }
    }

    out.push_str(rest);
    out
}

// ------------ Lines ------------

/// One visual line: the fragments a reader sees as sitting side by side
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub html: String,
    pub text: String,
    pub left: f64,
    /// Where the line's own words start, past any list marker. A wrapped list item lines up
    /// with this and not with the bullet, so this is what a continuation is measured against.
    pub content_left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
    /// Size of the largest run on the line — a heading with an inline footnote marker is
    /// still a heading.
    pub size: f64,
    pub char_width: f64,
    /// Every run on the line is fixed-width: a listing, not a sentence
    pub monospace: bool,
}

/// Group a page's fragments into the lines a reader would see.
///
/// Order matters and cannot be taken from the file: poppler emits list bullets *after* the
/// paragraphs they belong to, so reading order has to be recovered from the coordinates.
pub fn lines(page: &Page) -> Vec<Line> {
    let mut fragments = page.fragments.clone();
    fragments.sort_by(|a, b| {
        a.top
            .partial_cmp(&b.top)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                a.left
                    .partial_cmp(&b.left)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });

    let mut rows: Vec<Vec<Fragment>> = Vec::new();
    for fragment in fragments {
        let joins = rows
            .last()
            .is_some_and(|row| row.iter().any(|other| other.shares_line_with(&fragment)));

        match joins {
            true => rows.last_mut().expect("checked above").push(fragment),
            false => rows.push(vec![fragment]),
        }
    }

    rows.into_iter().map(assemble).collect()
}

fn assemble(mut row: Vec<Fragment>) -> Line {
    row.sort_by(|a, b| {
        a.left
            .partial_cmp(&b.left)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut html = String::new();
    let mut previous_right: Option<f64> = None;

    for fragment in &row {
        if let Some(right) = previous_right {
            let gap = fragment.left - right;
            let needs_space = gap > fragment.char_width() * SPACE_GAP_RATIO
                && !html.ends_with(char::is_whitespace)
                && !text_of(&fragment.html).starts_with(char::is_whitespace);
            if needs_space {
                html.push(' ');
            }
        }
        html.push_str(&fragment.html);
        previous_right = Some(fragment.right());
    }

    let widest = row.iter().max_by(|a, b| {
        a.size
            .partial_cmp(&b.size)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Line {
        text: text_of(&html),
        left: row.iter().map(|f| f.left).fold(f64::INFINITY, f64::min),
        content_left: content_left(&row),
        right: row
            .iter()
            .map(|f| f.right())
            .fold(f64::NEG_INFINITY, f64::max),
        top: row.iter().map(|f| f.top).fold(f64::INFINITY, f64::min),
        bottom: row
            .iter()
            .map(|f| f.top + f.height)
            .fold(f64::NEG_INFINITY, f64::max),
        size: widest.map(|f| f.size).unwrap_or_default(),
        char_width: row
            .iter()
            .map(Fragment::char_width)
            .fold(f64::NEG_INFINITY, f64::max),
        // Every run, not most: a sentence with one inline `identifier` in it is prose, and
        // laying it out as a listing would be worse than losing the inline styling.
        monospace: !row.is_empty() && row.iter().all(|fragment| fragment.monospace),
        html,
    }
}

/// Where a line's words begin, once a list marker has been stepped over.
///
/// Two shapes, and poppler produces both: the bullet in a run of its own — the usual case,
/// because it is set in a different font — or glued to the text that follows it, in which
/// case the marker's advance has to be estimated from the run's own character width.
fn content_left(row: &[Fragment]) -> f64 {
    let Some(first) = row.first() else { return 0.0 };
    let text = text_of(&first.html);

    let Some(marker) = bullet(&text) else {
        return first.left;
    };

    let after = text[marker..].trim_start();
    if after.is_empty() {
        return row.get(1).map(|next| next.left).unwrap_or(first.left);
    }

    let consumed = text.len() - after.len();
    let chars = text[..consumed].chars().count() as f64;
    first.left + chars * first.char_width()
}

// ------------ Blocks ------------

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Heading {
        level: u8,
        html: String,
    },
    Paragraph {
        html: String,
    },
    ListItem {
        html: String,
    },
    /// A run of fixed-width lines, kept as they were broken. A listing that reflows is a
    /// listing that no longer runs.
    Code {
        lines: Vec<String>,
    },
    Image {
        src: String,
    },
}

/// The body text size of a document: the size most of its lines are set in.
///
/// Counted over every page rather than per page, because a page that happens to be all
/// headings — a cover, a section divider — would otherwise decide that its own titles are
/// body text and emit no heading at all.
fn body_size(pages: &[Vec<Line>]) -> f64 {
    let mut tally: HashMap<u64, usize> = HashMap::new();
    for page in pages {
        for line in page {
            // Keyed on tenths: the same font reported as 17.99 and 18.0 is one size.
            *tally.entry((line.size * 10.0).round() as u64).or_default() +=
                line.text.chars().count();
        }
    }

    tally
        .into_iter()
        .filter(|(_, weight)| *weight as f64 > BODY_SIZE_QUORUM)
        .max_by_key(|(size, weight)| (*weight, *size))
        .map(|(size, _)| size as f64 / 10.0)
        .unwrap_or(0.0)
}

/// Right edge of the text column, as the lines themselves describe it.
///
/// The widest line would do if no document ever carried a stray full-width rule or a header
/// running into the margin; the ninth decile is the same answer without that risk.
fn column_right(lines: &[Line]) -> f64 {
    let mut rights: Vec<f64> = lines.iter().map(|line| line.right).collect();
    if rights.is_empty() {
        return 0.0;
    }
    rights.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    rights[(rights.len() * 9) / 10]
}

fn bullet(text: &str) -> Option<usize> {
    let trimmed = text.trim_start();
    let offset = text.len() - trimmed.len();

    if let Some(first) = trimmed.chars().next() {
        if BULLETS.contains(&first) {
            return Some(offset + first.len_utf8());
        }
    }

    // `1.`, `12)` — a numbered item. Bounded at three digits so a year opening a sentence
    // ("2026 was the year…") cannot turn a paragraph into a list.
    let digits: String = trimmed.chars().take_while(char::is_ascii_digit).collect();
    if !digits.is_empty() && digits.len() <= 3 {
        let after = &trimmed[digits.len()..];
        if after.starts_with('.') || after.starts_with(')') {
            let marker = digits.len() + 1;
            if after[1..].starts_with(char::is_whitespace) {
                return Some(offset + marker);
            }
        }
    }

    None
}

/// Rank the sizes above body text into heading levels, largest first.
fn heading_levels(pages: &[Vec<Line>], body: f64) -> HashMap<u64, u8> {
    let mut sizes: Vec<u64> = pages
        .iter()
        .flatten()
        .filter(|line| line.size > body * HEADING_RATIO)
        .map(|line| (line.size * 10.0).round() as u64)
        .collect();
    sizes.sort_unstable();
    sizes.dedup();
    sizes.reverse();

    sizes
        .into_iter()
        .enumerate()
        // Everything past the fourth distinct size is an h4: Word's outline has no use for
        // a tenth level, and a PDF with ten heading sizes is not really outlined at all.
        .map(|(rank, size)| (size, (rank as u8 + 1).min(4)))
        .collect()
}

/// Turn a document's lines into the blocks a word processor understands.
pub fn blocks(pages: &[Page]) -> Vec<Block> {
    let laid_out: Vec<Vec<Line>> = pages.iter().map(lines).collect();
    let body = body_size(&laid_out);
    let levels = heading_levels(&laid_out, body);

    let mut blocks = Vec::new();

    for (page, page_lines) in pages.iter().zip(&laid_out) {
        let margin = column_right(page_lines);
        let gap = usual_gap(page_lines);
        let mut open: Option<Open> = None;
        let mut code: Vec<String> = Vec::new();

        for (index, line) in page_lines.iter().enumerate() {
            let previous = index.checked_sub(1).map(|i| &page_lines[i]);

            // Before the heading test: a listing set larger than the body text is still a
            // listing, and turning `#!/bin/sh` into a Heading 1 is worse than losing a title.
            if line.monospace {
                flush(&mut open, &mut blocks);
                // Two listings with prose between them are two listings; so are two
                // separated by more than a line of white space.
                let apart = previous.is_some_and(|above| {
                    line.top - above.bottom > line.size.max(1.0) * PARAGRAPH_GAP_RATIO
                });
                if apart {
                    flush_code(&mut code, &mut blocks);
                }
                code.push(line.text.clone());
                continue;
            }
            flush_code(&mut code, &mut blocks);

            if let Some(level) = levels.get(&((line.size * 10.0).round() as u64)) {
                flush(&mut open, &mut blocks);
                blocks.push(Block::Heading {
                    level: *level,
                    html: line.html.clone(),
                });
                continue;
            }

            if let Some(marker) = bullet(&line.text) {
                flush(&mut open, &mut blocks);
                open = Some(Open::item(line, marker));
                continue;
            }

            let continues = open
                .as_ref()
                .is_some_and(|open| wraps(open, line, previous, margin, gap));

            match continues {
                true => {
                    let open = open.as_mut().expect("checked above");
                    join(&mut open.html, &line.html);
                    // A block whose second line sits further left had an indented opening
                    // line; from here on, that is the edge the rest must match.
                    open.indent = open.indent.min(line.left);
                    open.fresh = false;
                    open.line = line.clone();
                }
                false => {
                    flush(&mut open, &mut blocks);
                    open = Some(Open::paragraph(line));
                }
            }
        }

        flush(&mut open, &mut blocks);
        flush_code(&mut code, &mut blocks);

        // Images come after the page's text rather than at their own coordinates: placing
        // them between two paragraphs would need the reading order this module deliberately
        // does not claim to recover.
        for image in &page.images {
            blocks.push(Block::Image {
                src: image.src.clone(),
            });
        }
    }

    blocks
}

fn flush_code(code: &mut Vec<String>, blocks: &mut Vec<Block>) {
    if code.is_empty() {
        return;
    }
    blocks.push(Block::Code {
        lines: std::mem::take(code),
    });
}

/// A paragraph being built: its text so far, the last line that went into it, and the left
/// edge its continuations are expected to line up with.
struct Open {
    html: String,
    line: Line,
    indent: f64,
    /// Still true until a second line joins, which is when a first-line indent is settled
    fresh: bool,
    list: bool,
}

impl Open {
    fn paragraph(line: &Line) -> Open {
        Open {
            html: line.html.clone(),
            indent: line.left,
            line: line.clone(),
            fresh: true,
            list: false,
        }
    }

    fn item(line: &Line, marker: usize) -> Open {
        Open {
            html: strip_marker(&line.html, marker),
            // A wrapped item lines up under the text, not under the bullet
            indent: line.content_left,
            line: line.clone(),
            fresh: false,
            list: true,
        }
    }
}

fn flush(open: &mut Option<Open>, blocks: &mut Vec<Block>) {
    let Some(open) = open.take() else { return };
    if open.html.trim().is_empty() {
        return;
    }

    blocks.push(match open.list {
        true => Block::ListItem { html: open.html },
        false => Block::Paragraph { html: open.html },
    });
}

/// The gap this page puts between two lines of the same paragraph.
///
/// Taken from the page rather than from a constant because leading is a design decision: a
/// tightly set report and a double-spaced manuscript disagree about it by a factor of three,
/// and a fixed threshold would over-merge one and shred the other. The lower median is used
/// so that a page of one-line paragraphs — where most gaps *are* paragraph breaks — cannot
/// tip the estimate.
fn usual_gap(lines: &[Line]) -> f64 {
    let mut gaps: Vec<f64> = lines
        .windows(2)
        .map(|pair| pair[1].top - pair[0].bottom)
        .filter(|gap| *gap >= 0.0)
        .collect();

    if gaps.is_empty() {
        return 0.0;
    }

    gaps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    gaps[(gaps.len() - 1) / 2]
}

/// Is `line` the continuation of the paragraph `previous_in_block` ends?
///
/// Three questions. The first is the one that matters, and the one a right-margin threshold
/// gets wrong: a line does not end a paragraph because it stopped short of the margin, it
/// ends one because it stopped short *with room to spare*. The test is therefore whether the
/// next line's first word would have fitted in what was left — if it would have, the author
/// chose to break there; if it would not have, the layout engine broke it and the sentence
/// continues. Then: is this line at the same left edge, and is the gap ordinary leading?
fn wraps(
    open: &Open,
    line: &Line,
    previous_on_page: Option<&Line>,
    margin: f64,
    usual_gap: f64,
) -> bool {
    let previous_in_block = &open.line;
    let unit = line.char_width.max(previous_in_block.char_width).max(1.0);

    let first_word = line.text.split_whitespace().next().unwrap_or("");
    // Plus one for the space that would have preceded it
    let needed = (first_word.chars().count() as f64 + 1.0) * line.char_width.max(1.0);
    let room = margin - previous_in_block.right;
    let forced_break = room < needed;

    let offset = line.left - open.indent;
    let same_indent = offset.abs() <= unit * INDENT_SLACK_CHARS
        // Only ever on the second line of a block, and only leftwards: an indented opening
        // line is a convention, an indented *second* line is a different block.
        || (open.fresh && offset < 0.0 && -offset <= unit * FIRST_LINE_INDENT_CHARS);

    // Measured against the line physically above rather than against the paragraph's last
    // line: on a page whose text is interrupted by a figure, those are not the same line.
    let gap = previous_on_page
        .map(|above| line.top - above.bottom)
        .unwrap_or(0.0);
    // Clamped to the font size at the top so that a page whose median gap *is* the paragraph
    // spacing cannot merge the whole page into one block, and to a quarter of it at the
    // bottom so that a page set solid still merges at all.
    let allowed = (usual_gap * PARAGRAPH_GAP_RATIO).clamp(line.size * 0.25, line.size.max(1.0));
    let leading = gap <= allowed;

    forced_break && same_indent && leading
}

/// Append a wrapped line to the paragraph it continues.
///
/// A line broken mid-word carries a hyphen the layout engine added; it is dropped only when
/// what follows is lower case, so "state-of-the-art" split after "state-" survives intact.
fn join(paragraph: &mut String, line: &str) {
    let tail = text_of(paragraph);
    let head = text_of(line);

    let hyphenated = tail.ends_with('-')
        && !tail.ends_with("--")
        && head.chars().next().is_some_and(char::is_lowercase);

    if hyphenated {
        if let Some(at) = paragraph.rfind('-') {
            paragraph.replace_range(at..at + 1, "");
        }
        paragraph.push_str(line);
        return;
    }

    if !paragraph.ends_with(char::is_whitespace) && !head.starts_with(char::is_whitespace) {
        paragraph.push(' ');
    }
    paragraph.push_str(line);
}

/// Drop the bullet or number a list marker occupies, counted in bytes of the *text*, from
/// the markup that produced it.
fn strip_marker(html: &str, marker: usize) -> String {
    let mut seen = 0usize;
    let mut out = String::with_capacity(html.len());
    let mut rest = html;

    while seen < marker {
        let Some(next) = rest.chars().next() else {
            break;
        };

        if next == '<' {
            // Tags carry no text and are kept whole: the `<b>` around a bullet still opens
            // the run that follows it.
            let end = rest.find('>').map(|at| at + 1).unwrap_or(rest.len());
            out.push_str(&rest[..end]);
            rest = &rest[end..];
            continue;
        }

        if next == '&' {
            let end = rest.find(';').map(|at| at + 1).unwrap_or(1);
            seen += text_of(&rest[..end]).len();
            rest = &rest[end..];
            continue;
        }

        seen += next.len_utf8();
        rest = &rest[next.len_utf8()..];
    }

    out.push_str(rest.trim_start());
    out
}

// ------------ Rendering ------------

/// Schemes a link rebuilt out of a PDF is allowed to carry into the produced document.
const SAFE_SCHEMES: [&str; 4] = ["http://", "https://", "mailto:", "#"];

/// Render the blocks as the HTML pandoc turns into a document.
///
/// The output carries no styling on purpose: pandoc's reference document supplies Word's own
/// Heading 1, List Paragraph and Body Text styles, which is what makes the result editable
/// *and* restyleable. Reproducing the PDF's fonts and colours here would rebuild the picture
/// this module exists to get away from.
pub fn render(blocks: &[Block]) -> String {
    // No `<title>`: pandoc turns HTML title metadata into a Title paragraph at the top of
    // the docx, and a document that opens with the word "converted" is not the document the
    // caller uploaded.
    let mut html =
        String::from("<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"></head>\n<body>\n");

    let mut in_list = false;
    for block in blocks {
        let opens_list = matches!(block, Block::ListItem { .. });
        if opens_list && !in_list {
            html.push_str("<ul>\n");
        }
        if !opens_list && in_list {
            html.push_str("</ul>\n");
        }
        in_list = opens_list;

        match block {
            Block::Heading { level, html: inner } => {
                html.push_str(&format!(
                    "<h{level}>{}</h{level}>\n",
                    sanitize(inner),
                    level = level
                ));
            }
            Block::Paragraph { html: inner } => {
                html.push_str(&format!("<p>{}</p>\n", sanitize(inner)));
            }
            Block::ListItem { html: inner } => {
                html.push_str(&format!("<li>{}</li>\n", sanitize(inner)));
            }
            Block::Code { lines } => {
                let listing: Vec<String> = lines.iter().map(|line| escape(line)).collect();
                html.push_str(&format!("<pre><code>{}</code></pre>\n", listing.join("\n")));
            }
            Block::Image { src } => {
                html.push_str(&format!("<p><img src=\"{}\"/></p>\n", escape(src)));
            }
        }
    }

    if in_list {
        html.push_str("</ul>\n");
    }

    html.push_str("</body></html>\n");
    html
}

/// Keep the inline markup poppler produced, drop everything else.
///
/// The text is a stranger's document and the `href` values come from it: a PDF can carry
/// `javascript:` and `file:` links, and pandoc would faithfully write them into the docx for
/// someone to click. Only the schemes a rebuilt document has any business carrying survive;
/// the rest keep their text and lose their link.
fn sanitize(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;

    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        rest = &rest[open..];
        let Some(close) = rest.find('>') else {
            // An unterminated `<` is text, and text is escaped
            out.push_str("&lt;");
            rest = &rest[1..];
            continue;
        };

        let tag = &rest[1..close];
        let name = tag_name(tag).to_ascii_lowercase();
        let closing = tag.starts_with('/');

        match name.as_str() {
            "b" | "i" | "strong" | "em" | "sub" | "sup" => {
                out.push('<');
                if closing {
                    out.push('/');
                }
                out.push_str(&name);
                out.push('>');
            }
            "a" => {
                if closing {
                    out.push_str("</a>");
                } else {
                    match attr(tag, "href").filter(|href| is_safe_link(href)) {
                        Some(href) => out.push_str(&format!("<a href=\"{}\">", escape(href))),
                        // Dropped rather than refused: the sentence is still worth having.
                        None => out.push_str("<a>"),
                    }
                }
            }
            "br" => out.push_str("<br/>"),
            // Anything else poppler might emit is markup this module did not ask for
            _ => {}
        }

        rest = &rest[close + 1..];
    }

    out.push_str(rest);
    out
}

fn is_safe_link(href: &str) -> bool {
    let lowered = href.trim().to_ascii_lowercase();
    SAFE_SCHEMES
        .iter()
        .any(|scheme| lowered.starts_with(scheme))
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Everything at once: the XML pdftohtml wrote, the HTML pandoc should read.
pub fn to_html(xml: &str) -> String {
    render(&blocks(&parse(xml)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page of `-xml` output, from a list of `(top, left, width, height, size, html)`
    fn page(runs: &[(f64, f64, f64, f64, f64, &str)]) -> Page {
        Page {
            fragments: runs
                .iter()
                .map(|(top, left, width, height, size, html)| Fragment {
                    top: *top,
                    left: *left,
                    width: *width,
                    height: *height,
                    size: *size,
                    monospace: false,
                    html: html.to_string(),
                })
                .collect(),
            images: Vec::new(),
        }
    }

    /// The same, with every run set in a fixed-width face
    fn listing(runs: &[(f64, f64, f64, f64, f64, &str)]) -> Page {
        let mut page = page(runs);
        for fragment in &mut page.fragments {
            fragment.monospace = true;
        }
        page
    }

    /// A run of body text `chars` long, at a given position, 9px per character
    fn run(top: f64, left: f64, chars: usize) -> (f64, f64, f64, f64, f64, &'static str) {
        (
            top,
            left,
            chars as f64 * 9.0,
            18.0,
            18.0,
            "lorem ipsum dolor sit amet consectetur",
        )
    }

    #[test]
    fn a_run_is_read_with_its_position_its_size_and_its_markup() {
        let pages = parse(
            r##"<pdf2xml>
<page number="1" top="0" left="0" height="1262" width="892">
<fontspec id="0" size="40" family="Nimbus" color="#000000"/>
<text top="192" left="121" width="523" height="40" font="0"><b>Titre</b></text>
</page>
</pdf2xml>"##,
        );

        assert_eq!(pages.len(), 1);
        assert_eq!(
            pages[0].fragments,
            vec![Fragment {
                top: 192.0,
                left: 121.0,
                width: 523.0,
                height: 40.0,
                size: 40.0,
                monospace: false,
                html: "<b>Titre</b>".to_string(),
            }]
        );
    }

    /// `top` inside `stop`, and an attribute that merely starts with the name being read
    #[test]
    fn an_attribute_is_read_by_its_whole_name_and_not_by_a_prefix() {
        let tag = r#"text stopwatch="9" topmost="7" top="192" left="1""#;
        assert_eq!(attr(tag, "top"), Some("192"));
        assert_eq!(attr(tag, "left"), Some("1"));
        assert_eq!(attr(tag, "width"), None);
    }

    #[test]
    fn every_page_of_the_document_is_kept_apart() {
        let pages = parse(
            r#"<page number="1"><text top="1" left="1" width="9" height="18" font="0">un</text></page>
<page number="2"><text top="1" left="1" width="9" height="18" font="0">deux</text></page>"#,
        );

        assert_eq!(pages.len(), 2);
        assert_eq!(pages[1].fragments[0].html, "deux");
    }

    #[test]
    fn a_run_with_nothing_visible_in_it_is_not_a_run() {
        let pages = parse(
            r#"<page number="1">
<text top="1" left="1" width="9" height="18" font="0">   </text>
<text top="1" left="9" width="9" height="18" font="0">mot</text>
</page>"#,
        );

        assert_eq!(pages[0].fragments.len(), 1);
    }

    #[test]
    fn entities_are_resolved_when_the_text_is_measured_but_kept_in_the_markup() {
        let pages = parse(
            r#"<page number="1"><text top="1" left="1" width="9" height="18" font="0">a &amp; b &#233; &#x40;</text></page>"#,
        );

        assert_eq!(pages[0].fragments[0].html, "a &amp; b &#233; &#x40;");
        assert_eq!(text_of(&pages[0].fragments[0].html), "a & b é @");
    }

    #[test]
    fn an_entity_that_is_not_one_survives_as_the_ampersand_it_is() {
        assert_eq!(unescape("Tom & Jerry"), "Tom & Jerry");
        assert_eq!(unescape("&notreal; &amp;"), "&notreal; &");
    }

    /// Poppler emits list bullets after the paragraphs they mark: reading order comes from
    /// the coordinates, never from the file.
    #[test]
    fn lines_are_rebuilt_in_reading_order_and_not_in_file_order() {
        let page = page(&[
            (100.0, 200.0, 90.0, 18.0, 18.0, "monde"),
            (60.0, 100.0, 90.0, 18.0, 18.0, "titre"),
            (100.0, 100.0, 90.0, 18.0, 18.0, "bonjour"),
        ]);

        let lines = lines(&page);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "titre");
        assert_eq!(lines[1].text, "bonjour monde");
    }

    /// A superscript sits off the baseline of the text it annotates and is still on its line
    #[test]
    fn a_run_slightly_off_the_baseline_stays_on_the_same_line() {
        let page = page(&[
            (100.0, 100.0, 90.0, 18.0, 18.0, "texte"),
            (96.0, 190.0, 10.0, 10.0, 10.0, "<sup>1</sup>"),
        ]);

        assert_eq!(lines(&page).len(), 1);
    }

    #[test]
    fn a_gap_the_pdf_expressed_by_moving_the_cursor_becomes_a_space() {
        let page = page(&[
            (100.0, 100.0, 27.0, 18.0, 18.0, "abc"),
            (100.0, 200.0, 27.0, 18.0, 18.0, "def"),
        ]);

        assert_eq!(lines(&page)[0].text, "abc def");
    }

    #[test]
    fn a_space_the_pdf_already_carries_is_not_doubled() {
        let page = page(&[
            (100.0, 100.0, 36.0, 18.0, 18.0, "abc "),
            (100.0, 200.0, 27.0, 18.0, 18.0, "def"),
        ]);

        assert_eq!(lines(&page)[0].text, "abc def");
    }

    /// The heart of it: a line that ran to the margin was wrapped, one that stopped short
    /// had finished. Without this, every line of the PDF becomes its own paragraph.
    #[test]
    fn wrapped_lines_become_one_paragraph_and_a_short_line_ends_it() {
        let page = page(&[
            run(100.0, 100.0, 60),
            run(120.0, 100.0, 60),
            // Stops well short of the margin: end of the paragraph
            run(140.0, 100.0, 20),
            run(160.0, 100.0, 60),
        ]);

        let blocks = blocks(&[page]);
        assert_eq!(blocks.len(), 2, "{:?}", blocks);
        assert!(matches!(blocks[0], Block::Paragraph { .. }));
        assert!(matches!(blocks[1], Block::Paragraph { .. }));
    }

    /// The case a right-margin threshold gets wrong. Both lines stop short of the margin by
    /// the same handful of pixels; what separates them is that the first had no room for the
    /// word that follows it and the second had plenty.
    #[test]
    fn a_line_is_wrapped_when_the_next_word_would_not_have_fitted_and_not_otherwise() {
        // Margin sits at 100 + 60*9 = 640
        let wrapped = page(&[
            run(100.0, 100.0, 60),
            // Ends at 586; "lorem" plus its space needs 54, and only 54 was left
            (
                120.0,
                100.0,
                486.0,
                18.0,
                18.0,
                "lorem ipsum dolor sit amet consectetur",
            ),
            run(140.0, 100.0, 60),
        ]);
        let rebuilt = blocks(std::slice::from_ref(&wrapped));
        assert_eq!(rebuilt.len(), 1, "{:?}", rebuilt);

        let finished = page(&[
            run(100.0, 100.0, 60),
            // Ends at 460: room for the next word twice over, so the author broke here
            (
                120.0,
                100.0,
                360.0,
                18.0,
                18.0,
                "lorem ipsum dolor sit amet consectetur",
            ),
            run(140.0, 100.0, 60),
        ]);
        let rebuilt = blocks(std::slice::from_ref(&finished));
        assert_eq!(rebuilt.len(), 2, "{:?}", rebuilt);
    }

    /// Leading is a design decision, not a constant: the same absolute gap is ordinary in a
    /// double-spaced document and a paragraph break in a tightly set one.
    #[test]
    fn the_leading_that_counts_as_ordinary_is_read_from_the_page() {
        // Set solid: 2px between lines, so the 30px gap before the last one is a break
        let tight = page(&[
            run(100.0, 100.0, 60),
            run(120.0, 100.0, 60),
            run(140.0, 100.0, 60),
            run(190.0, 100.0, 60),
        ]);

        let blocks = blocks(std::slice::from_ref(&tight));
        assert_eq!(blocks.len(), 2, "{:?}", blocks);
    }

    #[test]
    fn a_blank_line_between_two_full_width_lines_still_separates_them() {
        let page = page(&[
            run(100.0, 100.0, 60),
            run(120.0, 100.0, 60),
            // A whole line of vertical space: a new paragraph, however wide the one above
            run(180.0, 100.0, 60),
        ]);

        assert_eq!(blocks(&[page]).len(), 2);
    }

    #[test]
    fn a_change_of_indent_starts_a_new_block() {
        let page = page(&[run(100.0, 100.0, 60), run(120.0, 160.0, 60)]);

        assert_eq!(blocks(&[page]).len(), 2);
    }

    #[test]
    fn the_largest_sizes_become_the_first_heading_levels() {
        let page = page(&[
            (60.0, 100.0, 300.0, 40.0, 40.0, "Titre"),
            (120.0, 100.0, 250.0, 28.0, 28.0, "Sous-titre"),
            run(180.0, 100.0, 60),
            run(200.0, 100.0, 60),
        ]);

        let blocks = blocks(&[page]);
        assert_eq!(
            blocks[0],
            Block::Heading {
                level: 1,
                html: "Titre".to_string()
            }
        );
        assert_eq!(
            blocks[1],
            Block::Heading {
                level: 2,
                html: "Sous-titre".to_string()
            }
        );
        assert!(matches!(blocks[2], Block::Paragraph { .. }));
    }

    /// A cover page is all headings; the body size has to be read from the whole document
    #[test]
    fn the_body_size_is_read_across_pages_so_a_title_page_is_not_its_own_body() {
        let cover = page(&[(60.0, 100.0, 300.0, 40.0, 40.0, "Titre seul")]);
        let body = page(&[run(100.0, 100.0, 60), run(120.0, 100.0, 60)]);

        let blocks = blocks(&[cover, body]);
        assert!(
            matches!(blocks[0], Block::Heading { level: 1, .. }),
            "{:?}",
            blocks[0]
        );
    }

    #[test]
    fn a_bulleted_line_becomes_a_list_item_without_its_bullet() {
        let page = page(&[
            (100.0, 100.0, 18.0, 18.0, 18.0, "• "),
            (100.0, 120.0, 200.0, 18.0, 18.0, "premier point"),
            (130.0, 100.0, 18.0, 18.0, 18.0, "• "),
            (130.0, 120.0, 200.0, 18.0, 18.0, "second point"),
        ]);

        let blocks = blocks(&[page]);
        assert_eq!(
            blocks,
            vec![
                Block::ListItem {
                    html: "premier point".to_string()
                },
                Block::ListItem {
                    html: "second point".to_string()
                },
            ]
        );
    }

    /// A wrapped list item lines up under its own text, not under the bullet. Measuring the
    /// continuation against the bullet's left edge broke every item of more than one line.
    #[test]
    fn a_list_item_that_runs_over_two_lines_stays_one_item() {
        let page = page(&[
            (100.0, 100.0, 18.0, 18.0, 18.0, "• "),
            run(100.0, 120.0, 55),
            // Lined up under the text at 120, not under the bullet at 100
            run(120.0, 120.0, 55),
        ]);

        let blocks = blocks(std::slice::from_ref(&page));
        assert_eq!(blocks.len(), 1, "{:?}", blocks);
        assert!(matches!(blocks[0], Block::ListItem { .. }));
    }

    /// The book convention: the opening line is indented, the rest of the paragraph is not
    #[test]
    fn a_paragraph_whose_first_line_is_indented_is_still_one_paragraph() {
        let page = page(&[
            run(100.0, 140.0, 58),
            run(120.0, 100.0, 60),
            run(140.0, 100.0, 60),
        ]);

        let blocks = blocks(std::slice::from_ref(&page));
        assert_eq!(blocks.len(), 1, "{:?}", blocks);
    }

    /// …but only the first line. A block that dedents halfway down is two blocks.
    #[test]
    fn a_line_that_dedents_partway_through_a_paragraph_starts_a_new_one() {
        let page = page(&[
            run(100.0, 140.0, 58),
            run(120.0, 140.0, 58),
            run(140.0, 100.0, 60),
        ]);

        assert_eq!(blocks(std::slice::from_ref(&page)).len(), 2);
    }

    #[test]
    fn a_numbered_item_is_a_list_item_and_a_year_is_not() {
        assert_eq!(bullet("1. premier"), Some(2));
        assert_eq!(bullet("12) douzième"), Some(3));
        assert_eq!(bullet("• point"), Some("•".len()));
        assert_eq!(bullet("2026 fut une année"), None);
        assert_eq!(bullet("1.5 fois plus"), None);
        assert_eq!(bullet("texte ordinaire"), None);
    }

    #[test]
    fn a_marker_is_removed_from_the_markup_without_taking_its_formatting_with_it() {
        assert_eq!(strip_marker("<b>• gras</b>", "•".len()), "<b>gras</b>");
        assert_eq!(strip_marker("1. suite", 2), "suite");
        assert_eq!(strip_marker("&#8226; puce", "•".len()), "puce");
    }

    #[test]
    fn a_word_broken_across_two_lines_is_put_back_together() {
        let mut paragraph = String::from("un mot inter-");
        join(&mut paragraph, "rompu");
        assert_eq!(paragraph, "un mot interrompu");
    }

    #[test]
    fn a_compound_word_that_merely_ends_a_line_keeps_its_hyphen() {
        let mut paragraph = String::from("un tiret bas-");
        join(&mut paragraph, "Normand");
        assert_eq!(paragraph, "un tiret bas- Normand");
    }

    #[test]
    fn a_link_the_pdf_carries_survives_and_one_that_runs_code_does_not() {
        assert_eq!(
            sanitize(r#"<a href="https://exemple.fr">site</a>"#),
            r#"<a href="https://exemple.fr">site</a>"#
        );
        assert_eq!(
            sanitize(r#"<a href="javascript:alert(1)">clic</a>"#),
            "<a>clic</a>"
        );
        assert_eq!(
            sanitize(r#"<a href="file:///etc/passwd">clic</a>"#),
            "<a>clic</a>"
        );
    }

    #[test]
    fn markup_the_module_did_not_ask_for_is_dropped_and_its_text_kept() {
        assert_eq!(sanitize("<script>alert(1)</script>"), "alert(1)");
        assert_eq!(
            sanitize("<b>gras</b> et <i>italique</i>"),
            "<b>gras</b> et <i>italique</i>"
        );
        assert_eq!(sanitize("2 < 3"), "2 &lt; 3");
    }

    #[test]
    fn a_fixed_width_face_is_recognised_through_its_subset_prefix_and_its_separators() {
        assert!(is_monospace("PXIPUI+Noto-Sans-Mono"));
        assert!(is_monospace("DejaVu Sans Mono"));
        assert!(is_monospace("Courier New"));
        assert!(is_monospace("Consolas"));
        assert!(!is_monospace("FTYHPZ+Nimbus-Sans"));
        assert!(!is_monospace("Times New Roman"));
        assert!(!is_monospace(""));
    }

    /// A listing that reflows is a listing that no longer runs: its lines are kept as the
    /// author broke them, and none of the paragraph rules apply to it.
    #[test]
    fn a_run_of_fixed_width_lines_is_kept_line_by_line() {
        let page = listing(&[
            (100.0, 100.0, 200.0, 18.0, 16.0, "if [ -f x ]; then"),
            (120.0, 100.0, 100.0, 18.0, 16.0, "  echo ok"),
            (140.0, 100.0, 40.0, 18.0, 16.0, "fi"),
        ]);

        let blocks = blocks(std::slice::from_ref(&page));
        assert_eq!(
            blocks,
            vec![Block::Code {
                lines: vec![
                    "if [ -f x ]; then".to_string(),
                    "  echo ok".to_string(),
                    "fi".to_string(),
                ]
            }]
        );
    }

    /// A listing set larger than the body text is a listing, not a title
    #[test]
    fn a_large_fixed_width_line_is_not_mistaken_for_a_heading() {
        let mut page = page(&[run(100.0, 100.0, 60), run(120.0, 100.0, 60)]);
        page.fragments.push(Fragment {
            top: 200.0,
            left: 100.0,
            width: 200.0,
            height: 40.0,
            size: 40.0,
            monospace: true,
            html: "#!/bin/sh".to_string(),
        });

        let blocks = blocks(std::slice::from_ref(&page));
        assert!(
            matches!(blocks.last(), Some(Block::Code { .. })),
            "{:?}",
            blocks
        );
    }

    #[test]
    fn a_listing_is_escaped_rather_than_read_as_markup() {
        let html = render(&[Block::Code {
            lines: vec!["<div a=\"b\"> && x".to_string()],
        }]);

        assert!(
            html.contains("<pre><code>&lt;div a=&quot;b&quot;&gt; &amp;&amp; x</code></pre>"),
            "{}",
            html
        );
    }

    /// pandoc turns HTML title metadata into a Title paragraph, and the caller's document
    /// must not open with a word this service chose.
    #[test]
    fn the_rendered_page_carries_no_title_for_pandoc_to_promote() {
        assert!(!to_html(
            "<page><text top=\"1\" left=\"1\" width=\"9\" height=\"18\">a</text></page>"
        )
        .contains("<title>"));
    }

    #[test]
    fn consecutive_items_are_wrapped_in_one_list() {
        let html = render(&[
            Block::ListItem {
                html: "un".to_string(),
            },
            Block::ListItem {
                html: "deux".to_string(),
            },
            Block::Paragraph {
                html: "après".to_string(),
            },
        ]);

        assert!(
            html.contains("<ul>\n<li>un</li>\n<li>deux</li>\n</ul>"),
            "{}",
            html
        );
        assert_eq!(html.matches("<ul>").count(), 1);
        assert_eq!(html.matches("</ul>").count(), 1);
    }

    #[test]
    fn a_document_with_no_text_at_all_still_renders_a_document() {
        assert!(to_html("").contains("<body>"));
        assert!(to_html("<pdf2xml></pdf2xml>").contains("</html>"));
    }

    /// Malformed input is a stranger's PDF put through poppler, not a reason to panic
    #[test]
    fn truncated_and_malformed_input_is_read_as_far_as_it_goes() {
        for xml in [
            "<page><text top=\"1\" left=\"1\" width=\"9\" height=\"18\">sans fermeture",
            "<page><text>pas de position</text></page>",
            "<<<>>>",
            "<text top=\"x\" left=\"y\" width=\"z\" height=\"w\">non numérique</text>",
            "&",
            "<page><text top=\"1\" left=\"1\" width=\"9\" height=\"18\">a</text>",
        ] {
            let _ = to_html(xml);
        }
    }
}

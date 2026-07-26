//! Layout Doctor: inspect a rendered PDF and propose corrective CSS.
//!
//! The analysis is deterministic and offline. `pdftotext -bbox-layout` gives the box of every
//! word and the size of every page, which is enough to find the pagination defects that
//! actually hurt: content past the margin, a heading alone at the foot of a page, a table cut
//! in two, a last page holding three lines. No model and no network call, so nothing about
//! the document leaves the machine and two renders of the same source always score the same
//! — which is what the cache and the corrective loop rely on.
//!
//! Each defect has its own `detect_*` function over a single pre-built view of the document;
//! `analyze` only aggregates them, so another detector can be added without touching the
//! others.

use crate::helpers;
use crate::pdfops::{self, PageBox, Word};
use crate::types::{AppError, LayoutIssue, LayoutReport};
use std::path::Path;
use std::process::Command;

/// A word has to miss the content box by more than this to be reported: the extractor rounds
/// to a fraction of a point, and a hairline is not a layout defect.
const SLACK_PT: f32 = 3.0;

/// Two word boxes starting this close belong to the same column
const COLUMN_TOLERANCE_PT: f32 = 2.0;
/// Below this many aligned columns, an alignment is a coincidence rather than a table
const MIN_SHARED_COLUMNS: usize = 3;
/// Floor under the measured cell gap, so a document with unusually tight spacing cannot make
/// every word boundary look like a column
const MIN_CELL_GAP_PT: f32 = 8.0;

/// A line this much taller than the body text is a heading
const HEADING_RATIO: f32 = 1.20;
/// A heading below this fraction of the content box counts as "at the foot of the page"
const HEADING_BOTTOM_FRACTION: f32 = 0.75;

/// Last page covering less than this fraction of what the other pages cover is a widow
const WIDOW_RATIO: f32 = 0.20;

/// A table taller than this fraction of the content box cannot be kept whole, whatever the
/// CSS says: reporting it is what stops `corrective_css` from emitting a rule that would be
/// ignored, or worse, honoured with a half-empty page in front.
const TALL_TABLE_FRACTION: f32 = 0.85;

/// A wordless page is rasterized before being called blank, so a page holding only a chart
/// or a picture is not reported as empty. 20 dpi is enough: anti-aliasing spreads even a
/// hairline over a whole pixel at that scale, and an A4 page is 39 kB of raw pixels.
const BLANK_PROBE_DPI: u32 = 20;
const BLANK_PROBE_MAX_PAGES: usize = 8;

/// What each repetition of a defect adds on top of its worst occurrence, and how many
/// repetitions still say something new
const REPEAT_PENALTY: u32 = 2;
const MAX_COUNTED_PER_KIND: usize = 4;
/// Hard ceiling on the report so a pathological document cannot inflate the response
const MAX_ISSUES: usize = 50;

// ------------ The report ------------

/// Report on a rendered PDF: overflowing content, empty pages, orphan headings, ...
/// A document with nothing to report scores 100.
pub fn analyze(pdf: &Path) -> Result<LayoutReport, AppError> {
    let pages = pdfops::page_count(pdf)?;
    let (boxes, words) = pdfops::words(pdf)?;
    let doc = Doc::build(pdf, pages, boxes, words);

    let mut issues = Vec::new();
    issues.extend(detect_overflow(&doc));
    issues.extend(detect_blank_pages(&doc));
    issues.extend(detect_widow_page(&doc));
    issues.extend(detect_orphan_headings(&doc));
    issues.extend(detect_tables(&doc));

    let score = score_for(&issues);
    issues.truncate(MAX_ISSUES);

    Ok(LayoutReport {
        pages,
        score,
        issues,
        passes: None,
    })
}

/// 0..=100, derived from what was found and from nothing else: the same PDF always scores
/// the same, otherwise the cached report and any diff between two renders would lie.
///
/// Severity dominates and repetition only adds a little. That ordering is what makes the
/// corrective loop work: turning one paragraph that runs off the sheet into three that
/// nudge two points into the margin has to read as an improvement, not as three problems
/// where there was one.
fn score_for(issues: &[LayoutIssue]) -> u8 {
    let mut penalty = 0u32;

    for kind in SCORED_KINDS {
        let matching = issues.iter().filter(|issue| issue.kind == kind);
        let worst = matching
            .clone()
            .map(|issue| weight(kind, &issue.severity))
            .max()
            .unwrap_or(0);
        let repeats = matching.count().saturating_sub(1).min(MAX_COUNTED_PER_KIND);

        penalty += worst + REPEAT_PENALTY * repeats as u32;
    }

    100u32.saturating_sub(penalty).min(100) as u8
}

/// `long_table` is deliberately absent: it describes a document the renderer laid out
/// correctly, it just cannot be improved, so it costs nothing.
const SCORED_KINDS: [&str; 5] = [
    "overflow",
    "blank_page",
    "widow_page",
    "orphan_heading",
    "split_table",
];

/// What the worst occurrence of a defect costs. The five of them together exceed 100, so a
/// document that manages every defect at once really does score zero.
fn weight(kind: &str, severity: &str) -> u32 {
    match (kind, severity) {
        ("overflow", "error") => 25,
        ("overflow", _) => 10,
        ("blank_page", "error") => 25,
        ("blank_page", _) => 12,
        ("widow_page", _) => 12,
        ("orphan_heading", _) => 10,
        ("split_table", _) => 12,
        _ => 0,
    }
}

// ------------ Detectors ------------

/// A word outside the content box.
///
/// Two tiers, and the harsher one is the one that cannot be argued with: a word past the
/// physical page edge is content the reader will never see, whatever the stylesheet intended.
/// The softer tier compares against the margins measured on the document itself, which is
/// where a wide table or a long URL first shows up.
///
/// Nothing is checked against the *bottom* of the content box: in paged media what does not
/// fit moves to the next page, so the only real vertical defect is running off the sheet,
/// and a running footer legitimately sits below the text.
fn detect_overflow(doc: &Doc) -> Vec<LayoutIssue> {
    let mut issues = Vec::new();

    for page in &doc.pages {
        let left = doc.insets.left;
        let right = page.width - doc.insets.overflow_right;

        let mut count = 0usize;
        let mut worst = 0.0f32;
        let mut worst_box = [0.0f32; 4];
        let mut off_page = false;

        for word in &page.words {
            let edge = (word.x1 - page.width)
                .max(-word.x0)
                .max(word.y1 - page.height)
                .max(-word.y0);
            let margin = (word.x1 - (right + SLACK_PT)).max((left - SLACK_PT) - word.x0);

            let excess = margin.max(edge);
            if excess <= 0.0 {
                continue;
            }

            count += 1;
            off_page |= edge > 1.0;
            if excess > worst {
                worst = excess;
                worst_box = [word.x0, word.y0, word.x1, word.y1];
            }
        }

        if count == 0 {
            continue;
        }

        let severity = if off_page { "error" } else { "warn" };
        let detail = format!(
            "{} word{} extend up to {:.0} pt past the content box{}",
            count,
            if count > 1 { "s" } else { "" },
            worst,
            if off_page { ", past the page edge" } else { "" }
        );

        let mut issue = LayoutIssue::new("overflow", page.number, severity, detail);
        issue.bbox = Some(worst_box);
        issues.push(issue);
    }

    issues
}

/// A page with no word at all. Confirmed on the pixels before being reported: a cover or a
/// full-page chart carries no text and is not an empty page.
fn detect_blank_pages(doc: &Doc) -> Vec<LayoutIssue> {
    let mut issues = Vec::new();

    for page in doc
        .pages
        .iter()
        .filter(|page| page.words.is_empty())
        // Each probe costs a rasterization, so a document made of empty pages cannot turn
        // one analysis into hundreds of pdftoppm runs
        .take(BLANK_PROBE_MAX_PAGES)
    {
        if !carries_no_ink(doc.pdf, page.number) {
            continue;
        }

        let last = page.number == doc.page_count;
        issues.push(LayoutIssue::new(
            "blank_page",
            page.number,
            if last { "warn" } else { "error" },
            if last {
                "The document ends on an empty page".to_string()
            } else {
                "Empty page in the middle of the document".to_string()
            },
        ));
    }

    issues
}

/// A last page holding almost nothing. Measured on the area the word boxes cover, not on
/// pixels, so an illustration does not make a three-line page look full.
fn detect_widow_page(doc: &Doc) -> Vec<LayoutIssue> {
    if doc.page_count < 2 {
        return Vec::new();
    }

    let last = match doc.pages.last() {
        // An empty last page is a blank page, and reporting it twice would double its cost
        Some(page) if !page.words.is_empty() => page,
        _ => return Vec::new(),
    };

    let mut others: Vec<f32> = doc
        .pages
        .iter()
        .filter(|page| page.number != last.number && !page.words.is_empty())
        .map(|page| doc.coverage(page))
        .collect();
    if others.is_empty() {
        return Vec::new();
    }

    let usual = median(&mut others);
    let coverage = doc.coverage(last);
    if usual <= 0.0 || coverage >= WIDOW_RATIO * usual {
        return Vec::new();
    }

    vec![LayoutIssue::new(
        "widow_page",
        last.number,
        "warn",
        format!(
            "Last page filled at {:.0} % of what the other pages carry",
            100.0 * coverage / usual
        ),
    )]
}

/// A heading alone at the foot of a page, its content starting on the next one.
///
/// Headings are read off the geometry rather than off the source: a line noticeably taller
/// than the body text is a heading whatever produced the PDF, which also covers the HTML and
/// template endpoints where there is no markdown to look at.
fn detect_orphan_headings(doc: &Doc) -> Vec<LayoutIssue> {
    let mut issues = Vec::new();

    for (index, page) in doc.pages.iter().enumerate() {
        let next_has_content = doc
            .pages
            .get(index + 1)
            .map(|next| !next.words.is_empty())
            .unwrap_or(false);
        if !next_has_content {
            continue;
        }

        let lines = doc.content_lines(page);
        let last = match lines.last() {
            Some(last) => last,
            None => continue,
        };

        if last.height < HEADING_RATIO * doc.body_height {
            continue;
        }

        // A heading in the middle of a sparse page is a heading, not an orphan
        let top = doc.insets.top;
        let bottom = page.height - doc.insets.bottom;
        if last.y0 < top + HEADING_BOTTOM_FRACTION * (bottom - top) {
            continue;
        }

        let mut issue = LayoutIssue::new(
            "orphan_heading",
            page.number,
            "warn",
            format!(
                "Heading alone at the foot of the page, its content starts on page {}",
                page.number + 1
            ),
        );
        issue.bbox = Some([last.x0, last.y0, last.x1, last.y1]);
        issues.push(issue);
    }

    issues
}

/// A table split across two pages, and tables too tall to ever be kept whole.
///
/// Tables are recognized by two signals that only occur together in a table: lines cut into
/// at least three cells by gaps far wider than a space, and those cells starting at the same
/// place on consecutive lines. Alignment alone is not enough — two paragraphs of repeated
/// text do line up — and that false positive is the one that would discredit the report.
fn detect_tables(doc: &Doc) -> Vec<LayoutIssue> {
    let mut issues = Vec::new();

    for (index, page) in doc.pages.iter().enumerate() {
        let lines = doc.content_lines(page);
        let blocks = tabular_blocks(lines);

        // A table taller than a page is a fact about the document, not a defect: it is
        // reported so the corrective pass knows it must not try to keep tables whole.
        let limit = TALL_TABLE_FRACTION * doc.text_height(page);
        if blocks
            .iter()
            .any(|block| block_height(lines, *block) > limit)
        {
            issues.push(LayoutIssue::new(
                "long_table",
                page.number,
                "info",
                "Table taller than the page: it cannot be kept on a single page".to_string(),
            ));
            continue;
        }

        let next = match doc.pages.get(index + 1) {
            Some(next) => next,
            None => continue,
        };
        let next_lines = doc.content_lines(next);
        let next_blocks = tabular_blocks(next_lines);

        // The split is only real when the table runs to the very bottom of one page and
        // resumes at the very top of the next
        let (tail, head) = match (blocks.last(), next_blocks.first()) {
            (Some(tail), Some(head))
                if tail.1 + 1 == lines.len() && head.0 == 0 && !next_lines.is_empty() =>
            {
                (*tail, *head)
            }
            _ => continue,
        };

        let across = match (lines.get(tail.1), next_lines.get(head.0)) {
            (Some(last), Some(first)) => shared_columns(last, first),
            _ => continue,
        };
        if across < MIN_SHARED_COLUMNS {
            continue;
        }

        let combined = block_height(lines, tail) + block_height(next_lines, head);
        if combined > TALL_TABLE_FRACTION * doc.text_height(page) {
            issues.push(LayoutIssue::new(
                "long_table",
                page.number,
                "info",
                "Table taller than the page: it cannot be kept on a single page".to_string(),
            ));
            continue;
        }

        issues.push(LayoutIssue::new(
            "split_table",
            page.number,
            "warn",
            format!(
                "Table split between pages {} and {} although it fits on one",
                page.number, next.number
            ),
        ));
    }

    issues
}

// ------------ Corrective CSS ------------

/// CSS that fixes what `analyze` found, and nothing else: every rule below is emitted only
/// when an issue in the report calls for it. An empty string means "nothing worth a second
/// render": the pipeline stops there.
pub fn corrective_css(report: &LayoutReport) -> String {
    let has = |kind: &str| report.issues.iter().any(|issue| issue.kind == kind);
    let severe = |kind: &str| {
        report
            .issues
            .iter()
            .any(|issue| issue.kind == kind && issue.severity == "error")
    };

    let mut css = String::new();

    if has("overflow") {
        // Content past the margin is nearly always a wide table, a long URL or a code line.
        // A fixed layout lets the columns shrink, and the break rules let the long tokens
        // wrap instead of running off the page.
        let scale = if severe("overflow") {
            "0.85em"
        } else {
            "0.95em"
        };
        css.push_str(&format!(
            "/* layout: content past the content box */\n\
             table {{ table-layout: fixed; width: 100%; font-size: {scale}; }}\n\
             th, td {{ overflow-wrap: break-word; word-break: break-word; }}\n\
             pre, code {{ white-space: pre-wrap; overflow-wrap: break-word; }}\n\
             a {{ overflow-wrap: break-word; word-break: break-word; }}\n\
             img {{ max-width: 100%; height: auto; }}\n"
        ));
    }

    if has("orphan_heading") {
        css.push_str(
            "/* layout: heading alone at the foot of a page */\n\
             h1, h2, h3, h4, h5, h6 { break-after: avoid; page-break-after: avoid; }\n\
             p, li { orphans: 3; widows: 3; }\n",
        );
    }

    // Keeping tables whole is only safe while none of them is taller than a page: the rule
    // would be dropped on such a table, or honoured at the price of a near-empty page.
    if has("split_table") && !has("long_table") {
        css.push_str(
            "/* layout: table split although it fits on one page */\n\
             table { break-inside: avoid; page-break-inside: avoid; }\n",
        );
    }

    if has("widow_page") {
        // Tighter leading and paragraph spacing, never a smaller font: the type size is what
        // the theme and the caller chose, and shrinking it would change the document.
        css.push_str(
            "/* layout: last page nearly empty */\n\
             body { line-height: 1.35; }\n\
             p, li { margin-top: 0.4em; margin-bottom: 0.4em; }\n",
        );
    }

    css
}

// ------------ The document as the detectors see it ------------

/// One line of text: the words the extractor put on the same baseline
struct Line {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    /// Median height of its words, which stands in for the font size
    height: f32,
    /// Horizontal span of each word, left to right
    spans: Vec<(f32, f32)>,
    /// Where each cell of the line starts. A cell boundary is a gap far wider than a space:
    /// that is what separates a table row from a sentence, and it is measured on the
    /// document's own typography rather than assumed.
    cells: Vec<f32>,
}

struct PageInfo {
    /// 1-based, as everywhere else in the API
    number: usize,
    width: f32,
    height: f32,
    words: Vec<Word>,
    lines: Vec<Line>,
    /// Area the word boxes cover, in square points
    ink: f32,
}

/// Distance from each page edge to the text, in points
struct Insets {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
    /// Right margin the overflow test compares against, deliberately the more permissive of
    /// the mirrored left margin and the tenth percentile of the measured right insets. Most
    /// documents have symmetric margins, which the left one measures with near certainty;
    /// the percentile is what keeps an asymmetric layout from reporting every full-width
    /// line as an overflow.
    overflow_right: f32,
}

struct Doc<'a> {
    pdf: &'a Path,
    page_count: usize,
    pages: Vec<PageInfo>,
    insets: Insets,
    /// Median word height: the body text size the rest is compared to
    body_height: f32,
    /// Where the running header ends and the running footer starts, when the document has
    /// them: both sit outside the text and must not be read as its first or last line.
    header_bottom: Option<f32>,
    footer_top: Option<f32>,
}

impl<'a> Doc<'a> {
    fn build(pdf: &'a Path, page_count: usize, boxes: Vec<PageBox>, words: Vec<Word>) -> Doc<'a> {
        let mut pages: Vec<PageInfo> = boxes
            .into_iter()
            .map(|page| PageInfo {
                number: page.page,
                width: page.width,
                height: page.height,
                words: Vec::new(),
                lines: Vec::new(),
                ink: 0.0,
            })
            .collect();

        for word in words {
            if !usable(&word) {
                continue;
            }
            if let Some(page) = pages.iter_mut().find(|page| page.number == word.page) {
                page.ink += (word.x1 - word.x0) * (word.y1 - word.y0);
                page.words.push(word);
            }
        }

        for page in &mut pages {
            page.lines = build_lines(&page.words);
        }

        let insets = measure_insets(&pages);
        let mut heights: Vec<f32> = pages
            .iter()
            .flat_map(|page| page.words.iter())
            .map(|word| word.y1 - word.y0)
            .collect();
        let body_height = median(&mut heights).max(1.0);

        // Cells can only be cut once the width of a space is known, and that is a property of
        // the whole document, not of one line
        let cell_gap = cell_gap(&pages);
        for page in &mut pages {
            for line in &mut page.lines {
                line.cells = cut_cells(&line.spans, cell_gap);
            }
        }

        let (header_bottom, footer_top) = margin_bands(&pages, body_height);

        Doc {
            pdf,
            page_count,
            pages,
            insets,
            body_height,
            header_bottom,
            footer_top,
        }
    }

    /// Height of the text area, measured rather than assumed: this is what tells a table
    /// that would fit on a page from one that never will.
    fn text_height(&self, page: &PageInfo) -> f32 {
        (page.height - self.insets.top - self.insets.bottom).max(1.0)
    }

    fn text_width(&self, page: &PageInfo) -> f32 {
        (page.width - self.insets.left - self.insets.right).max(1.0)
    }

    fn coverage(&self, page: &PageInfo) -> f32 {
        page.ink / (self.text_width(page) * self.text_height(page))
    }

    /// The lines of the page without its running header and footer, which sit outside the
    /// text and would otherwise be read as its first or last line.
    fn content_lines<'p>(&self, page: &'p PageInfo) -> &'p [Line] {
        let lines = &page.lines;

        let start = match self.header_bottom {
            Some(bottom) => lines.iter().take_while(|line| line.y1 <= bottom).count(),
            None => 0,
        };
        let end = match self.footer_top {
            Some(top) => lines.iter().take_while(|line| line.y0 < top).count(),
            None => lines.len(),
        };

        lines.get(start..end.max(start)).unwrap_or_default()
    }
}

/// Where the running header and footer live, if the document has any.
///
/// A trailing line is a footer when it is small, detached from the text *and* repeated at the
/// same height on most pages. The repetition is what makes the test safe: a page whose last
/// paragraph happens to fall low is not a footer, because the next page's does not fall in
/// exactly the same place.
fn margin_bands(pages: &[PageInfo], body_height: f32) -> (Option<f32>, Option<f32>) {
    let mut eligible = 0usize;
    let mut headers: Vec<f32> = Vec::new();
    let mut footers: Vec<f32> = Vec::new();

    for page in pages {
        if page.lines.len() < 3 {
            continue;
        }
        eligible += 1;

        let detached =
            |gap: f32, line: &Line| gap > 2.0 * body_height && line.height <= 1.1 * body_height;

        let first = &page.lines[0];
        if detached(page.lines[1].y0 - first.y1, first) {
            headers.push(first.y1);
        }

        let last = &page.lines[page.lines.len() - 1];
        if detached(last.y0 - page.lines[page.lines.len() - 2].y1, last) {
            footers.push(last.y0);
        }
    }

    if eligible < 2 {
        return (None, None);
    }

    (
        repeated(&mut headers, eligible).map(|y| y + 1.0),
        repeated(&mut footers, eligible).map(|y| y - 1.0),
    )
}

/// The position shared by most of the samples, or nothing when they do not agree
fn repeated(values: &mut [f32], pages: usize) -> Option<f32> {
    values.sort_by(f32::total_cmp);

    let mut best: Option<(usize, f32)> = None;
    for (index, value) in values.iter().enumerate() {
        let cluster = values[index..]
            .iter()
            .take_while(|other| **other - value <= COLUMN_TOLERANCE_PT)
            .count();
        if best.map(|(count, _)| cluster > count).unwrap_or(true) {
            best = Some((cluster, *value));
        }
    }

    match best {
        Some((count, value)) if count * 5 >= pages * 3 => Some(value),
        _ => None,
    }
}

/// Boxes the extractor could not place are dropped rather than trusted
fn usable(word: &Word) -> bool {
    [word.x0, word.y0, word.x1, word.y1]
        .iter()
        .all(|value| value.is_finite())
        && word.x1 > word.x0
        && word.y1 > word.y0
}

/// Group the words of a page into lines, top to bottom
fn build_lines(words: &[Word]) -> Vec<Line> {
    let mut ordered: Vec<&Word> = words.iter().collect();
    ordered.sort_by(|a, b| a.y0.total_cmp(&b.y0).then(a.x0.total_cmp(&b.x0)));

    let mut groups: Vec<Vec<&Word>> = Vec::new();
    for word in ordered {
        let joined = match groups.last_mut() {
            Some(group) => {
                let y0 = group.iter().map(|w| w.y0).fold(f32::MAX, f32::min);
                let y1 = group.iter().map(|w| w.y1).fold(f32::MIN, f32::max);
                let overlap = (y1.min(word.y1) - y0.max(word.y0)).max(0.0);
                if overlap >= 0.5 * (word.y1 - word.y0).min(y1 - y0) {
                    group.push(word);
                    true
                } else {
                    false
                }
            }
            None => false,
        };
        if !joined {
            groups.push(vec![word]);
        }
    }

    groups
        .into_iter()
        .map(|group| {
            let mut spans: Vec<(f32, f32)> = group.iter().map(|word| (word.x0, word.x1)).collect();
            spans.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut heights: Vec<f32> = group.iter().map(|word| word.y1 - word.y0).collect();

            Line {
                x0: group.iter().map(|w| w.x0).fold(f32::MAX, f32::min),
                y0: group.iter().map(|w| w.y0).fold(f32::MAX, f32::min),
                x1: group.iter().map(|w| w.x1).fold(f32::MIN, f32::max),
                y1: group.iter().map(|w| w.y1).fold(f32::MIN, f32::max),
                height: median(&mut heights),
                spans,
                cells: Vec::new(),
            }
        })
        .collect()
}

/// Gap above which two words belong to different cells rather than to the same sentence.
///
/// Derived from the median inter-word gap of the document: a space is a space whatever the
/// font and the type size, and cell padding is several times wider. Measured on real pandoc
/// output, prose gaps sit around 3 pt and table cell gaps above 20 pt.
fn cell_gap(pages: &[PageInfo]) -> f32 {
    let mut gaps: Vec<f32> = Vec::new();
    for page in pages {
        for line in &page.lines {
            for pair in line.spans.windows(2) {
                gaps.push(pair[1].0 - pair[0].1);
            }
        }
    }

    (3.0 * median(&mut gaps)).max(MIN_CELL_GAP_PT)
}

/// Left edge of each cell: the start of the line, then every word that follows a wide gap
fn cut_cells(spans: &[(f32, f32)], gap: f32) -> Vec<f32> {
    let mut cells = Vec::new();

    for (index, span) in spans.iter().enumerate() {
        match index
            .checked_sub(1)
            .and_then(|previous| spans.get(previous))
        {
            Some(previous) if span.0 - previous.1 <= gap => continue,
            _ => cells.push(span.0),
        }
    }

    cells
}

/// Margins read off the document itself: the second percentile of each inset, so a handful
/// of overflowing words cannot move the box they are measured against.
fn measure_insets(pages: &[PageInfo]) -> Insets {
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut top = Vec::new();
    let mut bottom = Vec::new();

    for page in pages {
        for word in &page.words {
            left.push(word.x0);
            right.push(page.width - word.x1);
            top.push(word.y0);
            bottom.push(page.height - word.y1);
        }
    }

    let left_inset = quantile(&mut left, 0.02).max(0.0);

    Insets {
        left: left_inset,
        right: quantile(&mut right, 0.02).max(0.0),
        top: quantile(&mut top, 0.02).max(0.0),
        bottom: quantile(&mut bottom, 0.02).max(0.0),
        overflow_right: left_inset.min(quantile(&mut right, 0.10)).max(0.0),
    }
}

// ------------ Tables ------------

/// Runs of consecutive lines that share a column grid, as inclusive index ranges
fn tabular_blocks(lines: &[Line]) -> Vec<(usize, usize)> {
    let mut blocks: Vec<(usize, usize)> = Vec::new();

    for index in 1..lines.len() {
        if shared_columns(&lines[index - 1], &lines[index]) < MIN_SHARED_COLUMNS {
            continue;
        }
        match blocks.last_mut() {
            Some(block) if block.1 == index - 1 => block.1 = index,
            _ => blocks.push((index - 1, index)),
        }
    }

    blocks
}

/// Cells two lines start at the same place. A line that is not cut into cells cannot match:
/// this is what keeps two paragraphs whose words happened to line up from being read as a
/// table, which was the noisiest failure this detector could have.
fn shared_columns(a: &Line, b: &Line) -> usize {
    if a.cells.len() < MIN_SHARED_COLUMNS || b.cells.len() < MIN_SHARED_COLUMNS {
        return 0;
    }

    a.cells
        .iter()
        .filter(|cell| {
            b.cells
                .iter()
                .any(|other| (other - *cell).abs() <= COLUMN_TOLERANCE_PT)
        })
        .count()
}

fn block_height(lines: &[Line], block: (usize, usize)) -> f32 {
    match (lines.get(block.0), lines.get(block.1)) {
        (Some(first), Some(last)) => (last.y1 - first.y0).max(0.0),
        _ => 0.0,
    }
}

// ------------ Blank pages ------------

/// Whether a page without text is also without ink.
///
/// The page is rasterized to a raw PPM on stdout and every pixel is compared to white. This
/// is exact rather than a threshold on a compressed size: a chart, a logo or a coloured
/// background makes the page not blank, and nothing else does.
///
/// Best effort by design: when the probe cannot run, the page is *not* reported. A missed
/// blank page costs nothing, a full-page illustration reported as empty discredits the whole
/// report.
fn carries_no_ink(pdf: &Path, page: usize) -> bool {
    let path = match helpers::path_to_str(pdf) {
        Ok(path) => path.to_string(),
        Err(_) => return false,
    };

    let output = helpers::run_capture(
        Command::new("pdftoppm")
            .arg("-r")
            .arg(BLANK_PROBE_DPI.to_string())
            .arg("-f")
            .arg(page.to_string())
            .arg("-l")
            .arg(page.to_string())
            .arg(path),
        "pdftoppm",
        "pdftoppm failed",
    );

    match output {
        Ok(output) => match ppm_pixels(&output.stdout) {
            Some(pixels) => !pixels.is_empty() && pixels.iter().all(|value| *value == 0xff),
            None => false,
        },
        Err(e) => {
            warn!("Could not check whether page {page} is blank: {e:?}");
            false
        }
    }
}

/// Pixels of a binary PPM: magic, width, height and maximum value, then one separator
fn ppm_pixels(data: &[u8]) -> Option<&[u8]> {
    if !data.starts_with(b"P6") {
        return None;
    }

    let mut index = 0usize;
    let mut fields = 0usize;
    while fields < 4 {
        while index < data.len() && data[index].is_ascii_whitespace() {
            index += 1;
        }
        // A comment is legal anywhere in the header
        if data.get(index) == Some(&b'#') {
            while index < data.len() && data[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if index >= data.len() {
            return None;
        }
        while index < data.len() && !data[index].is_ascii_whitespace() {
            index += 1;
        }
        fields += 1;
    }

    data.get(index + 1..)
}

// ------------ Statistics ------------

fn median(values: &mut [f32]) -> f32 {
    quantile(values, 0.5)
}

fn quantile(values: &mut [f32], q: f32) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f32::total_cmp);
    let index = ((values.len() - 1) as f32 * q).round() as usize;
    values[index.min(values.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::LayoutReport;

    const A4_WIDTH: f32 = 595.276;
    const A4_HEIGHT: f32 = 841.89;
    /// 2 cm page margin plus the 2 em body padding of the default stylesheet
    const MARGIN: f32 = 92.0;
    const BODY: f32 = 14.0;
    const PITCH: f32 = 22.0;
    /// Where a running footer sits: inside the bottom page margin, below the text
    const FOOTER_Y: f32 = 795.0;

    fn word(page: usize, text: &str, x0: f32, y0: f32, width: f32, height: f32) -> Word {
        Word {
            page,
            text: text.to_string(),
            x0,
            y0,
            x1: x0 + width,
            y1: y0 + height,
        }
    }

    /// A page of plain paragraphs, ragged right, from the top margin down
    fn full_page(page: usize, lines: usize) -> Vec<Word> {
        let mut words = Vec::new();
        for index in 0..lines {
            let y = MARGIN + index as f32 * PITCH;
            let mut x = MARGIN;
            // Widths shift by three points from one line to the next, so no column of a line
            // ever lands within tolerance of a column of the next: this is prose, not a table
            for slot in 0..6 {
                let width = 30.0 + (index % 7) as f32 * 3.0 + (slot % 3) as f32 * 5.0;
                words.push(word(page, "lorem", x, y, width, BODY));
                x += width + 4.0;
            }
        }
        words
    }

    fn boxes(pages: usize) -> Vec<PageBox> {
        (1..=pages)
            .map(|page| PageBox {
                page,
                width: A4_WIDTH,
                height: A4_HEIGHT,
            })
            .collect()
    }

    fn doc(pages: usize, words: Vec<Word>) -> Doc<'static> {
        Doc::build(Path::new("/nonexistent.pdf"), pages, boxes(pages), words)
    }

    fn report(issues: Vec<LayoutIssue>) -> LayoutReport {
        LayoutReport {
            pages: 2,
            score: score_for(&issues),
            issues,
            passes: None,
        }
    }

    #[test]
    fn a_clean_document_reports_nothing() {
        let mut words = full_page(1, 30);
        words.extend(full_page(2, 30));

        let doc = doc(2, words);
        assert!(detect_overflow(&doc).is_empty());
        assert!(detect_widow_page(&doc).is_empty());
        assert!(detect_orphan_headings(&doc).is_empty());
        assert!(detect_tables(&doc).is_empty());
    }

    #[test]
    fn a_word_past_the_margin_is_an_overflow_and_past_the_page_an_error() {
        let mut words = full_page(1, 30);
        // Still on the page, but well into the right margin
        words.push(word(1, "wide", A4_WIDTH - MARGIN + 20.0, 300.0, 40.0, BODY));
        let issues = detect_overflow(&doc(1, words));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, "warn");
        assert!(issues[0].bbox.is_some());

        let mut words = full_page(1, 30);
        words.push(word(1, "wide", A4_WIDTH - 10.0, 300.0, 120.0, BODY));
        let issues = detect_overflow(&doc(1, words));
        assert_eq!(issues[0].severity, "error");
        assert!(issues[0].detail.contains("past the page edge"));
    }

    #[test]
    fn a_word_a_hair_outside_the_box_is_not_an_issue() {
        let mut words = full_page(1, 30);
        words.push(word(1, "x", A4_WIDTH - MARGIN, 300.0, SLACK_PT - 0.5, BODY));
        assert!(detect_overflow(&doc(1, words)).is_empty());
    }

    #[test]
    fn a_running_footer_is_neither_an_overflow_nor_the_last_line_of_the_page() {
        let mut words = full_page(1, 26);
        words.extend(full_page(2, 26));
        // Page numbers live below the content box, in the page margin
        words.push(word(1, "1", A4_WIDTH / 2.0, FOOTER_Y, 6.0, 10.0));
        words.push(word(2, "2", A4_WIDTH / 2.0, FOOTER_Y, 6.0, 10.0));

        let doc = doc(2, words);
        assert!(detect_overflow(&doc).is_empty());
        assert_eq!(doc.footer_top, Some(FOOTER_Y - 1.0));
        assert_eq!(doc.content_lines(&doc.pages[0]).len(), 26);
    }

    #[test]
    fn a_last_page_of_three_lines_is_a_widow() {
        let mut words = full_page(1, 30);
        words.extend(full_page(2, 30));
        words.extend(full_page(3, 3));

        let issues = detect_widow_page(&doc(3, words));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].page, 3);

        // A last page that is merely shorter than the others is not
        let mut words = full_page(1, 30);
        words.extend(full_page(2, 20));
        assert!(detect_widow_page(&doc(2, words)).is_empty());
    }

    #[test]
    fn a_single_page_document_has_no_widow() {
        assert!(detect_widow_page(&doc(1, full_page(1, 3))).is_empty());
    }

    /// A heading is a taller line, placed after the last body line of the page
    fn heading(page: usize, y0: f32) -> Word {
        word(page, "Chapitre", MARGIN, y0, 90.0, 24.0)
    }

    #[test]
    fn a_heading_at_the_foot_of_a_page_is_orphaned() {
        let mut words = full_page(1, 26);
        words.push(heading(1, MARGIN + 26.0 * PITCH));
        words.extend(full_page(2, 29));

        let issues = detect_orphan_headings(&doc(2, words));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].page, 1);
        assert!(issues[0].bbox.is_some());

        // The same heading on a page that ends early is where the author put it
        let mut words = full_page(1, 10);
        words.push(heading(1, MARGIN + 10.0 * PITCH));
        words.extend(full_page(2, 29));
        assert!(detect_orphan_headings(&doc(2, words)).is_empty());
    }

    #[test]
    fn a_footer_does_not_hide_the_orphan_heading_above_it() {
        let mut words = full_page(1, 26);
        words.push(heading(1, MARGIN + 26.0 * PITCH));
        words.push(word(1, "1", A4_WIDTH / 2.0, FOOTER_Y, 6.0, 10.0));
        words.extend(full_page(2, 29));
        words.push(word(2, "2", A4_WIDTH / 2.0, FOOTER_Y, 6.0, 10.0));

        assert_eq!(detect_orphan_headings(&doc(2, words)).len(), 1);
    }

    #[test]
    fn the_last_page_never_carries_an_orphan_heading() {
        let mut words = full_page(1, 29);
        words.extend(full_page(2, 26));
        words.push(heading(2, MARGIN + 26.0 * PITCH));

        assert!(detect_orphan_headings(&doc(2, words)).is_empty());
    }

    /// Rows of a four column table, at fixed column positions
    fn table_rows(page: usize, rows: usize, first_y: f32) -> Vec<Word> {
        let mut words = Vec::new();
        for row in 0..rows {
            let y = first_y + row as f32 * (BODY + 6.0);
            for (column, x) in [MARGIN, MARGIN + 110.0, MARGIN + 220.0, MARGIN + 330.0]
                .iter()
                .enumerate()
            {
                words.push(word(page, &format!("c{column}"), *x, y, 40.0, BODY));
            }
        }
        words
    }

    #[test]
    fn a_table_cut_between_two_pages_is_reported_once() {
        let mut words = full_page(1, 20);
        words.extend(table_rows(1, 4, MARGIN + 20.0 * (BODY + 8.0)));
        words.extend(table_rows(2, 4, MARGIN));
        words.extend(full_page(2, 10).into_iter().map(|mut w| {
            w.y0 += 400.0;
            w.y1 += 400.0;
            w
        }));

        let issues = detect_tables(&doc(2, words));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].kind, "split_table");
        assert_eq!(issues[0].page, 1);
    }

    #[test]
    fn a_table_taller_than_a_page_is_reported_as_such_and_not_as_a_split() {
        let mut words = table_rows(1, 34, MARGIN);
        words.extend(table_rows(2, 10, MARGIN));

        let issues = detect_tables(&doc(2, words));
        assert!(issues.iter().all(|issue| issue.kind == "long_table"));
        assert!(issues.iter().all(|issue| issue.severity == "info"));
    }

    #[test]
    fn prose_and_lists_are_never_read_as_tables() {
        // Bullets: two shared columns, the marker and the text, never three
        let mut words = Vec::new();
        for index in 0..12 {
            let y = MARGIN + index as f32 * (BODY + 6.0);
            words.push(word(1, "-", MARGIN, y, 6.0, BODY));
            words.push(word(1, "item", MARGIN + 14.0, y, 40.0, BODY));
            words.push(word(
                1,
                "suite",
                MARGIN + 60.0 + (index % 5) as f32 * 7.0,
                y,
                50.0,
                BODY,
            ));
        }
        words.extend(full_page(2, 20));

        assert!(detect_tables(&doc(2, words)).is_empty());
    }

    fn overflows(pages: usize, severity: &str) -> Vec<LayoutIssue> {
        (1..=pages)
            .map(|page| LayoutIssue::new("overflow", page, severity, String::new()))
            .collect()
    }

    #[test]
    fn the_score_is_stable_monotone_and_bounded() {
        assert_eq!(score_for(&[]), 100);

        // More of the same defect is worse, and a worse severity is worse still
        assert!(score_for(&overflows(2, "warn")) < score_for(&overflows(1, "warn")));
        assert!(score_for(&overflows(1, "error")) < score_for(&overflows(1, "warn")));
        assert!(score_for(&overflows(1, "warn")) < 100);

        // Severity dominates repetition: what the corrective loop needs to see as progress
        assert!(score_for(&overflows(3, "warn")) > score_for(&overflows(1, "error")));

        // An informational issue costs nothing
        assert_eq!(
            score_for(&[LayoutIssue::new("long_table", 1, "info", String::new())]),
            100
        );

        // Twenty issues do not wrap around or saturate into a better score
        assert_eq!(score_for(&overflows(20, "error")), 100 - 25 - 4 * 2);

        let mixed: Vec<LayoutIssue> = (1..=20)
            .flat_map(|page| {
                [
                    LayoutIssue::new("overflow", page, "error", String::new()),
                    LayoutIssue::new("blank_page", page, "error", String::new()),
                    LayoutIssue::new("widow_page", page, "warn", String::new()),
                    LayoutIssue::new("split_table", page, "warn", String::new()),
                    LayoutIssue::new("orphan_heading", page, "warn", String::new()),
                ]
            })
            .collect();
        assert_eq!(score_for(&mixed), 0);
    }

    #[test]
    fn corrective_css_only_answers_what_the_report_holds() {
        assert!(corrective_css(&report(Vec::new())).is_empty());

        let css = corrective_css(&report(vec![LayoutIssue::new(
            "overflow",
            1,
            "warn",
            String::new(),
        )]));
        assert!(css.contains("table-layout: fixed"));
        assert!(css.contains("0.95em"));
        assert!(!css.contains("break-after"));
        assert!(!css.contains("line-height"));

        // A hard overflow shrinks the table further
        let css = corrective_css(&report(vec![LayoutIssue::new(
            "overflow",
            1,
            "error",
            String::new(),
        )]));
        assert!(css.contains("0.85em"));
    }

    #[test]
    fn tables_are_kept_whole_only_when_that_is_possible() {
        let split = LayoutIssue::new("split_table", 1, "warn", String::new());
        assert!(corrective_css(&report(vec![split.clone()])).contains("break-inside: avoid"));

        let with_long = vec![
            split,
            LayoutIssue::new("long_table", 3, "info", String::new()),
        ];
        assert!(!corrective_css(&report(with_long)).contains("break-inside: avoid"));
    }

    #[test]
    fn a_widow_page_is_fixed_without_touching_the_type_size() {
        let css = corrective_css(&report(vec![LayoutIssue::new(
            "widow_page",
            2,
            "warn",
            String::new(),
        )]));
        assert!(css.contains("line-height: 1.35"));
        assert!(!css.contains("font-size"));
    }

    #[test]
    fn lines_are_grouped_by_baseline_not_by_order() {
        let words = vec![
            word(1, "b", 100.0, 100.0, 20.0, BODY),
            word(1, "a", 50.0, 101.0, 20.0, BODY),
            word(1, "c", 50.0, 130.0, 20.0, BODY),
        ];
        let lines = build_lines(&words);

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].spans, vec![(50.0, 70.0), (100.0, 120.0)]);
        assert!((lines[0].height - BODY).abs() < 0.01);
    }

    #[test]
    fn a_ppm_page_is_blank_only_when_every_pixel_is_white() {
        let mut white = b"P6\n2 1\n255\n".to_vec();
        white.extend_from_slice(&[0xff; 6]);
        assert!(ppm_pixels(&white).unwrap().iter().all(|v| *v == 0xff));

        let mut inked = b"P6 # comment\n2 1\n255\n".to_vec();
        inked.extend_from_slice(&[0xff, 0xff, 0xff, 0x00, 0x10, 0x20]);
        assert!(!ppm_pixels(&inked).unwrap().iter().all(|v| *v == 0xff));

        // Anything that is not a binary PPM leaves the page unreported
        assert!(ppm_pixels(b"P5\n2 1\n255\n\xff").is_none());
        assert!(ppm_pixels(b"").is_none());
        assert!(ppm_pixels(b"P6\n2 1\n").is_none());
    }

    #[test]
    fn quantiles_survive_an_empty_sample() {
        assert_eq!(quantile(&mut [], 0.5), 0.0);
        assert_eq!(median(&mut [3.0, 1.0, 2.0]), 2.0);
        assert_eq!(quantile(&mut [1.0, 2.0, 3.0, 4.0], 0.02), 1.0);
    }
}

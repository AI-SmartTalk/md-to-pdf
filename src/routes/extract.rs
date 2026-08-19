//! PDF → structured text, for a document the service did not write itself.
//!
//! A PDF does not carry its structure: it carries glyphs at coordinates. Headings, lists and
//! tables are *inferred* here from the shape `pdftotext -layout` gives back, and every
//! heuristic below abstains when it is not sure. A paragraph wrongly promoted to a heading is
//! worse than a heading left as a paragraph, because the caller has no way of telling that
//! the structure was invented — so in doubt this route hands back plain text.

use crate::auth::PublicOrKey;
use crate::exec;
use crate::helpers;
use crate::types::AppError;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

/// One call may not return more than this many characters. A thousand-page report otherwise
/// builds a JSON body nothing on the other end can hold in memory either.
const MAX_CONTENT_CHARS: usize = 400_000;

/// A heading is a label, not a sentence. Beyond this many words the line is prose.
const MAX_HEADING_WORDS: usize = 12;

/// Two aligned spaces are a column separator; one is word spacing
const MIN_COLUMN_GAP: usize = 2;

/// Under this many characters per page the text layer is too thin to be the real content
const THIN_TEXT_PER_PAGE: usize = 10;

#[derive(Deserialize)]
pub struct ExtractRequest {
    /// `asset://as_…` or `/download/<client_id>/<name>.pdf`
    pub pdf: String,
    /// `"markdown"` (default), `"text"` or `"json"`
    pub format: Option<String>,
    /// `"1"`, `"2-5"` or `"all"`; absent means the whole document
    pub pages: Option<String>,
    /// Keep the column layout of the page (default). Turning it off gives reading order
    /// instead, at the cost of every table.
    pub layout: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ExtractResponse {
    /// Echoed back so a caller that omitted it knows what it got
    pub format: String,
    /// Number of pages actually represented in `content`
    pub pages: usize,
    /// A string for `markdown` and `text`, an array of pages of blocks for `json`
    pub content: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tables: Option<Vec<Table>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,
}

/// A table the column heuristic recognised, given raw so a caller can reshape it without
/// parsing the Markdown back
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Table {
    /// 1-based page it was found on
    pub page: usize,
    pub rows: Vec<Vec<String>>,
}

#[post("/extract", format = "json", data = "<req>")]
pub async fn extract(
    _key: PublicOrKey,
    req: Json<ExtractRequest>,
) -> Result<Json<ExtractResponse>, AppError> {
    let req = req.into_inner();

    // A typo in "format" must not queue behind twenty renders to be told about
    Params::parse(&req)?;

    let response = exec::offload(move || run(req)).await?;
    Ok(Json(response))
}

/// All the blocking work. Public and free of Rocket: the asynchronous job dispatcher calls
/// it as it is.
pub fn run(req: ExtractRequest) -> Result<ExtractResponse, AppError> {
    let params = Params::parse(&req)?;
    let source = helpers::resolve_pdf_source(&req.pdf)?;

    // An encrypted source is refused here as a 400 rather than escaping as a 500 further
    // down: pdfinfo is the first thing that touches the caller's file.
    let total = crate::pdfops::page_count(&source)
        .map_err(crate::routes::compress::name_encrypted_source)?;
    if total == 0 {
        return Err(AppError::BadRequest(
            "This document has no page to extract".to_string(),
        ));
    }

    let (first, last) = params.pages.resolve(total)?;
    let raw = pdftotext(&source, first, last, params.layout)?;

    let (pages, truncated) = within_budget(&raw, first);
    let mut warnings = Vec::new();

    if truncated {
        warnings.push(format!(
            "truncated: the text stops after page {} because one call returns at most {} \
             characters; ask for the next pages with \"pages\"",
            pages.last().map(|(page, _)| *page).unwrap_or(first),
            MAX_CONTENT_CHARS
        ));
    }

    if let Some(warning) = scan_warning(&pages) {
        warnings.push(warning);
    }

    let (content, tables) = match params.format {
        Format::Text => (serde_json::Value::String(plain_text(&pages)), Vec::new()),
        Format::Markdown => {
            let parsed = parse(&pages);
            let markdown = parsed
                .iter()
                .map(|(_, blocks)| render_markdown(blocks))
                .filter(|page| !page.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n");
            (serde_json::Value::String(markdown), tables_of(&parsed))
        }
        Format::Json => {
            let parsed = parse(&pages);
            let pages_json = parsed
                .iter()
                .map(|(page, blocks)| {
                    serde_json::json!({
                        "page": page,
                        "blocks": blocks.iter().map(Block::to_json).collect::<Vec<_>>(),
                    })
                })
                .collect::<Vec<_>>();
            (serde_json::Value::Array(pages_json), tables_of(&parsed))
        }
    };

    Ok(ExtractResponse {
        format: params.format.as_str().to_string(),
        pages: pages.len(),
        content,
        tables: if tables.is_empty() {
            None
        } else {
            Some(tables)
        },
        truncated: if truncated { Some(true) } else { None },
        warnings: if warnings.is_empty() {
            None
        } else {
            Some(warnings)
        },
    })
}

// ------------ Request parameters ------------

struct Params {
    format: Format,
    pages: Pages,
    layout: bool,
}

impl Params {
    fn parse(req: &ExtractRequest) -> Result<Params, AppError> {
        Ok(Params {
            format: Format::parse(req.format.as_deref())?,
            pages: match req.pages.as_deref() {
                Some(spec) => Pages::parse(spec)?,
                None => Pages::All,
            },
            layout: req.layout.unwrap_or(true),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Format {
    Markdown,
    Text,
    Json,
}

impl Format {
    fn parse(spec: Option<&str>) -> Result<Format, AppError> {
        let spec = match spec {
            Some(spec) => spec.trim(),
            None => return Ok(Format::Markdown),
        };

        if spec.eq_ignore_ascii_case("markdown") || spec.eq_ignore_ascii_case("md") {
            Ok(Format::Markdown)
        } else if spec.eq_ignore_ascii_case("text") || spec.eq_ignore_ascii_case("txt") {
            Ok(Format::Text)
        } else if spec.eq_ignore_ascii_case("json") {
            Ok(Format::Json)
        } else {
            Err(AppError::BadRequest(format!(
                "\"format\" must be \"markdown\", \"text\" or \"json\", not \"{}\"",
                clip(spec)
            )))
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Format::Markdown => "markdown",
            Format::Text => "text",
            Format::Json => "json",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Pages {
    All,
    Range { first: usize, last: usize },
}

impl Pages {
    fn parse(spec: &str) -> Result<Pages, AppError> {
        let spec = spec.trim();

        if spec.eq_ignore_ascii_case("all") {
            return Ok(Pages::All);
        }

        let (head, tail) = match spec.split_once('-') {
            Some((head, tail)) => (head.trim(), tail.trim()),
            None => (spec, spec),
        };

        let (first, last) = match (head.parse::<usize>().ok(), tail.parse::<usize>().ok()) {
            (Some(first), Some(last)) if first >= 1 && last >= 1 => (first, last),
            _ => {
                return Err(AppError::BadRequest(format!(
                    "\"pages\" must be a page number, a range such as \"2-5\", or \"all\", \
                     not \"{}\"",
                    clip(spec)
                )))
            }
        };

        if last < first {
            return Err(AppError::BadRequest(format!(
                "\"pages\": {} is before {}",
                last, first
            )));
        }

        Ok(Pages::Range { first, last })
    }

    fn resolve(self, count: usize) -> Result<(usize, usize), AppError> {
        match self {
            Pages::All => Ok((1, count)),
            Pages::Range { first, last } => {
                if first > count || last > count {
                    Err(AppError::BadRequest(format!(
                        "Pages {}-{} are outside the document, which has {} page{}",
                        first,
                        last,
                        count,
                        if count > 1 { "s" } else { "" }
                    )))
                } else {
                    Ok((first, last))
                }
            }
        }
    }
}

/// Echo a rejected value back without letting it grow the error body
fn clip(value: &str) -> String {
    value.chars().take(32).collect()
}

// ------------ Extraction ------------

fn pdftotext(pdf: &Path, first: usize, last: usize, layout: bool) -> Result<String, AppError> {
    let mut cmd = Command::new("pdftotext");
    cmd.arg("-enc")
        .arg("UTF-8")
        .arg("-f")
        .arg(first.to_string())
        .arg("-l")
        .arg(last.to_string());

    if layout {
        cmd.arg("-layout");
    }

    let output = helpers::run_capture(
        cmd.arg(helpers::path_to_str(pdf)?).arg("-"),
        "pdftotext",
        "Text extraction failed",
    )?;

    // pdftotext writes what it could decode; a lossy byte in a broken font must not fail
    // the whole extraction
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Split the form-feed separated output into numbered pages, stopping at the character
/// budget. Truncating at a page boundary rather than mid-sentence is what makes the
/// continuation call (`"pages": "N-…"`) obvious to the caller.
fn within_budget(raw: &str, first: usize) -> (Vec<(usize, String)>, bool) {
    let mut chunks: Vec<&str> = raw.split('\u{000C}').collect();
    // pdftotext ends the last page with a form feed too, leaving an empty tail
    if chunks.last().is_some_and(|tail| tail.trim().is_empty()) {
        chunks.pop();
    }

    let mut pages = Vec::with_capacity(chunks.len());
    let mut used = 0usize;

    for (index, chunk) in chunks.iter().enumerate() {
        let length = chunk.chars().count();

        if used + length > MAX_CONTENT_CHARS {
            if !pages.is_empty() {
                return (pages, true);
            }
            // A single page over the whole budget still has to come back with something
            let clipped: String = chunk.chars().take(MAX_CONTENT_CHARS).collect();
            pages.push((first + index, clipped));
            return (pages, true);
        }

        used += length;
        pages.push((first + index, (*chunk).to_string()));
    }

    (pages, false)
}

/// A scan has no text layer, and an empty `content` with no explanation is the worst answer
/// this route could give: it looks like the document was empty.
fn scan_warning(pages: &[(usize, String)]) -> Option<String> {
    if pages.is_empty() {
        return None;
    }

    let characters: usize = pages
        .iter()
        .map(|(_, text)| text.chars().filter(|c| !c.is_whitespace()).count())
        .sum();

    if characters == 0 {
        return Some(format!(
            "No extractable text on {} page{}: this document is almost certainly a scan. \
             Run POST /api/ocr on it first, then extract the result.",
            pages.len(),
            if pages.len() > 1 { "s" } else { "" }
        ));
    }

    if characters < THIN_TEXT_PER_PAGE * pages.len() {
        return Some(format!(
            "Only {} characters of text over {} pages: the text layer is too thin to be the \
             content of the document. POST /api/ocr would give more.",
            characters,
            pages.len()
        ));
    }

    None
}

fn plain_text(pages: &[(usize, String)]) -> String {
    pages
        .iter()
        .map(|(_, text)| text.trim_matches('\n'))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn parse(pages: &[(usize, String)]) -> Vec<(usize, Vec<Block>)> {
    let average = average_line_length(pages);
    pages
        .iter()
        .map(|(page, text)| (*page, blocks_of(text, average)))
        .collect()
}

fn tables_of(parsed: &[(usize, Vec<Block>)]) -> Vec<Table> {
    let mut tables = Vec::new();
    for (page, blocks) in parsed {
        for block in blocks {
            if let Block::Table(rows) = block {
                tables.push(Table {
                    page: *page,
                    rows: rows.clone(),
                });
            }
        }
    }
    tables
}

// ------------ Structure heuristics ------------
//
// Pure functions on purpose: this is where the value of the route is, and this is where it
// breaks. Everything below is tested without a PDF in sight.

#[derive(Debug, Clone, PartialEq)]
enum Block {
    Heading { level: u8, text: String },
    Paragraph(String),
    List { ordered: bool, items: Vec<String> },
    Table(Vec<Vec<String>>),
}

impl Block {
    fn to_json(&self) -> serde_json::Value {
        match self {
            Block::Heading { level, text } => {
                serde_json::json!({"type": "heading", "level": level, "text": text})
            }
            Block::Paragraph(text) => serde_json::json!({"type": "paragraph", "text": text}),
            Block::List { ordered, items } => {
                serde_json::json!({"type": "list", "ordered": ordered, "items": items})
            }
            Block::Table(rows) => serde_json::json!({"type": "table", "rows": rows}),
        }
    }
}

/// The yardstick a heading has to be shorter than. Measured over the whole extraction so a
/// title page full of short lines does not make every line of it a heading.
fn average_line_length(pages: &[(usize, String)]) -> usize {
    let mut total = 0usize;
    let mut count = 0usize;

    for (_, text) in pages {
        for line in text.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                total += trimmed.chars().count();
                count += 1;
            }
        }
    }

    total.checked_div(count).unwrap_or(0)
}

fn blocks_of(text: &str, average: usize) -> Vec<Block> {
    groups(text)
        .into_iter()
        .map(|group| block_of(&group, average))
        .collect()
}

/// Runs of consecutive non-blank lines. Leading whitespace is kept: it is the only signal
/// the column detector has.
fn groups(text: &str) -> Vec<Vec<&str>> {
    let mut groups = Vec::new();
    let mut current: Vec<&str> = Vec::new();

    for line in text.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            if !current.is_empty() {
                groups.push(std::mem::take(&mut current));
            }
        } else {
            current.push(line);
        }
    }

    if !current.is_empty() {
        groups.push(current);
    }

    groups
}

/// The order is the arbitration: a list marker outranks column alignment, and both outrank
/// the heading heuristic, which is the one most likely to be wrong.
fn block_of(lines: &[&str], average: usize) -> Block {
    if let Some(list) = list_block(lines) {
        return list;
    }

    if let Some(rows) = table_rows(lines) {
        return Block::Table(rows);
    }

    if lines.len() == 1 {
        if let Some(level) = heading_level(lines[0], average) {
            return Block::Heading {
                level,
                text: lines[0].trim().to_string(),
            };
        }
    }

    Block::Paragraph(
        lines
            .iter()
            .map(|line| line.trim())
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// A line alone between two blank lines, shorter than the average line, holding no sentence.
/// All capitals promote it one level, because a document that shouts a line is titling it.
fn heading_level(line: &str, average: usize) -> Option<u8> {
    let text = line.trim();

    if text.is_empty() || average == 0 || text.chars().count() >= average {
        return None;
    }

    // A bare number is a page footer, not a title
    if !text.chars().any(char::is_alphabetic) {
        return None;
    }

    if text.split_whitespace().count() > MAX_HEADING_WORDS {
        return None;
    }

    // Terminal punctuation means a sentence, and a sentence is prose
    if text.ends_with(['.', ',', ';', '!', '?']) {
        return None;
    }

    Some(if is_all_caps(text) { 1 } else { 2 })
}

/// No lowercase anywhere and enough uppercase to mean it. Scripts without case (CJK, Arabic)
/// never satisfy the second condition, so they are never mistaken for shouting.
fn is_all_caps(text: &str) -> bool {
    let mut uppercase = 0usize;

    for c in text.chars() {
        if c.is_lowercase() {
            return false;
        }
        if c.is_uppercase() {
            uppercase += 1;
        }
    }

    uppercase >= 3
}

/// `- item`, `• item`, `* item` or `1. item`, and nothing else: a marker without its space
/// is a hyphenated word or a footnote star.
fn list_marker(line: &str) -> Option<(bool, String)> {
    let trimmed = line.trim_start();

    for marker in ['-', '•', '*'] {
        if let Some(rest) = trimmed.strip_prefix(marker) {
            if rest.starts_with(' ') || rest.starts_with('\t') {
                let text = rest.trim();
                if !text.is_empty() {
                    return Some((false, text.to_string()));
                }
            }
        }
    }

    // Bounded to three digits so a year or an amount does not open an ordered list
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    if (1..=3).contains(&digits) {
        if let Some(rest) = trimmed[digits..].strip_prefix('.') {
            if rest.starts_with(' ') {
                let text = rest.trim();
                if !text.is_empty() {
                    return Some((true, text.to_string()));
                }
            }
        }
    }

    None
}

/// A group is a list when its *first* line carries a marker. A line without one in the
/// middle of it is a wrapped item, not a new one.
fn list_block(lines: &[&str]) -> Option<Block> {
    let (ordered, first) = list_marker(lines.first()?)?;
    let mut items = vec![first];

    for line in &lines[1..] {
        match list_marker(line) {
            Some((_, text)) => items.push(text),
            None => {
                let text = line.trim();
                if !text.is_empty() {
                    if let Some(last) = items.last_mut() {
                        last.push(' ');
                        last.push_str(text);
                    }
                }
            }
        }
    }

    Some(Block::List { ordered, items })
}

/// Columns of whitespace shared by every line of the block. `-layout` keeps the horizontal
/// position of each cell, so a table shows up as vertical gaps that never move; justified
/// prose does not, because its word spacing shifts from line to line.
fn table_rows(lines: &[&str]) -> Option<Vec<Vec<String>>> {
    if lines.len() < 2 {
        return None;
    }

    let rows: Vec<Vec<char>> = lines.iter().map(|line| line.chars().collect()).collect();
    let gaps = separator_gaps(&rows);
    if gaps.is_empty() {
        return None;
    }

    let cells: Vec<Vec<String>> = rows.iter().map(|row| split_row(row, &gaps)).collect();

    // A column empty on every row is an artefact of the alignment, not a column
    for column in 0..gaps.len() + 1 {
        if cells.iter().all(|row| row[column].is_empty()) {
            return None;
        }
    }

    // Half the rows fully populated: below that the alignment is a coincidence and the
    // honest answer is the paragraph the text already was
    let full = cells
        .iter()
        .filter(|row| row.iter().all(|cell| !cell.is_empty()))
        .count();
    if full * 2 < cells.len() {
        return None;
    }

    Some(cells)
}

/// Half-open char ranges that are blank on every row, at least `MIN_COLUMN_GAP` wide. A run
/// starting at column 0 is a shared indent, which separates nothing.
fn separator_gaps(rows: &[Vec<char>]) -> Vec<(usize, usize)> {
    let width = rows.iter().map(|row| row.len()).max().unwrap_or(0);
    let mut gaps = Vec::new();
    let mut start: Option<usize> = None;

    for column in 0..width {
        let blank = rows
            .iter()
            .all(|row| row.get(column).is_none_or(|c| *c == ' '));

        match (blank, start) {
            (true, None) => start = Some(column),
            (false, Some(from)) => {
                if from > 0 && column - from >= MIN_COLUMN_GAP {
                    gaps.push((from, column));
                }
                start = None;
            }
            _ => {}
        }
    }

    gaps
}

fn split_row(row: &[char], gaps: &[(usize, usize)]) -> Vec<String> {
    let mut cells = Vec::with_capacity(gaps.len() + 1);
    let mut from = 0usize;

    for (start, end) in gaps {
        cells.push(cell(row, from, *start));
        from = *end;
    }
    cells.push(cell(row, from, row.len()));

    cells
}

fn cell(row: &[char], from: usize, to: usize) -> String {
    let from = from.min(row.len());
    let to = to.min(row.len()).max(from);
    row[from..to].iter().collect::<String>().trim().to_string()
}

// ------------ Markdown rendering ------------

fn render_markdown(blocks: &[Block]) -> String {
    blocks
        .iter()
        .map(render_block)
        .filter(|rendered| !rendered.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_block(block: &Block) -> String {
    match block {
        Block::Heading { level, text } => {
            format!("{} {}", "#".repeat(*level as usize), text)
        }
        Block::Paragraph(text) => text.clone(),
        Block::List { ordered, items } => items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                if *ordered {
                    format!("{}. {}", index + 1, item)
                } else {
                    format!("- {}", item)
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Block::Table(rows) => render_table(rows),
    }
}

/// The first row is the header because a table read off a page has no other candidate
fn render_table(rows: &[Vec<String>]) -> String {
    let mut out = Vec::with_capacity(rows.len() + 1);

    for (index, row) in rows.iter().enumerate() {
        out.push(format!(
            "| {} |",
            row.iter()
                .map(|cell| cell.replace('|', "\\|"))
                .collect::<Vec<_>>()
                .join(" | ")
        ));

        if index == 0 {
            out.push(format!("| {} |", vec!["---"; row.len()].join(" | ")));
        }
    }

    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(text: &str) -> Vec<(usize, String)> {
        vec![(1, text.to_string())]
    }

    #[test]
    fn an_unknown_format_names_the_three_it_accepts() {
        let message = match Format::parse(Some("xml")) {
            Err(AppError::BadRequest(message)) => message,
            other => panic!(
                "expected a bad request, got {:?}",
                other.map(|f| f.as_str())
            ),
        };
        assert!(message.contains("markdown"), "{}", message);
        assert!(message.contains("text"), "{}", message);
        assert!(message.contains("json"), "{}", message);

        assert_eq!(Format::parse(None).unwrap(), Format::Markdown);
        assert_eq!(Format::parse(Some(" JSON ")).unwrap(), Format::Json);
        assert_eq!(Format::parse(Some("txt")).unwrap(), Format::Text);
    }

    #[test]
    fn a_page_specification_is_judged_before_the_document_is_opened() {
        assert_eq!(Pages::parse("all").unwrap(), Pages::All);
        assert_eq!(
            Pages::parse(" 2 - 5 ").unwrap(),
            Pages::Range { first: 2, last: 5 }
        );
        assert_eq!(
            Pages::parse("3").unwrap(),
            Pages::Range { first: 3, last: 3 }
        );
        assert!(Pages::parse("0").is_err());
        assert!(Pages::parse("5-2").is_err());
        assert!(Pages::parse("last").is_err());
    }

    #[test]
    fn a_range_outside_the_document_names_the_real_page_count() {
        assert_eq!(Pages::All.resolve(7).unwrap(), (1, 7));
        assert_eq!(
            Pages::Range { first: 2, last: 3 }.resolve(7).unwrap(),
            (2, 3)
        );

        let message = match (Pages::Range { first: 8, last: 9 }).resolve(7) {
            Err(AppError::BadRequest(message)) => message,
            _ => panic!("a range past the end must be refused"),
        };
        assert!(message.contains("7 pages"), "{}", message);
    }

    #[test]
    fn a_short_isolated_line_becomes_a_heading_and_a_sentence_does_not() {
        // Average line length of the sample below is around 30 characters
        assert_eq!(heading_level("Introduction", 30), Some(2));
        assert_eq!(heading_level("CHAPITRE PREMIER", 30), Some(1));
        assert_eq!(heading_level("Short but final.", 30), None);
        assert_eq!(heading_level("12", 30), None);
        assert_eq!(heading_level("", 30), None);
        // Longer than the average line: prose, whatever it looks like
        assert_eq!(heading_level("Introduction", 8), None);
        // No average to compare against means no heading at all
        assert_eq!(heading_level("Introduction", 0), None);
    }

    #[test]
    fn shouting_needs_letters_not_just_the_absence_of_lowercase() {
        assert!(is_all_caps("ANNEXE TECHNIQUE"));
        assert!(!is_all_caps("Annexe"));
        assert!(!is_all_caps("12 / 34"));
        // No cased script: never promoted, never demoted
        assert!(!is_all_caps("第一章"));
    }

    #[test]
    fn a_marker_without_its_space_is_not_a_list() {
        assert_eq!(
            list_marker("- premier"),
            Some((false, "premier".to_string()))
        );
        assert_eq!(
            list_marker("  • deuxième"),
            Some((false, "deuxième".to_string()))
        );
        assert_eq!(list_marker("* étoile"), Some((false, "étoile".to_string())));
        assert_eq!(list_marker("1. un"), Some((true, "un".to_string())));
        assert_eq!(list_marker("porte-clé"), None);
        assert_eq!(list_marker("2024. une année"), None);
        assert_eq!(list_marker("1.5 fois"), None);
        assert_eq!(list_marker("- "), None);
    }

    #[test]
    fn a_wrapped_line_extends_the_item_above_instead_of_opening_one() {
        let block = list_block(&["- premier point", "  qui continue ici", "- second point"]);
        assert_eq!(
            block,
            Some(Block::List {
                ordered: false,
                items: vec![
                    "premier point qui continue ici".to_string(),
                    "second point".to_string()
                ],
            })
        );

        assert_eq!(list_block(&["du texte ordinaire"]), None);
    }

    #[test]
    fn aligned_columns_become_a_table_and_prose_does_not() {
        let rows = table_rows(&[
            "Product      Qty   Price",
            "Widget        12   3.50",
            "Gadget         4  10.00",
        ])
        .expect("three aligned columns");

        assert_eq!(rows[0], vec!["Product", "Qty", "Price"]);
        assert_eq!(rows[2], vec!["Gadget", "4", "10.00"]);

        // Word spacing moves from line to line: no column survives every row
        assert_eq!(
            table_rows(&[
                "The quick brown fox jumps over the lazy dog",
                "and then runs away into the forest at dusk",
            ]),
            None
        );

        // A shared indent separates nothing
        assert_eq!(
            table_rows(&["    Indented line one", "    Indented line two"]),
            None
        );

        // One line is never a table
        assert_eq!(table_rows(&["Product      Qty   Price"]), None);
    }

    #[test]
    fn a_block_where_half_the_rows_have_holes_stays_a_paragraph() {
        assert_eq!(
            table_rows(&[
                "Nom          Valeur",
                "Alpha              ",
                "Beta               ",
            ]),
            None
        );
    }

    #[test]
    fn a_table_renders_with_its_first_row_as_the_header() {
        let markdown = render_table(&[
            vec!["a".to_string(), "b|c".to_string()],
            vec!["1".to_string(), "2".to_string()],
        ]);
        assert_eq!(markdown, "| a | b\\|c |\n| --- | --- |\n| 1 | 2 |");
    }

    #[test]
    fn a_full_page_turns_into_the_markdown_it_looks_like() {
        let text = "RAPPORT ANNUEL\n\n\
                    Notre chiffre d'affaires progresse de douze pour cent sur l'exercice.\n\
                    La marge reste stable malgre la hausse des couts de production.\n\n\
                    Faits marquants\n\n\
                    - ouverture de deux agences\n\
                    - recrutement de quinze personnes\n";

        let average = average_line_length(&page(text));
        let blocks = blocks_of(text, average);

        assert_eq!(
            blocks[0],
            Block::Heading {
                level: 1,
                text: "RAPPORT ANNUEL".to_string()
            }
        );
        assert!(matches!(blocks[1], Block::Paragraph(_)));
        assert_eq!(
            blocks[2],
            Block::Heading {
                level: 2,
                text: "Faits marquants".to_string()
            }
        );
        assert!(matches!(blocks[3], Block::List { ordered: false, .. }));

        let markdown = render_markdown(&blocks);
        assert!(markdown.starts_with("# RAPPORT ANNUEL"), "{}", markdown);
        assert!(markdown.contains("## Faits marquants"), "{}", markdown);
        assert!(
            markdown.contains("- ouverture de deux agences"),
            "{}",
            markdown
        );
    }

    #[test]
    fn pages_are_numbered_from_the_first_one_asked_for() {
        let (pages, truncated) = within_budget("un\u{000C}deux\u{000C}", 4);
        assert!(!truncated);
        assert_eq!(pages, vec![(4, "un".to_string()), (5, "deux".to_string())]);
    }

    #[test]
    fn the_character_budget_stops_at_a_page_boundary() {
        let big = "x".repeat(MAX_CONTENT_CHARS - 10);
        let raw = format!("{}\u{000C}{}\u{000C}", big, big);

        let (pages, truncated) = within_budget(&raw, 1);
        assert!(truncated);
        assert_eq!(pages.len(), 1);

        // A first page bigger than the whole budget still comes back, clipped
        let (pages, truncated) = within_budget(&"y".repeat(MAX_CONTENT_CHARS + 10), 1);
        assert!(truncated);
        assert_eq!(pages[0].1.chars().count(), MAX_CONTENT_CHARS);
    }

    #[test]
    fn a_page_without_text_points_at_the_ocr_route() {
        let warning = scan_warning(&page("   \n\n  \n")).expect("a scan must be named as one");
        assert!(warning.contains("/api/ocr"), "{}", warning);

        let thin = scan_warning(&page("p. 1\n")).expect("a thin text layer must be named");
        assert!(thin.contains("/api/ocr"), "{}", thin);

        assert_eq!(
            scan_warning(&page(
                "Un paragraphe entier de texte parfaitement extractible, sans aucun doute."
            )),
            None
        );
        assert_eq!(scan_warning(&[]), None);
    }

    #[test]
    fn json_blocks_carry_their_type_and_their_page() {
        let parsed = parse(&[(3, "Titre du chapitre\n\n- un\n- deux\n".to_string())]);
        let (page, blocks) = &parsed[0];
        assert_eq!(*page, 3);

        assert_eq!(blocks[1].to_json()["type"], "list");
        assert_eq!(blocks[1].to_json()["ordered"], false);
        assert_eq!(blocks[1].to_json()["items"][1], "deux");
    }

    #[test]
    fn every_table_found_is_reported_with_its_page() {
        let parsed = parse(&[(
            2,
            "Product      Qty   Price\nWidget        12   3.50\nGadget         4  10.00\n"
                .to_string(),
        )]);

        let tables = tables_of(&parsed);
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].page, 2);
        assert_eq!(tables[0].rows[0], vec!["Product", "Qty", "Price"]);
    }

    #[test]
    fn plain_text_keeps_the_pages_apart() {
        let text = plain_text(&[
            (1, "\npremiere\n".to_string()),
            (2, "\nseconde\n".to_string()),
        ]);
        assert_eq!(text, "premiere\n\nseconde");
    }
}

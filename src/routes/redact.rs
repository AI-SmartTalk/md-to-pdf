//! Provable redaction: the matched areas are painted over and the document is rebuilt from
//! page images, so the removed strings are simply not in the output file any more.
//!
//! There is deliberately no "overlay" mode that would keep the text layer. A black
//! rectangle drawn on top of live text is the failure this endpoint exists to prevent:
//! the text survives, a copy-paste reveals it, and the caller believes the job is done.
//! Offering the unsafe option next to the safe one only moves that mistake one field away.

use crate::auth::ApiKey;
use crate::exec;
use crate::helpers::{self, base64};
use crate::obs::{self, RequestId};
use crate::pdfops::{self, RasterPage, Word};
use crate::types::AppError;
use rocket::fs::NamedFile;
use rocket::serde::json::Json;
use rocket::Either;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;
use std::time::Instant;
use tempfile::{Builder, TempPath};

/// Resolution of the rebuilt pages. 200 dpi keeps 9pt body text comfortably readable.
const DEFAULT_DPI: u32 = 200;
const MIN_DPI: u32 = 72;
const MAX_DPI: u32 = 400;

const MAX_PATTERNS: usize = 64;
const MAX_PATTERN_CHARS: usize = 256;

/// Glyph ink can sit a fraction of a point outside the box pdftotext reports, and a
/// hairline of a letter is enough to read it: every box grows by this margin.
const BOX_PADDING_PT: f32 = 1.5;
/// Two boxes closer than this on the same line become one, so word boundaries inside a
/// redacted span do not show through as white gaps.
const BOX_MERGE_GAP_PT: f32 = 4.0;

/// Pixel budget for one batch of pages, which bounds the memory a 100-page job needs
const BATCH_PIXELS: f32 = 24_000_000.0;
const MAX_BATCH_PAGES: usize = 16;

/// Every page is rasterized, repainted and rasterized again, so a long document is three
/// external processes per batch with a render slot held throughout. `/api/diff` caps itself
/// the same way, for the same reason.
const MAX_REDACT_PAGES: usize = 200;

/// The rasterizer and the geometry we place boxes with must agree within this ratio,
/// otherwise a box would land somewhere else on the page
const GEOMETRY_TOLERANCE: f32 = 0.02;

#[derive(Deserialize)]
pub struct RedactRequest {
    pub pdf: String,
    pub patterns: Option<Vec<String>>,
    pub entities: Option<Vec<String>>,
    pub dpi: Option<u32>,
    pub client_id: Option<String>,
    pub pdf_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PageRedactions {
    /// 1-based page number
    pub page: usize,
    /// Areas painted on that page
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct RedactResponse {
    pub download_url: String,
    pub redactions: Vec<PageRedactions>,
    pub pages: usize,
    /// Always `flatten`: no mode of this endpoint keeps a text layer
    pub mode: &'static str,
    pub notice: &'static str,
}

/// The trade-off the caller is entitled to know about, in the response itself
const NOTICE: &str = "Pages were rebuilt from images: the redacted strings are gone from the \
file, and so is every text layer, bookmark, annotation, attachment and metadata entry of \
the source. In exchange the result is neither selectable nor searchable, and it is larger \
than the original.";

#[post("/redact", format = "json", data = "<req>")]
pub async fn redact(
    _key: ApiKey,
    trace: RequestId,
    req: Json<RedactRequest>,
) -> Result<Either<NamedFile, Json<RedactResponse>>, AppError> {
    let req = req.into_inner();

    let source = helpers::resolve_pdf_path(&req.pdf)?;
    let patterns = compile_patterns(req.patterns.as_deref())?;
    let entities = parse_entities(req.entities.as_deref())?;
    let dpi = resolve_dpi(req.dpi)?;

    if patterns.is_empty() && entities.is_empty() {
        return Err(AppError::BadRequest(
            "Provide at least one of \"patterns\" or \"entities\"".to_string(),
        ));
    }

    let started = Instant::now();
    let job = exec::offload(move || flatten(&source, &patterns, &entities, dpi)).await;

    let redacted = match job {
        Ok(redacted) => redacted,
        Err(err) => {
            obs::event(
                obs::Level::Error,
                "redact failed",
                vec![
                    ("trace_id", json!(trace.0)),
                    ("route", json!("/api/redact")),
                    obs::err_field(err.kind(), &format!("{:?}", err)),
                ],
            );
            return Err(err);
        }
    };

    let painted: usize = redacted.redactions.iter().map(|page| page.count).sum();
    obs::event(
        obs::Level::Info,
        "redact",
        vec![
            ("trace_id", json!(trace.0)),
            ("route", json!("/api/redact")),
            ("pages", json!(redacted.pages)),
            ("boxes", json!(painted)),
            ("dpi", json!(dpi)),
            ("duration_ms", json!(started.elapsed().as_millis())),
        ],
    );

    if let (Some(client_id), Some(pdf_name)) = (req.client_id, req.pdf_name) {
        let download_url = helpers::save_pdf(&redacted.pdf, &client_id, &pdf_name)?;
        Ok(Either::Right(Json(RedactResponse {
            download_url,
            redactions: redacted.redactions,
            pages: redacted.pages,
            mode: "flatten",
            notice: NOTICE,
        })))
    } else {
        Ok(Either::Left(
            NamedFile::open(&redacted.pdf).await.map_err(AppError::Io)?,
        ))
    }
}

struct Redacted {
    pdf: TempPath,
    pages: usize,
    redactions: Vec<PageRedactions>,
}

/// A rectangle to paint, in PDF points, origin at the top-left of the page
#[derive(Debug, Clone, Copy, PartialEq)]
struct Rect {
    page: usize,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

// ------------ The redaction itself ------------

fn flatten(
    source: &Path,
    patterns: &[Vec<char>],
    entities: &[Entity],
    dpi: u32,
) -> Result<Redacted, AppError> {
    // Rotation lives in the page dictionary: pdftotext reports rotated word coordinates on
    // an unrotated page box while pdftoppm renders the rotated page. Baking the rotation
    // into the content stream first is what makes both tools agree on where a word is.
    let upright = flatten_rotation(source);
    let work: &Path = upright.as_deref().unwrap_or(source);

    let pages = pdfops::page_count(work)?;
    if pages > MAX_REDACT_PAGES {
        return Err(AppError::BadRequest(format!(
            "the document has {} pages; at most {} may be redacted at once",
            pages, MAX_REDACT_PAGES
        )));
    }

    let (_, words) = pdfops::words(work)?;
    let rects = redaction_boxes(&words, patterns, entities)?;

    let batch = batch_pages(dpi);
    let mut parts: Vec<TempPath> = Vec::new();
    let mut first = 1usize;
    while first <= pages {
        let last = (first + batch - 1).min(pages);
        parts.push(flatten_batch(work, first, last, &rects, dpi)?);
        first = last + 1;
    }

    let pdf = match parts.len() {
        1 => parts.remove(0),
        _ => join(&parts)?,
    };

    // Fail closed: the whole promise of this endpoint is that nothing readable survived,
    // and that the caller gets back the document they sent, not a truncated one.
    let produced = pdfops::page_count(&pdf)?;
    if produced != pages {
        return Err(AppError::ProcessFailed {
            message: format!("Redaction produced {} pages instead of {}", produced, pages),
            stderr: String::new(),
        });
    }
    ensure_no_text_layer(&pdf)?;

    Ok(Redacted {
        pdf,
        pages,
        redactions: counts_per_page(&rects, pages),
    })
}

/// One batch: rasterize, paint, and rasterize again.
///
/// The second pass is what makes the black opaque in the pixels themselves. Painting
/// directly on the PNG would need an image codec, which this service does not carry; a
/// single pass would leave the untouched page image embedded under a vector rectangle,
/// which is exactly the fake redaction this endpoint refuses to produce.
fn flatten_batch(
    pdf: &Path,
    first: usize,
    last: usize,
    rects: &[Rect],
    dpi: u32,
) -> Result<TempPath, AppError> {
    let rendered = pdfops::rasterize(pdf, first, last, dpi)?;
    check_geometry(&rendered, dpi)?;

    let painted = helpers::run_weasyprint_plain(&overlay_html(&rendered, rects))?;

    let flattened = pdfops::rasterize(&painted, 1, rendered.len(), dpi)?;
    if flattened.len() != rendered.len() {
        return Err(AppError::ProcessFailed {
            message: format!(
                "Painting pages {}..{} produced {} pages instead of {}",
                first,
                last,
                flattened.len(),
                rendered.len()
            ),
            stderr: String::new(),
        });
    }

    pdfops::compose_pdf_from_pngs(&flattened)
}

/// Page images plus opaque boxes, positioned in points because that is the unit
/// pdftotext reports and the unit an `@page` of the same size gives us back.
fn overlay_html(pages: &[RasterPage], rects: &[Rect]) -> String {
    let mut styles = String::new();
    let mut body = String::new();

    for (index, page) in pages.iter().enumerate() {
        let name = format!("p{}", index + 1);
        let width = page.page_box.width.max(1.0);
        let height = page.page_box.height.max(1.0);

        let _ = write!(
            styles,
            "@page {name} {{ size: {width:.3}pt {height:.3}pt; margin: 0; }}\n\
             .{name} {{ page: {name}; width: {width:.3}pt; height: {height:.3}pt; }}\n",
            name = name,
            width = width,
            height = height,
        );

        let _ = write!(
            body,
            "<div class=\"{}\"><img src=\"data:image/png;base64,{}\">",
            name,
            base64(&page.png)
        );

        for rect in rects.iter().filter(|rect| rect.page == page.page) {
            let x0 = (rect.x0 - BOX_PADDING_PT).clamp(0.0, width);
            let y0 = (rect.y0 - BOX_PADDING_PT).clamp(0.0, height);
            let x1 = (rect.x1 + BOX_PADDING_PT).clamp(0.0, width);
            let y1 = (rect.y1 + BOX_PADDING_PT).clamp(0.0, height);
            let _ = write!(
                body,
                "<b style=\"left:{:.3}pt;top:{:.3}pt;width:{:.3}pt;height:{:.3}pt\"></b>",
                x0,
                y0,
                x1 - x0,
                y1 - y0
            );
        }

        body.push_str("</div>");
    }

    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><style>\n\
         html, body {{ margin: 0; padding: 0; }}\n\
         div {{ margin: 0; padding: 0; position: relative; }}\n\
         img {{ display: block; width: 100%; height: 100%; }}\n\
         b {{ display: block; position: absolute; background: #000; }}\n\
         {}</style></head><body>{}</body></html>",
        styles, body
    )
}

/// Refuse to place a single box when the raster and the page box disagree: a document
/// whose geometry we misread would get its black rectangles somewhere else than on the
/// text, and that silent failure is a leak.
fn check_geometry(pages: &[RasterPage], dpi: u32) -> Result<(), AppError> {
    let expected = dpi as f32 / 72.0;

    for page in pages {
        let width = page.page_box.width;
        let height = page.page_box.height;
        if width <= 0.0 || height <= 0.0 {
            return Err(geometry_error(page.page));
        }

        let ratio_x = page.width_px as f32 / width;
        let ratio_y = page.height_px as f32 / height;
        if (ratio_x - expected).abs() > expected * GEOMETRY_TOLERANCE
            || (ratio_y - expected).abs() > expected * GEOMETRY_TOLERANCE
        {
            return Err(geometry_error(page.page));
        }
    }

    Ok(())
}

fn geometry_error(page: usize) -> AppError {
    AppError::ProcessFailed {
        message: format!(
            "Page {} has a geometry the text extractor and the rasterizer disagree on; \
             redaction would place its boxes in the wrong place",
            page
        ),
        stderr: String::new(),
    }
}

/// Prove, on the file we are about to hand back, that no text survived
fn ensure_no_text_layer(pdf: &Path) -> Result<(), AppError> {
    let output = helpers::run_capture(
        Command::new("pdftotext")
            .arg(helpers::path_to_str(pdf)?)
            .arg("-"),
        "pdftotext",
        "pdftotext failed",
    )?;

    let text = String::from_utf8_lossy(&output.stdout);
    if text.chars().any(|c| c.is_alphanumeric()) {
        return Err(AppError::ProcessFailed {
            message: "The redacted document still carries a text layer; it was not returned"
                .to_string(),
            stderr: String::new(),
        });
    }

    Ok(())
}

/// `qpdf --flatten-rotation` writes the rotation into the page content. Best effort: when
/// it is unavailable the geometry check refuses the rotated document instead.
fn flatten_rotation(source: &Path) -> Option<TempPath> {
    let temp = Builder::new().suffix(".pdf").tempfile().ok()?;
    let input = helpers::path_to_str(source).ok()?.to_string();
    let output = helpers::path_to_str(temp.path()).ok()?.to_string();

    let mut cmd = Command::new("qpdf");
    cmd.arg("--flatten-rotation").arg(&input).arg(&output);

    match helpers::run_tool(&mut cmd, "qpdf", "qpdf --flatten-rotation failed") {
        Ok(()) => Some(temp.into_temp_path()),
        Err(err) => {
            warn!("Redaction works on the original document: {:?}", err);
            None
        }
    }
}

fn join(parts: &[TempPath]) -> Result<TempPath, AppError> {
    let temp = Builder::new().suffix(".pdf").tempfile()?;
    let output = helpers::path_to_str(temp.path())?.to_string();

    let mut cmd = Command::new("pdfunite");
    for part in parts {
        cmd.arg(helpers::path_to_str(part)?);
    }
    cmd.arg(&output);

    helpers::run_tool(&mut cmd, "pdfunite", "Redacted pages could not be joined")?;
    Ok(temp.into_temp_path())
}

/// Rasterizing a whole book at once would hold every page image in memory at the same
/// time; the batch is sized on an A4 page at the requested resolution.
fn batch_pages(dpi: u32) -> usize {
    let per_page = 8.268 * dpi as f32 * 11.693 * dpi as f32;
    ((BATCH_PIXELS / per_page) as usize).clamp(1, MAX_BATCH_PAGES)
}

fn counts_per_page(rects: &[Rect], pages: usize) -> Vec<PageRedactions> {
    let mut counts: BTreeMap<usize, usize> = BTreeMap::new();
    for rect in rects {
        if rect.page >= 1 && rect.page <= pages {
            *counts.entry(rect.page).or_default() += 1;
        }
    }

    counts
        .into_iter()
        .map(|(page, count)| PageRedactions { page, count })
        .collect()
}

// ------------ Locating what has to disappear ------------

/// Words of one page laid end to end, with the word each character came from.
///
/// A match is looked for in this flat text rather than word by word, because pdftotext
/// splits on spaces: "Jean Dupont" spans two words, and "Dupont," is one word that only
/// partly overlaps the match. Any overlap redacts the whole word — painting more than
/// asked is acceptable, painting less is not.
struct PageText {
    chars: Vec<char>,
    owner: Vec<usize>,
}

/// The separator characters belong to no word
const NO_WORD: usize = usize::MAX;

impl PageText {
    fn build(words: &[&Word]) -> PageText {
        let mut chars = Vec::new();
        let mut owner = Vec::new();

        for (index, word) in words.iter().enumerate() {
            if !chars.is_empty() {
                chars.push(' ');
                owner.push(NO_WORD);
            }
            for c in word.text.chars() {
                chars.push(c);
                owner.push(index);
            }
        }

        PageText { chars, owner }
    }

    fn mark(&self, span: (usize, usize), marked: &mut [bool]) {
        for index in span.0..span.1.min(self.owner.len()) {
            let word = self.owner[index];
            if word != NO_WORD {
                marked[word] = true;
            }
        }
    }
}

fn redaction_boxes(
    words: &[Word],
    patterns: &[Vec<char>],
    entities: &[Entity],
) -> Result<Vec<Rect>, AppError> {
    let mut by_page: BTreeMap<usize, Vec<&Word>> = BTreeMap::new();
    for word in words {
        by_page.entry(word.page).or_default().push(word);
    }

    let mut rects = Vec::new();
    for (page, page_words) in by_page {
        let text = PageText::build(&page_words);
        let mut marked = vec![false; page_words.len()];

        for pattern in patterns {
            for span in find_literal(&text.chars, pattern) {
                text.mark(span, &mut marked);
            }
        }
        for entity in entities {
            for span in entity.scan(&text.chars) {
                text.mark(span, &mut marked);
            }
        }

        for (index, word) in page_words.iter().enumerate() {
            if !marked[index] {
                continue;
            }
            if ![word.x0, word.y0, word.x1, word.y1]
                .iter()
                .all(|value| value.is_finite())
            {
                // Giving up on a box we were asked to paint would be a silent leak
                return Err(AppError::ProcessFailed {
                    message: format!(
                        "A match on page {} has no usable position in the document",
                        page
                    ),
                    stderr: String::new(),
                });
            }

            rects.push(Rect {
                page,
                x0: word.x0.min(word.x1),
                y0: word.y0.min(word.y1),
                x1: word.x0.max(word.x1),
                y1: word.y0.max(word.y1),
            });
        }
    }

    Ok(merge_rects(rects))
}

/// Merge boxes that sit side by side on the same line, so the gaps between redacted words
/// do not draw their outline
fn merge_rects(mut rects: Vec<Rect>) -> Vec<Rect> {
    rects.sort_by(|a, b| {
        a.page
            .cmp(&b.page)
            .then(a.y0.total_cmp(&b.y0))
            .then(a.x0.total_cmp(&b.x0))
    });

    let mut merged: Vec<Rect> = Vec::with_capacity(rects.len());
    for rect in rects {
        match merged.last_mut() {
            Some(last)
                if last.page == rect.page
                    && (last.y0 - rect.y0).abs() <= 1.0
                    && (last.y1 - rect.y1).abs() <= 1.0
                    && rect.x0 <= last.x1 + BOX_MERGE_GAP_PT =>
            {
                last.x1 = last.x1.max(rect.x1);
            }
            _ => merged.push(rect),
        }
    }

    merged
}

/// Every occurrence of a literal, compared without case
fn find_literal(haystack: &[char], needle: &[char]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    if needle.is_empty() || needle.len() > haystack.len() {
        return spans;
    }

    for start in 0..=haystack.len() - needle.len() {
        if !char_eq_ci(haystack[start], needle[0]) {
            continue;
        }
        if needle
            .iter()
            .enumerate()
            .all(|(offset, c)| char_eq_ci(haystack[start + offset], *c))
        {
            spans.push((start, start + needle.len()));
        }
    }

    spans
}

fn char_eq_ci(a: char, b: char) -> bool {
    if a == b {
        return true;
    }
    if a.is_ascii() || b.is_ascii() {
        return a.eq_ignore_ascii_case(&b);
    }
    a.to_lowercase().eq(b.to_lowercase())
}

// ------------ Request validation ------------

/// Patterns are matched literally, not as regular expressions.
///
/// The alternative was pulling `regex` in for this one endpoint. Literals plus the
/// entity scanners cover what redaction actually needs, and the scanners do something no
/// regular expression can: they validate a checksum, which is what keeps a fourteen-digit
/// invoice number from being mistaken for a SIRET. A pattern that reads like a regular
/// expression is rejected rather than matched literally, because a caller who sent
/// `\d{4}` and got a document back with nothing painted would believe the job was done.
fn compile_patterns(patterns: Option<&[String]>) -> Result<Vec<Vec<char>>, AppError> {
    let patterns = match patterns {
        Some(patterns) => patterns,
        None => return Ok(Vec::new()),
    };

    if patterns.len() > MAX_PATTERNS {
        return Err(AppError::BadRequest(format!(
            "\"patterns\" is limited to {} entries",
            MAX_PATTERNS
        )));
    }

    let mut compiled = Vec::with_capacity(patterns.len());
    for pattern in patterns {
        // Whitespace is collapsed on both sides: a match may span a line break, where
        // pdftotext leaves exactly one space.
        let normalized: Vec<char> = pattern
            .split_whitespace()
            .collect::<Vec<&str>>()
            .join(" ")
            .chars()
            .collect();

        if normalized.is_empty() {
            return Err(AppError::BadRequest(
                "\"patterns\" must not contain an empty entry".to_string(),
            ));
        }
        if normalized.len() > MAX_PATTERN_CHARS {
            return Err(AppError::BadRequest(format!(
                "A pattern is limited to {} characters",
                MAX_PATTERN_CHARS
            )));
        }
        if let Some(token) = regex_syntax(&normalized) {
            return Err(AppError::BadRequest(format!(
                "\"patterns\" are matched literally: {} looks like a regular expression. \
                 Use \"entities\" for structured data (email, iban, phone, siret, credit_card).",
                token
            )));
        }

        compiled.push(normalized);
    }

    Ok(compiled)
}

/// Constructs that only make sense in a regular expression, reported as-is to the caller
fn regex_syntax(pattern: &[char]) -> Option<&'static str> {
    for (index, c) in pattern.iter().enumerate() {
        let next = pattern.get(index + 1).copied();
        match c {
            '\\' => {
                if matches!(next, Some(n) if n.is_ascii_alphanumeric() || "\\.[]()|+*?^${}".contains(n))
                {
                    return Some("a backslash escape");
                }
            }
            '[' => {
                if pattern[index + 1..].contains(&']') {
                    return Some("a character class");
                }
            }
            '{' => {
                if matches!(next, Some(n) if n.is_ascii_digit())
                    && pattern[index + 1..].contains(&'}')
                {
                    return Some("a repetition count");
                }
            }
            '.' => {
                if matches!(next, Some('*') | Some('+')) {
                    return Some("a quantifier");
                }
            }
            '(' if next == Some('?') => return Some("a group modifier"),
            _ => {}
        }
    }

    None
}

fn resolve_dpi(dpi: Option<u32>) -> Result<u32, AppError> {
    let dpi = dpi.unwrap_or(DEFAULT_DPI);

    if !(MIN_DPI..=MAX_DPI).contains(&dpi) {
        return Err(AppError::BadRequest(format!(
            "\"dpi\" must be between {} and {}",
            MIN_DPI, MAX_DPI
        )));
    }

    Ok(dpi)
}

// ------------ Entities ------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entity {
    Email,
    Iban,
    Phone,
    Siret,
    CreditCard,
}

const ENTITY_NAMES: [&str; 5] = ["email", "iban", "phone", "siret", "credit_card"];

fn parse_entities(names: Option<&[String]>) -> Result<Vec<Entity>, AppError> {
    let names = match names {
        Some(names) => names,
        None => return Ok(Vec::new()),
    };

    let mut entities = Vec::new();
    for name in names {
        let entity = match name.trim().to_ascii_lowercase().as_str() {
            "email" => Entity::Email,
            "iban" => Entity::Iban,
            "phone" => Entity::Phone,
            "siret" | "siren" => Entity::Siret,
            "credit_card" => Entity::CreditCard,
            other => {
                return Err(AppError::BadRequest(format!(
                    "Unknown entity \"{}\"; known entities: {}",
                    other,
                    ENTITY_NAMES.join(", ")
                )))
            }
        };
        if !entities.contains(&entity) {
            entities.push(entity);
        }
    }

    Ok(entities)
}

impl Entity {
    fn scan(&self, chars: &[char]) -> Vec<(usize, usize)> {
        match self {
            Entity::Email => find_emails(chars),
            Entity::Iban => find_ibans(chars),
            Entity::Phone => find_phones(chars),
            Entity::Siret => find_numbers(chars, |digits| {
                matches!(digits.len(), 9 | 14) && luhn(digits)
            }),
            Entity::CreditCard => find_numbers(chars, |digits| {
                (13..=19).contains(&digits.len()) && luhn(digits)
            }),
        }
    }
}

fn find_emails(chars: &[char]) -> Vec<(usize, usize)> {
    let local = |c: char| c.is_alphanumeric() || "._%+-'".contains(c);
    let domain = |c: char| c.is_alphanumeric() || c == '.' || c == '-';

    let mut spans = Vec::new();
    for (index, c) in chars.iter().enumerate() {
        if *c != '@' {
            continue;
        }

        let mut start = index;
        while start > 0 && local(chars[start - 1]) {
            start -= 1;
        }
        let mut end = index + 1;
        while end < chars.len() && domain(chars[end]) {
            end += 1;
        }
        // A sentence ends with a dot; a domain does not
        while end > index + 1 && matches!(chars[end - 1], '.' | '-') {
            end -= 1;
        }

        if start == index || !plausible_domain(&chars[index + 1..end]) {
            continue;
        }
        spans.push((start, end));
    }

    spans
}

/// At least two labels, and a top-level one that is alphabetic
fn plausible_domain(domain: &[char]) -> bool {
    let labels: Vec<&[char]> = domain.split(|c| *c == '.').collect();

    labels.len() >= 2
        && labels.iter().all(|label| !label.is_empty())
        && labels
            .last()
            .map(|tld| tld.len() >= 2 && tld.iter().all(|c| c.is_alphabetic()))
            .unwrap_or(false)
}

/// IBANs are printed either in one block or in groups of four, and the last two digits of
/// the country prefix are a modulo 97 checksum: a candidate that does not verify is not
/// an IBAN, which is what keeps a reference number from being painted over.
fn find_ibans(chars: &[char]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();

    for start in 0..chars.len() {
        if start > 0 && chars[start - 1].is_alphanumeric() {
            continue;
        }
        let head = &chars[start..chars.len().min(start + 4)];
        if head.len() < 4
            || !head[0].is_ascii_alphabetic()
            || !head[1].is_ascii_alphabetic()
            || !head[2].is_ascii_digit()
            || !head[3].is_ascii_digit()
        {
            continue;
        }

        let mut candidate: Vec<char> = Vec::new();
        // ends[n] is where the candidate stops if only its first n + 1 characters are kept
        let mut ends: Vec<usize> = Vec::new();
        let mut cursor = start;
        while cursor < chars.len() && candidate.len() < 34 {
            let c = chars[cursor];
            if c.is_ascii_alphanumeric() {
                candidate.push(c.to_ascii_uppercase());
                cursor += 1;
                ends.push(cursor);
            } else if c == ' '
                && chars
                    .get(cursor + 1)
                    .is_some_and(|next| next.is_ascii_alphanumeric())
            {
                cursor += 1;
            } else {
                break;
            }
        }

        // Longest valid prefix first: a trailing word absorbed by the grouping spaces
        // fails the checksum, the IBAN alone passes it
        for length in (15..=candidate.len()).rev() {
            if iban_checksum_ok(&candidate[..length]) {
                spans.push((start, ends[length - 1]));
                break;
            }
        }
    }

    spans
}

fn iban_checksum_ok(candidate: &[char]) -> bool {
    let mut remainder: u32 = 0;
    let rearranged = candidate[4..].iter().chain(candidate[..4].iter());

    for c in rearranged {
        let value = match c {
            '0'..='9' => *c as u32 - '0' as u32,
            'A'..='Z' => *c as u32 - 'A' as u32 + 10,
            _ => return false,
        };
        // Two decimal digits for a letter, one for a digit
        if value >= 10 {
            remainder = (remainder * 10 + value / 10) % 97;
            remainder = (remainder * 10 + value % 10) % 97;
        } else {
            remainder = (remainder * 10 + value) % 97;
        }
    }

    remainder == 1
}

/// Numbers written for a human: separated in groups, and either international or in the
/// national trunk form. A run that starts with any other digit is a quantity, not a phone.
fn find_phones(chars: &[char]) -> Vec<(usize, usize)> {
    let separator = |c: char| matches!(c, ' ' | '.' | '-' | '/' | '(' | ')' | '\u{a0}');

    let mut spans = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        let starts = c == '+' || c == '0';
        let boundary =
            index == 0 || !(chars[index - 1].is_alphanumeric() || chars[index - 1] == '+');
        if !starts || !boundary {
            index += 1;
            continue;
        }

        let mut cursor = index;
        if chars[cursor] == '+' {
            cursor += 1;
        }
        let mut digits = 0usize;
        let mut end = cursor;
        while cursor < chars.len() {
            let c = chars[cursor];
            if c.is_ascii_digit() {
                digits += 1;
                cursor += 1;
                end = cursor;
            } else if separator(c)
                && chars
                    .get(cursor + 1)
                    .is_some_and(|next| next.is_ascii_digit())
            {
                cursor += 1;
            } else {
                break;
            }
        }

        let international = chars[index] == '+';
        let plausible = if international {
            (8..=15).contains(&digits)
        } else {
            (9..=15).contains(&digits)
        };
        if plausible {
            spans.push((index, end));
        }

        index = cursor.max(index + 1);
    }

    spans
}

/// Digit runs, grouping spaces and dashes included, kept when the caller's rule accepts
/// the digits they carry
fn find_numbers(chars: &[char], accept: impl Fn(&str) -> bool) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut index = 0;

    while index < chars.len() {
        if !chars[index].is_ascii_digit() || (index > 0 && chars[index - 1].is_alphanumeric()) {
            index += 1;
            continue;
        }

        let mut digits = String::new();
        let mut cursor = index;
        let mut end = index;
        while cursor < chars.len() {
            let c = chars[cursor];
            if c.is_ascii_digit() {
                digits.push(c);
                cursor += 1;
                end = cursor;
            } else if matches!(c, ' ' | '-')
                && chars
                    .get(cursor + 1)
                    .is_some_and(|next| next.is_ascii_digit())
            {
                cursor += 1;
            } else {
                break;
            }
        }

        // A run glued to letters is an identifier of another kind
        let isolated = chars.get(end).map(|c| !c.is_alphanumeric()).unwrap_or(true);
        if isolated && accept(&digits) {
            spans.push((index, end));
        }

        index = cursor.max(index + 1);
    }

    spans
}

fn luhn(digits: &str) -> bool {
    if digits.len() < 2 {
        return false;
    }

    let mut sum = 0u32;
    for (position, c) in digits.bytes().rev().enumerate() {
        if !c.is_ascii_digit() {
            return false;
        }
        let mut value = (c - b'0') as u32;
        if position % 2 == 1 {
            value *= 2;
            if value > 9 {
                value -= 9;
            }
        }
        sum += value;
    }

    sum.is_multiple_of(10)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(page: usize, text: &str, x0: f32) -> Word {
        Word {
            page,
            text: text.to_string(),
            x0,
            y0: 100.0,
            x1: x0 + 20.0,
            y1: 112.0,
        }
    }

    fn chars(value: &str) -> Vec<char> {
        value.chars().collect()
    }

    fn spans(value: &str, spans: Vec<(usize, usize)>) -> Vec<String> {
        let chars = chars(value);
        spans
            .into_iter()
            .map(|(start, end)| chars[start..end].iter().collect())
            .collect()
    }

    #[test]
    fn a_match_spanning_words_redacts_every_word_it_touches() {
        // "Dupont," only overlaps the match: it is painted whole, comma included
        let words = vec![
            word(1, "est", 0.0),
            word(1, "Jean", 30.0),
            word(1, "Dupont,", 52.0),
            word(1, "joignable", 200.0),
        ];

        let rects = redaction_boxes(&words, &[chars("jean dupont")], &[]).unwrap();

        // The two adjacent words merge into a single box, the far one is untouched
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].x0, 30.0);
        assert_eq!(rects[0].x1, 72.0);
    }

    #[test]
    fn a_pattern_may_span_a_line_break() {
        let words = vec![word(1, "Jean", 0.0), word(2, "Dupont", 0.0)];
        // Different pages never join
        assert!(redaction_boxes(&words, &[chars("jean dupont")], &[])
            .unwrap()
            .is_empty());

        // A pattern typed across a line break matches text pdftotext joined with one space
        let patterns = compile_patterns(Some(&["Jean \n Dupont".to_string()])).unwrap();
        let words = vec![word(1, "Jean", 0.0), word(1, "Dupont", 300.0)];
        assert_eq!(redaction_boxes(&words, &patterns, &[]).unwrap().len(), 2);
    }

    #[test]
    fn patterns_that_look_like_regular_expressions_are_refused() {
        for pattern in ["\\d{4}", "[A-Z]+", "Jean.*Dupont", "(?i)jean", "a{2,3}"] {
            assert!(
                compile_patterns(Some(&[pattern.to_string()])).is_err(),
                "{} should be refused",
                pattern
            );
        }

        // Punctuation that occurs in real text stays usable
        for pattern in ["Total (HT)", "M. Dupont", "+33 6 12", "50% (net)", "a}b"] {
            assert!(
                compile_patterns(Some(&[pattern.to_string()])).is_ok(),
                "{} should be accepted",
                pattern
            );
        }
    }

    #[test]
    fn patterns_are_normalized_and_bounded() {
        let compiled = compile_patterns(Some(&["  Jean \n Dupont ".to_string()])).unwrap();
        assert_eq!(compiled[0], chars("Jean Dupont"));

        assert!(compile_patterns(Some(&["   ".to_string()])).is_err());
        assert!(compile_patterns(Some(&["x".repeat(MAX_PATTERN_CHARS + 1)])).is_err());
        assert!(compile_patterns(Some(&vec!["x".to_string(); MAX_PATTERNS + 1])).is_err());
        assert!(compile_patterns(None).unwrap().is_empty());
    }

    #[test]
    fn finds_emails_without_the_surrounding_punctuation() {
        let text = "ecrire a jean.dupont@example.com, ou a b@x.fr.";
        assert_eq!(
            spans(text, find_emails(&chars(text))),
            vec!["jean.dupont@example.com", "b@x.fr"]
        );

        for text in ["@example.com", "jean@", "jean@localhost", "a@b.c"] {
            assert!(
                find_emails(&chars(text)).is_empty(),
                "{} is not an address",
                text
            );
        }
    }

    #[test]
    fn finds_ibans_only_when_the_checksum_verifies() {
        let text = "IBAN FR7630006000011234567890189 fin";
        assert_eq!(
            spans(text, find_ibans(&chars(text))),
            vec!["FR7630006000011234567890189"]
        );

        // Same number printed in groups, and followed by a word the scan must not swallow
        let text = "FR76 3000 6000 0112 3456 7890 189 Merci";
        assert_eq!(
            spans(text, find_ibans(&chars(text))),
            vec!["FR76 3000 6000 0112 3456 7890 189"]
        );

        // One digit changed: not an IBAN any more
        let text = "FR7630006000011234567890188";
        assert!(find_ibans(&chars(text)).is_empty());
    }

    #[test]
    fn finds_phone_numbers_and_ignores_quantities() {
        let text = "au +33 6 12 34 56 78. ou 06 12 34 56 78,";
        assert_eq!(
            spans(text, find_phones(&chars(text))),
            vec!["+33 6 12 34 56 78", "06 12 34 56 78"]
        );

        // Amounts and dates keep their place
        for text in ["1 234 567 890 euros", "12/05/2024", "4111 1111 1111 1111"] {
            assert!(find_phones(&chars(text)).is_empty(), "{}", text);
        }
    }

    #[test]
    fn finds_card_numbers_and_sirets_by_their_check_digit() {
        let card = Entity::CreditCard;
        let text = "Carte : 4111 1111 1111 1111 fin";
        assert_eq!(
            spans(text, card.scan(&chars(text))),
            vec!["4111 1111 1111 1111"]
        );
        let text = "Carte : 4111 1111 1111 1112";
        assert!(card.scan(&chars(text)).is_empty());

        let siret = Entity::Siret;
        let text = "SIRET : 552 100 554 00021 fin";
        assert_eq!(
            spans(text, siret.scan(&chars(text))),
            vec!["552 100 554 00021"]
        );
        // A SIREN alone, and a reference of the wrong length
        let text = "SIREN 552100554 / ref 12345678901";
        assert_eq!(spans(text, siret.scan(&chars(text))), vec!["552100554"]);
    }

    #[test]
    fn validates_luhn() {
        assert!(luhn("4111111111111111"));
        assert!(luhn("55210055400021"));
        assert!(!luhn("4111111111111112"));
        assert!(!luhn("12a"));
        assert!(!luhn(""));
    }

    #[test]
    fn unknown_entities_are_reported_with_the_known_ones() {
        assert!(parse_entities(Some(&["email".to_string(), "iban".to_string()])).is_ok());
        assert_eq!(
            parse_entities(Some(&["Email".to_string(), "email".to_string()]))
                .unwrap()
                .len(),
            1
        );
        assert!(parse_entities(Some(&["ssn".to_string()])).is_err());
        assert!(parse_entities(None).unwrap().is_empty());
    }

    #[test]
    fn boxes_never_leave_the_page_and_carry_their_margin() {
        let pages = vec![RasterPage {
            page: 3,
            png: vec![1, 2, 3],
            width_px: 100,
            height_px: 200,
            page_box: pdfops::PageBox {
                page: 3,
                width: 595.276,
                height: 841.89,
            },
        }];
        let rects = vec![
            Rect {
                page: 3,
                x0: 0.0,
                y0: 10.0,
                x1: 40.0,
                y1: 22.0,
            },
            // Another page: must not be painted on this one
            Rect {
                page: 4,
                x0: 0.0,
                y0: 0.0,
                x1: 10.0,
                y1: 10.0,
            },
        ];

        let html = overlay_html(&pages, &rects);
        assert!(html.contains("@page p1 { size: 595.276pt 841.890pt; margin: 0; }"));
        assert!(html.contains("data:image/png;base64,AQID"));
        assert_eq!(html.matches("<b style=").count(), 1);
        // Clamped on the left edge, grown by the margin elsewhere
        assert!(html.contains("left:0.000pt;top:8.500pt;width:41.500pt;height:15.000pt"));
    }

    #[test]
    fn refuses_a_geometry_the_tools_disagree_on() {
        let page = |width: f32, height: f32, width_px: u32, height_px: u32| RasterPage {
            page: 1,
            png: Vec::new(),
            width_px,
            height_px,
            page_box: pdfops::PageBox {
                page: 1,
                width,
                height,
            },
        };

        assert!(check_geometry(&[page(595.276, 841.89, 1654, 2339)], 200).is_ok());
        // A page rotated by 90 degrees: the raster is landscape, the box portrait
        assert!(check_geometry(&[page(595.276, 841.89, 2339, 1654)], 200).is_err());
        assert!(check_geometry(&[page(0.0, 841.89, 1654, 2339)], 200).is_err());
    }

    #[test]
    fn sizes_the_batch_on_the_resolution() {
        assert_eq!(batch_pages(200), 6);
        assert_eq!(batch_pages(400), 1);
        assert_eq!(batch_pages(72), MAX_BATCH_PAGES);
    }

    #[test]
    fn counts_the_boxes_of_each_page() {
        let rect = |page| Rect {
            page,
            x0: 0.0,
            y0: 0.0,
            x1: 1.0,
            y1: 1.0,
        };
        let counts = counts_per_page(&[rect(1), rect(3), rect(3), rect(9)], 4);

        assert_eq!(counts.len(), 2);
        assert_eq!(counts[0].page, 1);
        assert_eq!(counts[1].count, 2);
    }
}

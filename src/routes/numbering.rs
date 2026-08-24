//! Page numbers on a document this service did not render.
//!
//! The renderer has always been able to number what it lays out itself, through the `@page`
//! margin boxes of the stylesheet. That stops at the door: a contract signed elsewhere and
//! uploaded here has no stylesheet to amend. So the numbers are drawn on a transparent PDF
//! of the same geometry and the same page count, and `qpdf --overlay` stamps it on.
//!
//! The geometry is read from the source rather than assumed, and read *per page*: qpdf centres
//! the overlay page on its destination and only ever shrinks it — it never enlarges it. An
//! overlay built for the A5 first page of a document therefore lands centred, unscaled, in the
//! middle of the A4 pages that follow, which puts the number in the body text rather than in
//! the margin. So the layer carries one named `@page` per distinct size, and each page of the
//! layer is exactly the size of the page it stamps.

use crate::auth::PublicOrKey;
use crate::exec;
use crate::helpers;
use crate::pdfops;
use crate::types::{AppError, Check, ToolOutput, ToolResponse, Verdict};
use rocket::fs::NamedFile;
use rocket::serde::json::Json;
use rocket::Either;
use serde::Deserialize;
use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;
use tempfile::Builder;

const DEFAULT_FORMAT: &str = "{page}";
const MAX_FORMAT_CHARS: usize = 64;

const DEFAULT_FONT_SIZE: f32 = 10.0;
const MIN_FONT_SIZE: f32 = 4.0;
const MAX_FONT_SIZE: f32 = 72.0;

const DEFAULT_MARGIN: &str = "1.5cm";
const MAX_MARGIN_CHARS: usize = 16;
const MAX_MARGIN_VALUE: f32 = 200.0;
const MARGIN_UNITS: [&str; 5] = ["pt", "mm", "cm", "in", "px"];

/// A displayed number is a label, not a count: a caller numbering an appendix from 900 is
/// legitimate, one numbering from a billion is a typo that would bloat the overlay HTML.
const MAX_START: i64 = 1_000_000;
const MAX_FROM_PAGE: i64 = 100_000;

/// Two page sizes closer than this are the same page size: poppler prints three decimals
/// and a document assembled from several tools rounds them differently.
const GEOMETRY_TOLERANCE_PT: f32 = 0.5;

#[derive(Deserialize)]
pub struct NumberPagesRequest {
    pub pdf: String,
    pub format: Option<String>,
    pub position: Option<String>,
    pub start: Option<i64>,
    pub from_page: Option<i64>,
    pub font_size: Option<f32>,
    pub margin: Option<String>,
    pub client_id: Option<String>,
    pub pdf_name: Option<String>,
    pub output: Option<ToolOutput>,
}

#[post("/pages/number", format = "json", data = "<req>")]
pub async fn number_pages(
    key: PublicOrKey,
    req: Json<NumberPagesRequest>,
) -> Result<Either<NamedFile, Json<ToolResponse>>, AppError> {
    let req = req.into_inner();

    // A malformed format must not cost a render slot to find out
    options(&req)?;

    let (produced, response) = exec::as_owner(key.0, exec::offload(move || run(req))).await?;
    helpers::deliver_tool(produced, response, "numbering").await
}

/// All the blocking work. Public and free of Rocket: the async job dispatcher calls it as is.
pub fn run(req: NumberPagesRequest) -> Result<(tempfile::TempPath, ToolResponse), AppError> {
    let opts = options(&req)?;
    let source = helpers::resolve_readable_pdf(&req.pdf)?;

    let source_pages = pdfops::page_count(&source)?;
    let geometries = read_geometry(&source, source_pages)?;
    let first = *geometries.first().ok_or_else(|| AppError::ProcessFailed {
        message: "pdfinfo did not report a page size".to_string(),
        stderr: String::new(),
    })?;

    // A report we cannot map onto the pages one by one leaves us with page 1's size for the
    // whole layer — the old behaviour, and the verdict says what it costs.
    let per_page = per_page_geometry(&geometries, source_pages);
    let layers = per_page
        .clone()
        .unwrap_or_else(|| vec![first; source_pages]);

    let overlay = helpers::run_weasyprint_plain(&build_overlay(&layers, &opts))?;
    let overlay_pages = pdfops::page_count(&overlay)?;

    let out_temp = Builder::new().suffix(".pdf").tempfile()?;
    let out_path = helpers::path_to_str(out_temp.path())?.to_string();

    helpers::run_tool(
        Command::new("qpdf")
            .arg(helpers::path_to_str(&source)?)
            .arg("--overlay")
            .arg(helpers::path_to_str(&overlay)?)
            .arg("--")
            .arg(&out_path),
        "qpdf",
        "Page numbering failed",
    )?;

    let produced = out_temp.into_temp_path();
    let out_pages = pdfops::page_count(&produced)?;

    let mut response = helpers::finish_tool(
        &produced,
        req.client_id,
        req.pdf_name,
        req.output.unwrap_or_default(),
        "numbered.pdf",
    )?;
    response.pages = Some(out_pages);
    response.verdict = Some(verdict(
        source_pages,
        out_pages,
        overlay_pages,
        &layers,
        per_page.is_some(),
        &opts,
    ));

    Ok((produced, response))
}

// ------------ Options ------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Position {
    BottomCenter,
    BottomLeft,
    BottomRight,
    TopCenter,
    TopLeft,
    TopRight,
}

impl Position {
    fn parse(value: &str) -> Result<Position, AppError> {
        match value {
            "bottom-center" => Ok(Position::BottomCenter),
            "bottom-left" => Ok(Position::BottomLeft),
            "bottom-right" => Ok(Position::BottomRight),
            "top-center" => Ok(Position::TopCenter),
            "top-left" => Ok(Position::TopLeft),
            "top-right" => Ok(Position::TopRight),
            other => Err(AppError::BadRequest(format!(
                "\"position\" must be one of bottom-center, bottom-left, bottom-right, \
top-center, top-left, top-right (got \"{}\")",
                other
            ))),
        }
    }

    /// The two CSS declarations that pin the number to the named corner, at `margin` from
    /// each edge it touches
    fn css(self, margin: &str) -> (String, String) {
        let vertical = match self {
            Position::TopCenter | Position::TopLeft | Position::TopRight => {
                format!("top: {};", margin)
            }
            _ => format!("bottom: {};", margin),
        };

        let horizontal = match self {
            Position::BottomLeft | Position::TopLeft => {
                format!("left: {}; text-align: left;", margin)
            }
            Position::BottomRight | Position::TopRight => {
                format!("right: {}; text-align: right;", margin)
            }
            _ => format!("left: {}; right: {}; text-align: center;", margin, margin),
        };

        (vertical, horizontal)
    }
}

struct Options {
    format: String,
    position: Position,
    start: i64,
    from_page: usize,
    font_size: f32,
    margin: String,
}

/// Validate the request into what the overlay needs. Called twice — once before queueing,
/// once inside the job — because `run` is also an entry point and must not trust its caller.
fn options(req: &NumberPagesRequest) -> Result<Options, AppError> {
    let format = req.format.as_deref().unwrap_or(DEFAULT_FORMAT);
    validate_format(format)?;

    let position = match req.position.as_deref() {
        Some(value) => Position::parse(value)?,
        None => Position::BottomCenter,
    };

    let start = req.start.unwrap_or(1);
    if !(0..=MAX_START).contains(&start) {
        return Err(AppError::BadRequest(format!(
            "\"start\" must be between 0 and {}",
            MAX_START
        )));
    }

    let from_page = req.from_page.unwrap_or(1);
    if !(1..=MAX_FROM_PAGE).contains(&from_page) {
        return Err(AppError::BadRequest(format!(
            "\"from_page\" must be between 1 and {} (the first page is 1)",
            MAX_FROM_PAGE
        )));
    }

    let font_size = req.font_size.unwrap_or(DEFAULT_FONT_SIZE);
    if !font_size.is_finite() || !(MIN_FONT_SIZE..=MAX_FONT_SIZE).contains(&font_size) {
        return Err(AppError::BadRequest(format!(
            "\"font_size\" must be between {} and {} points",
            MIN_FONT_SIZE, MAX_FONT_SIZE
        )));
    }

    let margin = validate_margin(req.margin.as_deref().unwrap_or(DEFAULT_MARGIN))?;

    Ok(Options {
        format: format.to_string(),
        position,
        start,
        from_page: from_page as usize,
        font_size,
        margin,
    })
}

/// `format` is a template, not a stylesheet and not markup.
///
/// The renderer's own `page_number_format` is a raw CSS `content` value, which is why
/// `helpers::validate_css_content_value` has to police it character by character. This field
/// is the opposite bargain: the only syntax it carries is `{page}` and `{pages}`, everything
/// else is literal text that gets HTML-escaped on the way out. So the validation exists to
/// refuse a caller who thought otherwise — a `{counter(page)}` or an `<img onerror=…>` — with
/// a message that says so, rather than to make an injection safe.
fn validate_format(format: &str) -> Result<(), AppError> {
    if format.is_empty() {
        return Err(AppError::BadRequest(
            "\"format\" must not be empty".to_string(),
        ));
    }

    if format.chars().count() > MAX_FORMAT_CHARS {
        return Err(AppError::BadRequest(format!(
            "\"format\" must be at most {} characters",
            MAX_FORMAT_CHARS
        )));
    }

    if format.contains(['<', '>']) {
        return Err(AppError::BadRequest(
            "\"format\" is plain text with the tokens {page} and {pages}, not HTML: \
remove `<` and `>`"
                .to_string(),
        ));
    }

    if format.chars().any(char::is_control) {
        return Err(AppError::BadRequest(
            "\"format\" must not contain control characters".to_string(),
        ));
    }

    let mut rest = format;
    while let Some(index) = rest.find(['{', '}']) {
        let tail = &rest[index..];
        if let Some(after) = tail
            .strip_prefix("{pages}")
            .or_else(|| tail.strip_prefix("{page}"))
        {
            rest = after;
            continue;
        }

        return Err(AppError::BadRequest(
            "\"format\" only knows the tokens {page} and {pages}".to_string(),
        ));
    }

    Ok(())
}

/// The margin lands unquoted in the stylesheet, so it is a number and a unit or nothing
fn validate_margin(value: &str) -> Result<String, AppError> {
    let refuse = || {
        AppError::BadRequest(format!(
            "\"margin\" must be a length such as \"1.5cm\" (units: {}) (got \"{}\")",
            MARGIN_UNITS.join(", "),
            value
        ))
    };

    if value.is_empty() || value.len() > MAX_MARGIN_CHARS {
        return Err(refuse());
    }

    let digits = value
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(digits);

    if !unit.is_empty() && !MARGIN_UNITS.contains(&unit) {
        return Err(refuse());
    }

    match number.parse::<f32>() {
        Ok(parsed) if parsed.is_finite() && (0.0..=MAX_MARGIN_VALUE).contains(&parsed) => {
            // `bottom: 10;` is not a declaration: the browser drops it and the number goes to
            // the top-left corner of the layer without a word in the verdict. Zero is the one
            // length CSS lets go bare, so it is the one value that may arrive without a unit.
            if unit.is_empty() && parsed != 0.0 {
                return Err(AppError::BadRequest(format!(
                    "\"margin\": \"{}\" has no unit, so it is not a CSS length — write \"{}pt\" \
or \"{}mm\" (units: {})",
                    value,
                    value,
                    value,
                    MARGIN_UNITS.join(", ")
                )));
            }
            Ok(value.to_string())
        }
        _ => Err(refuse()),
    }
}

// ------------ Overlay ------------

/// Page geometry in points, as the page is seen once its rotation is applied
#[derive(Debug, Clone, Copy, PartialEq)]
struct Geometry {
    width: f32,
    height: f32,
}

impl Geometry {
    fn matches(&self, other: Geometry) -> bool {
        (self.width - other.width).abs() <= GEOMETRY_TOLERANCE_PT
            && (self.height - other.height).abs() <= GEOMETRY_TOLERANCE_PT
    }
}

fn read_geometry(pdf: &Path, pages: usize) -> Result<Vec<Geometry>, AppError> {
    let output = helpers::run_capture(
        Command::new("pdfinfo")
            .arg("-f")
            .arg("1")
            .arg("-l")
            .arg(pages.to_string())
            .arg(helpers::path_to_str(pdf)?),
        "pdfinfo",
        "pdfinfo failed",
    )?;

    Ok(parse_geometry(&String::from_utf8_lossy(&output.stdout)))
}

/// Read the `Page N size:` / `Page N rot:` pairs pdfinfo prints, in that order
fn parse_geometry(report: &str) -> Vec<Geometry> {
    let mut geometries: Vec<Geometry> = Vec::new();

    for line in report.lines() {
        let mut tokens = line.split_whitespace();
        if tokens.next() != Some("Page") {
            continue;
        }

        // `Page size:` on a uniform document, `Page 3 size:` when a range was requested
        let mut field = match tokens.next() {
            Some(token) => token,
            None => continue,
        };
        if field.parse::<usize>().is_ok() {
            field = match tokens.next() {
                Some(token) => token,
                None => continue,
            };
        }

        match field {
            "size:" => {
                let width = tokens.next().and_then(|value| value.parse::<f32>().ok());
                let separator = tokens.next();
                let height = tokens.next().and_then(|value| value.parse::<f32>().ok());
                if let (Some(width), Some("x"), Some(height)) = (width, separator, height) {
                    if width > 0.0 && height > 0.0 {
                        geometries.push(Geometry { width, height });
                    }
                }
            }
            "rot:" => {
                // A quarter-turned page is displayed, and overlaid by qpdf, in the swapped
                // geometry: an overlay built on the MediaBox would land sideways.
                let rotation = tokens
                    .next()
                    .and_then(|value| value.parse::<i32>().ok())
                    .unwrap_or(0);
                if rotation.rem_euclid(180) == 90 {
                    if let Some(last) = geometries.last_mut() {
                        std::mem::swap(&mut last.width, &mut last.height);
                    }
                }
            }
            _ => {}
        }
    }

    geometries
}

/// One geometry per source page, or `None` when the report cannot be mapped onto the pages.
///
/// `pdfinfo` prints a single `Page size:` line for a document whose pages are all alike and
/// one pair of lines per page otherwise. Any other count is a report we do not understand,
/// and guessing which page is which size would move numbers onto the wrong geometry.
fn per_page_geometry(geometries: &[Geometry], pages: usize) -> Option<Vec<Geometry>> {
    match geometries.len() {
        0 => None,
        1 => Some(vec![geometries[0]; pages]),
        found if found == pages => Some(geometries.to_vec()),
        _ => None,
    }
}

/// The distinct sizes of a layer, in the order they first appear, and the size each page uses.
///
/// Two sizes within `GEOMETRY_TOLERANCE_PT` share a `@page` rule: a document assembled from
/// several tools rounds 841.89 and 841.890 differently, and one rule per rounding would only
/// make the stylesheet longer.
fn size_classes(layers: &[Geometry]) -> (Vec<Geometry>, Vec<usize>) {
    let mut sizes: Vec<Geometry> = Vec::new();
    let mut classes: Vec<usize> = Vec::with_capacity(layers.len());

    for geometry in layers {
        let mut found = None;
        for (index, known) in sizes.iter().enumerate() {
            if known.matches(*geometry) {
                found = Some(index);
                break;
            }
        }

        classes.push(match found {
            Some(index) => index,
            None => {
                sizes.push(*geometry);
                sizes.len() - 1
            }
        });
    }

    (sizes, classes)
}

/// What each page carries, `None` for the pages before `from_page`
fn labels(pages: usize, opts: &Options) -> Vec<Option<String>> {
    (1..=pages)
        .map(|page| {
            if page < opts.from_page {
                return None;
            }
            let number = opts.start + (page - opts.from_page) as i64;
            Some(render_format(&opts.format, number, pages))
        })
        .collect()
}

/// Substitute the tokens, then escape: the substituted values are digits, so escaping the
/// rendered string is the same as escaping the literal parts and shorter to read.
fn render_format(format: &str, number: i64, pages: usize) -> String {
    let text = format
        .replace("{pages}", &pages.to_string())
        .replace("{page}", &number.to_string());
    helpers::escape_html(&text)
}

fn build_overlay(layers: &[Geometry], opts: &Options) -> String {
    let (vertical, horizontal) = opts.position.css(&opts.margin);
    let pages = layers.len();
    let (sizes, classes) = size_classes(layers);

    // One named `@page` per distinct size, and every layer page routed to the rule of the
    // page it stamps. qpdf then has nothing to scale: the layer already fits, so the margin
    // asked for is the margin measured on the result.
    let mut sheet = String::new();
    for (index, size) in sizes.iter().enumerate() {
        let _ = writeln!(
            sheet,
            "@page g{index} {{ size: {width:.3}pt {height:.3}pt; margin: 0; }}\n\
             .g{index} {{ page: g{index}; }}",
            index = index,
            width = size.width,
            height = size.height,
        );
    }

    let mut html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
{sheet}html, body {{ margin: 0; padding: 0; }}
/* Zero-height blocks that only break the page: the numbers are positioned against the page
   box itself, so nothing here can overflow and slip an extra page into the overlay. */
.page {{ height: 0; }}
.brk {{ break-after: page; }}
.num {{
  position: absolute;
  font-family: 'Helvetica', 'Arial', sans-serif;
  font-size: {font_size}pt;
  color: #000;
  {vertical}
  {horizontal}
}}
</style>
</head>
<body>
"#,
        sheet = sheet,
        font_size = opts.font_size,
        vertical = vertical,
        horizontal = horizontal,
    );

    for (index, label) in labels(pages, opts).into_iter().enumerate() {
        let break_after = if index + 1 == pages { "" } else { " brk" };
        let size_class = classes[index];
        match label {
            Some(text) => {
                let _ = writeln!(
                    html,
                    r#"<div class="page{} g{}"><span class="num">{}</span></div>"#,
                    break_after, size_class, text
                );
            }
            None => {
                let _ = writeln!(
                    html,
                    r#"<div class="page{} g{}"></div>"#,
                    break_after, size_class
                );
            }
        }
    }

    html.push_str("</body>\n</html>");
    html
}

// ------------ Verdict ------------

/// Check names are the translatable contract: `page-count` carries the same meaning as in
/// `/api/compress` and `/api/crop`; `overlay-alignment`, `page-geometry` and
/// `numbered-pages` belong to this route. The public site translates them by name.
fn verdict(
    source_pages: usize,
    out_pages: usize,
    overlay_pages: usize,
    layers: &[Geometry],
    per_page: bool,
    opts: &Options,
) -> Verdict {
    let mut checks = Vec::new();

    // An overlay that truncated the document is the silent damage this endpoint is most
    // able to cause, so it is the first thing the answer says about itself.
    checks.push(if out_pages == source_pages {
        Check::ok(
            "page-count",
            format!("{} pages in, {} pages out", source_pages, out_pages),
        )
    } else {
        Check::fail(
            "page-count",
            format!(
                "the overlay changed the page count: {} in, {} out",
                source_pages, out_pages
            ),
        )
    });

    checks.push(if overlay_pages == source_pages {
        Check::ok(
            "overlay-alignment",
            "one overlay page per source page".to_string(),
        )
    } else {
        Check::warn(
            "overlay-alignment",
            format!(
                "the numbering layer has {} pages for {} source pages: numbers may be missing \
past page {}",
                overlay_pages, source_pages, overlay_pages
            ),
        )
    });

    // What this check must never do is describe qpdf as a scaler. `--overlay` centres the
    // layer on its destination page and shrinks it only when it is too big; it never enlarges
    // it. A single layer stamped on a taller page therefore puts the number where the smaller
    // page's margin was, which is the body text of the bigger one — so either every layer
    // page matches its destination exactly, or the caller hears about the drift.
    let (sizes, _) = size_classes(layers);
    let first = sizes.first().copied().unwrap_or(Geometry {
        width: 0.0,
        height: 0.0,
    });
    checks.push(match (per_page, sizes.len()) {
        (true, 0 | 1) => Check::ok(
            "page-geometry",
            format!("every page is {:.0} x {:.0} pt", first.width, first.height),
        ),
        (true, count) => Check::ok(
            "page-geometry",
            format!(
                "{} page sizes in the document: the numbering layer carries one page of each \
size, so the margin asked for is the margin every number sits at",
                count
            ),
        ),
        (false, _) => Check::warn(
            "page-geometry",
            format!(
                "the page sizes could not be read one by one, so the whole numbering layer was \
built {:.0} x {:.0} pt like page 1. qpdf centres that layer and only ever shrinks it, never \
enlarges it: on a page bigger than page 1 the number lands towards the middle of the sheet \
rather than at the margin asked for",
                first.width, first.height
            ),
        ),
    });

    let numbered = source_pages.saturating_sub(opts.from_page.saturating_sub(1));
    checks.push(if numbered == 0 {
        Check::warn(
            "numbered-pages",
            format!(
                "\"from_page\" is {} on a {}-page document: no page was numbered",
                opts.from_page, source_pages
            ),
        )
    } else {
        Check::ok(
            "numbered-pages",
            format!(
                "pages {} to {} numbered, starting at {}",
                opts.from_page, source_pages, opts.start
            ),
        )
    });

    Verdict::from_checks(
        format!("{} of {} pages numbered", numbered, source_pages),
        checks,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> NumberPagesRequest {
        NumberPagesRequest {
            pdf: "asset://as_00000000000000000000000000000000".to_string(),
            format: None,
            position: None,
            start: None,
            from_page: None,
            font_size: None,
            margin: None,
            client_id: None,
            pdf_name: None,
            output: None,
        }
    }

    #[test]
    fn an_empty_body_numbers_every_page_from_one_at_the_bottom_centre() {
        let opts = options(&request()).unwrap();

        assert_eq!(opts.format, "{page}");
        assert_eq!(opts.position, Position::BottomCenter);
        assert_eq!(opts.start, 1);
        assert_eq!(opts.from_page, 1);

        let labels = labels(3, &opts);
        assert_eq!(
            labels,
            vec![
                Some("1".to_string()),
                Some("2".to_string()),
                Some("3".to_string())
            ]
        );
    }

    #[test]
    fn both_tokens_are_substituted_with_the_page_and_the_total() {
        assert_eq!(render_format("{page} / {pages}", 3, 12), "3 / 12");
        assert_eq!(render_format("Page {page} of {pages}", 1, 1), "Page 1 of 1");
    }

    #[test]
    fn literal_text_of_the_format_is_html_escaped() {
        assert_eq!(render_format("A & B — {page}", 2, 9), "A &amp; B — 2");
        // Quotes cannot break out of the span attribute either
        assert_eq!(render_format("\"{page}\"", 2, 9), "&quot;2&quot;");
    }

    #[test]
    fn a_format_may_only_use_the_two_known_tokens() {
        assert!(validate_format("{page}").is_ok());
        assert!(validate_format("- {page} / {pages} -").is_ok());
        assert!(validate_format("no token at all").is_ok());

        assert!(validate_format("{total}").is_err());
        assert!(validate_format("{counter(page)}").is_err());
        assert!(validate_format("{page").is_err());
        assert!(validate_format("page}").is_err());
        assert!(validate_format("{{page}}").is_err());
    }

    #[test]
    fn a_format_that_tries_to_be_markup_or_css_is_refused() {
        assert!(validate_format("<b>{page}</b>").is_err());
        assert!(validate_format("<img src=x onerror=alert(1)>").is_err());
        assert!(validate_format("{page}</span><script>").is_err());
    }

    #[test]
    fn a_format_is_bounded_and_never_empty_or_multiline() {
        assert!(validate_format("").is_err());
        assert!(validate_format("{page}\n.num{color:red}").is_err());
        assert!(validate_format(&"p".repeat(MAX_FORMAT_CHARS)).is_ok());
        assert!(validate_format(&"p".repeat(MAX_FORMAT_CHARS + 1)).is_err());
    }

    #[test]
    fn an_unknown_position_lists_the_ones_that_exist() {
        assert_eq!(Position::parse("top-right").unwrap(), Position::TopRight);

        let err = Position::parse("middle").unwrap_err();
        match err {
            AppError::BadRequest(message) => {
                assert!(message.contains("bottom-center"));
                assert!(message.contains("top-right"));
            }
            other => panic!("expected a bad request, got {:?}", other),
        }
    }

    #[test]
    fn each_position_pins_the_number_to_the_edges_it_names() {
        let (vertical, horizontal) = Position::TopLeft.css("1cm");
        assert!(vertical.contains("top: 1cm"));
        assert!(horizontal.contains("left: 1cm"));

        let (vertical, horizontal) = Position::BottomRight.css("1cm");
        assert!(vertical.contains("bottom: 1cm"));
        assert!(horizontal.contains("right: 1cm"));

        let (_, horizontal) = Position::BottomCenter.css("1cm");
        assert!(horizontal.contains("text-align: center"));
    }

    #[test]
    fn a_margin_that_is_not_a_length_cannot_reach_the_stylesheet() {
        assert_eq!(validate_margin("1.5cm").unwrap(), "1.5cm");
        assert_eq!(validate_margin("36pt").unwrap(), "36pt");
        assert_eq!(validate_margin("0").unwrap(), "0");

        assert!(validate_margin("1.5cm; color: red").is_err());
        assert!(validate_margin("expression(1)").is_err());
        assert!(validate_margin("calc(1cm)").is_err());
        assert!(validate_margin("-1cm").is_err());
        assert!(validate_margin("9999cm").is_err());
        assert!(validate_margin("").is_err());
    }

    #[test]
    fn a_margin_without_a_unit_is_refused_rather_than_dropped_by_the_stylesheet() {
        // `bottom: 10;` is invalid CSS: the number would silently move to the corner
        for value in ["10", "1.5", "0.5", "200"] {
            let error = validate_margin(value).unwrap_err();
            match error {
                AppError::BadRequest(message) => {
                    assert!(message.contains("no unit"), "{}: {}", value, message);
                    assert!(message.contains("pt"), "{}: {}", value, message);
                }
                other => panic!("expected a bad request for {:?}, got {:?}", value, other),
            }
        }

        // Zero is the one length CSS accepts bare
        assert_eq!(validate_margin("0").unwrap(), "0");
        assert_eq!(validate_margin("0.0").unwrap(), "0.0");
    }

    #[test]
    fn a_cover_page_keeps_no_number_and_the_count_restarts_where_asked() {
        let mut req = request();
        req.from_page = Some(3);
        req.start = Some(1);
        let opts = options(&req).unwrap();

        assert_eq!(
            labels(5, &opts),
            vec![
                None,
                None,
                Some("1".to_string()),
                Some("2".to_string()),
                Some("3".to_string())
            ]
        );
    }

    #[test]
    fn a_from_page_before_the_first_page_is_refused() {
        let mut req = request();
        req.from_page = Some(0);
        assert!(options(&req).is_err());

        req.from_page = Some(-2);
        assert!(options(&req).is_err());
    }

    #[test]
    fn a_font_size_outside_the_readable_range_is_refused() {
        let mut req = request();
        req.font_size = Some(0.0);
        assert!(options(&req).is_err());

        req.font_size = Some(f32::NAN);
        assert!(options(&req).is_err());

        req.font_size = Some(9.0);
        assert!(options(&req).is_ok());
    }

    #[test]
    fn the_overlay_has_exactly_one_page_per_source_page() {
        let opts = options(&request()).unwrap();
        let a4 = Geometry {
            width: 595.276,
            height: 841.89,
        };
        let html = build_overlay(&[a4; 4], &opts);

        assert_eq!(html.matches("class=\"page").count(), 4);
        assert_eq!(html.matches("page brk").count(), 3);
        assert!(html.contains("size: 595.276pt 841.890pt"));
        // One size, one rule: a uniform document gains nothing from a second one
        assert_eq!(html.matches("@page ").count(), 1);
    }

    #[test]
    fn the_overlay_carries_no_number_where_the_source_carries_none() {
        let mut req = request();
        req.from_page = Some(2);
        let opts = options(&req).unwrap();
        let a4 = Geometry {
            width: 595.0,
            height: 842.0,
        };
        let html = build_overlay(&[a4; 2], &opts);

        assert!(html.contains("<div class=\"page brk g0\"></div>"));
        assert_eq!(html.matches("class=\"num\"").count(), 1);
    }

    #[test]
    fn each_layer_page_is_the_size_of_the_page_it_stamps() {
        let opts = options(&request()).unwrap();
        let a5 = Geometry {
            width: 419.528,
            height: 595.276,
        };
        let a4 = Geometry {
            width: 595.276,
            height: 841.89,
        };

        // The document the review reproduced: an A5 cover, then A4. Without a rule per size
        // the A5 layer is centred unscaled on the A4 pages and the number lands 151 pt up.
        let html = build_overlay(&[a5, a4, a4], &opts);

        assert!(html.contains("@page g0 { size: 419.528pt 595.276pt; margin: 0; }"));
        assert!(html.contains("@page g1 { size: 595.276pt 841.890pt; margin: 0; }"));
        assert!(html.contains(".g1 { page: g1; }"));
        assert!(html.contains(r#"<div class="page brk g0">"#));
        assert_eq!(html.matches(r#"class="page brk g1""#).count(), 1);
        assert_eq!(html.matches(r#"class="page g1""#).count(), 1);
    }

    #[test]
    fn sizes_within_the_rounding_tolerance_share_one_page_rule() {
        let opts = options(&request()).unwrap();
        let rounded = Geometry {
            width: 595.276,
            height: 841.89,
        };
        let printed = Geometry {
            width: 595.28,
            height: 841.9,
        };

        let html = build_overlay(&[rounded, printed], &opts);
        assert_eq!(html.matches("@page ").count(), 1);
        assert_eq!(html.matches(r#"class="page brk g0""#).count(), 1);
        assert_eq!(html.matches(r#"class="page g0""#).count(), 1);
    }

    #[test]
    fn a_size_report_that_cannot_be_matched_to_the_pages_is_not_guessed() {
        let a4 = Geometry {
            width: 595.276,
            height: 841.89,
        };
        let letter = Geometry {
            width: 612.0,
            height: 792.0,
        };

        // One line for a uniform document, one per page otherwise: those two we can map
        assert_eq!(per_page_geometry(&[a4], 3), Some(vec![a4; 3]));
        assert_eq!(per_page_geometry(&[a4, letter], 2), Some(vec![a4, letter]));
        // Anything else would put a number on the wrong geometry
        assert_eq!(per_page_geometry(&[a4, letter], 5), None);
        assert_eq!(per_page_geometry(&[], 2), None);
    }

    #[test]
    fn page_geometry_is_read_per_page_and_follows_the_rotation() {
        let report = "Pages:           3\n\
                      Page    1 size:  595.276 x 841.89 pts (A4)\n\
                      Page    1 rot:   0\n\
                      Page    2 size:  595.276 x 841.89 pts (A4)\n\
                      Page    2 rot:   90\n\
                      Page    3 size:  612 x 792 pts (letter)\n\
                      Page    3 rot:   180\n\
                      File size:       4210 bytes\n";

        let geometries = parse_geometry(report);
        assert_eq!(geometries.len(), 3);
        assert_eq!(geometries[0].width, 595.276);
        // Quarter turned: the page is seen, and overlaid, in landscape
        assert_eq!(geometries[1].width, 841.89);
        assert_eq!(geometries[1].height, 595.276);
        // Half turned: same geometry as unrotated
        assert_eq!(geometries[2].width, 612.0);
    }

    #[test]
    fn a_uniform_document_reports_its_size_without_a_page_number() {
        let report = "Pages:           2\n\
                      Page size:       612 x 792 pts (letter)\n\
                      Page rot:        0\n";

        let geometries = parse_geometry(report);
        assert_eq!(geometries.len(), 1);
        assert_eq!(geometries[0].height, 792.0);
    }

    #[test]
    fn a_report_without_a_page_size_yields_nothing_rather_than_a_default() {
        assert!(parse_geometry("Producer: none\nPages: 2\n").is_empty());
        assert!(parse_geometry("Page 1 size: wide x tall pts\n").is_empty());
    }

    #[test]
    fn a_page_count_the_overlay_changed_fails_the_verdict() {
        let opts = options(&request()).unwrap();
        let a4 = Geometry {
            width: 595.276,
            height: 841.89,
        };

        let clean = verdict(12, 12, 12, &[a4; 12], true, &opts);
        assert_eq!(clean.status, "ok");
        assert_eq!(clean.score, 100);

        let truncated = verdict(12, 11, 12, &[a4; 12], true, &opts);
        assert_eq!(truncated.status, "fail");
        assert!(truncated
            .checks
            .iter()
            .any(|check| check.name == "page-count"
                && check.status == "fail"
                && check.detail.contains("11")));
    }

    #[test]
    fn a_document_of_mixed_geometry_gets_one_layer_per_size_and_says_so() {
        let opts = options(&request()).unwrap();
        let a4 = Geometry {
            width: 595.276,
            height: 841.89,
        };
        let letter = Geometry {
            width: 612.0,
            height: 792.0,
        };

        let mixed = verdict(2, 2, 2, &[a4, letter], true, &opts);
        assert_eq!(mixed.status, "ok");
        let geometry = mixed
            .checks
            .iter()
            .find(|check| check.name == "page-geometry")
            .unwrap();
        assert_eq!(geometry.status, "ok");
        assert!(
            geometry.detail.contains("2 page sizes"),
            "{}",
            geometry.detail
        );
    }

    #[test]
    fn a_single_layer_on_mixed_geometry_says_qpdf_never_enlarges_it() {
        let opts = options(&request()).unwrap();
        let a4 = Geometry {
            width: 595.276,
            height: 841.89,
        };

        // pdfinfo gave a report we could not map page by page: the layer is page 1's size
        // everywhere, and the caller must hear that the number can miss its margin rather
        // than read the old, comfortable and false "scaled to fit".
        let blind = verdict(3, 3, 3, &[a4; 3], false, &opts);
        assert_eq!(blind.status, "warn");
        let geometry = blind
            .checks
            .iter()
            .find(|check| check.name == "page-geometry")
            .unwrap();
        assert_eq!(geometry.status, "warn");
        assert!(geometry.detail.contains("never"), "{}", geometry.detail);
        assert!(geometry.detail.contains("shrinks"), "{}", geometry.detail);
        assert!(
            !geometry.detail.contains("scaled to fit"),
            "the verdict must not describe qpdf as a scaler: {}",
            geometry.detail
        );
    }

    #[test]
    fn numbering_that_starts_past_the_last_page_warns_instead_of_numbering_nothing_silently() {
        let mut req = request();
        req.from_page = Some(20);
        let opts = options(&req).unwrap();
        let a4 = Geometry {
            width: 595.276,
            height: 841.89,
        };

        let report = verdict(12, 12, 12, &[a4; 12], true, &opts);
        assert_eq!(report.status, "warn");
        assert!(report.summary.starts_with("0 of 12"));
    }
}

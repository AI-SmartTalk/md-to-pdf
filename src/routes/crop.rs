//! Crop pages by rewriting their CropBox, either to a box the caller gives or to the
//! bounding box of the text Poppler can see.

use crate::auth::PublicOrKey;
use crate::exec;
use crate::helpers;
use crate::pdfops::{self, PageBox, Word};
use crate::types::*;
use rocket::fs::NamedFile;
use rocket::serde::json::Json;
use rocket::Either;
use serde::Deserialize;
use std::fmt::Write as _;
use std::io::Write as _;
use std::process::Command;
use tempfile::Builder;

/// White space kept around the detected content when no `margin` is given
const DEFAULT_MARGIN: f64 = 12.0;
const MAX_MARGIN: f64 = 500.0;
/// Acrobat refuses a user space larger than 14400 pt; anything past this is a typo
const MAX_COORD: f64 = 20_000.0;

/// `[left, bottom, right, top]` in PostScript points, or the string `"auto"`
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum CropBox {
    Explicit([f64; 4]),
    Keyword(String),
}

#[derive(Deserialize)]
pub struct CropRequest {
    pub pdf: String,
    /// Absent means `"auto"`: the caller who does not know the geometry wants it computed
    #[serde(rename = "box")]
    pub crop_box: Option<CropBox>,
    pub margin: Option<f64>,
    /// `"all"`, `"3"`, `"2-5"` or `"1,4,7-9"`. Untargeted pages keep their box.
    pub pages: Option<String>,
    pub client_id: Option<String>,
    pub pdf_name: Option<String>,
    pub output: Option<ToolOutput>,
}

#[post("/crop", format = "json", data = "<req>")]
pub async fn crop(
    key: PublicOrKey,
    req: Json<CropRequest>,
) -> Result<Either<NamedFile, Json<ToolResponse>>, AppError> {
    let req = req.into_inner();

    // A malformed box or page list is answered before the request costs a render slot
    plan(&req)?;

    let (produced, response) = exec::as_owner(key.0, exec::offload(move || run(req))).await?;
    helpers::deliver_tool(produced, response, "crop").await
}

/// All the blocking work. Public and free of Rocket: the async job dispatcher calls it as is.
pub fn run(req: CropRequest) -> Result<(tempfile::TempPath, ToolResponse), AppError> {
    let (mode, selection) = plan(&req)?;
    let source = helpers::resolve_pdf_source(&req.pdf)?;

    // One pdftotext run yields both the page geometry and the words the auto mode needs
    let (pages, words) = pdfops::words(&source)?;
    selection.validate(pages.len())?;

    let mut warnings: Vec<String> = Vec::new();
    let mut boxes: Vec<Option<Rect>> = Vec::with_capacity(pages.len());
    let mut without_content: Vec<usize> = Vec::new();

    for page in &pages {
        if !selection.contains(page.page) {
            boxes.push(None);
            continue;
        }

        let rect = match mode {
            Mode::Explicit(rect) => {
                let (fitted, clamped) = fit_to_page(rect, page)?;
                if clamped {
                    warnings.push(format!(
                        "Page {}: \"box\" reached past the page and was clamped to it",
                        page.page
                    ));
                }
                Some(fitted)
            }
            // A page whose content cannot be measured keeps its own box: cropping to
            // nothing would hand back a document with blank pages
            Mode::Auto { margin } => content_box(&words, page, margin),
        };

        if rect.is_none() {
            without_content.push(page.page);
        }
        boxes.push(rect);
    }

    let cropped = boxes.iter().filter(|rect| rect.is_some()).count();
    let produced = if cropped == 0 {
        // Nothing to rewrite: copying beats a Ghostscript round trip that would re-encode
        // every font and image of the document for no change at all
        warnings.push("No page was cropped: the source is returned unchanged".to_string());
        copy_of(&source)?
    } else {
        rewrite_crop_boxes(&source, &boxes)?
    };

    // Measured before `finish_tool`, which moves the file away when an asset was asked for
    let out_pages = pdfops::page_count(&produced)?;

    let mut response = helpers::finish_tool(
        &produced,
        req.client_id,
        req.pdf_name,
        req.output.unwrap_or_default(),
        "cropped.pdf",
    )?;
    response.pages = Some(out_pages);
    response.verdict = Some(verdict(
        pages.len(),
        out_pages,
        cropped,
        &without_content,
        mode,
    ));
    if !warnings.is_empty() {
        response.warnings = Some(warnings);
    }

    Ok((produced, response))
}

// ------------ Request planning ------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Explicit(Rect),
    Auto { margin: f64 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Rect {
    left: f64,
    bottom: f64,
    right: f64,
    top: f64,
}

impl Rect {
    fn width(&self) -> f64 {
        self.right - self.left
    }

    fn height(&self) -> f64 {
        self.top - self.bottom
    }

    fn clamp(self, width: f64, height: f64) -> Rect {
        Rect {
            left: self.left.clamp(0.0, width),
            bottom: self.bottom.clamp(0.0, height),
            right: self.right.clamp(0.0, width),
            top: self.top.clamp(0.0, height),
        }
    }
}

/// Everything that can be decided without touching the document
fn plan(req: &CropRequest) -> Result<(Mode, Selection), AppError> {
    let margin = req.margin.unwrap_or(DEFAULT_MARGIN);
    if !margin.is_finite() || !(0.0..=MAX_MARGIN).contains(&margin) {
        return Err(AppError::BadRequest(format!(
            "\"margin\" must be between 0 and {} points",
            MAX_MARGIN
        )));
    }

    let mode = match &req.crop_box {
        None => Mode::Auto { margin },
        Some(CropBox::Keyword(word)) if word.trim().eq_ignore_ascii_case("auto") => {
            Mode::Auto { margin }
        }
        Some(CropBox::Keyword(word)) => {
            return Err(AppError::BadRequest(format!(
                "\"box\" must be [left, bottom, right, top] in points or \"auto\", not \"{}\"",
                clip(word)
            )))
        }
        Some(CropBox::Explicit(values)) => Mode::Explicit(explicit_rect(values)?),
    };

    Ok((mode, Selection::parse(req.pages.as_deref())?))
}

fn explicit_rect(values: &[f64; 4]) -> Result<Rect, AppError> {
    if values.iter().any(|value| !value.is_finite()) {
        return Err(AppError::BadRequest(
            "\"box\" must hold four finite numbers".to_string(),
        ));
    }

    if values
        .iter()
        .any(|value| *value < 0.0 || *value > MAX_COORD)
    {
        return Err(AppError::BadRequest(format!(
            "\"box\" coordinates must be between 0 and {} points",
            MAX_COORD
        )));
    }

    let rect = Rect {
        left: values[0],
        bottom: values[1],
        right: values[2],
        top: values[3],
    };

    // An inverted or flat box makes a viewer render an empty page: say so instead
    if rect.width() < 1.0 || rect.height() < 1.0 {
        return Err(AppError::BadRequest(format!(
            "\"box\" is [left, bottom, right, top]: left must be at least 1 pt below right \
             and bottom at least 1 pt below top, got [{} {} {} {}]",
            values[0], values[1], values[2], values[3]
        )));
    }

    Ok(rect)
}

// ------------ Page selection ------------

#[derive(Debug, Clone, PartialEq)]
enum Selection {
    All,
    /// Inclusive 1-based ranges, in the order the caller wrote them
    Ranges(Vec<(usize, usize)>),
}

impl Selection {
    fn parse(spec: Option<&str>) -> Result<Selection, AppError> {
        let spec = match spec {
            Some(spec) => spec.trim(),
            None => return Ok(Selection::All),
        };

        if spec.is_empty() || spec.eq_ignore_ascii_case("all") {
            return Ok(Selection::All);
        }

        let mut ranges = Vec::new();
        for part in spec.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }

            let (head, tail) = match part.split_once('-') {
                Some((head, tail)) => (head.trim(), tail.trim()),
                None => (part, part),
            };

            let bounds = (head.parse::<usize>().ok(), tail.parse::<usize>().ok());
            let (first, last) = match bounds {
                (Some(first), Some(last)) if first >= 1 && last >= 1 => (first, last),
                _ => {
                    return Err(AppError::BadRequest(format!(
                        "\"pages\" must be page numbers, ranges such as \"2-5\", or \"all\", \
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

            ranges.push((first, last));
        }

        if ranges.is_empty() {
            return Ok(Selection::All);
        }

        Ok(Selection::Ranges(ranges))
    }

    fn validate(&self, count: usize) -> Result<(), AppError> {
        let last = match self {
            Selection::All => return Ok(()),
            Selection::Ranges(ranges) => ranges.iter().map(|(_, last)| *last).max().unwrap_or(0),
        };

        if last > count {
            return Err(AppError::BadRequest(format!(
                "\"pages\" reaches page {} but the document has {}",
                last, count
            )));
        }

        Ok(())
    }

    fn contains(&self, page: usize) -> bool {
        match self {
            Selection::All => true,
            Selection::Ranges(ranges) => ranges
                .iter()
                .any(|(first, last)| page >= *first && page <= *last),
        }
    }
}

// ------------ Geometry ------------

/// Bounding box of the words Poppler found on this page, plus `margin` of white space.
///
/// `pdftotext` measures from the top-left corner while a CropBox is measured from the
/// bottom-left, hence the flip. `None` means nothing measurable was found.
fn content_box(words: &[Word], page: &PageBox, margin: f64) -> Option<Rect> {
    let (width, height) = (page.width as f64, page.height as f64);
    if width <= 0.0 || height <= 0.0 {
        return None;
    }

    let mut content: Option<Rect> = None;
    for word in words.iter().filter(|word| word.page == page.page) {
        if word.text.trim().is_empty() {
            continue;
        }
        let (x0, y0, x1, y1) = (
            word.x0 as f64,
            word.y0 as f64,
            word.x1 as f64,
            word.y1 as f64,
        );
        if ![x0, y0, x1, y1].iter().all(|value| value.is_finite()) {
            continue;
        }

        content = Some(match content {
            None => Rect {
                left: x0,
                bottom: y0,
                right: x1,
                top: y1,
            },
            Some(seen) => Rect {
                left: seen.left.min(x0),
                bottom: seen.bottom.min(y0),
                right: seen.right.max(x1),
                top: seen.top.max(y1),
            },
        });
    }

    // `content` is still in top-left coordinates here: bottom holds the smallest y
    let content = content?;
    let rect = Rect {
        left: content.left - margin,
        bottom: height - content.top - margin,
        right: content.right + margin,
        top: height - content.bottom + margin,
    }
    .clamp(width, height);

    if rect.width() < 1.0 || rect.height() < 1.0 {
        return None;
    }

    Some(rect)
}

/// Keep an explicit box inside the page, and refuse one that misses it entirely
fn fit_to_page(rect: Rect, page: &PageBox) -> Result<(Rect, bool), AppError> {
    let (width, height) = (page.width as f64, page.height as f64);
    // Geometry Poppler could not report: the caller's box is all we have
    if width <= 0.0 || height <= 0.0 {
        return Ok((rect, false));
    }

    let fitted = rect.clamp(width, height);
    if fitted.width() < 1.0 || fitted.height() < 1.0 {
        return Err(AppError::BadRequest(format!(
            "\"box\" [{:.0} {:.0} {:.0} {:.0}] lies outside page {}, which is {:.0}x{:.0} pt",
            rect.left, rect.bottom, rect.right, rect.top, page.page, width, height
        )));
    }

    Ok((fitted, fitted != rect))
}

// ------------ Ghostscript ------------

/// A pdfmark per page, dispatched on the number of pages already written.
///
/// Ghostscript's PDF interpreter calls `BeginPage` several times for one page, so a counter
/// of our own would drift; its operand does not. `/PAGES` would have been shorter but it
/// applies one box to the whole document, which auto cropping cannot use.
fn crop_program(boxes: &[Option<Rect>]) -> String {
    let mut table = String::new();
    for rect in boxes {
        match rect {
            Some(rect) => {
                let _ = write!(
                    table,
                    " [{:.4} {:.4} {:.4} {:.4}]",
                    rect.left, rect.bottom, rect.right, rect.top
                );
            }
            None => table.push_str(" null"),
        }
    }

    format!(
        "%!PS\n\
         true setglobal\n\
         globaldict /MdToPdfCropBoxes [{}] put\n\
         false setglobal\n\
         << /BeginPage {{\n\
         \x20 dup globaldict /MdToPdfCropBoxes get length lt {{\n\
         \x20   globaldict /MdToPdfCropBoxes get exch get\n\
         \x20   dup null eq {{ pop }} {{ mark exch /CropBox exch /PAGE pdfmark }} ifelse\n\
         \x20 }} {{ pop }} ifelse\n\
         }} bind >> setpagedevice\n",
        table
    )
}

fn rewrite_crop_boxes(
    source: &std::path::Path,
    boxes: &[Option<Rect>],
) -> Result<tempfile::TempPath, AppError> {
    let mut program = Builder::new().suffix(".ps").tempfile()?;
    program.write_all(crop_program(boxes).as_bytes())?;
    let program = program.into_temp_path();

    let out_temp = Builder::new().suffix(".pdf").tempfile()?;
    let out_path = helpers::path_to_str(out_temp.path())?.to_string();

    // The bytes come from whoever uploaded them: Ghostscript never runs without its sandbox
    helpers::run_tool(
        Command::new("gs")
            .arg("-dSAFER")
            .arg("-dBATCH")
            .arg("-dNOPAUSE")
            .arg("-dNOOUTERSAVE")
            .arg("-sDEVICE=pdfwrite")
            .arg("-o")
            .arg(&out_path)
            .arg(helpers::path_to_str(&program)?)
            .arg(helpers::path_to_str(source)?),
        "gs",
        "PDF cropping failed",
    )?;

    Ok(out_temp.into_temp_path())
}

fn copy_of(source: &std::path::Path) -> Result<tempfile::TempPath, AppError> {
    let out_temp = Builder::new().suffix(".pdf").tempfile()?;
    std::fs::copy(source, out_temp.path())?;
    Ok(out_temp.into_temp_path())
}

// ------------ Verdict ------------

/// Check names are the translatable contract: `page-count` is the same name, on the same
/// question, as in `/api/compress`, `/api/repair` and `/api/pdfa`; `pages-cropped` and
/// `content-detected` belong to this route. The public site translates them by name, so
/// only the English `detail` is written here.
fn verdict(
    source_pages: usize,
    out_pages: usize,
    cropped: usize,
    without_content: &[usize],
    mode: Mode,
) -> Verdict {
    let mut checks = vec![if out_pages == source_pages {
        Check::ok(
            "page-count",
            format!("{} pages, as in the source", out_pages),
        )
    } else {
        Check::fail(
            "page-count",
            format!("{} pages out, {} in", out_pages, source_pages),
        )
    }];

    checks.push(if cropped > 0 {
        Check::ok(
            "pages-cropped",
            format!("{} of {} pages cropped", cropped, source_pages),
        )
    } else {
        Check::warn("pages-cropped", "no page was cropped")
    });

    if matches!(mode, Mode::Auto { .. }) && !without_content.is_empty() {
        let mut check = Check::warn(
            "content-detected",
            format!(
                "{} page(s) hold no measurable text and kept their box",
                without_content.len()
            ),
        );
        if without_content.len() == 1 {
            check = check.on_page(without_content[0]);
        }
        checks.push(check);
    }

    // Cropping here rewrites the CropBox and nothing else. Every viewer, and every printer
    // driver that goes through one, honours it — but an imposition or platemaking chain that
    // reads the MediaBox still measures the whole original sheet and will report the crop as
    // having had no effect. That is a real limit of what was done, so it is said, every time,
    // rather than left for the caller to discover on a press.
    if cropped > 0 {
        checks.push(Check::warn(
            "mediabox",
            "the CropBox was rewritten, the MediaBox was left as the source wrote it: a tool \
             that reads the MediaBox — imposition, platemaking — still sees the uncropped page",
        ));
    }

    Verdict::from_checks(
        format!("{} of {} pages cropped", cropped, source_pages),
        checks,
    )
}

/// Keep a bad value quotable in an error message
fn clip(value: &str) -> String {
    value.chars().take(40).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(body: &str) -> CropRequest {
        serde_json::from_str(body).expect("valid request body")
    }

    fn page(number: usize, width: f32, height: f32) -> PageBox {
        PageBox {
            page: number,
            width,
            height,
        }
    }

    fn word(page: usize, x0: f32, y0: f32, x1: f32, y1: f32) -> Word {
        Word {
            page,
            text: "x".to_string(),
            x0,
            y0,
            x1,
            y1,
        }
    }

    #[test]
    fn a_body_with_only_a_pdf_crops_automatically_with_a_twelve_point_margin() {
        let (mode, selection) = plan(&request(r#"{"pdf":"asset://as_1"}"#)).unwrap();
        assert_eq!(mode, Mode::Auto { margin: 12.0 });
        assert_eq!(selection, Selection::All);

        let (mode, _) = plan(&request(r#"{"pdf":"p","box":"AUTO","margin":0}"#)).unwrap();
        assert_eq!(mode, Mode::Auto { margin: 0.0 });
    }

    #[test]
    fn an_explicit_box_is_read_as_left_bottom_right_top() {
        let (mode, _) = plan(&request(r#"{"pdf":"p","box":[10,20,300,400]}"#)).unwrap();
        assert_eq!(
            mode,
            Mode::Explicit(Rect {
                left: 10.0,
                bottom: 20.0,
                right: 300.0,
                top: 400.0
            })
        );
    }

    #[test]
    fn an_inverted_or_flat_box_is_refused_instead_of_producing_an_empty_pdf() {
        for body in [
            r#"{"pdf":"p","box":[300,20,10,400]}"#,
            r#"{"pdf":"p","box":[10,400,300,20]}"#,
            r#"{"pdf":"p","box":[10,20,10,400]}"#,
            r#"{"pdf":"p","box":[0,0,0,0]}"#,
        ] {
            let error = plan(&request(body)).unwrap_err();
            assert!(matches!(error, AppError::BadRequest(_)), "{}", body);
        }
    }

    #[test]
    fn a_box_that_is_not_four_numbers_or_auto_is_refused() {
        assert!(plan(&request(r#"{"pdf":"p","box":"tight"}"#)).is_err());
        assert!(serde_json::from_str::<CropRequest>(r#"{"pdf":"p","box":[1,2,3]}"#).is_err());
        // Serde parses NaN out of a JSON body only as a bare token, so guard the value too
        assert!(explicit_rect(&[0.0, 0.0, f64::NAN, 10.0]).is_err());
        assert!(explicit_rect(&[-1.0, 0.0, 100.0, 100.0]).is_err());
        assert!(explicit_rect(&[0.0, 0.0, 99_999.0, 100.0]).is_err());
    }

    #[test]
    fn a_margin_outside_its_range_is_refused() {
        assert!(plan(&request(r#"{"pdf":"p","margin":-1}"#)).is_err());
        assert!(plan(&request(r#"{"pdf":"p","margin":9000}"#)).is_err());
    }

    #[test]
    fn page_lists_accept_numbers_ranges_and_all() {
        assert_eq!(Selection::parse(None).unwrap(), Selection::All);
        assert_eq!(Selection::parse(Some(" All ")).unwrap(), Selection::All);
        assert_eq!(
            Selection::parse(Some("1,4,7-9")).unwrap(),
            Selection::Ranges(vec![(1, 1), (4, 4), (7, 9)])
        );

        let selection = Selection::parse(Some("2-3,7")).unwrap();
        assert!(!selection.contains(1));
        assert!(selection.contains(2) && selection.contains(3) && selection.contains(7));
        assert!(!selection.contains(4));
        assert!(Selection::All.contains(999));
    }

    #[test]
    fn a_page_list_that_is_not_a_page_list_is_refused() {
        assert!(Selection::parse(Some("first")).is_err());
        assert!(Selection::parse(Some("0")).is_err());
        assert!(Selection::parse(Some("5-2")).is_err());
        assert!(Selection::parse(Some("1,,3")).is_ok());
    }

    #[test]
    fn a_page_list_past_the_end_of_the_document_is_refused() {
        let selection = Selection::parse(Some("2-5")).unwrap();
        assert!(selection.validate(5).is_ok());
        assert!(selection.validate(4).is_err());
        assert!(Selection::All.validate(1).is_ok());
    }

    #[test]
    fn the_auto_box_flips_the_origin_and_keeps_the_margin() {
        let page = page(1, 600.0, 800.0);
        let words = vec![
            word(1, 100.0, 50.0, 500.0, 90.0),
            word(2, 0.0, 0.0, 1.0, 1.0),
        ];

        let rect = content_box(&words, &page, 10.0).unwrap();
        assert_eq!(
            rect,
            Rect {
                left: 90.0,
                bottom: 700.0,
                right: 510.0,
                top: 760.0
            }
        );
    }

    #[test]
    fn the_auto_box_never_reaches_past_the_page() {
        let page = page(1, 600.0, 800.0);
        let words = vec![word(1, 2.0, 2.0, 598.0, 798.0)];

        let rect = content_box(&words, &page, 40.0).unwrap();
        assert_eq!(
            rect,
            Rect {
                left: 0.0,
                bottom: 0.0,
                right: 600.0,
                top: 800.0
            }
        );
    }

    #[test]
    fn a_page_without_measurable_text_keeps_its_own_box() {
        let third = page(3, 600.0, 800.0);
        assert!(content_box(&[], &third, 12.0).is_none());
        // Words belonging to another page do not crop this one
        assert!(content_box(&[word(1, 10.0, 10.0, 20.0, 20.0)], &third, 12.0).is_none());
        // A word made of white space is not content either
        let mut blank = word(3, 10.0, 10.0, 20.0, 20.0);
        blank.text = "  ".to_string();
        assert!(content_box(&[blank], &third, 12.0).is_none());
        // Nor is a page whose geometry Poppler could not report
        let unknown = page(4, 0.0, 0.0);
        assert!(content_box(&[word(4, 10.0, 10.0, 20.0, 20.0)], &unknown, 12.0).is_none());
    }

    #[test]
    fn an_explicit_box_is_clamped_to_the_page_and_says_so() {
        let page = page(1, 595.0, 842.0);
        let rect = Rect {
            left: 20.0,
            bottom: 20.0,
            right: 900.0,
            top: 800.0,
        };

        let (fitted, clamped) = fit_to_page(rect, &page).unwrap();
        assert!(clamped);
        assert_eq!(fitted.right, 595.0);

        let (fitted, clamped) = fit_to_page(fitted, &page).unwrap();
        assert!(!clamped);
        assert_eq!(fitted.left, 20.0);
    }

    #[test]
    fn an_explicit_box_that_misses_the_page_is_refused() {
        let page = page(2, 595.0, 842.0);
        let outside = Rect {
            left: 700.0,
            bottom: 20.0,
            right: 900.0,
            top: 800.0,
        };

        let error = fit_to_page(outside, &page).unwrap_err();
        match error {
            AppError::BadRequest(message) => assert!(message.contains("page 2"), "{}", message),
            other => panic!("expected a 400, got {:?}", other.kind()),
        }
    }

    #[test]
    fn the_program_holds_one_entry_per_page_and_leaves_untargeted_pages_null() {
        let boxes = vec![
            Some(Rect {
                left: 10.0,
                bottom: 20.0,
                right: 300.0,
                top: 400.0,
            }),
            None,
        ];

        let program = crop_program(&boxes);
        assert!(program.starts_with("%!PS\n"));
        assert!(program.contains("[ [10.0000 20.0000 300.0000 400.0000] null]"));
        assert!(program.contains("/CropBox exch /PAGE pdfmark"));
        // The dispatch reads Ghostscript's own page counter, never one we keep
        assert!(program.contains("/BeginPage"));
        assert!(!program.contains("1 add"));
    }

    #[test]
    fn the_verdict_reports_what_was_left_alone() {
        let good = verdict(10, 10, 7, &[3], Mode::Auto { margin: 12.0 });
        assert_eq!(good.status, "warn");
        assert_eq!(good.summary, "7 of 10 pages cropped");
        assert_eq!(good.checks[2].page, Some(3));

        let untouched = verdict(
            4,
            4,
            0,
            &[],
            Mode::Explicit(Rect {
                left: 0.0,
                bottom: 0.0,
                right: 1.0,
                top: 1.0,
            }),
        );
        assert_eq!(untouched.status, "warn");
        assert_eq!(untouched.checks.len(), 2);

        let lost = verdict(10, 9, 10, &[], Mode::Auto { margin: 12.0 });
        assert_eq!(lost.status, "fail");
    }

    #[test]
    fn a_crop_never_hides_that_the_mediabox_kept_the_original_page() {
        let cropped = verdict(3, 3, 3, &[], Mode::Auto { margin: 12.0 });
        let mediabox = cropped
            .checks
            .iter()
            .find(|check| check.name == "mediabox")
            .expect("a cropped document reports what was left untouched");
        assert_eq!(mediabox.status, "warn");
        assert!(mediabox.detail.contains("MediaBox"), "{}", mediabox.detail);

        // Nothing was rewritten, so there is nothing to warn about
        let untouched = verdict(3, 3, 0, &[], Mode::Auto { margin: 12.0 });
        assert!(!untouched
            .checks
            .iter()
            .any(|check| check.name == "mediabox"));
    }

    /// `assets::store` renames the file it is handed, so a measurement taken after
    /// `finish_tool` reads a path that no longer exists the day `/tmp` and the asset root
    /// share a filesystem. Only the order of the two statements prevents it, and `run` cannot
    /// be exercised in a unit test — it needs pdftotext and Ghostscript — so the order itself
    /// is what is asserted.
    #[test]
    fn the_page_count_is_taken_before_the_file_is_published() {
        let source = include_str!("crop.rs");
        let measured = source
            .find("page_count(&produced)")
            .expect("run measures the file it produced");
        let published = source
            .find("helpers::finish_tool")
            .expect("run publishes the file it produced");

        assert!(
            measured < published,
            "page_count must run before finish_tool moves the file away"
        );
    }
}

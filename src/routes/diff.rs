//! Visual diff between two PDFs, for a CI that needs to know whether a theme or a template
//! change moved anything on the page.
//!
//! The comparison is done on pixels rather than on the text layer: a font substitution, a
//! margin that shifted or an image that stopped loading leave the text identical and the
//! page unrecognisable. A per-channel tolerance is what makes the result usable — two runs
//! of the same document can differ by a hair of anti-aliasing, and without it every diff
//! would come back "changed".

use crate::auth::ApiKey;
use crate::exec;
use crate::helpers::{self, base64};
use crate::obs::{self, RequestId};
use crate::pdfops;
use crate::types::AppError;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use tempfile::{Builder, TempDir};

/// 100 dpi resolves a 9pt glyph into enough pixels for a changed word to stand out, and
/// costs a quarter of what the 200 dpi of redaction costs.
const DEFAULT_DPI: u32 = 100;
const MIN_DPI: u32 = 36;
const MAX_DPI: u32 = 300;

/// Zero by default: out of the box, a page that visibly moved makes the verdict "changed".
/// A regression test that only cares about large differences raises it; one that defaults
/// to tolerance would report "identical" on a renamed heading, which is the whole point of
/// running the diff.
const DEFAULT_THRESHOLD: f32 = 0.0;

/// A pixel counts as different when one of its channels moves by more than this. Anti-
/// aliasing and colour quantisation move edge pixels by a few units between two renders of
/// the same document; a real change moves them by a hundred.
const CHANNEL_TOLERANCE: u8 = 24;

/// A page is reported as changed above this share of differing pixels. Below it, what we
/// are looking at is rasterizer noise rather than a document change. At 100 dpi this is
/// about fifty pixels on an A4 page — under one character, over any plausible noise.
const PAGE_EPSILON: f32 = 0.00005;

/// Side of the grid the differing pixels are aggregated into, in pixels. It is what turns
/// scattered pixels into the handful of rectangles a highlight image can show.
const CELL_PX: u32 = 8;
/// Beyond this many boxes on one page the picture stops being readable; the largest ones
/// are kept because they are the ones a reviewer is looking for.
const MAX_RECTS: usize = 400;

const MAX_DIFF_PAGES: usize = 200;
/// Pixel budget for one batch of pages, which bounds the disk a long document needs
const BATCH_PIXELS: f64 = 8_000_000.0;
const MAX_BATCH_PAGES: usize = 8;

const MAX_IMAGES: usize = 12;
/// Same reasoning as the preview: the reverse proxies in `deploy/` accept 12 MB bodies
const MAX_IMAGE_BYTES: usize = 6 * 1024 * 1024;

#[derive(Deserialize)]
pub struct DiffRequest {
    pub before: String,
    pub after: String,
    pub dpi: Option<u32>,
    /// Share of changed pixels the verdict tolerates; 0, the default, tolerates nothing
    pub threshold: Option<f32>,
    pub images: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct DiffResponse {
    /// Pages of the longer of the two documents
    pub pages_total: usize,
    pub pages_before: usize,
    pub pages_after: usize,
    pub pages_changed: Vec<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pages_added: Vec<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pages_removed: Vec<usize>,
    /// Mean share of changed pixels over the whole document, the value `threshold` is
    /// compared against. Rounded for reading; the verdict is decided on the exact value.
    pub changed_ratio: f32,
    pub verdict: &'static str,
    pub threshold: f32,
    pub dpi: u32,
    pub per_page: Vec<PageDiff>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<DiffImage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct PageDiff {
    pub page: usize,
    pub ratio: f32,
    /// `added`, `removed` or `geometry` when the page is not simply a changed page
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct DiffImage {
    pub page: usize,
    pub png: String,
    pub width: u32,
    pub height: u32,
}

#[post("/diff", format = "json", data = "<req>")]
pub async fn diff(
    _key: ApiKey,
    trace: RequestId,
    req: Json<DiffRequest>,
) -> Result<Json<DiffResponse>, AppError> {
    let req = req.into_inner();

    let before = helpers::resolve_pdf_path(&req.before)?;
    let after = helpers::resolve_pdf_path(&req.after)?;
    let dpi = resolve_dpi(req.dpi)?;
    let threshold = resolve_threshold(req.threshold)?;
    let want_images = req.images.unwrap_or(false);

    let started = Instant::now();
    let job =
        exec::offload(move || compare_documents(&before, &after, dpi, threshold, want_images))
            .await;

    let report = match job {
        Ok(report) => report,
        Err(err) => {
            obs::event(
                obs::Level::Error,
                "diff failed",
                vec![
                    ("trace_id", json!(trace.0)),
                    ("route", json!("/api/diff")),
                    obs::err_field(err.kind(), &format!("{:?}", err)),
                ],
            );
            return Err(err);
        }
    };

    obs::event(
        obs::Level::Info,
        "diff",
        vec![
            ("trace_id", json!(trace.0)),
            ("route", json!("/api/diff")),
            ("pages", json!(report.pages_total)),
            ("pages_changed", json!(report.pages_changed.len())),
            ("changed_ratio", json!(report.changed_ratio)),
            ("verdict", json!(report.verdict)),
            ("dpi", json!(dpi)),
            ("duration_ms", json!(started.elapsed().as_millis())),
        ],
    );

    Ok(Json(report))
}

fn resolve_dpi(dpi: Option<u32>) -> Result<u32, AppError> {
    match dpi {
        None => Ok(DEFAULT_DPI),
        Some(dpi) if (MIN_DPI..=MAX_DPI).contains(&dpi) => Ok(dpi),
        Some(dpi) => Err(AppError::BadRequest(format!(
            "\"dpi\" must be between {} and {}, not {}",
            MIN_DPI, MAX_DPI, dpi
        ))),
    }
}

fn resolve_threshold(threshold: Option<f32>) -> Result<f32, AppError> {
    match threshold {
        None => Ok(DEFAULT_THRESHOLD),
        Some(threshold) if threshold.is_finite() && (0.0..=1.0).contains(&threshold) => {
            Ok(threshold)
        }
        Some(threshold) => Err(AppError::BadRequest(format!(
            "\"threshold\" must be a share between 0 and 1, not {}",
            threshold
        ))),
    }
}

// ------------ The comparison ------------

fn compare_documents(
    before: &Path,
    after: &Path,
    dpi: u32,
    threshold: f32,
    want_images: bool,
) -> Result<DiffResponse, AppError> {
    let pages_before = pdfops::page_count(before)?;
    let pages_after = pdfops::page_count(after)?;
    let pages_total = pages_before.max(pages_after);
    let common = pages_before.min(pages_after);
    let compared = common.min(MAX_DIFF_PAGES);

    let mut per_page: Vec<PageDiff> = Vec::new();
    let mut pages_changed: Vec<usize> = Vec::new();
    let mut images: Vec<DiffImage> = Vec::new();
    let mut image_bytes = 0usize;
    let mut total_ratio = 0.0f64;

    let batch = batch_pages(dpi);
    let mut first = 1usize;
    while first <= compared {
        let last = (first + batch - 1).min(compared);
        // Both documents are rasterized at the same resolution: comparing pixels only
        // means anything when the two grids describe the same area of paper.
        let before_pages = rasterize_ppm(before, first, last, dpi)?;
        let after_pages = rasterize_ppm(after, first, last, dpi)?;

        for page in first..=last {
            let (left, right) = match (before_pages.get(page), after_pages.get(page)) {
                (Some(left), Some(right)) => (read_ppm(left)?, read_ppm(right)?),
                _ => {
                    return Err(AppError::ProcessFailed {
                        message: format!("pdftoppm did not produce page {}", page),
                        stderr: String::new(),
                    })
                }
            };

            let resized = left.width != right.width || left.height != right.height;
            let comparison = compare(&left, &right);

            if comparison.ratio > PAGE_EPSILON {
                // Noise below the floor contributes nothing, so `changed_ratio > 0` and
                // `pages_changed` non-empty always say the same thing to a CI
                total_ratio += comparison.ratio as f64;
                pages_changed.push(page);
                if want_images && images.len() < MAX_IMAGES && image_bytes < MAX_IMAGE_BYTES {
                    let image = highlight(after, page, dpi, &comparison)?;
                    image_bytes += image.png.len();
                    images.push(image);
                }
            }

            per_page.push(PageDiff {
                page,
                ratio: round_ratio(comparison.ratio),
                status: if resized { Some("geometry") } else { None },
            });
        }

        first = last + 1;
    }

    // A page count that moved is a result, not a failure: the extra pages are reported as
    // fully changed so the ratio reflects what a reviewer would see.
    let pages_added: Vec<usize> = ((pages_before + 1)..=pages_after)
        .take(MAX_DIFF_PAGES)
        .collect();
    let pages_removed: Vec<usize> = ((pages_after + 1)..=pages_before)
        .take(MAX_DIFF_PAGES)
        .collect();

    for (page, status) in pages_added
        .iter()
        .map(|page| (*page, "added"))
        .chain(pages_removed.iter().map(|page| (*page, "removed")))
    {
        total_ratio += 1.0;
        pages_changed.push(page);
        per_page.push(PageDiff {
            page,
            ratio: 1.0,
            status: Some(status),
        });
    }

    per_page.sort_by_key(|entry| entry.page);
    pages_changed.sort_unstable();

    let changed_ratio = (total_ratio / pages_total.max(1) as f64) as f32;
    let truncated = common > compared
        || pages_added.len() < pages_after.saturating_sub(pages_before)
        || pages_removed.len() < pages_before.saturating_sub(pages_after);

    Ok(DiffResponse {
        pages_total,
        pages_before,
        pages_after,
        pages_changed,
        pages_added,
        pages_removed,
        changed_ratio: round_ratio(changed_ratio),
        verdict: if changed_ratio > threshold {
            "changed"
        } else {
            "identical"
        },
        threshold,
        dpi,
        per_page,
        images,
        truncated: if truncated { Some(true) } else { None },
    })
}

/// Per-page result: how much moved, and a coarse grid of where
struct Comparison {
    ratio: f32,
    cols: u32,
    rows: u32,
    cells: Vec<bool>,
}

fn compare(before: &Ppm, after: &Ppm) -> Comparison {
    let cols = after.width.div_ceil(CELL_PX);
    let rows = after.height.div_ceil(CELL_PX);

    // Different geometry: there is no pixel-to-pixel mapping left to compute
    if before.width != after.width || before.height != after.height {
        return Comparison {
            ratio: 1.0,
            cols,
            rows,
            cells: vec![true; (cols * rows) as usize],
        };
    }

    let mut cells = vec![false; (cols * rows) as usize];
    let mut changed = 0u64;
    let width = after.width as usize;

    for y in 0..after.height as usize {
        let row = y * width * 3;
        let cell_row = (y as u32 / CELL_PX) * cols;
        for x in 0..width {
            let index = row + x * 3;
            let delta = before.pixels[index]
                .abs_diff(after.pixels[index])
                .max(before.pixels[index + 1].abs_diff(after.pixels[index + 1]))
                .max(before.pixels[index + 2].abs_diff(after.pixels[index + 2]));

            if delta > CHANNEL_TOLERANCE {
                changed += 1;
                cells[(cell_row + x as u32 / CELL_PX) as usize] = true;
            }
        }
    }

    let total = after.width as u64 * after.height as u64;
    Comparison {
        ratio: changed as f32 / total.max(1) as f32,
        cols,
        rows,
        cells,
    }
}

/// Rectangle in grid cells, upper bounds exclusive
#[derive(Debug, Clone, Copy, PartialEq)]
struct CellRect {
    c0: u32,
    c1: u32,
    r0: u32,
    r1: u32,
}

impl CellRect {
    fn area(&self) -> u32 {
        (self.c1 - self.c0) * (self.r1 - self.r0)
    }
}

/// Merge the marked cells into rectangles: runs on one row, extended downwards while the
/// row below repeats them. A paragraph that moved becomes one box instead of eight hundred.
fn rects(cells: &[bool], cols: u32, rows: u32) -> Vec<CellRect> {
    let mut closed: Vec<CellRect> = Vec::new();
    let mut open: Vec<CellRect> = Vec::new();

    for row in 0..rows {
        let mut runs: Vec<(u32, u32)> = Vec::new();
        let mut col = 0;
        while col < cols {
            if cells
                .get((row * cols + col) as usize)
                .copied()
                .unwrap_or(false)
            {
                let start = col;
                while col < cols
                    && cells
                        .get((row * cols + col) as usize)
                        .copied()
                        .unwrap_or(false)
                {
                    col += 1;
                }
                runs.push((start, col));
            } else {
                col += 1;
            }
        }

        let mut next: Vec<CellRect> = Vec::new();
        for (c0, c1) in runs {
            match open.iter().position(|rect| rect.c0 == c0 && rect.c1 == c1) {
                Some(index) => {
                    let mut rect = open.remove(index);
                    rect.r1 = row + 1;
                    next.push(rect);
                }
                None => next.push(CellRect {
                    c0,
                    c1,
                    r0: row,
                    r1: row + 1,
                }),
            }
        }

        closed.append(&mut open);
        open = next;
    }
    closed.append(&mut open);

    if closed.len() > MAX_RECTS {
        closed.sort_by_key(|rect| std::cmp::Reverse(rect.area()));
        closed.truncate(MAX_RECTS);
    }

    closed
}

/// The "after" page with the differing areas tinted red.
///
/// Painting on the PNG itself would need an image codec this service does not carry, so
/// the page is laid back out at its own size and rasterized again — the same round trip
/// redaction uses to burn its boxes into the pixels.
fn highlight(
    after: &Path,
    page: usize,
    dpi: u32,
    comparison: &Comparison,
) -> Result<DiffImage, AppError> {
    let raster = pdfops::rasterize(after, page, page, dpi)?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::ProcessFailed {
            message: format!("Page {} could not be rasterized for the diff image", page),
            stderr: String::new(),
        })?;

    let width = raster.page_box.width.max(1.0);
    let height = raster.page_box.height.max(1.0);
    let scale_x = width / raster.width_px.max(1) as f32;
    let scale_y = height / raster.height_px.max(1) as f32;

    let mut body = format!(
        "<div><img src=\"data:image/png;base64,{}\">",
        base64(&raster.png)
    );

    for rect in rects(&comparison.cells, comparison.cols, comparison.rows) {
        let x0 = ((rect.c0 * CELL_PX) as f32 * scale_x).clamp(0.0, width);
        let y0 = ((rect.r0 * CELL_PX) as f32 * scale_y).clamp(0.0, height);
        let x1 = ((rect.c1 * CELL_PX) as f32 * scale_x).clamp(0.0, width);
        let y1 = ((rect.r1 * CELL_PX) as f32 * scale_y).clamp(0.0, height);
        let _ = write!(
            body,
            "<b style=\"left:{:.3}pt;top:{:.3}pt;width:{:.3}pt;height:{:.3}pt\"></b>",
            x0,
            y0,
            (x1 - x0).max(0.5),
            (y1 - y0).max(0.5)
        );
    }
    body.push_str("</div>");

    let html = format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><style>\n\
         @page {{ size: {width:.3}pt {height:.3}pt; margin: 0; }}\n\
         html, body {{ margin: 0; padding: 0; }}\n\
         div {{ margin: 0; padding: 0; position: relative; \
         width: {width:.3}pt; height: {height:.3}pt; }}\n\
         img {{ display: block; width: 100%; height: 100%; }}\n\
         b {{ display: block; position: absolute; box-sizing: border-box; \
         background: rgba(220, 38, 38, 0.30); border: 0.5pt solid rgba(170, 16, 16, 0.9); }}\n\
         </style></head><body>{body}</body></html>",
        width = width,
        height = height,
        body = body
    );

    let pdf = helpers::run_weasyprint_plain(&html)?;
    let painted = pdfops::rasterize(&pdf, 1, 1, dpi)?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::ProcessFailed {
            message: format!("The diff image of page {} produced no PNG", page),
            stderr: String::new(),
        })?;

    Ok(DiffImage {
        page,
        png: base64(&painted.png),
        width: painted.width_px,
        height: painted.height_px,
    })
}

// ------------ Raw rasters ------------

/// A batch of pages rendered to PPM, kept on disk until this value is dropped
struct Rasters {
    _dir: TempDir,
    pages: BTreeMap<usize, PathBuf>,
}

impl Rasters {
    fn get(&self, page: usize) -> Option<&PathBuf> {
        self.pages.get(&page)
    }
}

/// `pdftoppm` without a format flag writes PPM, three bytes per pixel with a text header.
/// Reading pixels out of a PNG would mean an image decoder; this needs twenty lines.
fn rasterize_ppm(pdf: &Path, first: usize, last: usize, dpi: u32) -> Result<Rasters, AppError> {
    let dir = Builder::new().prefix("diff-").tempdir()?;
    let prefix = dir.path().join("page");

    helpers::run_capture(
        Command::new("pdftoppm")
            .arg("-f")
            .arg(first.to_string())
            .arg("-l")
            .arg(last.to_string())
            .arg("-r")
            .arg(dpi.to_string())
            .arg(helpers::path_to_str(pdf)?)
            .arg(helpers::path_to_str(&prefix)?),
        "pdftoppm",
        "pdftoppm failed",
    )?;

    let mut pages = BTreeMap::new();
    for entry in fs::read_dir(dir.path())?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("ppm") {
            continue;
        }
        if let Some(number) = trailing_number(&path) {
            pages.insert(number, path);
        }
    }

    if pages.is_empty() {
        return Err(AppError::ProcessFailed {
            message: format!("pdftoppm produced no image for pages {}..{}", first, last),
            stderr: String::new(),
        });
    }

    Ok(Rasters { _dir: dir, pages })
}

struct Ppm {
    width: u32,
    height: u32,
    /// Three bytes per pixel, row major
    pixels: Vec<u8>,
}

fn read_ppm(path: &Path) -> Result<Ppm, AppError> {
    let bytes = fs::read(path)?;
    parse_ppm(&bytes).ok_or_else(|| AppError::ProcessFailed {
        message: "pdftoppm produced a file that is not a P6 image".to_string(),
        stderr: String::new(),
    })
}

fn parse_ppm(bytes: &[u8]) -> Option<Ppm> {
    let (magic, cursor) = token(bytes, 0)?;
    if magic != b"P6" {
        return None;
    }

    let (width, cursor) = number(bytes, cursor)?;
    let (height, cursor) = number(bytes, cursor)?;
    let (max_value, cursor) = number(bytes, cursor)?;
    if max_value != 255 || width == 0 || height == 0 {
        return None;
    }

    // Exactly one whitespace byte separates the header from the raster
    let start = cursor + 1;
    let needed = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(3)?;
    let pixels = bytes.get(start..start.checked_add(needed)?)?.to_vec();

    Some(Ppm {
        width,
        height,
        pixels,
    })
}

/// Next whitespace-delimited token, skipping the `#` comments the format allows
fn token(bytes: &[u8], mut cursor: usize) -> Option<(&[u8], usize)> {
    loop {
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor < bytes.len() && bytes[cursor] == b'#' {
            while cursor < bytes.len() && bytes[cursor] != b'\n' {
                cursor += 1;
            }
        } else {
            break;
        }
    }

    let start = cursor;
    while cursor < bytes.len() && !bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }

    if cursor == start {
        return None;
    }

    Some((&bytes[start..cursor], cursor))
}

fn number(bytes: &[u8], cursor: usize) -> Option<(u32, usize)> {
    let (token, cursor) = token(bytes, cursor)?;
    std::str::from_utf8(token)
        .ok()?
        .parse()
        .ok()
        .map(|value| (value, cursor))
}

/// Trailing index of a `<prefix>-<n>.ppm` file name, zero-padded on some poppler versions
fn trailing_number(path: &Path) -> Option<usize> {
    path.file_stem()?.to_str()?.rsplit('-').next()?.parse().ok()
}

fn batch_pages(dpi: u32) -> usize {
    const A4_WIDTH_PT: f64 = 595.276;
    const A4_HEIGHT_PT: f64 = 841.89;

    let scale = dpi as f64 / 72.0;
    let per_page = (A4_WIDTH_PT * scale) * (A4_HEIGHT_PT * scale);
    ((BATCH_PIXELS / per_page.max(1.0)) as usize).clamp(1, MAX_BATCH_PAGES)
}

/// Six decimals: a single changed character covers a millionth of a page, and a ratio
/// that rounded to zero next to a "changed" verdict would read as a contradiction.
fn round_ratio(value: f32) -> f32 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ppm(width: u32, height: u32, fill: u8) -> Ppm {
        Ppm {
            width,
            height,
            pixels: vec![fill; (width * height * 3) as usize],
        }
    }

    #[test]
    fn parses_a_p6_header_with_comments() {
        let mut bytes = b"P6\n# generated by pdftoppm\n2 1\n255\n".to_vec();
        bytes.extend_from_slice(&[1, 2, 3, 4, 5, 6]);

        let image = parse_ppm(&bytes).expect("header should parse");
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(image.pixels, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn a_truncated_or_foreign_file_is_rejected_instead_of_panicking() {
        assert!(parse_ppm(b"").is_none());
        assert!(parse_ppm(b"P5\n2 1\n255\nxx").is_none());
        // Announced 2x1 but only one pixel of data
        assert!(parse_ppm(b"P6\n2 1\n255\n\x01\x02\x03").is_none());
    }

    #[test]
    fn the_same_page_twice_is_identical() {
        let comparison = compare(&ppm(16, 16, 200), &ppm(16, 16, 200));
        assert_eq!(comparison.ratio, 0.0);
        assert!(comparison.cells.iter().all(|cell| !cell));
    }

    #[test]
    fn anti_aliasing_noise_stays_below_the_tolerance() {
        let before = ppm(16, 16, 200);
        let mut after = ppm(16, 16, 200);
        for byte in after.pixels.iter_mut() {
            *byte = 200 + CHANNEL_TOLERANCE;
        }

        let comparison = compare(&before, &after);
        assert_eq!(
            comparison.ratio, 0.0,
            "a whole page of noise must not count"
        );
    }

    #[test]
    fn a_real_change_is_located_on_the_grid() {
        let before = ppm(16, 16, 255);
        let mut after = ppm(16, 16, 255);
        // Blacken the top-left pixel: one cell of the 2x2 grid, one pixel out of 256
        after.pixels[0] = 0;
        after.pixels[1] = 0;
        after.pixels[2] = 0;

        let comparison = compare(&before, &after);
        assert!((comparison.ratio - 1.0 / 256.0).abs() < 1e-6);
        assert_eq!((comparison.cols, comparison.rows), (2, 2));
        assert_eq!(comparison.cells, vec![true, false, false, false]);
    }

    #[test]
    fn a_page_of_another_size_is_entirely_changed() {
        let comparison = compare(&ppm(16, 16, 255), &ppm(24, 16, 255));
        assert_eq!(comparison.ratio, 1.0);
        assert!(comparison.cells.iter().all(|cell| *cell));
    }

    #[test]
    fn contiguous_cells_merge_into_one_rectangle() {
        // A 4x3 grid whose middle two columns are marked on every row
        let cells: Vec<bool> = (0..12)
            .map(|index| index % 4 == 1 || index % 4 == 2)
            .collect();
        let boxes = rects(&cells, 4, 3);

        assert_eq!(
            boxes,
            vec![CellRect {
                c0: 1,
                c1: 3,
                r0: 0,
                r1: 3
            }]
        );
    }

    #[test]
    fn runs_that_do_not_line_up_stay_separate() {
        let cells = vec![true, false, false, false, false, true];
        let boxes = rects(&cells, 3, 2);
        assert_eq!(boxes.len(), 2);
        assert!(boxes.contains(&CellRect {
            c0: 0,
            c1: 1,
            r0: 0,
            r1: 1
        }));
    }

    #[test]
    fn bounds_are_enforced_before_anything_is_rasterized() {
        assert_eq!(resolve_dpi(None).unwrap(), DEFAULT_DPI);
        assert!(resolve_dpi(Some(1200)).is_err());
        assert_eq!(resolve_threshold(None).unwrap(), DEFAULT_THRESHOLD);
        assert!(resolve_threshold(Some(-0.1)).is_err());
        assert!(resolve_threshold(Some(2.0)).is_err());
        assert!(resolve_threshold(Some(f32::NAN)).is_err());
        // A batch always holds at least one page, however high the resolution
        assert!(batch_pages(300) >= 1);
        assert!(batch_pages(36) <= MAX_BATCH_PAGES);
    }
}

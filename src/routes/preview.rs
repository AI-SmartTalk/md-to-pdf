//! Preview rendering: one page as a PNG, a page range as images, or a contact sheet.
//!
//! A request that names none of the new fields gets exactly what it always got: page 1,
//! 150 dpi, `image/png`. Everything below only happens when the caller asks for it.

use crate::auth::ApiKey;
use crate::config::config;
use crate::exec;
use crate::helpers::{self, base64, PREVIEW_DPI};
use crate::obs::{self, RequestId};
use crate::pdfops::{self, RasterPage};
use crate::pipeline::{self, RenderSpec, Source, UrlPolicy};
use crate::types::*;
use rocket::http::ContentType;
use rocket::serde::json::Json;
use rocket::Either;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fmt::Write as _;
use std::time::Instant;

const MIN_DPI: u32 = 36;
const MAX_DPI: u32 = 300;

/// Pixel ceiling for one preview. Twenty A4 pages at 150 dpi sit at 43 Mpx, so this only
/// bites when a high dpi is combined with many pages — which is the resource attack the
/// per-field bounds cannot catch on their own.
const MAX_PREVIEW_PIXELS: f64 = 48_000_000.0;

/// Ceiling on the PNG bytes a JSON preview may carry. Base64 inflates by 4/3, and the
/// reverse proxies in `deploy/` are configured for 12 MB bodies, so 6 MB of PNG lands at
/// about 8 MB on the wire and keeps room for the rest of the document.
const MAX_JSON_PNG_BYTES: usize = 6 * 1024 * 1024;

/// Contact sheet geometry, in points
const SHEET_THUMB_WIDTH: f32 = 200.0;
const SHEET_GAP: f32 = 16.0;
const SHEET_MARGIN: f32 = 24.0;
const SHEET_LABEL: f32 = 16.0;
const SHEET_MAX_COLUMNS: usize = 4;

/// Everything `POST /api/preview` accepts. The historical body is flattened in so the two
/// shapes stay one type: a client that never heard of `pages` is deserialized identically.
#[derive(Deserialize)]
pub struct PreviewParams {
    #[serde(flatten)]
    pub base: PreviewRequest,
    /// `"1"`, `"2-5"` or `"all"`; absent means page 1 alone
    pub pages: Option<String>,
    pub dpi: Option<u32>,
    /// `"png"`, `"images"` or `"sheet"`
    pub layout: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PreviewPages {
    pub pages: Vec<PreviewPage>,
    /// Pages in the document, which may be more than what was returned
    pub pages_total: usize,
    /// Set only when the page cap cut the answer short
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct PreviewPage {
    /// 1-based page number in the source document
    pub page: usize,
    pub png: String,
    pub width: u32,
    pub height: u32,
}

#[post("/preview", format = "json", data = "<req>")]
pub async fn preview(
    _key: ApiKey,
    trace: RequestId,
    req: Json<PreviewParams>,
) -> Result<Either<(ContentType, Vec<u8>), Json<PreviewPages>>, AppError> {
    let PreviewParams {
        base,
        pages,
        dpi,
        layout,
    } = req.into_inner();

    let max_pages = config().preview_max_pages.max(1);
    let selection = match pages.as_deref() {
        Some(spec) => Selection::parse(spec, max_pages)?,
        None => Selection::Range { first: 1, last: 1 },
    };
    let dpi = resolve_dpi(dpi)?;
    let sheet = Sheet::parse(layout.as_deref(), selection)?;

    let PreviewRequest {
        markdown,
        html,
        template,
        data,
        css,
        engine,
        options,
        header_html,
        footer_html,
        header_template,
        footer_template,
    } = base;

    // Input modes are exclusive and evaluated in this order: markdown, template+data, html
    let source = if let Some(markdown) = markdown {
        Source::Markdown(markdown)
    } else if let Some(template) = template {
        let data = data.ok_or_else(|| {
            AppError::BadRequest("\"template\" requires a \"data\" object".to_string())
        })?;
        Source::Template { template, data }
    } else if let Some(html) = html {
        Source::Html(html)
    } else {
        return Err(AppError::BadRequest(
            "Provide one of: markdown, html, or template+data".to_string(),
        ));
    };

    let spec = RenderSpec {
        source,
        css,
        engine: engine.unwrap_or_default(),
        options: options.unwrap_or_default(),
        header_html,
        footer_html,
        header_template,
        footer_template,
        url_policy: UrlPolicy::default(),
    };

    let started = Instant::now();
    // Render and rasterize inside one slot: splitting them would make a preview queue twice
    let job = exec::offload(move || build(spec, selection, dpi, sheet, max_pages)).await;

    let preview = match job {
        Ok(preview) => preview,
        Err(err) => {
            obs::event(
                obs::Level::Error,
                "preview failed",
                vec![
                    ("trace_id", json!(trace.0)),
                    ("route", json!("/api/preview")),
                    obs::err_field(err.kind(), &format!("{:?}", err)),
                ],
            );
            return Err(err);
        }
    };

    obs::event(
        obs::Level::Info,
        "preview",
        vec![
            ("trace_id", json!(trace.0)),
            ("route", json!("/api/preview")),
            ("layout", json!(sheet.tag())),
            ("pages", json!(preview.pages)),
            ("dpi", json!(dpi)),
            ("duration_ms", json!(started.elapsed().as_millis())),
        ],
    );

    match preview.output {
        Output::Png(png) => Ok(Either::Left((ContentType::PNG, png))),
        Output::Images(pages) => Ok(Either::Right(Json(pages))),
    }
}

// ------------ Page selection ------------

/// What the caller asked for, before the document exists and its page count is known
#[derive(Debug, Clone, Copy, PartialEq)]
enum Selection {
    All,
    Range { first: usize, last: usize },
}

impl Selection {
    fn parse(spec: &str, max_pages: usize) -> Result<Selection, AppError> {
        let spec = spec.trim();

        if spec.eq_ignore_ascii_case("all") {
            return Ok(Selection::All);
        }

        let (head, tail) = match spec.split_once('-') {
            Some((head, tail)) => (head.trim(), tail.trim()),
            None => (spec, spec),
        };

        let bounds = (head.parse::<usize>().ok(), tail.parse::<usize>().ok());
        let (first, last) = match bounds {
            (Some(first), Some(last)) if first >= 1 && last >= 1 => (first, last),
            _ => {
                return Err(AppError::BadRequest(format!(
                "\"pages\" must be a page number, a range such as \"2-5\", or \"all\", not \"{}\"",
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

        if last - first + 1 > max_pages {
            return Err(AppError::BadRequest(format!(
                "\"pages\" covers {} pages; at most {} may be previewed at once",
                last - first + 1,
                max_pages
            )));
        }

        Ok(Selection::Range { first, last })
    }

    /// Turn the request into a real range now that the document is rendered.
    /// Returns the range and whether the page cap cut it short.
    fn resolve(self, count: usize, max_pages: usize) -> Result<(usize, usize, bool), AppError> {
        match self {
            // "all" means "as much as you can give me": the cap truncates instead of failing
            Selection::All => Ok((1, count.min(max_pages), count > max_pages)),
            Selection::Range { first, last } => {
                if first > count || last > count {
                    Err(AppError::BadRequest(format!(
                        "Pages {}-{} are outside the document, which has {} page{}",
                        first,
                        last,
                        count,
                        if count > 1 { "s" } else { "" }
                    )))
                } else {
                    Ok((first, last, false))
                }
            }
        }
    }
}

/// How the selected pages come back
#[derive(Debug, Clone, Copy, PartialEq)]
enum Sheet {
    /// A bare PNG of the first selected page, the historical answer
    Png,
    Images,
    Contact,
}

impl Sheet {
    fn parse(spec: Option<&str>, selection: Selection) -> Result<Sheet, AppError> {
        let spec = match spec {
            Some(spec) => spec.trim(),
            // No layout asked for: one page is a PNG, several can only be JSON
            None => {
                return Ok(match selection {
                    Selection::Range { first, last } if first == last => Sheet::Png,
                    _ => Sheet::Images,
                })
            }
        };

        if spec.eq_ignore_ascii_case("png") || spec.eq_ignore_ascii_case("single") {
            Ok(Sheet::Png)
        } else if spec.eq_ignore_ascii_case("images") {
            Ok(Sheet::Images)
        } else if spec.eq_ignore_ascii_case("sheet") {
            Ok(Sheet::Contact)
        } else {
            Err(AppError::BadRequest(format!(
                "\"layout\" must be \"png\", \"images\" or \"sheet\", not \"{}\"",
                clip(spec)
            )))
        }
    }

    fn tag(&self) -> &'static str {
        match self {
            Sheet::Png => "png",
            Sheet::Images => "images",
            Sheet::Contact => "sheet",
        }
    }
}

fn resolve_dpi(dpi: Option<u32>) -> Result<u32, AppError> {
    match dpi {
        None => Ok(PREVIEW_DPI),
        Some(dpi) if (MIN_DPI..=MAX_DPI).contains(&dpi) => Ok(dpi),
        Some(dpi) => Err(AppError::BadRequest(format!(
            "\"dpi\" must be between {} and {}, not {}",
            MIN_DPI, MAX_DPI, dpi
        ))),
    }
}

/// Echo a rejected value back without letting it grow the error body
fn clip(value: &str) -> String {
    value.chars().take(32).collect()
}

// ------------ Rendering ------------

struct Preview {
    output: Output,
    pages: usize,
}

enum Output {
    Png(Vec<u8>),
    Images(PreviewPages),
}

fn build(
    spec: RenderSpec,
    selection: Selection,
    dpi: u32,
    sheet: Sheet,
    max_pages: usize,
) -> Result<Preview, AppError> {
    let outcome = pipeline::render_blocking(spec)?;

    let count = pdfops::page_count(&outcome.pdf)?;
    if count == 0 {
        return Err(AppError::ProcessFailed {
            message: "The rendered document has no page to preview".to_string(),
            stderr: String::new(),
        });
    }

    let (first, mut last, truncated) = selection.resolve(count, max_pages)?;
    if sheet == Sheet::Png {
        // A bare PNG carries one image: a wider range only says where to start
        last = first;
    }

    // Thumbnails are drawn 200pt wide, so rasterizing the source at the full preview
    // resolution would throw away nine tenths of the pixels it just paid for.
    let source_dpi = match sheet {
        Sheet::Contact => thumbnail_dpi(dpi),
        _ => dpi,
    };
    guard_pixel_budget(last - first + 1, source_dpi)?;

    let rendered = pdfops::rasterize(&outcome.pdf, first, last, source_dpi)?;

    let output = match sheet {
        Sheet::Png => Output::Png(
            rendered
                .into_iter()
                .next()
                .map(|page| page.png)
                .ok_or_else(|| AppError::ProcessFailed {
                    message: "PNG file not found after pdftoppm".to_string(),
                    stderr: String::new(),
                })?,
        ),
        Sheet::Contact => Output::Png(contact_sheet(&rendered, dpi)?),
        Sheet::Images => Output::Images(images(&rendered, count, truncated)?),
    };

    Ok(Preview {
        output,
        pages: last - first + 1,
    })
}

/// Refuse the combination before a single pixel is allocated. A4 is the reference because
/// the real page geometry is only known once the pages are rasterized, which is too late.
fn guard_pixel_budget(pages: usize, dpi: u32) -> Result<(), AppError> {
    const A4_WIDTH_PT: f64 = 595.276;
    const A4_HEIGHT_PT: f64 = 841.89;

    let scale = dpi as f64 / 72.0;
    let estimate = pages as f64 * (A4_WIDTH_PT * scale) * (A4_HEIGHT_PT * scale);

    if estimate > MAX_PREVIEW_PIXELS {
        return Err(AppError::BadRequest(format!(
            "{} pages at {} dpi is about {} megapixels, over the {} megapixel budget of a \
             preview: ask for fewer pages or a lower dpi",
            pages,
            dpi,
            (estimate / 1_000_000.0).round() as u64,
            (MAX_PREVIEW_PIXELS / 1_000_000.0) as u64
        )));
    }

    Ok(())
}

/// Resolution the source pages need so a `SHEET_THUMB_WIDTH` thumbnail is sharp once the
/// sheet itself is rasterized at `dpi`, with half again as many pixels as strictly needed.
fn thumbnail_dpi(dpi: u32) -> u32 {
    const A4_WIDTH_PT: f32 = 595.276;

    let needed = (SHEET_THUMB_WIDTH * dpi as f32 / A4_WIDTH_PT * 1.5).ceil() as u32;
    needed.clamp(MIN_DPI, dpi.max(MIN_DPI))
}

fn images(
    rendered: &[RasterPage],
    pages_total: usize,
    truncated: bool,
) -> Result<PreviewPages, AppError> {
    let bytes: usize = rendered.iter().map(|page| page.png.len()).sum();
    if bytes > MAX_JSON_PNG_BYTES {
        return Err(AppError::BadRequest(format!(
            "This preview is {} MB of PNG, about {} MB once base64-encoded, over the {} MB \
             a JSON preview may carry: ask for fewer pages or a lower dpi",
            bytes / (1024 * 1024),
            bytes * 4 / 3 / (1024 * 1024),
            MAX_JSON_PNG_BYTES / (1024 * 1024)
        )));
    }

    Ok(PreviewPages {
        pages: rendered
            .iter()
            .map(|page| PreviewPage {
                page: page.page,
                png: base64(&page.png),
                width: page.width_px,
                height: page.height_px,
            })
            .collect(),
        pages_total,
        truncated: if truncated { Some(true) } else { None },
    })
}

// ------------ Contact sheet ------------

/// Thumbnails laid out on one page, rendered by weasyprint and rasterized back.
///
/// The grid is computed in points here rather than left to a CSS layout: every box is
/// placed absolutely, so the sheet cannot spill onto a second page and the geometry is the
/// same whatever the weasyprint version in the image.
fn contact_sheet(pages: &[RasterPage], dpi: u32) -> Result<Vec<u8>, AppError> {
    if pages.is_empty() {
        return Err(AppError::ProcessFailed {
            message: "No page to lay out on a contact sheet".to_string(),
            stderr: String::new(),
        });
    }

    let columns = (pages.len() as f64).sqrt().ceil() as usize;
    let columns = columns.clamp(1, SHEET_MAX_COLUMNS).min(pages.len());
    let rows = pages.len().div_ceil(columns);

    // One cell height for the whole grid, driven by the tallest page: mixing portrait and
    // landscape must not make the rows drift out of alignment.
    let tallest = pages.iter().map(aspect).fold(1.0f32, f32::max);
    let thumb_height = SHEET_THUMB_WIDTH * tallest;
    let cell_height = thumb_height + SHEET_LABEL;

    let sheet_width =
        2.0 * SHEET_MARGIN + columns as f32 * SHEET_THUMB_WIDTH + (columns - 1) as f32 * SHEET_GAP;
    let sheet_height =
        2.0 * SHEET_MARGIN + rows as f32 * cell_height + (rows - 1) as f32 * SHEET_GAP;

    let mut body = String::new();
    for (index, page) in pages.iter().enumerate() {
        let cell_x = SHEET_MARGIN + (index % columns) as f32 * (SHEET_THUMB_WIDTH + SHEET_GAP);
        let cell_y = SHEET_MARGIN + (index / columns) as f32 * (cell_height + SHEET_GAP);

        // Fit the page inside the cell and centre it, so a landscape page keeps its shape
        let ratio = aspect(page);
        let scale = (1.0f32).min(thumb_height / (SHEET_THUMB_WIDTH * ratio));
        let width = SHEET_THUMB_WIDTH * scale;
        let height = width * ratio;

        let _ = write!(
            body,
            "<img style=\"left:{:.3}pt;top:{:.3}pt;width:{:.3}pt;height:{:.3}pt\" \
             src=\"data:image/png;base64,{}\">\
             <span style=\"left:{:.3}pt;top:{:.3}pt;width:{:.3}pt\">Page {}</span>",
            cell_x + (SHEET_THUMB_WIDTH - width) / 2.0,
            cell_y + (thumb_height - height) / 2.0,
            width,
            height,
            base64(&page.png),
            cell_x,
            cell_y + thumb_height + 2.0,
            SHEET_THUMB_WIDTH,
            page.page
        );
    }

    let html = format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><style>\n\
         @page {{ size: {width:.3}pt {height:.3}pt; margin: 0; }}\n\
         html, body {{ margin: 0; padding: 0; background: #ededed; }}\n\
         img {{ position: absolute; box-sizing: border-box; background: #fff; \
         border: 0.5pt solid #b0b0b0; }}\n\
         span {{ position: absolute; display: block; text-align: center; \
         font: 9pt/12pt sans-serif; color: #333; }}\n\
         </style></head><body>{body}</body></html>",
        // A hair of slack: an exact fit can round the last box past the page and spill
        width = sheet_width,
        height = sheet_height + 1.0,
        body = body
    );

    let pdf = helpers::run_weasyprint_plain(&html)?;
    let sheet = pdfops::rasterize(&pdf, 1, 1, dpi)?;

    sheet
        .into_iter()
        .next()
        .map(|page| page.png)
        .ok_or_else(|| AppError::ProcessFailed {
            message: "The contact sheet produced no image".to_string(),
            stderr: String::new(),
        })
}

/// Height over width, from the page box when it is usable and from the raster otherwise
fn aspect(page: &RasterPage) -> f32 {
    if page.page_box.width > 0.0 && page.page_box.height > 0.0 {
        page.page_box.height / page.page_box.width
    } else if page.width_px > 0 {
        page.height_px as f32 / page.width_px as f32
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_without_the_new_fields_is_the_historical_preview() {
        let selection = Selection::Range { first: 1, last: 1 };
        assert_eq!(Sheet::parse(None, selection).unwrap(), Sheet::Png);
        assert_eq!(resolve_dpi(None).unwrap(), PREVIEW_DPI);
        assert_eq!(selection.resolve(9, 20).unwrap(), (1, 1, false));
    }

    #[test]
    fn parses_page_specifications() {
        assert_eq!(
            Selection::parse("3", 20).unwrap(),
            Selection::Range { first: 3, last: 3 }
        );
        assert_eq!(
            Selection::parse(" 2 - 5 ", 20).unwrap(),
            Selection::Range { first: 2, last: 5 }
        );
        assert_eq!(Selection::parse("ALL", 20).unwrap(), Selection::All);

        for spec in ["0", "-3", "2-", "abc", "", "1-0"] {
            assert!(Selection::parse(spec, 20).is_err(), "{spec} was accepted");
        }
        // The cap is refused up front, before anything is rendered
        assert!(Selection::parse("1-21", 20).is_err());
    }

    #[test]
    fn an_out_of_document_range_names_the_real_page_count() {
        let err = Selection::Range { first: 4, last: 6 }
            .resolve(3, 20)
            .unwrap_err();
        match err {
            AppError::BadRequest(message) => assert!(message.contains("has 3 pages"), "{message}"),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn all_truncates_at_the_cap_instead_of_failing() {
        assert_eq!(Selection::All.resolve(30, 20).unwrap(), (1, 20, true));
        assert_eq!(Selection::All.resolve(4, 20).unwrap(), (1, 4, false));
    }

    #[test]
    fn several_pages_without_a_layout_can_only_be_json() {
        let many = Selection::Range { first: 1, last: 4 };
        assert_eq!(Sheet::parse(None, many).unwrap(), Sheet::Images);
        assert_eq!(Sheet::parse(Some("sheet"), many).unwrap(), Sheet::Contact);
        assert!(Sheet::parse(Some("mosaic"), many).is_err());
    }

    #[test]
    fn the_pixel_budget_stops_a_high_dpi_on_many_pages() {
        assert!(guard_pixel_budget(1, 300).is_ok());
        assert!(guard_pixel_budget(20, 150).is_ok());
        assert!(guard_pixel_budget(20, 300).is_err());
    }

    #[test]
    fn thumbnails_are_rasterized_far_below_the_sheet_resolution() {
        assert!(thumbnail_dpi(150) < 150);
        // Never below the floor, whatever the sheet is rendered at
        assert!(thumbnail_dpi(MIN_DPI) >= MIN_DPI);
    }
}

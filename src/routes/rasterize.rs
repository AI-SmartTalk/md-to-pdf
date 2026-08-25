//! PDF → images, for a document the service did not render itself.
//!
//! `/api/preview` already rasterizes, but only what it has just produced from markdown. The
//! same pixels are what a caller wants from a file they uploaded — a thumbnail, a page to
//! show in a viewer, an image to feed to something that cannot read PDF — and until the
//! asset store existed there was no way to hand one in.

use crate::assets::AssetMeta;
use crate::auth::PublicOrKey;
use crate::config::config;
use crate::exec;
use crate::helpers::{self, PREVIEW_DPI};
use crate::types::*;
use rocket::fs::NamedFile;
use rocket::serde::json::Json;
use rocket::Either;
use serde::Deserialize;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::Builder;

const MIN_DPI: u32 = 36;
const MAX_DPI: u32 = 300;

/// Pixel ceiling for one call, as in `/api/preview`: the per-field bounds cannot catch a
/// high dpi combined with many pages, which is the cheapest way to buy a lot of memory.
const MAX_PIXELS: f64 = 48_000_000.0;

/// Quality of the JPEGs. Above this the files grow much faster than the page looks better.
const JPEG_QUALITY: u32 = 90;

#[derive(Deserialize)]
pub struct RasterizeRequest {
    /// `asset://as_…` or `/download/<client_id>/<name>.pdf`
    pub pdf: String,
    /// `"1"`, `"2-5"` or `"all"`; absent means the whole document, up to the page ceiling
    pub pages: Option<String>,
    pub dpi: Option<u32>,
    /// `"png"` (default) or `"jpeg"`
    pub format: Option<String>,
    pub client_id: Option<String>,
    pub pdf_name: Option<String>,
    pub output: Option<ToolOutput>,
}

#[post("/rasterize", format = "json", data = "<req>")]
pub async fn rasterize(
    key: PublicOrKey,
    req: Json<RasterizeRequest>,
) -> Result<Either<NamedFile, Json<ToolResponse>>, AppError> {
    let req = req.into_inner();

    // Everything that can be judged without the document is judged here: a typo in "format"
    // must not queue behind twenty conversions to be told about.
    Params::parse(&req)?;

    let (produced, response) = exec::as_owner(key.0, exec::offload(move || run(req))).await?;
    helpers::deliver_tool(produced, response, "rasterize").await
}

/// All the blocking work. Public and free of Rocket: the asynchronous job dispatcher calls
/// it as it is.
pub fn run(req: RasterizeRequest) -> Result<(tempfile::TempPath, ToolResponse), AppError> {
    let params = Params::parse(&req)?;
    let source = helpers::resolve_readable_pdf(&req.pdf)?;

    // An encrypted source is refused here as a 400 rather than escaping as a 500 further
    // down: pdfinfo is the first thing that touches the caller's file.
    let total = crate::pdfops::page_count(&source)
        .map_err(crate::routes::compress::name_encrypted_source)?;
    if total == 0 {
        return Err(AppError::BadRequest(
            "This document has no page to rasterize".to_string(),
        ));
    }

    let (first, last, truncated) = params.pages.resolve(total, params.max_pages)?;
    guard_pixel_budget(last - first + 1, params.dpi)?;

    let images = match params.format {
        Format::Png => png_pages(&source, first, last, params.dpi)?,
        Format::Jpeg => jpeg_pages(&source, first, last, params.dpi)?,
    };

    let mut warnings = Vec::new();
    if truncated {
        warnings.push(format!(
            "truncated: {} of the {} pages were rasterized, at most {} per call",
            images.len(),
            total,
            params.max_pages
        ));
    }

    let produced = write_temp(&images[0].bytes, params.format)?;

    let mut response = if images.len() == 1 {
        let response = helpers::finish_tool(
            &produced,
            req.client_id,
            req.pdf_name,
            req.output.unwrap_or_default(),
            &page_name(images[0].page, params.format),
        )?;
        if response.download_url.is_some() {
            // /download serves everything it holds as application/pdf; an image saved there
            // reaches the browser mislabelled, and the caller has to hear it from us.
            warnings.push(
                "download_url is served as application/pdf whatever it holds: ask for \
                 \"output\":\"asset\" to get the image with its own content type"
                    .to_string(),
            );
        }
        response
    } else {
        if req.client_id.is_some() && req.pdf_name.is_some() {
            warnings.push(format!(
                "\"client_id\" and \"pdf_name\" were ignored: {} images cannot share one \
                 download URL, each page is in \"assets\"",
                images.len()
            ));
        }
        let mut assets = Vec::with_capacity(images.len());
        for image in &images {
            assets.push(store_page(image, params.format)?);
        }
        ToolResponse {
            assets: Some(assets),
            ..Default::default()
        }
    };

    response.pages = Some(images.len());
    if !warnings.is_empty() {
        response.warnings = Some(warnings);
    }

    Ok((produced, response))
}

// ------------ Request parameters ------------

struct Params {
    pages: Pages,
    dpi: u32,
    format: Format,
    max_pages: usize,
}

impl Params {
    fn parse(req: &RasterizeRequest) -> Result<Params, AppError> {
        let max_pages = config().preview_max_pages.max(1);

        Ok(Params {
            pages: match req.pages.as_deref() {
                Some(spec) => Pages::parse(spec, max_pages)?,
                None => Pages::All,
            },
            dpi: resolve_dpi(req.dpi)?,
            format: Format::parse(req.format.as_deref())?,
            max_pages,
        })
    }
}

/// What the caller asked for, before the page count of the document is known
#[derive(Debug, Clone, Copy, PartialEq)]
enum Pages {
    All,
    Range { first: usize, last: usize },
}

impl Pages {
    fn parse(spec: &str, max_pages: usize) -> Result<Pages, AppError> {
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

        if last - first + 1 > max_pages {
            return Err(AppError::BadRequest(format!(
                "\"pages\" covers {} pages; at most {} may be rasterized at once",
                last - first + 1,
                max_pages
            )));
        }

        Ok(Pages::Range { first, last })
    }

    /// Turn the request into a real range, and say whether the ceiling cut it short
    fn resolve(self, count: usize, max_pages: usize) -> Result<(usize, usize, bool), AppError> {
        match self {
            // "all" means "as much as you can give me": the ceiling truncates, it never fails
            Pages::All => Ok((1, count.min(max_pages), count > max_pages)),
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
                    Ok((first, last, false))
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Format {
    Png,
    Jpeg,
}

impl Format {
    fn parse(spec: Option<&str>) -> Result<Format, AppError> {
        let spec = match spec {
            Some(spec) => spec.trim(),
            None => return Ok(Format::Png),
        };

        if spec.eq_ignore_ascii_case("png") {
            Ok(Format::Png)
        } else if spec.eq_ignore_ascii_case("jpeg") || spec.eq_ignore_ascii_case("jpg") {
            Ok(Format::Jpeg)
        } else {
            Err(AppError::BadRequest(format!(
                "\"format\" must be \"png\" or \"jpeg\", not \"{}\"",
                clip(spec)
            )))
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpeg => "jpg",
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

/// Refuse the combination before a single pixel is allocated. A4 is the reference because
/// the real page geometry is only known once the pages are drawn, which is too late.
fn guard_pixel_budget(pages: usize, dpi: u32) -> Result<(), AppError> {
    const A4_WIDTH_PT: f64 = 595.276;
    const A4_HEIGHT_PT: f64 = 841.89;

    let scale = dpi as f64 / 72.0;
    let estimate = pages as f64 * (A4_WIDTH_PT * scale) * (A4_HEIGHT_PT * scale);

    if estimate > MAX_PIXELS {
        return Err(AppError::BadRequest(format!(
            "{} pages at {} dpi is about {} megapixels, over the {} megapixel budget of one \
             call: ask for fewer pages or a lower dpi",
            pages,
            dpi,
            (estimate / 1_000_000.0).round() as u64,
            (MAX_PIXELS / 1_000_000.0) as u64
        )));
    }

    Ok(())
}

// ------------ Drawing ------------

struct Image {
    /// 1-based page number in the source document
    page: usize,
    bytes: Vec<u8>,
}

fn png_pages(pdf: &Path, first: usize, last: usize, dpi: u32) -> Result<Vec<Image>, AppError> {
    Ok(crate::pdfops::rasterize(pdf, first, last, dpi)?
        .into_iter()
        .map(|page| Image {
            page: page.page,
            bytes: page.png,
        })
        .collect())
}

/// Ghostscript draws the JPEGs: `pdftoppm`, behind `pdfops::rasterize`, only writes PNG, and
/// turning one into the other would need an image codec — a dependency for something the
/// image already ships and does in a single pass.
fn jpeg_pages(pdf: &Path, first: usize, last: usize, dpi: u32) -> Result<Vec<Image>, AppError> {
    let workdir = Builder::new().prefix("raster-").tempdir()?;
    let pattern = workdir.path().join("page-%d.jpg");

    helpers::run_tool(
        Command::new("gs")
            .arg("-dSAFER")
            .arg("-dBATCH")
            .arg("-dNOPAUSE")
            .arg("-dNOOUTERSAVE")
            .arg("-sDEVICE=jpeg")
            .arg(format!("-dJPEGQ={}", JPEG_QUALITY))
            .arg(format!("-r{}", dpi))
            .arg(format!("-dFirstPage={}", first))
            .arg(format!("-dLastPage={}", last))
            .arg(format!("-sOutputFile={}", helpers::path_to_str(&pattern)?))
            .arg(helpers::path_to_str(pdf)?),
        "gs",
        "PDF rasterization failed",
    )?;

    let written = fs::read_dir(workdir.path())?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("jpg"))
        .collect();

    let mut images = Vec::new();
    for (index, path) in in_page_order(written).into_iter().enumerate() {
        images.push(Image {
            page: first + index,
            bytes: fs::read(&path)?,
        });
    }

    if images.is_empty() {
        return Err(AppError::ProcessFailed {
            message: "No image found after Ghostscript".to_string(),
            stderr: String::new(),
        });
    }

    Ok(images)
}

/// Ghostscript numbers its output files from one whatever `-dFirstPage` said, so the rank of
/// a file is what maps it back to a source page — never the number in its name.
fn in_page_order(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.sort_by_key(|path| (trailing_number(path).unwrap_or(0), path.clone()));
    paths
}

fn trailing_number(path: &Path) -> Option<usize> {
    path.file_stem()?.to_str()?.rsplit('-').next()?.parse().ok()
}

// ------------ Delivery ------------

fn page_name(page: usize, format: Format) -> String {
    format!("page-{}.{}", page, format.extension())
}

fn write_temp(bytes: &[u8], format: Format) -> Result<tempfile::TempPath, AppError> {
    let mut file = Builder::new()
        .suffix(&format!(".{}", format.extension()))
        .tempfile()?;
    file.write_all(bytes)?;
    Ok(file.into_temp_path())
}

fn store_page(image: &Image, format: Format) -> Result<AssetMeta, AppError> {
    let temp = write_temp(&image.bytes, format)?;
    crate::assets::store(&temp, &page_name(image.page, format))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_naming_nothing_draws_the_whole_document_as_png() {
        assert_eq!(Format::parse(None).unwrap(), Format::Png);
        assert_eq!(resolve_dpi(None).unwrap(), PREVIEW_DPI);

        let params = Params::parse(&RasterizeRequest {
            pdf: "/download/c/f.pdf".to_string(),
            pages: None,
            dpi: None,
            format: None,
            client_id: None,
            pdf_name: None,
            output: None,
        })
        .unwrap();
        assert_eq!(params.pages, Pages::All);
    }

    #[test]
    fn parses_page_specifications() {
        assert_eq!(
            Pages::parse("3", 20).unwrap(),
            Pages::Range { first: 3, last: 3 }
        );
        assert_eq!(
            Pages::parse(" 2 - 5 ", 20).unwrap(),
            Pages::Range { first: 2, last: 5 }
        );
        assert_eq!(Pages::parse("ALL", 20).unwrap(), Pages::All);

        for spec in ["0", "-3", "2-", "abc", "", "1-0"] {
            assert!(Pages::parse(spec, 20).is_err(), "{spec} was accepted");
        }
        // The ceiling is refused up front, before a single page is drawn
        assert!(Pages::parse("1-21", 20).is_err());
    }

    #[test]
    fn all_truncates_at_the_ceiling_instead_of_failing() {
        assert_eq!(Pages::All.resolve(30, 20).unwrap(), (1, 20, true));
        assert_eq!(Pages::All.resolve(4, 20).unwrap(), (1, 4, false));
    }

    #[test]
    fn an_out_of_document_range_names_the_real_page_count() {
        let err = Pages::Range { first: 4, last: 6 }
            .resolve(3, 20)
            .unwrap_err();
        match err {
            AppError::BadRequest(message) => assert!(message.contains("has 3 pages"), "{message}"),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn accepts_only_the_two_formats_it_can_write() {
        assert_eq!(Format::parse(Some("PNG")).unwrap(), Format::Png);
        assert_eq!(Format::parse(Some(" jpeg ")).unwrap(), Format::Jpeg);
        // "jpg" is what a caller types, and refusing it would teach nothing
        assert_eq!(Format::parse(Some("jpg")).unwrap(), Format::Jpeg);

        for spec in ["tiff", "webp", "pdf", ""] {
            assert!(Format::parse(Some(spec)).is_err(), "{spec} was accepted");
        }
    }

    #[test]
    fn a_resolution_outside_the_band_is_refused_with_its_bounds() {
        assert_eq!(resolve_dpi(Some(MIN_DPI)).unwrap(), MIN_DPI);
        assert_eq!(resolve_dpi(Some(MAX_DPI)).unwrap(), MAX_DPI);

        match resolve_dpi(Some(MAX_DPI + 1)).unwrap_err() {
            AppError::BadRequest(message) => assert!(message.contains("301"), "{message}"),
            other => panic!("unexpected error: {other:?}"),
        }
        assert!(resolve_dpi(Some(MIN_DPI - 1)).is_err());
        assert!(resolve_dpi(Some(0)).is_err());
    }

    #[test]
    fn the_pixel_budget_stops_a_high_dpi_on_many_pages() {
        assert!(guard_pixel_budget(1, 300).is_ok());
        assert!(guard_pixel_budget(20, 150).is_ok());
        assert!(guard_pixel_budget(20, 300).is_err());
    }

    #[test]
    fn ghostscript_files_are_ranked_by_their_number_not_by_their_name() {
        let paths = vec![
            PathBuf::from("/tmp/r/page-10.jpg"),
            PathBuf::from("/tmp/r/page-2.jpg"),
            PathBuf::from("/tmp/r/page-1.jpg"),
        ];

        let ordered: Vec<String> = in_page_order(paths)
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(ordered, ["page-1.jpg", "page-2.jpg", "page-10.jpg"]);

        assert_eq!(trailing_number(Path::new("/tmp/r/page-07.jpg")), Some(7));
        assert_eq!(trailing_number(Path::new("/tmp/r/page.jpg")), None);
    }

    #[test]
    fn a_stored_page_is_named_after_its_page_and_its_format() {
        assert_eq!(page_name(3, Format::Png), "page-3.png");
        assert_eq!(page_name(12, Format::Jpeg), "page-12.jpg");
    }
}

//! Images → one PDF, in the order they were given.
//!
//! Scans arrive as photographs, a signed page comes back from a phone, a designer hands
//! over PNGs: every competitor turns those into a document and, until the asset store
//! existed, this service could not even receive them. The order of `images` is the order of
//! the pages — it is the contract, and it is the only thing a caller cannot fix afterwards.
//!
//! The conversion runs on `img2pdf`, which embeds JPEG and PNG data as it is, without
//! re-encoding. WeasyPrint would have been the shorter road — an `<img src="file://…">` per
//! page — but `deploy/weasyprint-safe.py` refuses local files on purpose, and widening a
//! security guard for the comfort of an implementation is how a guard stops meaning
//! anything.

use crate::auth::PublicOrKey;
use crate::exec;
use crate::helpers;
use crate::types::*;
use rocket::fs::NamedFile;
use rocket::serde::json::Json;
use rocket::Either;
use serde::Deserialize;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use tempfile::Builder;

/// One call is one document. Past this the caller wants several documents, not a longer one.
const MAX_IMAGES: usize = 200;

/// Two inches. Every paper size below measures at least 297pt on its short side, so a
/// margin under this bound always leaves a positive area to draw in: the two constants only
/// hold together, which is why they are stated together.
const MAX_MARGIN_PT: f64 = 144.0;

/// Page sizes img2pdf knows by name. Landscape is the same name with its `^T` suffix.
const PAPER_SIZES: [&str; 7] = ["A3", "A4", "A5", "A6", "Letter", "Legal", "Tabloid"];

#[derive(Deserialize)]
pub struct ImagesToPdfRequest {
    /// `asset://as_…` references, one per page, in page order
    pub images: Vec<String>,
    /// One of `PAPER_SIZES`, `"A4"` by default. Ignored when `fit` is `actual`.
    pub paper_size: Option<String>,
    /// `"portrait"` (default), `"landscape"`, or `"auto"` to follow each image
    pub orientation: Option<String>,
    /// `"contain"` (default) or `"actual"`
    pub fit: Option<String>,
    /// Distance between the image and the page border, in points
    pub margin: Option<f64>,
    pub client_id: Option<String>,
    pub pdf_name: Option<String>,
    pub output: Option<ToolOutput>,
}

#[post("/images-to-pdf", format = "json", data = "<req>")]
pub async fn images_to_pdf(
    key: PublicOrKey,
    req: Json<ImagesToPdfRequest>,
) -> Result<Either<NamedFile, Json<ToolResponse>>, AppError> {
    let req = req.into_inner();

    // A typo in "fit" is knowable without touching a single file: saying so now costs the
    // caller nothing, saying so later costs them a place in the render queue.
    Params::parse(&req)?;

    let (produced, response) = exec::as_owner(key.0, exec::offload(move || run(req))).await?;
    helpers::deliver_tool(produced, response, "images-to-pdf").await
}

/// All the blocking work. Public and free of Rocket: the asynchronous job dispatcher calls
/// it as it is.
pub fn run(req: ImagesToPdfRequest) -> Result<(tempfile::TempPath, ToolResponse), AppError> {
    let params = Params::parse(&req)?;
    let sources = resolve_images(&req.images)?;
    let list = write_list(&sources)?;

    let output_temp = Builder::new().suffix(".pdf").tempfile()?;
    let args = arguments(
        &params,
        helpers::path_to_str(&list)?,
        helpers::path_to_str(output_temp.path())?,
    );

    helpers::run_tool(
        Command::new("img2pdf").args(&args),
        "img2pdf",
        "Image to PDF conversion failed",
    )?;

    let produced = output_temp.into_temp_path();

    // Measured before `finish_tool`, which moves the file away when an asset was asked for
    let pages = crate::pdfops::page_count(&produced)?;

    let mut response = helpers::finish_tool(
        &produced,
        req.client_id,
        req.pdf_name,
        req.output.unwrap_or_default(),
        "images.pdf",
    )?;
    response.pages = Some(pages);

    let mut warnings = Vec::new();
    if params.fit == Fit::Actual && (req.paper_size.is_some() || req.orientation.is_some()) {
        warnings.push(
            "\"paper_size\" and \"orientation\" were ignored: with \"fit\":\"actual\" every \
             page takes the size of its own image"
                .to_string(),
        );
    }
    if pages != sources.len() {
        warnings.push(format!(
            "{} images produced {} pages: a multi-frame image (animated GIF, multi-page \
             TIFF) contributes one page per frame",
            sources.len(),
            pages
        ));
    }
    if !warnings.is_empty() {
        response.warnings = Some(warnings);
    }

    Ok((produced, response))
}

// ------------ Request parameters ------------

#[derive(Debug)]
struct Params {
    fit: Fit,
    paper: &'static str,
    orientation: PageOrientation,
    margin: f64,
}

impl Params {
    fn parse(req: &ImagesToPdfRequest) -> Result<Params, AppError> {
        if req.images.is_empty() {
            return Err(AppError::BadRequest(
                "\"images\" must list at least one image".to_string(),
            ));
        }

        if req.images.len() > MAX_IMAGES {
            return Err(AppError::BadRequest(format!(
                "\"images\" holds {} references, at most {} per document",
                req.images.len(),
                MAX_IMAGES
            )));
        }

        for (index, reference) in req.images.iter().enumerate() {
            if crate::assets::strip_scheme(reference).is_none() {
                return Err(not_an_asset(index, reference));
            }
        }

        Ok(Params {
            fit: Fit::parse(req.fit.as_deref())?,
            paper: parse_paper(req.paper_size.as_deref())?,
            orientation: PageOrientation::parse(req.orientation.as_deref())?,
            margin: parse_margin(req.margin)?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Fit {
    /// A page of the requested size, the image scaled to fit inside it
    Contain,
    /// A page the size of the image, as its own dpi describes it
    Actual,
}

impl Fit {
    fn parse(value: Option<&str>) -> Result<Fit, AppError> {
        match value.map(str::trim) {
            None => Ok(Fit::Contain),
            Some(value) if value.eq_ignore_ascii_case("contain") => Ok(Fit::Contain),
            Some(value) if value.eq_ignore_ascii_case("actual") => Ok(Fit::Actual),
            Some(other) => Err(AppError::BadRequest(format!(
                "\"fit\" must be \"contain\" or \"actual\", not \"{}\"",
                clip(other)
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum PageOrientation {
    Portrait,
    Landscape,
    /// Each page follows the shape of the image it shows
    Auto,
}

impl PageOrientation {
    fn parse(value: Option<&str>) -> Result<PageOrientation, AppError> {
        match value.map(str::trim) {
            None => Ok(PageOrientation::Portrait),
            Some(value) if value.eq_ignore_ascii_case("portrait") => Ok(PageOrientation::Portrait),
            Some(value) if value.eq_ignore_ascii_case("landscape") => {
                Ok(PageOrientation::Landscape)
            }
            Some(value) if value.eq_ignore_ascii_case("auto") => Ok(PageOrientation::Auto),
            Some(other) => Err(AppError::BadRequest(format!(
                "\"orientation\" must be \"portrait\", \"landscape\" or \"auto\", not \"{}\"",
                clip(other)
            ))),
        }
    }
}

fn parse_paper(value: Option<&str>) -> Result<&'static str, AppError> {
    let Some(value) = value.map(str::trim) else {
        return Ok("A4");
    };

    PAPER_SIZES
        .into_iter()
        .find(|known| known.eq_ignore_ascii_case(value))
        .ok_or_else(|| {
            AppError::BadRequest(format!(
                "\"paper_size\" must be one of {}, not \"{}\"",
                PAPER_SIZES.join(", "),
                clip(value)
            ))
        })
}

fn parse_margin(value: Option<f64>) -> Result<f64, AppError> {
    let margin = value.unwrap_or(0.0);

    if !margin.is_finite() || margin < 0.0 || margin > MAX_MARGIN_PT {
        return Err(AppError::BadRequest(format!(
            "\"margin\" is a distance in points between 0 and {}, not {}",
            MAX_MARGIN_PT, margin
        )));
    }

    Ok(margin)
}

fn not_an_asset(index: usize, reference: &str) -> AppError {
    AppError::BadRequest(format!(
        "\"images\"[{}]: \"{}\" is not an uploaded file. Send the image to POST /api/files \
         and pass the \"asset://as_…\" reference it returns",
        index,
        clip(reference)
    ))
}

fn clip(value: &str) -> String {
    value.chars().take(60).collect()
}

// ------------ Conversion ------------

/// The command line, built apart from the process so the contract this route promises can
/// be checked without img2pdf installed.
fn arguments(params: &Params, list: &str, output: &str) -> Vec<String> {
    let mut args = vec![
        "--from-file".to_string(),
        list.to_string(),
        "--output".to_string(),
        output.to_string(),
    ];

    if params.margin > 0.0 {
        args.push("--border".to_string());
        args.push(format!("{:.2}pt", params.margin));
    }

    if params.fit == Fit::Contain {
        args.push("--pagesize".to_string());
        args.push(match params.orientation {
            PageOrientation::Landscape => format!("{}^T", params.paper),
            _ => params.paper.to_string(),
        });
        // `into` scales in both directions and never deforms: a small scan fills the page
        // and a 40-megapixel photograph stops overflowing it
        args.push("--fit".to_string());
        args.push("into".to_string());

        if params.orientation == PageOrientation::Auto {
            args.push("--auto-orient".to_string());
        }
    }

    args
}

/// Resolve every reference, refusing anything whose bytes are not an image.
///
/// The kind comes from `assets::meta`, which read it from the first bytes of the file: a
/// spreadsheet renamed `photo.jpg` is caught here, by name, rather than as a PIL traceback
/// in the stderr of a Python program.
fn resolve_images(references: &[String]) -> Result<Vec<PathBuf>, AppError> {
    let mut paths = Vec::with_capacity(references.len());

    for (index, reference) in references.iter().enumerate() {
        let id =
            crate::assets::strip_scheme(reference).ok_or_else(|| not_an_asset(index, reference))?;
        let meta = crate::assets::meta(id)?;

        if !meta.kind.is_image() {
            return Err(AppError::BadRequest(format!(
                "\"images\"[{}]: \"{}\" is a {} file, not an image — the kind is read from \
                 the bytes of the file, never from its name",
                index,
                clip(&meta.name),
                meta.kind.as_str()
            )));
        }

        paths.push(crate::assets::path(id)?);
    }

    Ok(paths)
}

/// img2pdf reads its inputs NUL-separated from a file: a path is never split on a space,
/// never mistaken for an option, and two hundred of them never reach the argument limit.
fn write_list(paths: &[PathBuf]) -> Result<tempfile::TempPath, AppError> {
    let mut file = Builder::new().suffix(".img2pdf-list").tempfile()?;

    for (index, path) in paths.iter().enumerate() {
        if index > 0 {
            file.write_all(&[0])?;
        }
        file.write_all(helpers::path_to_str(path)?.as_bytes())?;
    }

    Ok(file.into_temp_path())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(images: &[&str]) -> ImagesToPdfRequest {
        ImagesToPdfRequest {
            images: images.iter().map(|value| value.to_string()).collect(),
            paper_size: None,
            orientation: None,
            fit: None,
            margin: None,
            client_id: None,
            pdf_name: None,
            output: None,
        }
    }

    #[test]
    fn a_request_naming_nothing_makes_portrait_a4_pages_without_a_margin() {
        let params = Params::parse(&request(&["asset://as_0123"])).unwrap();

        assert_eq!(params.fit, Fit::Contain);
        assert_eq!(params.paper, "A4");
        assert_eq!(params.orientation, PageOrientation::Portrait);
        assert_eq!(params.margin, 0.0);

        let args = arguments(&params, "/tmp/list", "/tmp/out.pdf");
        assert_eq!(
            args,
            [
                "--from-file",
                "/tmp/list",
                "--output",
                "/tmp/out.pdf",
                "--pagesize",
                "A4",
                "--fit",
                "into"
            ]
        );
    }

    #[test]
    fn the_page_order_is_the_order_of_the_images() {
        let paths = [
            PathBuf::from("/assets/first.png"),
            PathBuf::from("/assets/second.jpg"),
            PathBuf::from("/assets/third.tiff"),
        ];

        let list = write_list(&paths).unwrap();
        let written = std::fs::read(&list).unwrap();

        assert_eq!(
            written,
            b"/assets/first.png\0/assets/second.jpg\0/assets/third.tiff"
        );
    }

    #[test]
    fn a_landscape_page_is_the_paper_name_with_its_transpose_suffix() {
        let mut req = request(&["asset://as_0123"]);
        req.paper_size = Some("letter".to_string());
        req.orientation = Some("LANDSCAPE".to_string());

        let params = Params::parse(&req).unwrap();
        assert_eq!(params.paper, "Letter");

        let args = arguments(&params, "list", "out.pdf");
        assert!(args.contains(&"Letter^T".to_string()), "{args:?}");
        assert!(!args.contains(&"--auto-orient".to_string()), "{args:?}");
    }

    #[test]
    fn an_automatic_orientation_lets_each_image_shape_its_page() {
        let mut req = request(&["asset://as_0123"]);
        req.orientation = Some("auto".to_string());

        let args = arguments(&Params::parse(&req).unwrap(), "list", "out.pdf");
        assert!(args.contains(&"--auto-orient".to_string()), "{args:?}");
        assert!(args.contains(&"A4".to_string()), "{args:?}");
    }

    #[test]
    fn fit_actual_leaves_the_page_size_to_the_image() {
        let mut req = request(&["asset://as_0123"]);
        req.fit = Some("Actual".to_string());
        req.paper_size = Some("A3".to_string());
        req.margin = Some(12.0);

        let args = arguments(&Params::parse(&req).unwrap(), "list", "out.pdf");
        assert!(!args.contains(&"--pagesize".to_string()), "{args:?}");
        assert!(!args.contains(&"--fit".to_string()), "{args:?}");
        // The margin still applies: img2pdf grows the page around the image
        assert_eq!(args[args.len() - 2..], ["--border", "12.00pt"]);
    }

    #[test]
    fn an_empty_or_oversized_list_is_refused_before_any_file_is_read() {
        assert!(Params::parse(&request(&[])).is_err());

        let too_many: Vec<String> = (0..=MAX_IMAGES)
            .map(|_| "asset://as_0123".to_string())
            .collect();
        let mut req = request(&[]);
        req.images = too_many;

        match Params::parse(&req).unwrap_err() {
            AppError::BadRequest(message) => assert!(message.contains("201"), "{message}"),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn only_an_uploaded_file_can_become_a_page() {
        // A /download/... path holds PDFs the service produced, and carries no kind to check
        match Params::parse(&request(&["/download/acme/report.pdf"])).unwrap_err() {
            AppError::BadRequest(message) => {
                assert!(message.contains("images\"[0]"), "{message}");
                assert!(message.contains("/api/files"), "{message}");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn an_unknown_paper_size_answers_with_the_ones_that_exist() {
        assert_eq!(parse_paper(Some(" a5 ")).unwrap(), "A5");
        assert_eq!(parse_paper(None).unwrap(), "A4");

        match parse_paper(Some("A2")).unwrap_err() {
            AppError::BadRequest(message) => assert!(message.contains("Tabloid"), "{message}"),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn a_margin_outside_the_band_is_refused_with_its_bound() {
        assert_eq!(parse_margin(None).unwrap(), 0.0);
        assert_eq!(parse_margin(Some(MAX_MARGIN_PT)).unwrap(), MAX_MARGIN_PT);

        for value in [-1.0, MAX_MARGIN_PT + 1.0, f64::INFINITY, f64::NAN] {
            assert!(parse_margin(Some(value)).is_err(), "{value} was accepted");
        }
    }

    /// `assets::store` renames the file it is handed, so a measurement taken after
    /// `finish_tool` reads a path that no longer exists the day `/tmp` and the asset root
    /// share a filesystem. Only the order of the two statements prevents it, and `run` cannot
    /// be exercised in a unit test — it needs img2pdf — so the order itself is what is
    /// asserted.
    #[test]
    fn the_page_count_is_taken_before_the_file_is_published() {
        let source = include_str!("images_to_pdf.rs");
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

    #[test]
    fn a_misspelled_fit_names_the_two_that_work() {
        assert_eq!(Fit::parse(None).unwrap(), Fit::Contain);
        assert_eq!(Fit::parse(Some(" CONTAIN ")).unwrap(), Fit::Contain);
        assert_eq!(Fit::parse(Some("actual")).unwrap(), Fit::Actual);

        for value in ["cover", "fill", ""] {
            assert!(Fit::parse(Some(value)).is_err(), "{value} was accepted");
        }
        assert!(PageOrientation::parse(Some("sideways")).is_err());
    }
}

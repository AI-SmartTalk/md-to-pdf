//! OCR: giving a scan the text layer it never had.
//!
//! Everything downstream depends on it. `/api/redact` returns `redactions: []` and
//! `/api/extract` returns an empty document when the pages are pictures of text, because
//! there is nothing to search. What separates this route from a thin `ocrmypdf` wrapper is
//! the sidecar: the recognised text is read back and measured page by page, so a page that
//! came out empty is named instead of travelling silently inside a file that looks fine.

use crate::auth::PublicOrKey;
use crate::exec;
use crate::helpers;
use crate::types::{AppError, Check, ToolOutput, ToolResponse, Verdict};
use rocket::fs::NamedFile;
use rocket::serde::json::Json;
use rocket::Either;
use serde::Deserialize;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::Command;
use tempfile::Builder;

/// The traineddata files installed in the image. `-l` is a process argument that picks
/// files off the disk by name, so it cannot be a free-form string from a request body.
const INSTALLED_LANGUAGES: [&str; 5] = ["fra", "eng", "deu", "spa", "ita"];

const DEFAULT_LANGUAGES: [&str; 2] = ["fra", "eng"];

/// An OCR pass costs minutes of CPU per hundred pages. `ASSET_MAX_PAGES` sizes storage,
/// not tesseract, so this route keeps its own ceiling and says so when it refuses.
const MAX_PAGES: usize = 300;

/// Under this many glyphs a page carries nothing a reader could search: a blank sheet, a
/// photograph, or a scan too degraded for tesseract to resolve.
const MIN_PAGE_CHARS: usize = 50;

/// What ocrmypdf writes in the sidecar for a page it chose not to read — with
/// `--skip-text`, every page that already carried its own text layer. Without this the
/// marker itself would count as two dozen characters and the page would look unreadable.
const SKIPPED_MARKER: &str = "[OCR skipped on page";

/// How many pages a summary names before it gives up and defers to the checks
const MAX_LISTED_PAGES: usize = 5;

#[derive(Deserialize)]
pub struct OcrRequest {
    pub pdf: String,
    pub languages: Option<Vec<String>>,
    pub mode: Option<String>,
    pub pdfa: Option<bool>,
    pub client_id: Option<String>,
    pub pdf_name: Option<String>,
    pub output: Option<ToolOutput>,
}

/// Which pages ocrmypdf is allowed to touch
#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    /// Only the pages that carry no text at all
    Auto,
    /// Rasterize everything and read it again, losing the original text layer
    Force,
    /// Replace a text layer that a previous OCR produced, keep a native one
    Redo,
}

impl Mode {
    fn flag(self) -> &'static str {
        match self {
            Mode::Auto => "--skip-text",
            Mode::Force => "--force-ocr",
            Mode::Redo => "--redo-ocr",
        }
    }
}

#[post("/ocr", format = "json", data = "<req>")]
pub async fn ocr(
    key: PublicOrKey,
    req: Json<OcrRequest>,
) -> Result<Either<NamedFile, Json<ToolResponse>>, AppError> {
    let req = req.into_inner();

    // A misspelled language costs nothing here and costs a render slot once queued.
    validate_reference(&req.pdf)?;
    parse_mode(req.mode.as_deref())?;
    parse_languages(req.languages.as_deref())?;

    let (produced, response) = exec::as_owner(key.0, exec::offload(move || run(req))).await?;
    helpers::deliver_tool(produced, response).await
}

/// All the blocking work. Public and free of Rocket: the asynchronous job dispatcher calls
/// it as is.
pub fn run(req: OcrRequest) -> Result<(tempfile::TempPath, ToolResponse), AppError> {
    validate_reference(&req.pdf)?;
    let mode = parse_mode(req.mode.as_deref())?;
    let languages = parse_languages(req.languages.as_deref())?;
    let pdfa = req.pdfa.unwrap_or(false);

    let source = helpers::resolve_pdf_source(&req.pdf)?;
    // An encrypted source is refused here as a 400 rather than escaping as a 500 further
    // down: pdfinfo is the first thing that touches the caller's file.
    let incoming = crate::pdfops::page_count(&source)
        .map_err(crate::routes::compress::name_encrypted_source)?;
    let ceiling = page_ceiling();
    if incoming > ceiling {
        return Err(AppError::BadRequest(format!(
            "This document has {} pages and OCR is capped at {} per request: split it with /api/pages, then OCR each part",
            incoming, ceiling
        )));
    }

    let out_temp = Builder::new().suffix(".pdf").tempfile()?;
    let sidecar = Builder::new().suffix(".txt").tempfile()?.into_temp_path();

    let mut command = Command::new("ocrmypdf");
    command
        // Concurrency is already bounded by exec::offload; eight tesseract children per
        // request is how a container hits its memory limit instead of its queue.
        .arg("--jobs")
        .arg("1")
        .arg("-l")
        .arg(languages.join("+"))
        .arg(mode.flag())
        .arg("--output-type")
        .arg(if pdfa { "pdfa" } else { "pdf" })
        .arg("--sidecar")
        .arg(helpers::path_to_str(&sidecar)?)
        .arg(helpers::path_to_str(&source)?)
        .arg(helpers::path_to_str(out_temp.path())?);

    let mut warnings = Vec::new();
    if let Err(err) = helpers::run_tool(&mut command, "ocrmypdf", "OCR failed") {
        // ocrmypdf documents non-zero exits that are not breakdowns: a document that
        // already had text, a PDF/A conversion that did not take. Both leave the caller
        // with a usable file, so they are warnings, not a 500.
        match tolerated(&err) {
            Some(warning) => warnings.push(warning),
            None => return Err(err),
        }
    }

    // A tolerated failure sometimes writes no output at all. Handing the original back is
    // the honest answer — with the warning that says why nothing changed.
    if !is_pdf(out_temp.path()) {
        fs::copy(&source, out_temp.path())?;
    }

    let texts = parse_sidecar(&read_lossy(&sidecar));
    let produced = out_temp.into_temp_path();
    let pages = crate::pdfops::page_count(&produced)?;
    let verdict = build_verdict(pages, &texts, &warnings);

    // Measured before `finish_tool`, which moves the file away when an asset was asked for.
    let mut response = helpers::finish_tool(
        &produced,
        req.client_id,
        req.pdf_name,
        req.output.unwrap_or_default(),
        "ocr.pdf",
    )?;
    response.pages = Some(pages);
    response.verdict = Some(verdict);
    if !warnings.is_empty() {
        response.warnings = Some(warnings);
    }

    Ok((produced, response))
}

/// What the sidecar says about one page of the produced document
#[derive(Debug, Clone, PartialEq)]
struct PageText {
    /// 1-based
    page: usize,
    /// Glyphs recognised, whitespace excluded
    chars: usize,
    /// ocrmypdf left this page alone because it already carried text
    skipped: bool,
}

fn page_ceiling() -> usize {
    MAX_PAGES.min(crate::config::config().asset_max_pages)
}

fn validate_reference(pdf: &str) -> Result<(), AppError> {
    if pdf.trim().is_empty() {
        return Err(AppError::BadRequest(
            "\"pdf\" must be an asset reference (asset://as_...) or a /download/<client_id>/<name>.pdf path".to_string(),
        ));
    }
    Ok(())
}

fn parse_mode(mode: Option<&str>) -> Result<Mode, AppError> {
    match mode.unwrap_or("auto") {
        "auto" => Ok(Mode::Auto),
        "force" => Ok(Mode::Force),
        "redo" => Ok(Mode::Redo),
        other => Err(AppError::BadRequest(format!(
            "\"mode\" must be one of auto (OCR only the pages without text), force (rasterize and read everything again), redo (replace a previous OCR layer) (got \"{}\")",
            other
        ))),
    }
}

/// Only the traineddata actually installed, in the order the caller gave: tesseract reads
/// the first language as the primary one.
fn parse_languages(languages: Option<&[String]>) -> Result<Vec<String>, AppError> {
    let requested: Vec<String> = match languages {
        Some(list) => list.iter().map(|lang| lang.trim().to_string()).collect(),
        None => return Ok(DEFAULT_LANGUAGES.iter().map(|l| l.to_string()).collect()),
    };

    if requested.is_empty() {
        return Err(AppError::BadRequest(format!(
            "\"languages\" must not be empty: pick from {}",
            INSTALLED_LANGUAGES.join(", ")
        )));
    }

    let mut kept: Vec<String> = Vec::new();
    for language in requested {
        if !INSTALLED_LANGUAGES.contains(&language.as_str()) {
            return Err(AppError::BadRequest(format!(
                "\"languages\" only accepts the languages installed in this image: {} (got \"{}\")",
                INSTALLED_LANGUAGES.join(", "),
                language
            )));
        }
        // `-l fra+fra` is a tesseract error, and asking twice was never the intent
        if !kept.contains(&language) {
            kept.push(language);
        }
    }

    Ok(kept)
}

/// The documented non-zero exits that still leave the caller with a usable document.
///
/// The exit code itself never reaches this far — every process goes through `run_tool`,
/// which keeps the stderr — so the two cases are recognised by what ocrmypdf prints.
fn tolerated(err: &AppError) -> Option<String> {
    let stderr = match err {
        AppError::ProcessFailed { stderr, .. } => stderr.to_lowercase(),
        _ => return None,
    };

    if stderr.contains("priorocrfound") || stderr.contains("already has text") {
        return Some(
            "ocrmypdf: this document already carries a text layer, it is returned untouched — use \"mode\":\"force\" to read it anyway".to_string(),
        );
    }

    if stderr.contains("pdf/a conversion failed") || stderr.contains("pdfa conversion failed") {
        return Some(
            "ocrmypdf: the PDF/A conversion failed, the delivered file is an ordinary OCRed PDF"
                .to_string(),
        );
    }

    None
}

fn read_lossy(path: &Path) -> String {
    match fs::read(path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        // A missing sidecar is not a reason to lose the document: the verdict says so
        Err(_) => String::new(),
    }
}

/// ocrmypdf separates the pages of its sidecar with a form feed, one segment per page.
fn parse_sidecar(text: &str) -> Vec<PageText> {
    if text.trim().is_empty() {
        return Vec::new();
    }

    text.split('\u{c}')
        .enumerate()
        .map(|(index, segment)| {
            let trimmed = segment.trim();
            let skipped = trimmed.starts_with(SKIPPED_MARKER);
            PageText {
                page: index + 1,
                chars: if skipped {
                    0
                } else {
                    trimmed.chars().filter(|c| !c.is_whitespace()).count()
                },
                skipped,
            }
        })
        .collect()
}

/// An OCR that hands back a file without saying that two pages came out empty is an OCR
/// that lies: those pages are named one by one, with their character count.
///
/// Check names are the translatable contract, kebab-case and shared across the tools:
/// `unreadable-page` names a page this route could not read, and `tool-warning` carries a
/// tolerated warning from the external binary — the same name `/api/unlock` uses for qpdf.
/// `ocr-coverage` is this route's own, and says coverage rather than confidence because the
/// sidecar gives character counts, never a confidence figure.
fn build_verdict(pages: usize, texts: &[PageText], warnings: &[String]) -> Verdict {
    let ocred: Vec<&PageText> = texts.iter().filter(|page| !page.skipped).collect();
    let skipped = texts.len() - ocred.len();
    let weak: Vec<&PageText> = ocred
        .iter()
        .copied()
        .filter(|page| page.chars < MIN_PAGE_CHARS)
        .collect();

    let mut checks = vec![coverage_check(
        pages,
        ocred.len(),
        skipped,
        texts.is_empty(),
    )];

    for page in &weak {
        checks.push(Check::new(
            "unreadable-page",
            "warn",
            format!(
                "page {}: {} characters recognised — blank page, photograph, or a scan too degraded to read",
                page.page, page.chars
            ),
            Some(page.page),
        ));
    }

    for warning in warnings {
        checks.push(Check::warn("tool-warning", warning.clone()));
    }

    Verdict::from_checks(summarize(pages, ocred.len(), skipped, &weak), checks)
}

fn coverage_check(pages: usize, ocred: usize, skipped: usize, empty_sidecar: bool) -> Check {
    if empty_sidecar {
        return Check::warn(
            "ocr-coverage",
            format!(
                "no text recognised on any of the {} pages: the document is returned untouched",
                pages
            ),
        );
    }

    if ocred == 0 {
        return Check::ok(
            "ocr-coverage",
            format!(
                "the {} pages already carried a text layer, none was OCRed",
                skipped
            ),
        );
    }

    Check::ok(
        "ocr-coverage",
        format!("{} of {}", ocred_phrase(ocred), plural(pages, "page")),
    )
}

fn ocred_phrase(count: usize) -> String {
    format!("{} OCRed", plural(count, "page"))
}

fn summarize(pages: usize, ocred: usize, skipped: usize, weak: &[&PageText]) -> String {
    if ocred == 0 && skipped == 0 {
        return format!("{}, no text recognised", plural(pages, "page"));
    }

    let mut parts = Vec::new();

    if ocred > 0 {
        parts.push(ocred_phrase(ocred));
    }

    if skipped > 0 {
        parts.push(format!(
            "{} already carrying a text layer",
            plural(skipped, "page")
        ));
    }

    if !weak.is_empty() {
        parts.push(format!(
            "{} barely readable ({})",
            plural(weak.len(), "page"),
            list_pages(weak)
        ));
    }

    parts.join(", ")
}

/// `p. 7, p. 19`, and an ellipsis rather than a summary nobody reads to the end
fn list_pages(weak: &[&PageText]) -> String {
    let listed: Vec<String> = weak
        .iter()
        .take(MAX_LISTED_PAGES)
        .map(|page| format!("p. {}", page.page))
        .collect();

    if weak.len() > MAX_LISTED_PAGES {
        return format!("{}, …", listed.join(", "));
    }

    listed.join(", ")
}

/// Agreement on the count, because a summary reading "1 pages" is a summary nobody trusts.
fn plural(count: usize, noun: &str) -> String {
    if count > 1 {
        format!("{} {}s", count, noun)
    } else {
        format!("{} {}", count, noun)
    }
}

/// Did the tool leave a real document behind? Only the header is read: a file that
/// ocrmypdf never wrote is empty, and one it half wrote is not a PDF.
fn is_pdf(path: &Path) -> bool {
    let mut header = [0u8; 5];
    match fs::File::open(path).and_then(|mut file| file.read_exact(&mut header)) {
        Ok(()) => &header == b"%PDF-",
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn text(page: usize, chars: usize) -> PageText {
        PageText {
            page,
            chars,
            skipped: false,
        }
    }

    fn skipped(page: usize) -> PageText {
        PageText {
            page,
            chars: 0,
            skipped: true,
        }
    }

    fn refs(pages: &[PageText]) -> Vec<&PageText> {
        pages.iter().collect()
    }

    fn check<'a>(verdict: &'a Verdict, name: &str) -> &'a Check {
        verdict
            .checks
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no {} check in {:?}", name, verdict.checks))
    }

    fn process_failed(stderr: &str) -> AppError {
        AppError::ProcessFailed {
            message: "OCR failed".to_string(),
            stderr: stderr.to_string(),
        }
    }

    #[test]
    fn an_empty_pdf_reference_is_refused_before_any_tool_runs() {
        let err = validate_reference("   ").unwrap_err();
        match err {
            AppError::BadRequest(message) => assert!(message.contains("asset://"), "{}", message),
            other => panic!("expected a bad request, got {:?}", other),
        }
    }

    #[test]
    fn french_and_english_are_read_when_no_language_is_given() {
        assert_eq!(parse_languages(None).unwrap(), vec!["fra", "eng"]);
    }

    #[test]
    fn only_the_languages_installed_in_the_image_reach_tesseract() {
        for language in INSTALLED_LANGUAGES {
            let asked = vec![language.to_string()];
            assert_eq!(parse_languages(Some(&asked)).unwrap(), vec![language]);
        }

        let err = parse_languages(Some(&["../../etc/passwd".to_string()])).unwrap_err();
        match err {
            AppError::BadRequest(message) => {
                assert!(message.contains("fra"), "{}", message);
                assert!(message.contains("ita"), "{}", message);
                assert!(message.contains("passwd"), "{}", message);
            }
            other => panic!("expected a bad request, got {:?}", other),
        }
    }

    #[test]
    fn an_empty_language_list_is_refused_with_the_installed_list() {
        let err = parse_languages(Some(&[])).unwrap_err();
        match err {
            AppError::BadRequest(message) => assert!(message.contains("fra"), "{}", message),
            other => panic!("expected a bad request, got {:?}", other),
        }
    }

    #[test]
    fn a_language_asked_twice_becomes_one_tesseract_argument() {
        let asked = vec!["fra".to_string(), " eng ".to_string(), "fra".to_string()];
        assert_eq!(parse_languages(Some(&asked)).unwrap().join("+"), "fra+eng");
    }

    #[test]
    fn the_default_mode_only_reads_the_pages_without_text() {
        assert_eq!(parse_mode(None).unwrap(), Mode::Auto);
        assert_eq!(parse_mode(None).unwrap().flag(), "--skip-text");
        assert_eq!(parse_mode(Some("force")).unwrap().flag(), "--force-ocr");
        assert_eq!(parse_mode(Some("redo")).unwrap().flag(), "--redo-ocr");
    }

    #[test]
    fn an_unknown_mode_is_refused_with_the_three_modes() {
        let err = parse_mode(Some("off")).unwrap_err();
        match err {
            AppError::BadRequest(message) => {
                assert!(message.contains("auto"), "{}", message);
                assert!(message.contains("force"), "{}", message);
                assert!(message.contains("redo"), "{}", message);
                assert!(message.contains("off"), "{}", message);
            }
            other => panic!("expected a bad request, got {:?}", other),
        }
    }

    #[test]
    fn the_sidecar_is_one_page_per_form_feed() {
        let parsed = parse_sidecar("Contrat de prestation\n\u{c}Article 1 — objet\n\u{c}   ");

        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].page, 1);
        assert_eq!(parsed[0].chars, "Contratdeprestation".chars().count());
        assert_eq!(parsed[1].page, 2);
        assert_eq!(parsed[2].chars, 0);
        assert!(parsed.iter().all(|page| !page.skipped));
    }

    #[test]
    fn a_page_ocrmypdf_left_alone_is_not_counted_as_unreadable() {
        let parsed = parse_sidecar(
            "[OCR skipped on page 1]\u{c}Article 2 — le prestataire s'engage à livrer le rapport avant le 30 juin",
        );
        assert!(parsed[1].chars > MIN_PAGE_CHARS);

        assert!(parsed[0].skipped);
        assert_eq!(parsed[0].chars, 0);
        assert!(!parsed[1].skipped);

        let verdict = build_verdict(2, &parsed, &[]);
        assert!(verdict.checks.iter().all(|c| c.name != "unreadable-page"));
        assert_eq!(verdict.status, "ok");
    }

    #[test]
    fn an_empty_sidecar_yields_no_page_instead_of_a_phantom_one() {
        assert!(parse_sidecar("").is_empty());
        assert!(parse_sidecar("  \n \u{c} ").is_empty());
    }

    #[test]
    fn a_clean_ocr_reads_as_one_sentence() {
        let pages: Vec<PageText> = (1..=24).map(|page| text(page, 1_400)).collect();
        let verdict = build_verdict(24, &pages, &[]);

        assert_eq!(verdict.status, "ok");
        assert_eq!(verdict.score, 100);
        assert_eq!(verdict.summary, "24 pages OCRed");
        assert_eq!(
            check(&verdict, "ocr-coverage").detail,
            "24 pages OCRed of 24 pages"
        );
    }

    #[test]
    fn pages_under_the_character_floor_are_named_one_by_one() {
        let mut pages: Vec<PageText> = (1..=24).map(|page| text(page, 1_400)).collect();
        pages[6] = text(7, 3);
        pages[18] = text(19, 0);

        let verdict = build_verdict(24, &pages, &[]);

        assert_eq!(
            verdict.summary,
            "24 pages OCRed, 2 pages barely readable (p. 7, p. 19)"
        );
        assert_eq!(verdict.status, "warn");

        let named: Vec<Option<usize>> = verdict
            .checks
            .iter()
            .filter(|c| c.name == "unreadable-page")
            .map(|c| c.page)
            .collect();
        assert_eq!(named, vec![Some(7), Some(19)]);
    }

    #[test]
    fn a_single_unreadable_page_is_written_in_the_singular() {
        let pages = vec![text(1, 900), text(2, 4)];
        let verdict = build_verdict(2, &pages, &[]);

        assert_eq!(
            verdict.summary,
            "2 pages OCRed, 1 page barely readable (p. 2)"
        );
    }

    #[test]
    fn a_long_list_of_unreadable_pages_stops_at_five() {
        let weak: Vec<PageText> = (1..=8).map(|page| text(page, 0)).collect();
        assert_eq!(list_pages(&refs(&weak)), "p. 1, p. 2, p. 3, p. 4, p. 5, …");
    }

    #[test]
    fn a_document_that_already_had_text_is_not_reported_as_ocred() {
        let pages: Vec<PageText> = (1..=12).map(skipped).collect();
        let verdict = build_verdict(12, &pages, &[]);

        assert_eq!(verdict.status, "ok");
        assert_eq!(verdict.summary, "12 pages already carrying a text layer");
    }

    #[test]
    fn a_missing_sidecar_warns_instead_of_claiming_success() {
        let verdict = build_verdict(9, &[], &[]);

        assert_eq!(check(&verdict, "ocr-coverage").status, "warn");
        assert_eq!(verdict.summary, "9 pages, no text recognised");
    }

    #[test]
    fn a_prior_text_layer_is_a_warning_and_the_original_comes_back() {
        let warning = tolerated(&process_failed(
            "PriorOcrFoundError: page already has text! - aborting (use --force-ocr to force OCR)",
        ))
        .expect("a prior OCR layer is a documented, tolerable exit");
        assert!(warning.contains("force"), "{}", warning);

        let verdict = build_verdict(4, &[], &[warning]);
        assert_eq!(check(&verdict, "tool-warning").status, "warn");
        assert_eq!(verdict.status, "warn");
    }

    #[test]
    fn a_pdfa_conversion_that_did_not_take_still_delivers_a_pdf() {
        let warning = tolerated(&process_failed(
            "ERROR - PDF/A conversion failed because Ghostscript failed",
        ))
        .expect("a failed PDF/A conversion still leaves a usable PDF");
        assert!(warning.contains("PDF/A"), "{}", warning);
    }

    #[test]
    fn an_unrelated_ocrmypdf_failure_stays_a_failure() {
        assert!(tolerated(&process_failed("InputFileError: not a PDF")).is_none());
        assert!(tolerated(&AppError::BadRequest("nope".to_string())).is_none());
    }

    #[test]
    fn the_page_ceiling_never_exceeds_what_an_ocr_pass_can_afford() {
        assert!(page_ceiling() <= MAX_PAGES);
        assert!(page_ceiling() > 0);
    }

    #[test]
    fn only_a_real_pdf_counts_as_a_produced_file() {
        let mut file = Builder::new().suffix(".pdf").tempfile().unwrap();
        assert!(!is_pdf(file.path()), "an empty file is not a document");

        file.write_all(b"%PDF-1.7\n%\xc7\xec\x8f\xa2").unwrap();
        file.flush().unwrap();
        assert!(is_pdf(file.path()));
    }
}

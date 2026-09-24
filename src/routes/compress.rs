//! Compression, with the bill attached.
//!
//! Compressing a PDF is the one thing every service does. None of them says what it cost:
//! a scan pushed through `-dPDFSETTINGS=/screen` comes back four times smaller and no
//! longer searchable, and the caller finds out months later, in front of a client. So this
//! route measures the two things a compression destroys silently — the page count and the
//! extractable text — and refuses to hand back a file that grew.

use crate::auth::PublicOrKey;
use crate::exec;
use crate::helpers;
// Reading the refusal itself lives in `helpers`: `resolve_readable_pdf` asks the same
// question of a document the store could not open, and the two answers must not drift.
use crate::helpers::refused_for_password;
use crate::types::{AppError, Check, ToolOutput, ToolResponse, Verdict};
use rocket::fs::NamedFile;
use rocket::serde::json::Json;
use rocket::Either;
use serde::Deserialize;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::Builder;

const DEFAULT_LEVEL: &str = "ebook";

/// Below 36 dpi nothing on the page is legible any more, and above 1200 the downsampling
/// switch is a no-op that only makes the run slower.
const MIN_DPI: u32 = 36;
const MAX_DPI: u32 = 1200;

/// Ghostscript rewrites text streams, so a handful of characters can legitimately move.
/// Below this share of the original glyphs something was actually lost.
const TEXT_WARN_RATIO: f64 = 0.98;
/// Below this, whole pages lost their text layer: the compression rasterized them.
const TEXT_FAIL_RATIO: f64 = 0.5;

#[derive(Deserialize)]
pub struct CompressRequest {
    pub pdf: String,
    pub level: Option<String>,
    pub dpi: Option<u32>,
    pub client_id: Option<String>,
    pub pdf_name: Option<String>,
    pub output: Option<ToolOutput>,
}

#[post("/compress", format = "json", data = "<req>")]
pub async fn compress(
    key: PublicOrKey,
    req: Json<CompressRequest>,
) -> Result<Either<NamedFile, Json<ToolResponse>>, AppError> {
    let req = req.into_inner();

    // Rejecting a misspelled level costs nothing here and costs a render slot once queued.
    parse_level(req.level.as_deref())?;
    parse_dpi(req.dpi)?;

    let (produced, response) = exec::as_owner(key.0, exec::offload(move || run(req))).await?;
    helpers::deliver_tool(produced, response, "compress").await
}

/// All the blocking work. Public and free of Rocket: the async job dispatcher calls it
/// as is.
pub fn run(req: CompressRequest) -> Result<(tempfile::TempPath, ToolResponse), AppError> {
    let source = helpers::resolve_readable_pdf(&req.pdf)?;
    let level = parse_level(req.level.as_deref())?;
    let dpi = parse_dpi(req.dpi)?;

    let before = Metrics::measure(&source).map_err(name_encrypted_source)?;

    let out_temp = Builder::new().suffix(".pdf").tempfile()?;
    let out_path = helpers::path_to_str(out_temp.path())?.to_string();

    let mut command = Command::new("gs");
    command
        .arg("-dSAFER")
        .arg("-dBATCH")
        .arg("-dNOPAUSE")
        .arg("-dNOOUTERSAVE")
        .arg("-sDEVICE=pdfwrite")
        .arg(format!("-dPDFSETTINGS=/{}", level))
        .arg("-dCompatibilityLevel=1.5");

    // Each preset carries its own resolutions, so an explicit dpi has to come after
    // `-dPDFSETTINGS` to win over it.
    if let Some(dpi) = dpi {
        command
            .arg("-dDownsampleColorImages=true")
            .arg("-dDownsampleGrayImages=true")
            .arg("-dDownsampleMonoImages=true")
            .arg(format!("-dColorImageResolution={}", dpi))
            .arg(format!("-dGrayImageResolution={}", dpi))
            .arg(format!("-dMonoImageResolution={}", dpi));
    }

    command
        .arg(format!("-sOutputFile={}", out_path))
        .arg(helpers::path_to_str(&source)?);

    helpers::run_tool(&mut command, "gs", "PDF compression failed")?;

    let after = Metrics::measure(out_temp.path())?;

    // A compression that produced a bigger file is a compression that failed, whatever
    // Ghostscript's exit code says. An already optimised PDF does not compress twice, and
    // returning the larger file without a word is the one thing a caller cannot detect.
    let kept_original = after.bytes >= before.bytes;

    let produced = if kept_original {
        let copy = Builder::new().suffix(".pdf").tempfile()?;
        fs::copy(&source, copy.path())?;
        copy.into_temp_path()
    } else {
        out_temp.into_temp_path()
    };

    // The verdict describes the file the caller receives, not the attempt we threw away.
    let delivered = if kept_original { &before } else { &after };
    let verdict = build_verdict(&before, delivered, after.bytes, kept_original);
    let pages = delivered.pages;

    // Measured before `finish_tool`, which moves the file away when an asset was asked for.
    let mut response = helpers::finish_tool(
        &produced,
        req.client_id,
        req.pdf_name,
        req.output.unwrap_or_default(),
        "compressed.pdf",
    )?;
    response.pages = Some(pages);
    response.verdict = Some(verdict);

    Ok((produced, response))
}

/// Turn a refusal to open a password-protected PDF into the 400 it is.
///
/// `pdfinfo` and `pdftotext` exit 1 with `Command Line Error: Incorrect password` on a
/// document carrying a user password, and every route that works on a caller's PDF opens it
/// with one of them before anything else. Left as a `ProcessFailed` it reaches the caller as
/// a 500 — the status that means "our fault, nothing you can do" — when the cause is
/// perfectly diagnosable and the remedy is one call away.
///
/// Written once here and reused by the five routes that read a caller's PDF before touching
/// it: `/api/compress`, `/api/rasterize`, `/api/pdfa`, `/api/ocr` and `/api/extract`. Only
/// the first read of the *source* is wrapped; everything measured afterwards is a file this
/// service has just produced, and a password on that one would be our bug, not the caller's
/// mistake.
pub fn name_encrypted_source(error: AppError) -> AppError {
    match &error {
        AppError::ProcessFailed { stderr, .. } if refused_for_password(stderr) => {
            AppError::BadRequest(
                "This PDF is encrypted: it asks for a password before anything can be read \
                 from it. Remove the protection with POST /api/unlock, which takes the \
                 password, then send the file it hands back to this route."
                    .to_string(),
            )
        }
        _ => error,
    }
}

/// What a document weighs, in the three units that matter to a compression
struct Metrics {
    bytes: u64,
    pages: usize,
    chars: usize,
}

impl Metrics {
    fn measure(pdf: &Path) -> Result<Metrics, AppError> {
        Ok(Metrics {
            bytes: fs::metadata(pdf)?.len(),
            pages: crate::pdfops::page_count(pdf)?,
            chars: text_chars(pdf)?,
        })
    }
}

/// Characters of the extractable text layer, whitespace excluded.
///
/// Whitespace is dropped because Ghostscript rewrites the spacing of a text stream as a
/// matter of course; what must not change is how many glyphs a reader can select, search
/// and copy.
fn text_chars(pdf: &Path) -> Result<usize, AppError> {
    let output = helpers::run_capture(
        Command::new("pdftotext")
            .arg("-q")
            .arg(helpers::path_to_str(pdf)?)
            .arg("-"),
        "pdftotext",
        "text extraction failed",
    )?;

    Ok(String::from_utf8_lossy(&output.stdout)
        .chars()
        .filter(|c| !c.is_whitespace())
        .count())
}

/// The four Ghostscript presets and nothing else: `-dPDFSETTINGS` accepts any string and
/// silently ignores the ones it does not know, so a typo would come back as a file that
/// was never compressed.
fn parse_level(level: Option<&str>) -> Result<&'static str, AppError> {
    match level.unwrap_or(DEFAULT_LEVEL) {
        "screen" => Ok("screen"),
        "ebook" => Ok("ebook"),
        "printer" => Ok("printer"),
        "prepress" => Ok("prepress"),
        other => Err(AppError::BadRequest(format!(
            "\"level\" must be one of screen, ebook, printer, prepress (got \"{}\")",
            other
        ))),
    }
}

fn parse_dpi(dpi: Option<u32>) -> Result<Option<u32>, AppError> {
    match dpi {
        Some(value) if !(MIN_DPI..=MAX_DPI).contains(&value) => Err(AppError::BadRequest(format!(
            "\"dpi\" must be between {} and {}",
            MIN_DPI, MAX_DPI
        ))),
        value => Ok(value),
    }
}

/// Check names are the translatable contract: the public site keys its wording on them, so
/// they are stable, kebab-case, and shared with the other tools that mean the same thing —
/// `page-count` and `text-preserved` are the ones this route has in common with
/// `/api/office-to-pdf`, `/api/pdfa`, `/api/crop` and `/api/repair`. Only the English
/// `detail` is written here; the translation lives in `static/outils/app.js`.
fn build_verdict(
    before: &Metrics,
    delivered: &Metrics,
    attempt_bytes: u64,
    kept_original: bool,
) -> Verdict {
    let mut checks = Vec::new();

    if kept_original {
        // `delivered` is the source here, so these two checks compare the document with
        // itself and cannot say anything but "ok". Reading "12 pages, unchanged" and
        // "20000 of 20000 characters kept" next to a returned original is how a caller
        // concludes the compression was audited — when it was thrown away unmeasured.
        checks.push(Check::ok(
            "page-count",
            format!(
                "{} pages: this is the original, returned untouched — the count is its own, \
                 the discarded attempt was never measured",
                before.pages
            ),
        ));
        checks.push(Check::ok(
            "text-preserved",
            format!(
                "{} characters: this is the original, returned untouched — nothing was \
                 rewritten, so nothing could be lost, and the discarded attempt was never \
                 checked",
                before.chars
            ),
        ));
    } else {
        checks.push(if delivered.pages == before.pages {
            Check::ok("page-count", format!("{} pages, unchanged", before.pages))
        } else {
            Check::fail(
                "page-count",
                format!(
                    "{} pages in, {} out: pages were lost",
                    before.pages, delivered.pages
                ),
            )
        });

        checks.push(text_check(before.chars, delivered.chars));
    }

    checks.push(Check::ok(
        "size",
        format!(
            "{} → {} ({})",
            human_bytes(before.bytes),
            human_bytes(delivered.bytes),
            reduction_label(before.bytes, delivered.bytes)
        ),
    ));

    if kept_original {
        checks.push(Check::warn(
            "no-gain",
            format!(
                "compression produced a larger file ({} against {}): this PDF is already optimised, the original is returned untouched",
                human_bytes(attempt_bytes),
                human_bytes(before.bytes)
            ),
        ));
    }

    let summary = if kept_original {
        format!(
            "{}, already optimised: original returned untouched, {} pages",
            human_bytes(before.bytes),
            before.pages
        )
    } else {
        format!(
            "{} → {}, {}, {} pages",
            human_bytes(before.bytes),
            human_bytes(delivered.bytes),
            text_phrase(before.chars, delivered.chars),
            delivered.pages
        )
    };

    Verdict::from_checks(summary, checks)
}

/// The check that catches a compression which rasterized the pages and destroyed the
/// text selection — the failure nobody sees until they search the document.
fn text_check(before: usize, after: usize) -> Check {
    if before == 0 {
        return Check::ok(
            "text-preserved",
            "the source document carries no text layer: nothing to preserve",
        );
    }

    let ratio = after as f64 / before as f64;

    if ratio >= TEXT_WARN_RATIO {
        Check::ok(
            "text-preserved",
            format!("{} of {} characters kept", after, before),
        )
    } else if ratio >= TEXT_FAIL_RATIO {
        Check::warn(
            "text-preserved",
            format!(
                "{} characters in, {} out: {}% of the text is gone",
                before,
                after,
                percent(1.0 - ratio)
            ),
        )
    } else {
        Check::fail(
            "text-preserved",
            format!(
                "{} characters in, {} out: the compression rasterized the pages, the text is no longer selectable nor searchable",
                before, after
            ),
        )
    }
}

fn text_phrase(before: usize, after: usize) -> String {
    if before == 0 {
        return "no text layer".to_string();
    }

    let ratio = after as f64 / before as f64;

    if ratio >= TEXT_WARN_RATIO {
        "text intact".to_string()
    } else if ratio >= TEXT_FAIL_RATIO {
        format!("text down by {}%", percent(1.0 - ratio))
    } else {
        format!("text lost ({}% kept)", percent(ratio))
    }
}

fn reduction_label(before: u64, after: u64) -> String {
    if before == 0 || after >= before {
        return "no gain".to_string();
    }

    format!("-{}%", percent((before - after) as f64 / before as f64))
}

fn percent(ratio: f64) -> String {
    format!("{:.0}", (ratio * 100.0).clamp(0.0, 100.0))
}

/// Sizes the way the API writes them everywhere else — English units, decimal point — so a
/// summary can be read, logged and translated without being reformatted first.
fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "kB", "MB", "GB"];

    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 || value >= 10.0 {
        format!("{:.0} {}", value, UNITS[unit])
    } else {
        format!("{:.1} {}", value, UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics(bytes: u64, pages: usize, chars: usize) -> Metrics {
        Metrics {
            bytes,
            pages,
            chars,
        }
    }

    fn check<'a>(verdict: &'a Verdict, name: &str) -> &'a Check {
        verdict
            .checks
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no {} check in {:?}", name, verdict.checks))
    }

    #[test]
    fn the_default_level_is_ebook_and_the_four_presets_are_accepted() {
        assert_eq!(parse_level(None).unwrap(), "ebook");
        for level in ["screen", "ebook", "printer", "prepress"] {
            assert_eq!(parse_level(Some(level)).unwrap(), level);
        }
    }

    #[test]
    fn an_unknown_level_is_refused_with_the_list_of_levels() {
        let error = parse_level(Some("small")).unwrap_err();
        match error {
            AppError::BadRequest(message) => {
                assert!(message.contains("screen"), "{}", message);
                assert!(message.contains("prepress"), "{}", message);
                assert!(message.contains("small"), "{}", message);
            }
            other => panic!("expected a bad request, got {:?}", other),
        }
    }

    #[test]
    fn a_dpi_outside_the_legible_range_is_refused_before_any_work() {
        assert_eq!(parse_dpi(None).unwrap(), None);
        assert_eq!(parse_dpi(Some(150)).unwrap(), Some(150));
        assert_eq!(parse_dpi(Some(MIN_DPI)).unwrap(), Some(MIN_DPI));
        assert_eq!(parse_dpi(Some(MAX_DPI)).unwrap(), Some(MAX_DPI));
        assert!(parse_dpi(Some(MIN_DPI - 1)).is_err());
        assert!(parse_dpi(Some(MAX_DPI + 1)).is_err());
        assert!(parse_dpi(Some(0)).is_err());
    }

    #[test]
    fn a_clean_compression_reads_as_one_sentence() {
        let before = metrics(4_404_019, 12, 20_000);
        let after = metrics(798_720, 12, 20_000);
        let verdict = build_verdict(&before, &after, after.bytes, false);

        assert_eq!(verdict.status, "ok");
        assert_eq!(verdict.score, 100);
        assert_eq!(verdict.summary, "4.2 MB → 780 kB, text intact, 12 pages");
        assert!(check(&verdict, "size").detail.contains("-82%"));
    }

    #[test]
    fn losing_pages_fails_the_verdict() {
        let before = metrics(1_000_000, 12, 20_000);
        let after = metrics(400_000, 9, 20_000);
        let verdict = build_verdict(&before, &after, after.bytes, false);

        assert_eq!(check(&verdict, "page-count").status, "fail");
        assert_eq!(verdict.status, "fail");
    }

    #[test]
    fn a_rasterized_text_layer_fails_instead_of_warning() {
        let before = metrics(1_000_000, 5, 20_000);
        let after = metrics(200_000, 5, 200);
        let verdict = build_verdict(&before, &after, after.bytes, false);

        let text = check(&verdict, "text-preserved");
        assert_eq!(text.status, "fail");
        assert!(text.detail.contains("rasterized"), "{}", text.detail);
        assert_eq!(verdict.status, "fail");
    }

    #[test]
    fn a_slightly_shorter_text_layer_only_warns() {
        let before = metrics(1_000_000, 5, 20_000);
        let after = metrics(500_000, 5, 19_000);
        let verdict = build_verdict(&before, &after, after.bytes, false);

        assert_eq!(check(&verdict, "text-preserved").status, "warn");
        assert_eq!(verdict.status, "warn");
        assert!(verdict.summary.contains("text down by 5%"));
    }

    #[test]
    fn a_scan_without_a_text_layer_is_not_penalised() {
        let before = metrics(8_000_000, 30, 0);
        let after = metrics(2_000_000, 30, 0);
        let verdict = build_verdict(&before, &after, after.bytes, false);

        assert_eq!(check(&verdict, "text-preserved").status, "ok");
        assert_eq!(verdict.status, "ok");
    }

    #[test]
    fn a_file_that_grew_is_announced_and_the_original_is_the_one_described() {
        let before = metrics(500_000, 8, 12_000);
        let verdict = build_verdict(&before, &before, 620_000, true);

        let no_gain = check(&verdict, "no-gain");
        assert_eq!(no_gain.status, "warn");
        assert!(no_gain.detail.contains("original"), "{}", no_gain.detail);
        assert!(
            verdict.summary.contains("already optimised"),
            "{}",
            verdict.summary
        );
        assert!(check(&verdict, "size").detail.contains("no gain"));
        assert_eq!(verdict.status, "warn");
    }

    #[test]
    fn the_two_checks_that_would_measure_the_original_against_itself_say_so() {
        let before = metrics(500_000, 8, 12_000);
        let verdict = build_verdict(&before, &before, 620_000, true);

        // Both are "ok" — they describe an intact document — but neither may read as the
        // audit of a compression, because no compression was delivered.
        let pages = check(&verdict, "page-count");
        assert_eq!(pages.status, "ok");
        assert!(pages.detail.contains("original"), "{}", pages.detail);
        assert!(pages.detail.contains("discarded"), "{}", pages.detail);

        let text = check(&verdict, "text-preserved");
        assert_eq!(text.status, "ok");
        assert!(text.detail.contains("original"), "{}", text.detail);
        assert!(text.detail.contains("discarded"), "{}", text.detail);
        assert!(
            !text.detail.contains("characters kept"),
            "a kept original must not claim a preservation ratio: {}",
            text.detail
        );
    }

    #[test]
    fn a_password_protected_source_is_the_callers_error_and_names_the_way_out() {
        // What pdfinfo prints, verbatim, on a PDF carrying a user password
        for stderr in [
            "Command Line Error: Incorrect password\n",
            "qpdf: enc.pdf: invalid password",
            "**** This file requires a password for access.",
        ] {
            let error = name_encrypted_source(AppError::ProcessFailed {
                message: "pdfinfo failed".to_string(),
                stderr: stderr.to_string(),
            });
            match error {
                AppError::BadRequest(message) => {
                    assert!(message.contains("encrypted"), "{}", message);
                    assert!(message.contains("/api/unlock"), "{}", message);
                }
                other => panic!("expected a bad request for {:?}, got {:?}", stderr, other),
            }
        }
    }

    #[test]
    fn a_failure_that_is_not_about_a_password_keeps_its_five_hundred() {
        let error = name_encrypted_source(AppError::ProcessFailed {
            message: "pdfinfo failed".to_string(),
            stderr: "Syntax Error: Couldn't find trailer dictionary".to_string(),
        });
        assert!(matches!(error, AppError::ProcessFailed { .. }));

        let error = name_encrypted_source(AppError::BadRequest("nope".to_string()));
        assert!(matches!(error, AppError::BadRequest(_)));
    }

    #[test]
    fn a_gainful_compression_carries_no_no_gain_check() {
        let before = metrics(1_000_000, 3, 900);
        let after = metrics(900_000, 3, 900);
        let verdict = build_verdict(&before, &after, after.bytes, false);

        assert!(verdict.checks.iter().all(|c| c.name != "no-gain"));
    }

    #[test]
    fn sizes_are_written_the_way_a_human_reads_them() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1.0 kB");
        assert_eq!(human_bytes(798_720), "780 kB");
        assert_eq!(human_bytes(4_404_019), "4.2 MB");
        assert_eq!(human_bytes(3_221_225_472), "3.0 GB");
    }

    #[test]
    fn an_empty_input_never_divides_by_zero() {
        assert_eq!(reduction_label(0, 0), "no gain");
        assert_eq!(text_phrase(0, 0), "no text layer");
    }
}

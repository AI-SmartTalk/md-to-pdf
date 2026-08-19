//! PDF/A — the form a legal archive accepts.
//!
//! Ghostscript is a PDF/A *producer*, not a validator: it writes the identification
//! metadata, converts the colours and drops what the standard forbids. Nothing in this
//! image proves the result passes veraPDF, so the verdict says so in as many words. What we
//! can check, we check: the pages are still there, the file really carries the PDF/A
//! identification it was asked for, and every adjustment Ghostscript reported is handed
//! back — a conversion that removed an annotation or flattened a transparency is not a
//! failure, but nobody should discover it after depositing the file in an archive.

use crate::auth::PublicOrKey;
use crate::exec;
use crate::helpers;
use crate::types::{AppError, Check, ToolOutput, ToolResponse, Verdict};
use rocket::fs::NamedFile;
use rocket::serde::json::Json;
use rocket::Either;
use serde::Deserialize;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::{Builder, TempPath};

const DEFAULT_VARIANT: &str = "pdf/a-2b";

/// Ghostscript's own words when it gave up on PDF/A and wrote a plain PDF instead
const REVERTED: &str = "reverting to normal PDF output";

/// The XMP identification Ghostscript writes into the file it produced. It sits in an
/// uncompressed packet, so scanning the bytes finds it.
const MARKER: &[u8] = b"pdfaid:part=";

/// A conversion that reports more than this has one systemic problem repeated on every
/// page; listing it fifty times would only bury the rest of the verdict.
const MAX_MESSAGES: usize = 12;

const SCAN_CHUNK: usize = 64 * 1024;

#[derive(Deserialize)]
pub struct PdfaRequest {
    pub pdf: String,
    /// `pdf/a-1b`, `pdf/a-2b` (default) or `pdf/a-3b`
    pub variant: Option<String>,
    pub client_id: Option<String>,
    pub pdf_name: Option<String>,
    pub output: Option<ToolOutput>,
}

#[post("/pdfa", format = "json", data = "<req>")]
pub async fn to_pdfa(
    key: PublicOrKey,
    req: Json<PdfaRequest>,
) -> Result<Either<NamedFile, Json<ToolResponse>>, AppError> {
    let req = req.into_inner();

    // A misspelled variant costs nothing here and costs a render slot once queued
    validate_reference(&req.pdf)?;
    parse_variant(req.variant.as_deref())?;

    let (produced, response) = exec::as_owner(key.0, exec::offload(move || run(req))).await?;
    helpers::deliver_tool(produced, response, "pdfa").await
}

/// All the blocking work. Public and free of Rocket: the asynchronous job dispatcher calls
/// it as is.
pub fn run(req: PdfaRequest) -> Result<(TempPath, ToolResponse), AppError> {
    validate_reference(&req.pdf)?;
    let part = parse_variant(req.variant.as_deref())?;
    let source = helpers::resolve_pdf_source(&req.pdf)?;
    // An encrypted source is refused here as a 400 rather than escaping as a 500 further
    // down: pdfinfo is the first thing that touches the caller's file.
    let source_pages = crate::pdfops::page_count(&source)
        .map_err(crate::routes::compress::name_encrypted_source)?;

    let out_temp = Builder::new().suffix(".pdf").tempfile()?;
    let out_path = helpers::path_to_str(out_temp.path())?.to_string();

    let profile = srgb_profile();
    let definition = match profile.as_deref() {
        Some(icc) => Some(write_definition(icc)?),
        None => None,
    };

    let mut command = Command::new("gs");
    command
        .arg("-dSAFER")
        .arg("-dBATCH")
        .arg("-dNOPAUSE")
        .arg("-dNOOUTERSAVE")
        .arg(format!("-dPDFA={}", part))
        // The setting that decides what the caller actually receives. At 0, the default,
        // Ghostscript keeps the offending feature and quietly drops the PDF/A
        // identification: a plain PDF handed back under an archival name. At 1 it removes
        // the feature and the output stays PDF/A, and it names every removal on stderr —
        // which is exactly what the verdict below reads back to the caller.
        .arg("-dPDFACompatibilityPolicy=1")
        .arg("-sColorConversionStrategy=UseDeviceIndependentColor")
        .arg("-sDEVICE=pdfwrite");

    if let Some(icc) = profile.as_deref() {
        // -dSAFER forbids the definition file from opening anything that was not granted
        // explicitly, and it has to open the profile to embed it
        command.arg(format!("--permit-file-read={}", helpers::path_to_str(icc)?));
    }

    command.arg(format!("-sOutputFile={}", out_path));
    if let Some(definition) = definition.as_deref() {
        command.arg(helpers::path_to_str(definition)?);
    }
    command.arg(helpers::path_to_str(&source)?);

    let output = helpers::run_capture(&mut command, "gs", "PDF/A conversion failed")?;

    let messages = compliance_messages(&[
        &String::from_utf8_lossy(&output.stderr),
        &String::from_utf8_lossy(&output.stdout),
    ]);

    // Measured before `finish_tool`, which moves the file away when an asset was asked for
    let pages = crate::pdfops::page_count(out_temp.path())?;
    let declared = declared_part(out_temp.path())?;

    let produced = out_temp.into_temp_path();
    let mut response = helpers::finish_tool(
        &produced,
        req.client_id,
        req.pdf_name,
        req.output.unwrap_or_default(),
        "pdfa.pdf",
    )?;
    response.pages = Some(pages);
    response.verdict = Some(build_verdict(
        part,
        declared,
        source_pages,
        pages,
        &messages,
        profile.is_none(),
    ));
    if !messages.is_empty() {
        response.warnings = Some(messages);
    }

    Ok((produced, response))
}

fn validate_reference(pdf: &str) -> Result<(), AppError> {
    if pdf.trim().is_empty() {
        return Err(AppError::BadRequest(
            "\"pdf\" must be an asset reference (asset://as_...) or a /download/<client_id>/<name>.pdf path".to_string(),
        ));
    }
    Ok(())
}

/// The three conformance levels this service produces, normalised so that `PDF/A-2B` and
/// `pdfa-2b` name the same thing.
///
/// Level B only — level A demands a tagged reading order that Ghostscript cannot invent
/// from an untagged input, and declaring a level we cannot produce would be worse than
/// refusing it.
fn parse_variant(variant: Option<&str>) -> Result<u8, AppError> {
    let requested = variant.unwrap_or(DEFAULT_VARIANT);
    let normalised: String = requested
        .to_ascii_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '/')
        .collect();

    match normalised.as_str() {
        "pdfa-1b" => Ok(1),
        "pdfa-2b" => Ok(2),
        "pdfa-3b" => Ok(3),
        _ => Err(AppError::BadRequest(format!(
            "\"variant\" must be one of pdf/a-1b, pdf/a-2b, pdf/a-3b (got \"{}\")",
            requested
        ))),
    }
}

fn label(part: u8) -> String {
    format!("PDF/A-{}B", part)
}

/// The sRGB profile shipped with Ghostscript, wherever this image put it. Debian nests it
/// under the version directory, so the exact path changes with every upgrade of the
/// package and hardcoding it would break silently — as a file with no output intent.
fn srgb_profile() -> Option<PathBuf> {
    let packaged = PathBuf::from("/usr/share/color/icc/ghostscript/srgb.icc");
    if packaged.is_file() {
        return Some(packaged);
    }

    fs::read_dir("/usr/share/ghostscript")
        .ok()?
        .flatten()
        .map(|entry| entry.path().join("iccprofiles").join("srgb.icc"))
        .find(|path| path.is_file())
}

fn write_definition(icc: &Path) -> Result<TempPath, AppError> {
    let mut file = Builder::new().suffix(".ps").tempfile()?;
    file.write_all(definition_source(helpers::path_to_str(icc)?).as_bytes())?;
    Ok(file.into_temp_path())
}

/// The PDF/A definition file Ghostscript requires, reduced to the one thing it cannot infer
/// from the input: the output intent, that is, which colour space the file's device colours
/// are to be read in.
///
/// Two deliberate departures from the sample that ships with Ghostscript. `/N 3` is stated
/// rather than derived: the profile below is sRGB, three components, and the sample's
/// derivation prints a scary warning as soon as `ColorConversionStrategy` is not a device
/// space — which is precisely our case. And no `/DOCINFO` block: the sample writes a
/// placeholder `Title` that would overwrite the one the document already carries.
fn definition_source(icc: &str) -> String {
    format!(
        "%!\n\
         % Output intent for PDF/A, generated by md-to-pdf\n\
         /ICCProfile ({icc}) def\n\
         [/_objdef {{icc_PDFA}} /type /stream /OBJ pdfmark\n\
         [{{icc_PDFA}} << /N 3 >> /PUT pdfmark\n\
         [{{icc_PDFA}} ICCProfile (r) file /PUT pdfmark\n\
         [/_objdef {{OutputIntent_PDFA}} /type /dict /OBJ pdfmark\n\
         [{{OutputIntent_PDFA}} <<\n\
         \x20 /Type /OutputIntent\n\
         \x20 /S /GTS_PDFA1\n\
         \x20 /DestOutputProfile {{icc_PDFA}}\n\
         \x20 /OutputConditionIdentifier (sRGB)\n\
         >> /PUT pdfmark\n\
         [{{Catalog}} <</OutputIntents [ {{OutputIntent_PDFA}} ]>> /PUT pdfmark\n",
        icc = ps_string(icc)
    )
}

/// A PostScript string ends at its first unbalanced `)`. The profile path comes from the
/// filesystem, not from the caller, but a directory named `sRGB (2024)` would still break
/// the definition file in a way nobody would think to look for.
fn ps_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for c in value.chars() {
        if matches!(c, '(' | ')' | '\\') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// What Ghostscript said about conformance, as sentences a human can read.
///
/// It spreads one message over two lines — what it was writing, then what the standard
/// forbids — and mixes them with its banner and a page counter. Joining on the trailing
/// comma is what keeps `Annotation set to non-printing, not permitted in PDF/A` one
/// sentence instead of two useless halves.
fn compliance_messages(streams: &[&str]) -> Vec<String> {
    let mut messages: Vec<String> = Vec::new();

    for stream in streams {
        for line in join_continuations(stream) {
            let message = strip_tool_prefix(&line);
            if !is_compliance(&message) || messages.iter().any(|kept| kept == &message) {
                continue;
            }
            messages.push(message);
        }
    }

    if messages.len() > MAX_MESSAGES {
        let hidden = messages.len() - MAX_MESSAGES;
        messages.truncate(MAX_MESSAGES);
        messages.push(format!(
            "{} further conformance warnings, not detailed",
            hidden
        ));
    }

    messages
}

fn join_continuations(stream: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut pending = String::new();

    for line in stream.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if !pending.is_empty() {
            pending.push(' ');
        }
        pending.push_str(line);
        if !pending.ends_with(',') {
            lines.push(std::mem::take(&mut pending));
        }
    }

    if !pending.is_empty() {
        lines.push(pending);
    }

    lines
}

/// Ghostscript names and versions itself in front of every message it emits — noise in a
/// verdict, and a version string that would make two identical warnings look different.
fn strip_tool_prefix(line: &str) -> String {
    line.strip_prefix("GPL Ghostscript")
        .and_then(|rest| rest.split_once(": "))
        .map(|(_, message)| message.trim().to_string())
        .unwrap_or_else(|| line.to_string())
}

fn is_compliance(message: &str) -> bool {
    const SIGNALS: [&str; 6] = [
        "PDF/A",
        "Warning",
        "Error",
        "Substituting font",
        "not embedded",
        "Can't find",
    ];

    SIGNALS.iter().any(|signal| message.contains(signal))
}

/// Which PDF/A part the produced file claims to be. `None` means it claims nothing, which
/// is the one conformance failure detectable without a validator: Ghostscript wrote a plain
/// PDF and the caller was about to archive it as an archival one.
fn declared_part(pdf: &Path) -> Result<Option<u8>, AppError> {
    Ok(scan_part(fs::File::open(pdf)?, SCAN_CHUNK)?)
}

fn scan_part<R: Read>(mut reader: R, chunk: usize) -> std::io::Result<Option<u8>> {
    // Enough to hold a marker split across two reads, plus the quoted digit behind it
    let overlap = MARKER.len() + 4;
    let mut window: Vec<u8> = Vec::new();
    let mut buffer = vec![0u8; chunk];

    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(None);
        }

        window.extend_from_slice(&buffer[..read]);
        if let Some(part) = part_in(&window) {
            return Ok(Some(part));
        }

        let drop = window.len().saturating_sub(overlap);
        window.drain(..drop);
    }
}

fn part_in(bytes: &[u8]) -> Option<u8> {
    let at = bytes.windows(MARKER.len()).position(|w| w == MARKER)?;

    bytes[at + MARKER.len()..]
        .iter()
        .take(3)
        .find(|byte| byte.is_ascii_digit())
        .map(|byte| byte - b'0')
}

/// Check names are the translatable contract: `page-count` is the same name, on the same
/// question, as in `/api/compress`, `/api/crop` and `/api/repair`; `pdfa-marker`,
/// `output-intent` and `pdfa-conformance` belong to conformance work. The public site keys
/// its wording on the name, so only the English `detail` is written here.
fn build_verdict(
    part: u8,
    declared: Option<u8>,
    before: usize,
    after: usize,
    messages: &[String],
    missing_profile: bool,
) -> Verdict {
    let mut checks = Vec::new();

    checks.push(if after == before {
        Check::ok("page-count", format!("{} pages, unchanged", before))
    } else {
        Check::fail(
            "page-count",
            format!(
                "{} pages in, {} out: the conversion lost pages",
                before, after
            ),
        )
    });

    checks.push(match declared {
        Some(found) if found == part => Check::ok(
            "pdfa-marker",
            format!(
                "{} declared in the XMP metadata of the produced file",
                label(part)
            ),
        ),
        Some(found) => Check::warn(
            "pdfa-marker",
            format!(
                "{} asked for, {} declared by Ghostscript",
                label(part),
                label(found)
            ),
        ),
        None => Check::fail(
            "pdfa-marker",
            "the produced file carries no PDF/A identification: Ghostscript fell back to an ordinary PDF",
        ),
    });

    if missing_profile {
        checks.push(Check::warn(
            "output-intent",
            "no sRGB ICC profile found on this server: the file is marked PDF/A but has no OutputIntent, a validator will reject it",
        ));
    }

    if messages.is_empty() {
        checks.push(Check::ok(
            "pdfa-conformance",
            "Ghostscript reported no conformance adjustment",
        ));
    } else {
        for message in messages {
            checks.push(if message.contains(REVERTED) {
                Check::fail("pdfa-conformance", message.clone())
            } else {
                Check::warn("pdfa-conformance", message.clone())
            });
        }
    }

    Verdict::from_checks(summary(part, after, messages.len()), checks)
}

/// The summary says what was produced and, deliberately, what was not verified: this
/// service has no PDF/A validator (veraPDF is not installed), so it can promise a file
/// Ghostscript declares conforming, never a file proven conforming.
fn summary(part: u8, pages: usize, adjustments: usize) -> String {
    let adjustments = match adjustments {
        0 => "no adjustment".to_string(),
        1 => "1 conformance adjustment".to_string(),
        count => format!("{} conformance adjustments", count),
    };

    format!(
        "{} produced over {}, {} — file declared conforming by Ghostscript, not validated: this service runs no PDF/A validator",
        label(part),
        plural(pages, "page"),
        adjustments
    )
}

fn plural(count: usize, word: &str) -> String {
    if count > 1 {
        format!("{} {}s", count, word)
    } else {
        format!("{} {}", count, word)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check<'a>(verdict: &'a Verdict, name: &str) -> &'a Check {
        verdict
            .checks
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no {} check in {:?}", name, verdict.checks))
    }

    #[test]
    fn the_default_variant_is_pdf_a_2b_and_the_three_levels_map_to_ghostscript_parts() {
        assert_eq!(parse_variant(None).unwrap(), 2);
        assert_eq!(parse_variant(Some("pdf/a-1b")).unwrap(), 1);
        assert_eq!(parse_variant(Some("pdf/a-2b")).unwrap(), 2);
        assert_eq!(parse_variant(Some("pdf/a-3b")).unwrap(), 3);
    }

    #[test]
    fn the_variant_is_read_whatever_the_case_and_the_slash() {
        assert_eq!(parse_variant(Some("PDF/A-3B")).unwrap(), 3);
        assert_eq!(parse_variant(Some("pdfa-1b")).unwrap(), 1);
        assert_eq!(parse_variant(Some(" pdf/a-2b ")).unwrap(), 2);
    }

    #[test]
    fn an_unsupported_variant_is_refused_with_the_list_of_variants() {
        for variant in ["pdf/a-2a", "pdf/a-4", "pdfa", ""] {
            let error = parse_variant(Some(variant)).unwrap_err();
            match error {
                AppError::BadRequest(message) => {
                    assert!(message.contains("pdf/a-1b"), "{}", message);
                    assert!(message.contains("pdf/a-3b"), "{}", message);
                }
                other => panic!("expected a bad request, got {:?}", other),
            }
        }
    }

    #[test]
    fn an_empty_reference_is_refused_before_any_work() {
        assert!(validate_reference("   ").is_err());
        assert!(validate_reference("asset://as_00").is_ok());
    }

    #[test]
    fn a_two_line_ghostscript_warning_comes_back_as_one_sentence() {
        let stderr = "GPL Ghostscript 10.00.0: Annotation set to non-printing,\n \
                      not permitted in PDF/A, annotation will not be present in output file\n";

        let messages = compliance_messages(&[stderr]);

        assert_eq!(
            messages,
            vec![
                "Annotation set to non-printing, not permitted in PDF/A, annotation will not be present in output file"
            ]
        );
    }

    #[test]
    fn the_banner_and_the_page_counter_are_not_conformance_messages() {
        let stdout = "GPL Ghostscript 10.0.0 (2022-09-21)\n\
                      Copyright (C) 2022 Artifex Software, Inc.  All rights reserved.\n\
                      This software is supplied under the GNU AGPLv3 and comes with NO WARRANTY:\n\
                      see the file COPYING for details.\n\
                      Processing pages 1 through 3.\n\
                      Page 1\nPage 2\nPage 3\n";

        assert!(compliance_messages(&[stdout]).is_empty());
    }

    #[test]
    fn the_same_warning_on_both_streams_is_reported_once() {
        let line = "Type 3 font not permitted in PDF/A, reverting to normal PDF output\n";

        assert_eq!(compliance_messages(&[line, line]).len(), 1);
    }

    #[test]
    fn a_flood_of_warnings_is_capped_and_says_how_many_were_hidden() {
        let stream: String = (0..30)
            .map(|page| format!("Image {} not permitted in PDF/A, values not set\n", page))
            .collect();

        let messages = compliance_messages(&[&stream]);

        assert_eq!(messages.len(), MAX_MESSAGES + 1);
        assert_eq!(
            messages.last().unwrap(),
            "18 further conformance warnings, not detailed"
        );
    }

    #[test]
    fn a_clean_conversion_scores_full_and_still_says_it_was_not_validated() {
        let verdict = build_verdict(2, Some(2), 12, 12, &[], false);

        assert_eq!(verdict.status, "ok");
        assert_eq!(verdict.score, 100);
        assert!(verdict.summary.contains("PDF/A-2B"), "{}", verdict.summary);
        assert!(
            verdict.summary.contains("not validated"),
            "{}",
            verdict.summary
        );
        assert_eq!(check(&verdict, "pdfa-conformance").status, "ok");
    }

    #[test]
    fn an_adjusted_conversion_warns_without_failing() {
        let messages = vec!["Annotation set to non-printing, not permitted in PDF/A, annotation will not be present in output file".to_string()];
        let verdict = build_verdict(2, Some(2), 4, 4, &messages, false);

        assert_eq!(verdict.status, "warn");
        assert_eq!(check(&verdict, "pdfa-conformance").status, "warn");
        assert!(
            verdict.summary.contains("1 conformance adjustment"),
            "{}",
            verdict.summary
        );
    }

    #[test]
    fn a_conversion_that_reverted_to_a_plain_pdf_fails_the_verdict() {
        let messages =
            vec!["Type 3 font not permitted in PDF/A, reverting to normal PDF output".to_string()];
        let verdict = build_verdict(1, None, 4, 4, &messages, false);

        assert_eq!(check(&verdict, "pdfa-marker").status, "fail");
        assert_eq!(check(&verdict, "pdfa-conformance").status, "fail");
        assert_eq!(verdict.status, "fail");
    }

    #[test]
    fn a_file_marked_with_another_part_than_the_one_requested_warns() {
        let verdict = build_verdict(3, Some(2), 2, 2, &[], false);

        let marker = check(&verdict, "pdfa-marker");
        assert_eq!(marker.status, "warn");
        assert!(marker.detail.contains("PDF/A-3B"), "{}", marker.detail);
        assert!(marker.detail.contains("PDF/A-2B"), "{}", marker.detail);
    }

    #[test]
    fn losing_pages_fails_the_verdict() {
        let verdict = build_verdict(2, Some(2), 12, 9, &[], false);

        assert_eq!(check(&verdict, "page-count").status, "fail");
        assert_eq!(verdict.status, "fail");
    }

    #[test]
    fn a_missing_icc_profile_is_announced_as_a_file_a_validator_will_refuse() {
        let verdict = build_verdict(2, Some(2), 3, 3, &[], true);

        let intent = check(&verdict, "output-intent");
        assert_eq!(intent.status, "warn");
        assert!(intent.detail.contains("OutputIntent"), "{}", intent.detail);
    }

    #[test]
    fn the_declared_part_is_read_from_the_xmp_packet() {
        assert_eq!(
            part_in(b"<rdf:Description pdfaid:part='2' pdfaid:conformance='B'/>"),
            Some(2)
        );
        assert_eq!(part_in(b"pdfaid:part=\"3\""), Some(3));
        assert_eq!(part_in(b"a plain pdf without any identification"), None);
        assert_eq!(part_in(b""), None);
    }

    #[test]
    fn the_marker_is_found_even_when_it_straddles_two_reads() {
        let mut bytes = vec![b'x'; 1000];
        bytes.extend_from_slice(b"pdfaid:part='1'");
        bytes.extend(vec![b'y'; 1000]);

        for chunk in [1, 7, 64, 1001, 4096] {
            assert_eq!(
                scan_part(bytes.as_slice(), chunk).unwrap(),
                Some(1),
                "chunk {}",
                chunk
            );
        }

        assert_eq!(scan_part(&b"nothing here"[..], 4).unwrap(), None);
    }

    #[test]
    fn the_definition_file_declares_the_output_intent_and_nothing_else() {
        let source = definition_source("/usr/share/ghostscript/10.00.0/iccprofiles/srgb.icc");

        assert!(source.starts_with("%!"));
        assert!(source.contains("/S /GTS_PDFA1"));
        assert!(source.contains("/OutputIntents"));
        assert!(source.contains("/N 3"));
        // A title of our own would replace the one the document carries
        assert!(!source.contains("DOCINFO"));
    }

    #[test]
    fn a_profile_path_with_parentheses_cannot_close_the_postscript_string() {
        assert_eq!(
            ps_string("/icc/sRGB (2024)/srgb.icc"),
            "/icc/sRGB \\(2024\\)/srgb.icc"
        );
        assert_eq!(ps_string("/a\\b"), "/a\\\\b");
        assert!(definition_source("/icc/(v2)/srgb.icc").contains("(/icc/\\(v2\\)/srgb.icc)"));
    }
}

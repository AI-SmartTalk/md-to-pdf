//! Getting a document back: rebuilding what a damaged file still holds, and removing an
//! encryption whose password the caller already has.

use crate::auth::PublicOrKey;
use crate::exec;
use crate::helpers;
use crate::types::*;
use rocket::fs::NamedFile;
use rocket::serde::json::Json;
use rocket::Either;
use serde::Deserialize;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use tempfile::{Builder, TempPath};

// ------------ repair ------------

#[derive(Deserialize)]
pub struct RepairRequest {
    pub pdf: String,
    pub client_id: Option<String>,
    pub pdf_name: Option<String>,
    pub output: Option<ToolOutput>,
}

/// Which pass produced the file the caller gets back
#[derive(Debug, Clone, Copy, PartialEq)]
enum Method {
    /// Cross-reference tables rebuilt, objects untouched
    Qpdf,
    /// Document decoded and written again from scratch
    Ghostscript,
}

impl Method {
    fn as_str(self) -> &'static str {
        match self {
            Method::Qpdf => "qpdf",
            Method::Ghostscript => "ghostscript",
        }
    }
}

/// What a repair pass produced, once it has cleared every bar
struct Pass {
    file: TempPath,
    pages: usize,
    /// What the tool complained about while still doing its job
    warnings: Vec<String>,
}

/// A pass either yields a file we are willing to hand back, or a sentence saying why it does
/// not. There is no third answer: a pass that "succeeded" without producing a document the
/// caller can use is the failure this module exists to avoid.
enum Outcome {
    Recovered(Pass),
    Rejected(String),
}

/// What poppler can still pull out of a document. `None`, in the probe below, means poppler
/// could not read the file at all — a different thing from an empty document, and the
/// distinction is exactly what tells a blank page from a vector drawing.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Content {
    text: bool,
    images: bool,
}

impl Content {
    fn is_empty(self) -> bool {
        !self.text && !self.images
    }
}

/// What ghostscript says when it never opened the file, on a run it still reports as a
/// success: the first three go to **stdout** with exit code 0, right before it writes a blank
/// A4 page, and the fourth to stderr.
///
/// Nothing qpdf says belongs here: it exits non-zero when it gives up, and its warnings are
/// the normal sound of a damaged file being recovered — a marker of ours would only reject
/// repairs that work.
const FATAL_MARKERS: [&str; 5] = [
    "couldn't initialise file",
    "couldn't initialize file",
    "no pages will be processed",
    "requires a password for access",
    "unrecoverable error",
];

#[post("/repair", format = "json", data = "<req>")]
pub async fn repair(
    key: PublicOrKey,
    req: Json<RepairRequest>,
) -> Result<Either<NamedFile, Json<ToolResponse>>, AppError> {
    let req = req.into_inner();
    validate_reference(&req.pdf)?;

    let (produced, response) = exec::as_owner(key.0, exec::offload(move || run(req))).await?;
    helpers::deliver_tool(produced, response, "repair").await
}

/// All the blocking work. Public and free of Rocket: the asynchronous job dispatcher calls
/// it as is.
pub fn run(req: RepairRequest) -> Result<(TempPath, ToolResponse), AppError> {
    validate_reference(&req.pdf)?;
    let source = helpers::resolve_pdf_source(&req.pdf)?;

    // Whatever pdfinfo can still read of the damaged file: the only baseline we have to
    // tell a recovery from a silent truncation. A file too broken to answer gets None.
    let original_pages = crate::pdfops::page_count(&source).ok();
    // The second baseline, and the one that catches the blank page: what the source still
    // held before we touched it.
    let original_content = content_probe(&source);

    let (pass, method) = match rebuild(&source, original_content)? {
        Outcome::Recovered(pass) => (pass, Method::Qpdf),
        Outcome::Rejected(first) => match rewrite(&source, original_content)? {
            Outcome::Recovered(pass) => (pass, Method::Ghostscript),
            // An empty PDF would be worse than an error: the caller would archive it and
            // discover the loss the day they need the document.
            Outcome::Rejected(second) => return Err(unrepairable(&source, &first, &second)?),
        },
    };

    let Pass {
        file: produced,
        pages,
        warnings: tool_warnings,
    } = pass;

    let mut response = helpers::finish_tool(
        &produced,
        req.client_id,
        req.pdf_name,
        req.output.unwrap_or_default(),
        "repaired.pdf",
    )?;
    response.pages = Some(pages);
    response.verdict = Some(repair_verdict(
        method,
        pages,
        original_pages,
        &tool_warnings,
    ));
    response.warnings = repair_warnings(method, &tool_warnings);

    Ok((produced, response))
}

/// The 400 both passes earned. An encrypted document is the one failure with an obvious next
/// step, so it gets named: neither engine can open it, and no amount of repair ever will.
fn unrepairable(source: &Path, first: &str, second: &str) -> Result<AppError, AppError> {
    let mut message = format!("This file could not be repaired: {}, and {}", first, second);

    if encryption_state(source)? == Encryption::Present {
        message.push_str(
            ". This document is encrypted: remove the protection with /api/unlock — you need its password — then repair the result",
        );
    }

    Ok(AppError::BadRequest(message))
}

/// Pass one: rebuild the cross-reference tables, keep every object as it was written.
///
/// `--warning-exit-0` is the convention shared with `/api/pages`: qpdf exits 3 on a document
/// it recovered from, which is the normal answer here, and the warning is reported rather
/// than turned into a failure or silently dropped.
fn rebuild(source: &Path, original: Option<Content>) -> Result<Outcome, AppError> {
    let out = Builder::new().suffix(".pdf").tempfile()?.into_temp_path();
    let out_arg = helpers::path_to_str(&out)?.to_string();

    attempt(
        Command::new("qpdf")
            .arg("--warning-exit-0")
            .arg("--qdf")
            .arg("--object-streams=disable")
            .arg(helpers::path_to_str(source)?)
            .arg(&out_arg),
        "qpdf",
        out,
        original,
    )
}

/// Pass two: decode the document and write a new one. It costs the structure — annotations,
/// form fields, attachments — and it reads files qpdf gives up on.
fn rewrite(source: &Path, original: Option<Content>) -> Result<Outcome, AppError> {
    let out = Builder::new().suffix(".pdf").tempfile()?.into_temp_path();
    let out_arg = format!("-sOutputFile={}", helpers::path_to_str(&out)?);

    attempt(
        Command::new("gs")
            .arg("-dSAFER")
            .arg("-dBATCH")
            .arg("-dNOPAUSE")
            .arg("-dNOOUTERSAVE")
            .arg("-sDEVICE=pdfwrite")
            .arg(&out_arg)
            .arg(helpers::path_to_str(source)?),
        "gs",
        out,
        original,
    )
}

/// A repair pass cannot be judged on its exit code alone, in either direction. Non-zero
/// means little: both tools complain as soon as they had anything to say, and on a damaged
/// file they always do — next to that complaint sits a file that is often perfectly
/// readable. Zero means even less: ghostscript answers **0** on a file it never managed to
/// open, after printing `Couldn't initialise file` on stdout and writing a blank A4 page.
///
/// So a pass has to clear three bars: no fatal message on either stream, at least one page,
/// and content that did not vanish between the source and the output.
fn attempt(
    cmd: &mut Command,
    label: &str,
    out: TempPath,
    original: Option<Content>,
) -> Result<Outcome, AppError> {
    let mut streams = String::new();
    let mut warnings = Vec::new();

    match helpers::run_capture(cmd, label, "PDF repair pass failed") {
        Ok(output) => {
            // Ghostscript reports its refusals on stdout, so both streams are read
            streams.push_str(&String::from_utf8_lossy(&output.stdout));
            streams.push('\n');
            streams.push_str(&String::from_utf8_lossy(&output.stderr));
        }
        // The time budget is the one failure the next pass must not inherit
        Err(err @ AppError::Timeout(_)) => return Err(err),
        Err(AppError::ProcessFailed { stderr, .. }) => streams.push_str(&stderr),
        // A tool that could not even be spawned leaves the next pass its chance
        Err(_) => return Ok(Outcome::Rejected(format!("{} could not be run", label))),
    }

    if let Some(warning) = tool_warning(&streams) {
        warnings.push(format!("{}: {}", label, warning));
    }

    if let Some(marker) = fatal_marker(&streams) {
        return Ok(Outcome::Rejected(format!(
            "{} never opened this document ({})",
            label, marker
        )));
    }

    let pages = match crate::pdfops::page_count(&out) {
        Ok(pages) if pages > 0 => pages,
        Err(err @ AppError::Timeout(_)) => return Err(err),
        _ => {
            return Ok(Outcome::Rejected(format!(
                "{} produced no readable page",
                label
            )))
        }
    };

    if let Some(loss) = content_loss(original, content_probe(&out)) {
        return Ok(Outcome::Rejected(format!("{} {}", label, loss)));
    }

    Ok(Outcome::Recovered(Pass {
        file: out,
        pages,
        warnings,
    }))
}

/// The first line of the streams that reads as a warning rather than as progress: qpdf
/// prefixes its own with `WARNING:`, ghostscript with `**** Warning`. Everything else on
/// these streams is a banner nobody needs to see.
fn tool_warning(streams: &str) -> Option<String> {
    streams
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("WARNING:") || line.contains("**** Warning"))
        .map(without_paths)
}

/// Both engines prefix their messages with the path they were given — a temporary file or an
/// asset directory, which tells the caller nothing and tells everyone else where the service
/// keeps its files. The sentence is what we owe the caller; the path is not ours to publish.
fn without_paths(line: &str) -> String {
    let kept: Vec<&str> = line
        .split(": ")
        .filter(|part| !part.contains('/'))
        .collect();

    // A line made of nothing but paths carries no message worth relaying
    if kept.is_empty() {
        return "no detail".to_string();
    }

    kept.join(": ")
}

/// A message that means the engine never read the document, whatever its exit code says
fn fatal_marker(streams: &str) -> Option<&'static str> {
    let lowered = streams.to_lowercase();
    FATAL_MARKERS
        .into_iter()
        .find(|marker| lowered.contains(marker))
}

/// What poppler can still pull out of a file: any text, any embedded image. `None` when it
/// could not read the file at all — an encrypted or truncated source answers that way, and
/// "unknown" has to stay distinct from "empty" for the comparison below to mean anything.
fn content_probe(pdf: &Path) -> Option<Content> {
    let path = helpers::path_to_str(pdf).ok()?;

    let text = helpers::run_capture(
        Command::new("pdftotext").arg("-q").arg(path).arg("-"),
        "pdftotext",
        "pdftotext failed",
    )
    .ok()?;

    let images = helpers::run_capture(
        Command::new("pdfimages").arg("-list").arg(path),
        "pdfimages",
        "pdfimages failed",
    )
    .ok();

    Some(Content {
        text: String::from_utf8_lossy(&text.stdout)
            .chars()
            .any(|c| !c.is_whitespace()),
        images: images
            .is_some_and(|out| count_listed_images(&String::from_utf8_lossy(&out.stdout)) > 0),
    })
}

/// `pdfimages -list` prints two header lines, then one line per image starting with its page
/// number. Counting those lines is enough: we only ever ask whether there is one.
fn count_listed_images(listing: &str) -> usize {
    listing
        .lines()
        .filter(|line| {
            line.split_whitespace()
                .next()
                .is_some_and(|field| field.parse::<usize>().is_ok())
        })
        .count()
}

/// The comparison that separates a repair from a loss, and the answer to the blank page
/// ghostscript hands back with exit code 0.
fn content_loss(source: Option<Content>, produced: Option<Content>) -> Option<&'static str> {
    let Some(produced) = produced else {
        // A file pdfinfo counts pages in but pdftotext cannot open is not a repaired document
        return Some("produced a file that could not be read back");
    };

    if !produced.is_empty() {
        return None;
    }

    match source {
        // The source carried something and the output carries nothing: that is the loss
        Some(source) if !source.is_empty() => {
            Some("produced a document with no text and no image, out of a source that had some")
        }
        // Both empty: a vector-only document loses nothing by staying vector-only
        Some(_) => None,
        // We never managed to read the source, and what came out is blank. That is the exact
        // shape of a pass over a file the engine never opened, not the shape of a recovery.
        None => {
            Some("produced a blank document, and the source could never be read to compare it with")
        }
    }
}

/// Check names are the translatable contract, kebab-case and shared across the tools:
/// `page-count` means here what it means in `/api/compress` and `/api/crop`, and
/// `recovery-method` is the name any tool that had a choice of engine reports under.
fn repair_verdict(
    method: Method,
    recovered: usize,
    original: Option<usize>,
    tool_warnings: &[String],
) -> Verdict {
    let structure = match method {
        Method::Qpdf => Check::ok(
            "recovery-method",
            "qpdf rebuilt the cross-reference tables, the objects are the original ones",
        ),
        Method::Ghostscript => Check::warn(
            "recovery-method",
            "qpdf could not read this file: ghostscript rewrote it, so annotations, form fields and attachments may not have survived",
        ),
    };

    let pages = match original {
        Some(original) if recovered < original => Check::warn(
            "page-count",
            format!("{} of the {} pages were recovered", recovered, original),
        ),
        Some(original) => Check::ok(
            "page-count",
            format!(
                "{} pages recovered, none lost ({} before)",
                recovered, original
            ),
        ),
        None => Check::ok(
            "page-count",
            format!(
                "{} pages recovered, the damaged file reported no page count to compare with",
                recovered
            ),
        ),
    };

    let mut checks = vec![structure, pages];

    // `tool-warning` is the name every route uses for a tolerated warning from the binary it
    // drove: the same one `/api/unlock` and `/api/ocr` report theirs under.
    for warning in tool_warnings {
        checks.push(Check::warn("tool-warning", warning.clone()));
    }

    Verdict::from_checks(
        format!("{} pages recovered by {}", recovered, method.as_str()),
        checks,
    )
}

fn repair_warnings(method: Method, tool_warnings: &[String]) -> Option<Vec<String>> {
    let mut warnings = Vec::new();

    // The caller has to know the difference: a rewritten document is a new document
    if method == Method::Ghostscript {
        warnings.push(
            "method=ghostscript: this document was rewritten, not repaired — qpdf could not read it".to_string(),
        );
    }

    warnings.extend(tool_warnings.iter().cloned());

    (!warnings.is_empty()).then_some(warnings)
}

// ------------ encryption ------------

/// What `qpdf --show-encryption` says about a file before anything is done to it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Encryption {
    /// qpdf read the file and found no encryption dictionary
    None,
    /// The file is encrypted, whether or not we hold the password
    Present,
    /// qpdf could not open the file: it says nothing about encryption, so neither do we
    Unknown,
}

/// Ask qpdf, and only qpdf: `--decrypt` exits 0 and stays silent on a file that carried no
/// protection at all, so the pass itself can never tell us whether there was one to remove.
fn encryption_state(pdf: &Path) -> Result<Encryption, AppError> {
    let path = helpers::path_to_str(pdf)?;

    match helpers::run_capture(
        Command::new("qpdf").arg("--show-encryption").arg(path),
        "qpdf",
        "Reading the encryption state failed",
    ) {
        Ok(output) => Ok(parse_encryption(&String::from_utf8_lossy(&output.stdout))),
        Err(err @ AppError::Timeout(_)) => Err(err),
        Err(_) => Ok(Encryption::Unknown),
    }
}

/// qpdf answers `File is not encrypted` on a plain file; on an encrypted one it lists the
/// permissions and the algorithms, prefixed with `Incorrect password supplied` when it was
/// not given the password. Anything else is a shape we do not know, and unknown it stays.
fn parse_encryption(report: &str) -> Encryption {
    let lowered = report.to_lowercase();

    if lowered.contains("file is not encrypted") {
        return Encryption::None;
    }

    let encrypted = lowered.lines().map(str::trim).any(|line| {
        line.starts_with("r = ")
            || line.contains("encryption method")
            || line.contains("incorrect password supplied")
    });

    if encrypted {
        Encryption::Present
    } else {
        Encryption::Unknown
    }
}

// ------------ unlock ------------

#[derive(Deserialize)]
pub struct UnlockRequest {
    pub pdf: String,
    /// Optional in the type only, so that a missing password gets our own 400 rather than
    /// a deserialization error nobody can act on
    pub password: Option<String>,
    pub client_id: Option<String>,
    pub pdf_name: Option<String>,
    pub output: Option<ToolOutput>,
}

#[post("/unlock", format = "json", data = "<req>")]
pub async fn unlock(
    key: PublicOrKey,
    req: Json<UnlockRequest>,
) -> Result<Either<NamedFile, Json<ToolResponse>>, AppError> {
    let req = req.into_inner();
    validate_reference(&req.pdf)?;
    validate_password(req.password.as_deref())?;

    let (produced, response) =
        exec::as_owner(key.0, exec::offload(move || run_unlock(req))).await?;
    helpers::deliver_tool(produced, response, "unlock").await
}

/// Same contract as `run`, for the second tool of this file.
pub fn run_unlock(req: UnlockRequest) -> Result<(TempPath, ToolResponse), AppError> {
    validate_reference(&req.pdf)?;
    let password = validate_password(req.password.as_deref())?;
    let source = helpers::resolve_pdf_source(&req.pdf)?;

    // Read before the pass, because after it there is nothing left to read: `--decrypt`
    // exits 0 and says nothing on a document that carried no protection, so this is the only
    // moment where the difference between "removed" and "there was nothing to remove"
    // exists.
    let source_encryption = encryption_state(&source)?;

    let out_temp = Builder::new().suffix(".pdf").tempfile()?;
    let out_arg = helpers::path_to_str(out_temp.path())?.to_string();

    // qpdf reads its arguments from an @argfile, one per line: the password never shows up
    // in the process list, where every other user of the host could read it.
    let mut arg_file = Builder::new().suffix(".qpdf-args").tempfile()?;
    arg_file
        .write_all(decrypt_args(password, helpers::path_to_str(&source)?, &out_arg).as_bytes())?;
    let arg_file_path = arg_file.into_temp_path();

    let mut warnings = Vec::new();
    match helpers::run_capture(
        Command::new("qpdf").arg(format!("@{}", helpers::path_to_str(&arg_file_path)?)),
        "qpdf",
        "PDF decryption failed",
    ) {
        // `--warning-exit-0` (in the argfile, same convention as `/api/pages`) keeps a mere
        // warning from reading as a failure, so the warning has to be picked up here
        Ok(output) => {
            if let Some(warning) = tool_warning(&String::from_utf8_lossy(&output.stderr)) {
                warnings.push(format!("qpdf finished with a warning: {}", warning));
            }
        }
        Err(err) => {
            // What settles a hard exit is whether a readable PDF came out anyway
            if !readable(out_temp.path()) {
                return Err(decryption_error(err));
            }
            warnings.push(qpdf_warning(&err));
        }
    }

    let produced = out_temp.into_temp_path();

    // qpdf can write a file out of a source whose page tree was already broken: it exits 0,
    // and what lands here has no readable page. Everywhere else in this service an
    // unreadable *output* is our bug and stays a 500 — here the input was, by design, a
    // document nothing else would accept, so the fault is diagnosable and belongs to it.
    let pages = match crate::pdfops::page_count(&produced) {
        Ok(pages) => pages,
        Err(err @ AppError::Timeout(_)) => return Err(err),
        Err(_) => {
            return Err(AppError::BadRequest(
                "The protection could be removed, but the result has no readable page: this \
                 document was already damaged underneath its encryption. POST /api/repair \
                 rebuilds what can be rebuilt and tells you what it recovered."
                    .to_string(),
            ))
        }
    };
    // The claim this endpoint makes is "no longer protected". Reading the output back is
    // what turns that claim into an observation.
    let produced_encryption = encryption_state(&produced)?;

    let mut response = helpers::finish_tool(
        &produced,
        req.client_id,
        req.pdf_name,
        req.output.unwrap_or_default(),
        "unlocked.pdf",
    )?;
    response.pages = Some(pages);
    response.verdict = Some(unlock_verdict(
        pages,
        &warnings,
        source_encryption,
        produced_encryption,
    ));
    if !warnings.is_empty() {
        response.warnings = Some(warnings);
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

/// This endpoint removes a protection whose key the caller already holds, and only that.
/// It never searches for one. Refusing an empty password is a product decision, not an
/// oversight: it is the line between a legitimate tool and a circumvention tool, and every
/// reviewer of this file should see that it was drawn deliberately.
fn validate_password(password: Option<&str>) -> Result<&str, AppError> {
    let password = password.unwrap_or_default();

    if password.is_empty() {
        return Err(AppError::BadRequest(
            "\"password\" is required: this endpoint removes a protection whose password you already hold, it never recovers or breaks one".to_string(),
        ));
    }

    // One argument per line in the argfile: a line break would be an argument injection
    if password.contains(['\n', '\r']) {
        return Err(AppError::BadRequest(
            "\"password\" must not contain line breaks".to_string(),
        ));
    }

    Ok(password)
}

fn decrypt_args(password: &str, input: &str, output: &str) -> String {
    format!(
        "--warning-exit-0\n--password={}\n--decrypt\n{}\n{}\n",
        password, input, output
    )
}

fn readable(pdf: &Path) -> bool {
    matches!(crate::pdfops::page_count(pdf), Ok(pages) if pages > 0)
}

/// A wrong password is a client mistake, not a tool breakdown: it deserves a 400 saying so
/// rather than a qpdf stderr dump three layers down.
fn decryption_error(err: AppError) -> AppError {
    if let AppError::ProcessFailed { stderr, .. } = &err {
        let lowered = stderr.to_lowercase();

        if lowered.contains("invalid password") {
            return AppError::BadRequest(
                "The supplied password does not open this document".to_string(),
            );
        }

        // This endpoint is one of the two that deliberately accept a document nothing else
        // can open, so it cannot lean on `resolve_readable_pdf`. It still owes the caller the
        // same distinction: a file qpdf cannot parse is damaged, not password-protected, and
        // answering 500 with qpdf's stderr sends them to look for a password that would not
        // have helped.
        if DAMAGE.iter().any(|phrase| lowered.contains(phrase)) {
            return AppError::BadRequest(
                "This file could not be opened at all: its structure is damaged, so there is \
                 no protection to remove. POST /api/repair rebuilds what can be rebuilt and \
                 tells you what it recovered."
                    .to_string(),
            );
        }
    }
    err
}

/// How qpdf words a file it cannot parse, as opposed to one it can parse but not decrypt
const DAMAGE: [&str; 4] = [
    "unable to find trailer dictionary",
    "can't find startxref",
    "file is damaged",
    "not a pdf file",
];

fn qpdf_warning(err: &AppError) -> String {
    let detail = match err {
        AppError::ProcessFailed { stderr, .. } => without_paths(
            stderr
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .unwrap_or("no detail"),
        ),
        other => format!("{:?}", other),
    };

    format!("qpdf finished with a warning: {}", detail)
}

/// The `encryption` check states what was observed on both sides of the pass, never what the
/// endpoint was asked to do. A document that was never protected gets a warning saying so:
/// certifying the removal of a protection that never existed is the one answer this service
/// must not give.
fn unlock_verdict(
    pages: usize,
    warnings: &[String],
    source: Encryption,
    produced: Encryption,
) -> Verdict {
    let (summary, encryption) = match (source, produced) {
        (Encryption::Present, Encryption::None) => (
            format!("Encryption removed from {} pages", pages),
            Check::ok(
                "encryption",
                format!("the {} pages are no longer password protected", pages),
            ),
        ),
        (Encryption::Present, _) => (
            "This document is still encrypted".to_string(),
            Check::fail(
                "encryption",
                "qpdf reported no error, but the file it wrote is still encrypted: the protection was not removed",
            ),
        ),
        (Encryption::None, _) => (
            format!("Nothing to unlock: {} pages rewritten as they were", pages),
            Check::warn(
                "encryption",
                "this document was not encrypted: there was no protection to remove, the file was rewritten unchanged",
            ),
        ),
        (Encryption::Unknown, Encryption::None) => (
            format!("{} pages rewritten, they are not encrypted", pages),
            Check::warn(
                "encryption",
                format!(
                    "qpdf could not read the encryption state of the source, so we cannot say a protection was removed — the {} pages that came out carry none",
                    pages
                ),
            ),
        ),
        (Encryption::Unknown, _) => (
            format!("{} pages rewritten, encryption state unknown", pages),
            Check::warn(
                "encryption",
                "qpdf could not read the encryption state of either file: this pass cannot be certified",
            ),
        ),
    };

    let mut checks = vec![encryption];

    // `tool-warning` is the name every route uses for a tolerated warning from the binary
    // it drove — `/api/ocr` reports ocrmypdf's under the same one.
    for warning in warnings {
        checks.push(Check::warn("tool-warning", warning.clone()));
    }

    Verdict::from_checks(summary, checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process_failed(stderr: &str) -> AppError {
        AppError::ProcessFailed {
            message: "PDF decryption failed".to_string(),
            stderr: stderr.to_string(),
        }
    }

    #[test]
    fn a_missing_or_empty_password_is_refused_before_any_tool_runs() {
        for password in [None, Some("")] {
            let err = validate_password(password).unwrap_err();
            match err {
                AppError::BadRequest(message) => {
                    assert!(
                        message.contains("never recovers or breaks one"),
                        "{}",
                        message
                    )
                }
                other => panic!("expected a bad request, got {:?}", other),
            }
        }
    }

    #[test]
    fn a_password_carrying_a_line_break_cannot_inject_a_qpdf_argument() {
        assert!(validate_password(Some("secret\n--encrypt")).is_err());
        assert!(validate_password(Some("secret\r--encrypt")).is_err());
        assert!(validate_password(Some("s3cr3t --encrypt")).is_ok());
    }

    #[test]
    fn the_password_travels_on_its_own_line_of_the_argument_file() {
        let args = decrypt_args("hunter2", "/tmp/in.pdf", "/tmp/out.pdf");
        let lines: Vec<&str> = args.lines().collect();
        assert_eq!(
            lines,
            vec![
                "--warning-exit-0",
                "--password=hunter2",
                "--decrypt",
                "/tmp/in.pdf",
                "/tmp/out.pdf"
            ]
        );
    }

    #[test]
    fn an_empty_pdf_reference_is_refused_with_the_two_accepted_forms() {
        let err = validate_reference("  ").unwrap_err();
        match err {
            AppError::BadRequest(message) => {
                assert!(message.contains("asset://"), "{}", message);
                assert!(message.contains("/download/"), "{}", message);
            }
            other => panic!("expected a bad request, got {:?}", other),
        }
        assert!(validate_reference("asset://as_00").is_ok());
    }

    #[test]
    fn a_wrong_password_answers_a_bad_request_not_a_tool_failure() {
        let err = decryption_error(process_failed("qpdf: in.pdf: invalid password"));
        match err {
            AppError::BadRequest(message) => assert!(message.contains("does not open")),
            other => panic!("expected a bad request, got {:?}", other),
        }
    }

    /// A file qpdf cannot parse at all has no protection to remove, and sending its stderr
    /// back as a 500 sends the caller looking for a password that would not have helped.
    #[test]
    fn a_document_qpdf_cannot_open_is_named_as_damaged_and_pointed_at_the_repair_route() {
        for stderr in [
            "qpdf: in.pdf: unable to find trailer dictionary while recovering damaged file",
            "WARNING: in.pdf: can't find startxref",
            "qpdf: in.pdf: file is damaged",
            "qpdf: in.pdf is not a PDF file",
        ] {
            match decryption_error(process_failed(stderr)) {
                AppError::BadRequest(message) => {
                    assert!(message.contains("/api/repair"), "{}", message);
                    assert!(message.contains("damaged"), "{}", message);
                }
                other => panic!("expected a bad request for {:?}, got {:?}", stderr, other),
            }
        }
    }

    /// Everything this module cannot diagnose stays a 500: guessing would be worse than
    /// admitting the service broke.
    #[test]
    fn an_unrelated_qpdf_failure_stays_a_tool_failure() {
        let err = decryption_error(process_failed("qpdf: out of memory allocating 4096 bytes"));
        assert!(matches!(err, AppError::ProcessFailed { .. }));
    }

    #[test]
    fn a_qpdf_warning_is_reported_with_its_first_meaningful_line() {
        let warning = qpdf_warning(&process_failed(
            "\n  file is not encrypted, password ignored\nqpdf: operation succeeded with warnings\n",
        ));
        assert_eq!(
            warning,
            "qpdf finished with a warning: file is not encrypted, password ignored"
        );
    }

    #[test]
    fn a_rewritten_document_tells_the_caller_which_method_produced_it() {
        let warnings = repair_warnings(Method::Ghostscript, &[]).expect("a rewrite must warn");
        assert!(warnings[0].starts_with("method=ghostscript:"));
        assert!(warnings[0].contains("rewritten, not repaired"));
        assert!(repair_warnings(Method::Qpdf, &[]).is_none());
    }

    #[test]
    fn a_qpdf_pass_that_warned_reports_the_warning_it_swallowed_before() {
        let warnings = repair_warnings(
            Method::Qpdf,
            &["qpdf: WARNING: file is damaged".to_string()],
        )
        .expect("a warned pass must report");
        assert_eq!(warnings, vec!["qpdf: WARNING: file is damaged"]);

        let verdict = repair_verdict(
            Method::Qpdf,
            3,
            Some(3),
            &["qpdf: WARNING: file is damaged".to_string()],
        );
        assert_eq!(verdict.status, "warn");
        assert!(verdict
            .checks
            .iter()
            .any(|check| check.name == "tool-warning"));
    }

    #[test]
    fn a_repair_that_keeps_every_page_has_nothing_to_report() {
        let verdict = repair_verdict(Method::Qpdf, 12, Some(12), &[]);
        assert_eq!(verdict.status, "ok");
        assert_eq!(verdict.score, 100);
        assert!(verdict.summary.contains("12 pages recovered by qpdf"));
    }

    #[test]
    fn losing_pages_during_a_repair_downgrades_the_verdict() {
        let verdict = repair_verdict(Method::Qpdf, 9, Some(12), &[]);
        assert_eq!(verdict.status, "warn");
        let pages = verdict
            .checks
            .iter()
            .find(|check| check.name == "page-count")
            .expect("a page-count check");
        assert_eq!(pages.status, "warn");
        assert!(pages.detail.contains("9 of the 12"));
    }

    #[test]
    fn a_ghostscript_rewrite_is_a_warning_even_when_no_page_was_lost() {
        let verdict = repair_verdict(Method::Ghostscript, 12, Some(12), &[]);
        assert_eq!(verdict.status, "warn");
        assert_eq!(verdict.score, 90);
    }

    #[test]
    fn a_file_too_damaged_to_count_its_pages_still_gets_a_verdict() {
        let verdict = repair_verdict(Method::Qpdf, 4, None, &[]);
        assert_eq!(verdict.status, "ok");
        assert!(verdict.checks[1]
            .detail
            .contains("no page count to compare"));
    }

    // ------------ B2: the blank page ghostscript hands back with exit code 0 ------------

    #[test]
    fn the_message_ghostscript_prints_on_a_file_it_never_opened_is_fatal() {
        // Observed on an encrypted PDF: exit code 0, these lines on stdout, one blank A4 page
        let streams = "GPL Ghostscript 10.0.0 (2022-09-21)\n   **** Error: Couldn't initialise file.\n               Output may be incorrect.\n   No pages will be processed (FirstPage > LastPage).\n";
        assert_eq!(fatal_marker(streams), Some("couldn't initialise file"));

        // And the one it prints on stderr for the same file
        assert_eq!(
            fatal_marker(
                "GPL Ghostscript 10.00.0: \n   **** This file requires a password for access.\n"
            ),
            Some("requires a password for access")
        );
    }

    #[test]
    fn the_banner_a_healthy_pass_prints_is_not_a_failure() {
        let streams = "GPL Ghostscript 10.0.0 (2022-09-21)\nCopyright (C) 2022 Artifex Software, Inc.  All rights reserved.\nProcessing pages 1 through 3.\n";
        assert_eq!(fatal_marker(streams), None);
        assert_eq!(tool_warning(streams), None);
    }

    #[test]
    fn a_qpdf_warning_line_is_picked_up_from_the_stream_of_a_successful_pass() {
        // What `--warning-exit-0` turns into an exit code of 0
        let streams = "WARNING: in.pdf: file is damaged\nWARNING: in.pdf: Attempting to reconstruct cross-reference table\n";
        assert_eq!(
            tool_warning(streams),
            Some("WARNING: in.pdf: file is damaged".to_string())
        );
        assert_eq!(fatal_marker(streams), None);
    }

    #[test]
    fn the_path_qpdf_echoes_back_never_reaches_the_caller() {
        // Observed: qpdf names the file it was given, which here is our own asset directory
        let warning =
            tool_warning("WARNING: /workdir/public/assets/as_0123/file.pdf: file is damaged\n")
                .expect("a warning");
        assert_eq!(warning, "WARNING: file is damaged");
        assert!(!warning.contains("/workdir"));

        assert_eq!(
            without_paths("WARNING: /tmp/x.pdf (offset 999999): xref not found"),
            "WARNING: xref not found"
        );
        // A line made of nothing but a path is dropped rather than echoed
        assert_eq!(without_paths("/tmp/x.pdf"), "no detail");
    }

    #[test]
    fn a_blank_output_out_of_a_source_that_had_content_is_a_loss_not_a_repair() {
        let source = Some(Content {
            text: true,
            images: false,
        });
        let blank = Some(Content {
            text: false,
            images: false,
        });
        assert!(content_loss(source, blank).is_some());
    }

    #[test]
    fn a_blank_output_out_of_a_source_nobody_could_read_is_refused_too() {
        // The encrypted-source case: pdftotext cannot open it, so there is no baseline — and
        // a blank page is precisely what a pass over an unopened file produces.
        let blank = Some(Content {
            text: false,
            images: false,
        });
        assert!(content_loss(None, blank).is_some());
        assert!(content_loss(None, None).is_some());
    }

    #[test]
    fn a_vector_only_document_is_not_treated_as_a_loss() {
        let empty = Content {
            text: false,
            images: false,
        };
        assert_eq!(content_loss(Some(empty), Some(empty)), None);

        let recovered = Content {
            text: true,
            images: true,
        };
        assert_eq!(content_loss(None, Some(recovered)), None);
        assert_eq!(content_loss(Some(recovered), Some(recovered)), None);
    }

    #[test]
    fn images_are_counted_from_the_listing_without_its_header() {
        let listing = "page   num  type   width height color comp bpc  enc interp  object ID x-ppi y-ppi size ratio\n--------------------------------------------------------------------------------------------\n   1    0 image     595   842  gray    1   8  image  no         9  0    72    72 4880B 1.0%\n";
        assert_eq!(count_listed_images(listing), 1);

        // The listing of a document without a single image is its header alone
        let header = listing.lines().take(2).collect::<Vec<_>>().join("\n");
        assert_eq!(count_listed_images(&header), 0);
    }

    // ------------ S1: certifying a protection that was never there ------------

    #[test]
    fn qpdf_reading_no_encryption_dictionary_is_read_as_no_encryption() {
        assert_eq!(
            parse_encryption("File is not encrypted\n"),
            Encryption::None
        );
    }

    #[test]
    fn an_encrypted_file_is_recognised_with_or_without_its_password() {
        // `--show-encryption` with the password
        let with_password = "R = 6\nP = -4\nUser password = \nstream encryption method: AESv3\nfile encryption method: AESv3\n";
        assert_eq!(parse_encryption(with_password), Encryption::Present);

        // and without it — qpdf still exits 0 and still describes the protection
        let without_password = format!("Incorrect password supplied\n{}", with_password);
        assert_eq!(parse_encryption(&without_password), Encryption::Present);
    }

    #[test]
    fn an_answer_we_do_not_recognise_stays_unknown_rather_than_becoming_a_claim() {
        assert_eq!(parse_encryption(""), Encryption::Unknown);
        assert_eq!(
            parse_encryption("some future qpdf phrasing"),
            Encryption::Unknown
        );
    }

    #[test]
    fn unlocking_a_document_that_was_never_encrypted_says_so_instead_of_certifying_it() {
        let verdict = unlock_verdict(3, &[], Encryption::None, Encryption::None);
        assert_eq!(verdict.status, "warn");
        let encryption = verdict
            .checks
            .iter()
            .find(|check| check.name == "encryption")
            .expect("an encryption check");
        assert_eq!(encryption.status, "warn");
        assert!(
            encryption.detail.contains("was not encrypted"),
            "{}",
            encryption.detail
        );
        assert!(
            !verdict.summary.contains("Encryption removed"),
            "{}",
            verdict.summary
        );
    }

    #[test]
    fn an_unlock_that_removed_a_real_protection_is_a_clean_verdict() {
        let verdict = unlock_verdict(3, &[], Encryption::Present, Encryption::None);
        assert_eq!(verdict.status, "ok");
        assert_eq!(verdict.score, 100);
        assert!(verdict.summary.contains("Encryption removed from 3 pages"));

        let noisy = unlock_verdict(
            3,
            &["qpdf finished with a warning: x".to_string()],
            Encryption::Present,
            Encryption::None,
        );
        assert_eq!(noisy.status, "warn");
    }

    #[test]
    fn an_output_still_encrypted_is_a_failed_check_whatever_qpdf_answered() {
        let verdict = unlock_verdict(3, &[], Encryption::Present, Encryption::Present);
        assert_eq!(verdict.status, "fail");
        assert!(verdict.checks[0].detail.contains("still encrypted"));
    }

    #[test]
    fn an_unreadable_encryption_state_is_reported_as_unverified() {
        let verdict = unlock_verdict(3, &[], Encryption::Unknown, Encryption::None);
        assert_eq!(verdict.status, "warn");
        assert!(verdict.checks[0].detail.contains("could not read"));
    }
}

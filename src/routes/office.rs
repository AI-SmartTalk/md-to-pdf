//! Office ↔ PDF, the two conversions the service could not do until it accepted files.
//!
//! One direction is faithful: a Word, Excel, PowerPoint or ODF document carries its own
//! structure, and LibreOffice lays it out the way its author wrote it. The other is a
//! reconstruction: a PDF holds glyphs at coordinates, not paragraphs, tables and styles —
//! those are guessed. Every service on the market sells both under the same word,
//! "convert", and lets the user discover the difference when they open the file. This one
//! says so in the response, and measures how much of the document survived.
//!
//! Both directions hand bytes nobody vetted to LibreOffice, which by default treats a
//! document's own references as instructions: an `xlink:href` pointing at an HTTP URL is
//! fetched while the file loads, and the answer is laid out in the result. What this module
//! does about it — a user profile written before each conversion — is described on
//! `SAFE_PROFILE`, along with what it does *not* do: the process still has the container's
//! network stack, its environment and a writable filesystem. Closing that is the worker
//! container's job, and it is not built yet.

use crate::assets::AssetKind;
use crate::auth::PublicOrKey;
use crate::exec;
use crate::helpers;
use crate::types::{AppError, Check, ToolOutput, ToolResponse, Verdict};
use rocket::fs::NamedFile;
use rocket::serde::json::Json;
use rocket::Either;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;
use tempfile::{Builder, TempPath};

/// A reconstruction rewrites the text stream, so a few characters legitimately move.
/// Below this share of the original glyphs, content was actually lost.
const TEXT_WARN_RATIO: f64 = 0.98;
/// Below this, whole pages did not survive the guess.
const TEXT_FAIL_RATIO: f64 = 0.5;

/// What a caller must be told before they send the file to anyone
const RECONSTRUCTION_WARNING: &str = "Rebuilding a PDF into an editable document is approximate by nature: a PDF holds no document structure (paragraphs, tables, styles), it is guessed from where the characters sit. Read the file before sending it to anyone.";

// ------------ Office → PDF ------------

#[derive(Deserialize)]
pub struct OfficeToPdfRequest {
    /// `asset://as_…` — this direction always starts from an uploaded file
    pub file: String,
    pub client_id: Option<String>,
    pub pdf_name: Option<String>,
    pub output: Option<ToolOutput>,
}

#[post("/office-to-pdf", format = "json", data = "<req>")]
pub async fn office_to_pdf(
    key: PublicOrKey,
    req: Json<OfficeToPdfRequest>,
) -> Result<Either<NamedFile, Json<ToolResponse>>, AppError> {
    let req = req.into_inner();

    // A reference the store cannot even name is refused before it costs a render slot;
    // what the bytes actually are is only knowable from the asset metadata, in `run`.
    asset_id(&req.file)?;

    let (produced, response) = exec::as_owner(key.0, exec::offload(move || run(req))).await?;
    helpers::deliver_tool(produced, response, "office-to-pdf").await
}

/// All the blocking work. Public and free of Rocket: the async job dispatcher calls it
/// as is.
pub fn run(req: OfficeToPdfRequest) -> Result<(TempPath, ToolResponse), AppError> {
    let id = asset_id(&req.file)?;
    let meta = crate::assets::meta(id)?;
    ensure_office(meta.kind)?;
    let source = crate::assets::path(id)?;

    let workdir = Builder::new().prefix("office-").tempdir()?;
    let converted = convert(&source, "pdf", None, "pdf", workdir.path())?;
    let produced = adopt(&converted, ".pdf")?;

    // Measured before `finish_tool`, which moves the file away when an asset was asked for
    let pages = page_texts(&produced)?;
    let verdict = office_verdict(meta.kind, &pages);

    let mut response = helpers::finish_tool(
        &produced,
        req.client_id,
        req.pdf_name,
        req.output.unwrap_or_default(),
        "converted.pdf",
    )?;
    response.pages = Some(pages.len());
    response.verdict = Some(verdict);

    Ok((produced, response))
}

// ------------ PDF → Office ------------

#[derive(Deserialize)]
pub struct PdfToOfficeRequest {
    /// `asset://as_…` or `/download/<client_id>/<name>.pdf`
    pub pdf: String,
    /// `docx`, `xlsx` or `pptx`
    pub to: String,
    pub client_id: Option<String>,
    pub pdf_name: Option<String>,
    pub output: Option<ToolOutput>,
}

#[post("/pdf-to-office", format = "json", data = "<req>")]
pub async fn pdf_to_office(
    key: PublicOrKey,
    req: Json<PdfToOfficeRequest>,
) -> Result<Either<NamedFile, Json<ToolResponse>>, AppError> {
    let req = req.into_inner();

    parse_target(&req.to)?;

    let (produced, response) =
        exec::as_owner(key.0, exec::offload(move || run_pdf_to_office(req))).await?;
    helpers::deliver_tool(produced, response, "pdf-to-office").await
}

/// All the blocking work. Public and free of Rocket: the async job dispatcher calls it
/// as is.
pub fn run_pdf_to_office(req: PdfToOfficeRequest) -> Result<(TempPath, ToolResponse), AppError> {
    let source = helpers::resolve_readable_pdf(&req.pdf)?;
    let target = parse_target(&req.to)?;

    let before = Measurement::of(&source)?;

    let workdir = Builder::new().prefix("office-").tempdir()?;
    let converted = match target.engine {
        Engine::Reflow => rebuild_flowing(&source, workdir.path())?,
        Engine::LibreOffice(infilter) => convert(
            &source,
            target.extension,
            Some(infilter),
            target.extension,
            workdir.path(),
        )?,
    };
    let produced = adopt(&converted, &format!(".{}", target.extension))?;

    // The only honest way to say what the reconstruction kept is to render it back and
    // read it. A failure here costs the caller nothing: the verdict says it could not be
    // checked, and the document is delivered all the same.
    let after = round_trip(&produced);
    let verdict = reconstruction_verdict(&before, after.as_ref());

    let name_hint = format!("converted.{}", target.extension);
    let mut response = helpers::finish_tool(
        &produced,
        req.client_id,
        req.pdf_name,
        req.output.unwrap_or_default(),
        &name_hint,
    )?;

    // The produced file is not a PDF, so `pages` describes the document that was read.
    response.pages = Some(before.pages);

    let mut warnings = vec![RECONSTRUCTION_WARNING.to_string()];
    if response.download_url.is_some() {
        // `save_pdf` names everything it stores `.pdf`; saying nothing would let a caller
        // hand a `.pdf` URL to a user whose bytes are a Word document.
        warnings.push(format!(
            "The stored file carries the .pdf extension the download store imposes, but its content is a {} document. Use \"output\":\"asset\" to get the file under its real name.",
            target.extension
        ));
    }
    response.warnings = Some(warnings);
    response.verdict = Some(verdict);

    Ok((produced, response))
}

/// Where a PDF can be reconstructed to, and by which route.
#[derive(Debug, PartialEq)]
struct Target {
    extension: &'static str,
    engine: Engine,
}

/// The two ways this service rebuilds a PDF, and why there are two.
///
/// LibreOffice's PDF import is faithful to the *picture*: every line becomes a text frame
/// pinned at its coordinates. For a slide deck that is the right answer — a slide is a
/// positioned canvas, and Impress rebuilds one as such. For a Word document it is the wrong
/// one: the file opens and cannot be edited, because nothing in it is a paragraph. Measured
/// on this project's README, that route produces 810 text boxes and no working hyperlink.
///
/// So the Writer direction goes through `reflow` instead, which reads the geometry poppler
/// reports and guesses paragraphs, headings and lists back out of it, then lets pandoc write
/// an ordinary document. Same text, a twenty-ninth of the size, editable.
#[derive(Debug, PartialEq, Clone, Copy)]
enum Engine {
    /// `pdftohtml -xml` → `reflow` → pandoc
    Reflow,
    /// `soffice --infilter=…`, the named LibreOffice import filter
    LibreOffice(&'static str),
}

fn parse_target(to: &str) -> Result<Target, AppError> {
    match to {
        "docx" => Ok(Target {
            extension: "docx",
            engine: Engine::Reflow,
        }),
        // Calc has no notion of flowing text to rebuild into, so there is nothing for
        // `reflow` to do: the sheet is whatever the import filter lays out.
        "xlsx" => Ok(Target {
            extension: "xlsx",
            engine: Engine::LibreOffice("calc_pdf_import"),
        }),
        "pptx" => Ok(Target {
            extension: "pptx",
            engine: Engine::LibreOffice("impress_pdf_import"),
        }),
        other => Err(AppError::BadRequest(format!(
            "\"to\" must be one of docx, xlsx, pptx (got \"{}\")",
            other
        ))),
    }
}

/// Rebuild a PDF into a flowing word-processor document.
///
/// Two processes, both cheap: poppler reports the geometry, `reflow` turns it into HTML, and
/// pandoc writes the docx with Word's own paragraph and heading styles. `--resource-path`
/// is what lets pandoc find the images poppler extracted next to the XML; without it every
/// figure would be silently dropped.
fn rebuild_flowing(source: &Path, workdir: &Path) -> Result<PathBuf, AppError> {
    let xml = workdir.join("page.xml");

    helpers::run_tool(
        Command::new("pdftohtml")
            .arg("-xml")
            .arg("-enc")
            .arg("UTF-8")
            // Poppler asks before overwriting, and an unanswered prompt is a timeout
            .arg("-nodrm")
            .arg(helpers::path_to_str(source)?)
            .arg(helpers::path_to_str(&xml)?),
        "pdftohtml",
        "Reading the PDF layout failed",
    )?;

    let html_path = workdir.join("page.html");
    fs::write(
        &html_path,
        crate::reflow::to_html(&fs::read_to_string(&xml)?),
    )?;

    let produced = workdir.join("converted.docx");
    helpers::run_tool(
        Command::new("pandoc")
            .arg("-f")
            .arg("html")
            .arg("-t")
            .arg("docx")
            .arg("--resource-path")
            .arg(helpers::path_to_str(workdir)?)
            .arg("-o")
            .arg(helpers::path_to_str(&produced)?)
            .arg(helpers::path_to_str(&html_path)?),
        "pandoc",
        "Writing the document failed",
    )?;

    if fs::metadata(&produced).map(|meta| meta.len()).unwrap_or(0) == 0 {
        return Err(AppError::ProcessFailed {
            message: "The rebuilt document came out empty".to_string(),
            stderr: String::new(),
        });
    }

    Ok(produced)
}

// ------------ LibreOffice ------------

/// The settings a conversion of an unvetted document must run under, in the only form
/// LibreOffice accepts them: the user layer of its configuration. `soffice` has no command
/// line flag for any of this.
///
/// Every path and property name below was read out of the schema LibreOffice actually
/// ships (`share/registry/main.xcd`) and then replayed against the binary. This matters
/// more than it looks: a mistyped path is **ignored in silence**, so a wrong name here
/// would leave the document free to reach the network while the code claims otherwise.
///
/// What each block buys, measured on a document carrying
/// `<draw:image xlink:href="http://…"/>`, flat and zipped, against a listener:
///
/// * `Inet/Settings` — every HTTP, HTTPS and FTP request is handed to a proxy that is a
///   closed port on the loopback, and `ooInetNoProxy` is empty so nothing is exempt. The
///   request is no longer emitted at all. Without it: `OPTIONS` then `GET` to the
///   attacker's URL, and the bytes returned end up as an image object in the PDF.
/// * `Security/Scripting/BlockUntrustedRefererLinks` — refuses, one layer higher, the
///   references a document makes to anything it did not carry itself. It stops the same
///   HTTP fetch on its own, and also stops `file://` references, which no proxy can.
/// * the macro keys — a document cannot run Basic on load. The fresh profile already
///   carried the shipped default (ask, unsigned refused); these state it rather than
///   inherit it.
/// * the link keys — a document does not silently refresh what it links to when it opens.
///   Not the layer that stopped the proven attack, and not one this module could exercise
///   on its own; it is here because the two above should not be the only thing standing
///   between an upload and an outbound request.
///
/// None of this isolates the process: a LibreOffice able to open a socket by another route
/// still can. Only the worker container of the plan (its own network namespace) closes
/// that, and it does not exist yet.
const SAFE_PROFILE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<oor:items xmlns:oor="http://openoffice.org/2001/registry" xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
 <item oor:path="/org.openoffice.Inet/Settings">
  <prop oor:name="ooInetProxyType" oor:op="fuse"><value>1</value></prop>
  <prop oor:name="ooInetHTTPProxyName" oor:op="fuse"><value>127.0.0.1</value></prop>
  <prop oor:name="ooInetHTTPProxyPort" oor:op="fuse"><value>1</value></prop>
  <prop oor:name="ooInetHTTPSProxyName" oor:op="fuse"><value>127.0.0.1</value></prop>
  <prop oor:name="ooInetHTTPSProxyPort" oor:op="fuse"><value>1</value></prop>
  <prop oor:name="ooInetFTPProxyName" oor:op="fuse"><value>127.0.0.1</value></prop>
  <prop oor:name="ooInetFTPProxyPort" oor:op="fuse"><value>1</value></prop>
  <prop oor:name="ooInetNoProxy" oor:op="fuse"><value></value></prop>
 </item>
 <item oor:path="/org.openoffice.Office.Common/Security/Scripting">
  <prop oor:name="BlockUntrustedRefererLinks" oor:op="fuse"><value>true</value></prop>
  <prop oor:name="DisableMacrosExecution" oor:op="fuse"><value>true</value></prop>
  <prop oor:name="MacroSecurityLevel" oor:op="fuse"><value>3</value></prop>
  <prop oor:name="OfficeBasic" oor:op="fuse"><value>0</value></prop>
 </item>
 <item oor:path="/org.openoffice.Office.Writer/Content/Update">
  <prop oor:name="Link" oor:op="fuse"><value>0</value></prop>
 </item>
 <item oor:path="/org.openoffice.Office.Calc/Content/Update">
  <prop oor:name="Link" oor:op="fuse"><value>0</value></prop>
 </item>
</oor:items>
"#;

/// A throwaway LibreOffice user profile, seeded before the first conversion uses it.
///
/// Two conversions sharing a profile deadlock on its lock file, and a profile that
/// outlives a request is state that hostile content can poison for the next caller.
///
/// Freshness alone is not safety, though: LibreOffice's *defaults* resolve what a document
/// points at while loading it, which on a service that accepts uploads is an SSRF onto
/// everything the container can reach. So the profile is not merely new, it is written —
/// see `SAFE_PROFILE`.
struct Profile(PathBuf);

impl Profile {
    fn new() -> Result<Profile, AppError> {
        let dir = PathBuf::from("/tmp").join(helpers::random_id("lo")?);

        // The user layer is read from `user/` at start-up; LibreOffice creates that
        // directory itself, but only after it has already looked for the file in it.
        fs::create_dir_all(dir.join("user"))?;

        // Built before the write so a failure below still takes the directory with it
        let profile = Profile(dir);

        // A profile that could not be seeded is a conversion with the network open. Fail
        // it: a document rejected costs the caller a retry, a document relayed costs
        // someone else their metadata endpoint.
        fs::write(profile.registry(), SAFE_PROFILE)?;

        Ok(profile)
    }

    fn registry(&self) -> PathBuf {
        self.0.join("user").join("registrymodifications.xcu")
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// One LibreOffice conversion under a seeded throwaway profile, returning the file it
/// wrote in `outdir`. "Sandbox" would be the wrong word: nothing here confines the process.
fn convert(
    input: &Path,
    convert_to: &str,
    infilter: Option<&str>,
    extension: &str,
    outdir: &Path,
) -> Result<PathBuf, AppError> {
    let profile = Profile::new()?;
    let profile_path = helpers::path_to_str(&profile.0)?;

    let mut command = Command::new("soffice");
    command
        .arg("--headless")
        .arg("--norestore")
        .arg("--nolockcheck")
        .arg("--nodefault")
        .arg("--nofirststartwizard")
        .arg("--invisible")
        .arg(format!("-env:UserInstallation=file://{}", profile_path))
        // LibreOffice writes recovery data, dictionaries and lock files next to HOME
        // whatever the profile says: point it at the same throwaway directory.
        .env("HOME", &profile.0);

    if let Some(filter) = infilter {
        command.arg(format!("--infilter={}", filter));
    }

    command
        .arg("--convert-to")
        .arg(convert_to)
        .arg("--outdir")
        .arg(helpers::path_to_str(outdir)?)
        .arg(helpers::path_to_str(input)?);

    let output = helpers::run_capture(&mut command, "soffice", "Document conversion failed")?;

    // LibreOffice reports a document it refused on stdout and still exits 0. The only
    // proof of a conversion is the file it was supposed to write.
    let produced = written_file(outdir, extension).ok_or_else(|| AppError::ProcessFailed {
        message: format!(
            "LibreOffice produced no {} file from this document",
            extension
        ),
        stderr: tail(&output),
    })?;

    if fs::metadata(&produced)?.len() == 0 {
        return Err(AppError::ProcessFailed {
            message: format!("LibreOffice produced an empty {} file", extension),
            stderr: tail(&output),
        });
    }

    Ok(produced)
}

/// The single file of that extension in a directory we created empty for this conversion
fn written_file(outdir: &Path, extension: &str) -> Option<PathBuf> {
    fs::read_dir(outdir).ok()?.flatten().find_map(|entry| {
        let path = entry.path();
        let matches = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case(extension));
        matches.then_some(path)
    })
}

/// Move a file out of the conversion directory into a temporary path that survives it
fn adopt(produced: &Path, suffix: &str) -> Result<TempPath, AppError> {
    let target = Builder::new().suffix(suffix).tempfile()?;

    // Rename is atomic and free on the same filesystem; the copy covers a temp directory
    // mounted elsewhere.
    if fs::rename(produced, target.path()).is_err() {
        fs::copy(produced, target.path())?;
    }

    Ok(target.into_temp_path())
}

fn tail(output: &Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text.chars().take(500).collect()
}

// ------------ Measuring what survived ------------

/// What a PDF holds, in the two units a conversion destroys silently
struct Measurement {
    pages: usize,
    glyphs: usize,
}

impl Measurement {
    fn of(pdf: &Path) -> Result<Measurement, AppError> {
        let pages = page_texts(pdf)?;
        Ok(Measurement {
            glyphs: pages.iter().map(|page| glyphs(page)).sum(),
            pages: pages.len(),
        })
    }
}

/// How long the fidelity check may take before the answer stops being worth the wait.
///
/// The check renders the produced document a second time, and a reconstruction can be
/// pathological to lay out: LibreOffice's own PDF import used to produce a docx of 810 text
/// frames that took longer to re-render than the whole request was allowed. Capping it turns
/// "the caller waits two minutes and gets nothing" into "the caller waits a second longer and
/// is told the fidelity is unverified".
const VERIFICATION_BUDGET: Duration = Duration::from_secs(20);

/// Render a reconstructed document back to PDF and measure it.
///
/// Everything here is best effort: it feeds a verdict, never the response itself.
fn round_trip(document: &Path) -> Option<Measurement> {
    let _cap = helpers::Budget::cap(VERIFICATION_BUDGET);

    let workdir = Builder::new().prefix("office-check-").tempdir().ok()?;
    let rendered = convert(document, "pdf", None, "pdf", workdir.path()).ok()?;
    Measurement::of(&rendered).ok()
}

/// Extractable text of each page, as `pdftotext` separates them: one form feed per page
fn page_texts(pdf: &Path) -> Result<Vec<String>, AppError> {
    let output = helpers::run_capture(
        Command::new("pdftotext")
            .arg("-q")
            .arg(helpers::path_to_str(pdf)?)
            .arg("-"),
        "pdftotext",
        "text extraction failed",
    )?;

    Ok(split_pages(&String::from_utf8_lossy(&output.stdout)))
}

fn split_pages(text: &str) -> Vec<String> {
    let mut pages: Vec<String> = text.split('\u{c}').map(str::to_string).collect();

    // The form feed terminates every page, the last one included, so the split always
    // leaves one empty tail that is not a page.
    if pages.last().is_some_and(|page| page.is_empty()) {
        pages.pop();
    }

    pages
}

/// Whitespace is not counted: what must survive a conversion is the glyphs a reader can
/// select, search and copy, not the spacing a layout engine recomputes anyway.
fn glyphs(text: &str) -> usize {
    text.chars().filter(|c| !c.is_whitespace()).count()
}

// ------------ Verdicts ------------

/// Check names are the translatable contract: `page-count` and `text-preserved` mean here
/// exactly what they mean in `/api/compress`, `/api/pdfa` and `/api/crop`, and the public
/// site keys its wording on them. `text-layer` is the one this direction owns — there is no
/// "before" to compare with, only the question of whether the output carries selectable text.
fn office_verdict(kind: AssetKind, pages: &[String]) -> Verdict {
    let blank = pages.iter().filter(|page| glyphs(page) == 0).count();
    let total = pages.len();
    let characters: usize = pages.iter().map(|page| glyphs(page)).sum();

    let mut checks = vec![Check::ok("page-count", format!("{} pages produced", total))];

    checks.push(if total == 0 || blank == 0 {
        Check::ok(
            "text-layer",
            format!("{} selectable and searchable characters", characters),
        )
    } else if blank == total {
        Check::warn(
            "text-layer",
            "no page carries extractable text: the source document is entirely graphical, or its font could not be embedded",
        )
    } else {
        Check::warn(
            "text-layer",
            format!("{} of {} pages without extractable text", blank, total),
        )
    });

    Verdict::from_checks(
        format!(
            "{} → PDF, {} pages, {} extractable characters",
            kind.as_str(),
            total,
            characters
        ),
        checks,
    )
}

fn reconstruction_verdict(before: &Measurement, after: Option<&Measurement>) -> Verdict {
    let after = match after {
        Some(after) => after,
        None => {
            return Verdict::from_checks(
                format!("{} pages rebuilt, fidelity unverified", before.pages),
                vec![Check::warn(
                    "reconstruction-checked",
                    "the produced document could not be read back for comparison: its fidelity to the source PDF is unverified",
                )],
            );
        }
    };

    let mut checks = Vec::new();

    // Repagination is expected, not a defect: an editable document reflows. It is still
    // worth naming, because a caller who prints the result gets a different object.
    checks.push(if after.pages == before.pages {
        Check::ok("page-count", format!("{} pages, unchanged", before.pages))
    } else {
        Check::warn(
            "page-count",
            format!(
                "{} pages in, {} after the rebuild: an editable document repaginates",
                before.pages, after.pages
            ),
        )
    });

    checks.push(text_check(before.glyphs, after.glyphs));

    Verdict::from_checks(
        format!(
            "{} pages, {}, a rebuild is approximate by nature",
            before.pages,
            text_phrase(before.glyphs, after.glyphs)
        ),
        checks,
    )
}

fn text_check(before: usize, after: usize) -> Check {
    if before == 0 {
        return Check::warn(
            "text-preserved",
            "the source PDF carries no text layer: the produced document holds images only, no text could be rebuilt",
        );
    }

    let ratio = after as f64 / before as f64;

    if ratio >= TEXT_WARN_RATIO {
        Check::ok(
            "text-preserved",
            format!("{} of {} characters recovered", after, before),
        )
    } else if ratio >= TEXT_FAIL_RATIO {
        Check::warn(
            "text-preserved",
            format!(
                "{} characters in, {} after the rebuild: {}% of the text was not recovered",
                before,
                after,
                percent(1.0 - ratio)
            ),
        )
    } else {
        Check::fail(
            "text-preserved",
            format!(
                "{} characters in, {} after the rebuild: most of the text was not recovered, this PDF does not lend itself to an editable conversion",
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
        "text recovered".to_string()
    } else if ratio >= TEXT_FAIL_RATIO {
        format!("{}% of the text recovered", percent(ratio))
    } else {
        format!("text lost ({}% recovered)", percent(ratio))
    }
}

fn percent(ratio: f64) -> String {
    format!("{:.0}", (ratio * 100.0).clamp(0.0, 100.0))
}

// ------------ Input ------------

/// This direction reads a format the download store never holds: the reference has to be
/// an uploaded file, and saying so beats a "PDF not found" three layers down.
fn asset_id(reference: &str) -> Result<&str, AppError> {
    crate::assets::strip_scheme(reference).ok_or_else(|| {
        AppError::BadRequest(format!(
            "\"file\" must reference an uploaded file as asset://as_… (got \"{}\") — upload it with POST /api/files first",
            reference
        ))
    })
}

fn ensure_office(kind: AssetKind) -> Result<(), AppError> {
    if kind.is_office() {
        return Ok(());
    }

    Err(AppError::BadRequest(format!(
        "This endpoint converts office documents (docx, xlsx, pptx, odt, ods, odp, doc, csv), the uploaded file is a {} file",
        kind.as_str()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measurement(pages: usize, glyphs: usize) -> Measurement {
        Measurement { pages, glyphs }
    }

    fn check<'a>(verdict: &'a Verdict, name: &str) -> &'a Check {
        verdict
            .checks
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no {} check in {:?}", name, verdict.checks))
    }

    /// Word goes through the reflow chain and the other two through the LibreOffice module
    /// that can actually write them — a Writer document cannot be saved as a spreadsheet.
    #[test]
    fn each_target_carries_the_engine_that_can_produce_something_usable() {
        assert_eq!(parse_target("docx").unwrap().engine, Engine::Reflow);
        assert_eq!(
            parse_target("xlsx").unwrap().engine,
            Engine::LibreOffice("calc_pdf_import")
        );
        assert_eq!(
            parse_target("pptx").unwrap().engine,
            Engine::LibreOffice("impress_pdf_import")
        );
        assert_eq!(parse_target("pptx").unwrap().extension, "pptx");
    }

    #[test]
    fn an_unknown_target_is_refused_with_the_list_of_targets() {
        let error = parse_target("doc").unwrap_err();
        match error {
            AppError::BadRequest(message) => {
                assert!(message.contains("docx"), "{}", message);
                assert!(message.contains("pptx"), "{}", message);
                assert!(message.contains("\"doc\""), "{}", message);
            }
            other => panic!("expected a bad request, got {:?}", other),
        }
    }

    #[test]
    fn a_document_to_convert_must_come_from_the_asset_store() {
        assert_eq!(asset_id("asset://as_0123").unwrap(), "as_0123");

        let error = asset_id("/download/acme/report.pdf").unwrap_err();
        match error {
            AppError::BadRequest(message) => {
                assert!(message.contains("POST /api/files"), "{}", message)
            }
            other => panic!("expected a bad request, got {:?}", other),
        }
    }

    #[test]
    fn a_pdf_or_an_image_is_refused_by_name_before_libreoffice_sees_it() {
        for kind in [
            AssetKind::Docx,
            AssetKind::Ods,
            AssetKind::Ole,
            AssetKind::Csv,
        ] {
            assert!(ensure_office(kind).is_ok(), "{:?}", kind);
        }

        let error = ensure_office(AssetKind::Png).unwrap_err();
        match error {
            AppError::BadRequest(message) => assert!(message.contains("png"), "{}", message),
            other => panic!("expected a bad request, got {:?}", other),
        }
    }

    #[test]
    fn pages_are_split_on_the_form_feed_pdftotext_writes_after_every_page() {
        assert_eq!(split_pages("un\u{c}deux\u{c}"), vec!["un", "deux"]);
        assert_eq!(split_pages(""), Vec::<String>::new());
        assert_eq!(split_pages("\u{c}"), vec![""]);
        // A trailing blank page is a page, and stays one
        assert_eq!(split_pages("un\u{c}\u{c}"), vec!["un", ""]);
    }

    #[test]
    fn only_the_glyphs_a_reader_can_select_are_counted() {
        assert_eq!(glyphs("  a b\n c \u{a0}"), 3);
        assert_eq!(glyphs("\n\n  "), 0);
    }

    #[test]
    fn an_office_document_that_kept_its_text_reads_as_one_sentence() {
        let verdict = office_verdict(
            AssetKind::Docx,
            &["Bonjour".to_string(), "Monde".to_string()],
        );

        assert_eq!(verdict.status, "ok");
        assert_eq!(
            verdict.summary,
            "docx → PDF, 2 pages, 12 extractable characters"
        );
    }

    #[test]
    fn a_converted_page_without_extractable_text_warns_without_blocking() {
        let verdict = office_verdict(AssetKind::Pptx, &["Titre".to_string(), "   ".to_string()]);

        assert_eq!(check(&verdict, "text-layer").status, "warn");
        assert_eq!(verdict.status, "warn");
        assert!(check(&verdict, "text-layer")
            .detail
            .contains("1 of 2 pages"));
    }

    #[test]
    fn a_faithful_reconstruction_reads_as_ok() {
        let verdict =
            reconstruction_verdict(&measurement(12, 20_000), Some(&measurement(12, 20_000)));

        assert_eq!(verdict.status, "ok");
        assert_eq!(verdict.score, 100);
        assert!(
            verdict.summary.contains("text recovered"),
            "{}",
            verdict.summary
        );
    }

    #[test]
    fn a_document_that_repaginates_warns_because_an_editable_file_reflows() {
        let verdict =
            reconstruction_verdict(&measurement(12, 20_000), Some(&measurement(14, 20_000)));

        assert_eq!(check(&verdict, "page-count").status, "warn");
        assert_eq!(check(&verdict, "text-preserved").status, "ok");
    }

    #[test]
    fn a_reconstruction_that_lost_most_of_the_text_says_so_instead_of_shipping_quietly() {
        let verdict = reconstruction_verdict(&measurement(12, 20_000), Some(&measurement(12, 400)));

        assert_eq!(check(&verdict, "text-preserved").status, "fail");
        assert_eq!(verdict.status, "fail");
    }

    #[test]
    fn a_scanned_pdf_says_that_nothing_could_be_reconstructed() {
        let verdict = reconstruction_verdict(&measurement(3, 0), Some(&measurement(3, 0)));

        assert_eq!(check(&verdict, "text-preserved").status, "warn");
        assert!(
            verdict.summary.contains("no text layer"),
            "{}",
            verdict.summary
        );
    }

    #[test]
    fn a_reconstruction_nobody_could_reread_admits_it_rather_than_claiming_success() {
        let verdict = reconstruction_verdict(&measurement(9, 5_000), None);

        assert_eq!(verdict.status, "warn");
        assert_eq!(check(&verdict, "reconstruction-checked").status, "warn");
    }

    #[test]
    fn the_profile_directory_is_unique_and_disappears_with_its_guard() {
        let first = Profile::new().unwrap();
        let second = Profile::new().unwrap();
        assert_ne!(first.0, second.0);
        assert!(first.0.is_dir());

        let path = second.0.clone();
        drop(second);
        assert!(!path.exists());
    }

    /// The profile exists to carry the settings; an empty one is the SSRF the review
    /// proved. The path is the one LibreOffice reads, and it reads it before writing it.
    #[test]
    fn a_profile_is_written_before_libreoffice_ever_sees_it() {
        let profile = Profile::new().unwrap();

        assert_eq!(
            profile.registry(),
            profile.0.join("user").join("registrymodifications.xcu")
        );

        let written = fs::read_to_string(profile.registry()).unwrap();
        assert_eq!(written, SAFE_PROFILE);
    }

    /// A mistyped configuration path is ignored in silence by LibreOffice, so these names
    /// are the whole mitigation. They were read from the schema it ships and replayed
    /// against the binary: changing one without doing the same reopens the hole.
    #[test]
    fn the_profile_sends_every_outbound_scheme_to_a_closed_port_with_no_exemption() {
        for path in [
            "/org.openoffice.Inet/Settings",
            "/org.openoffice.Office.Common/Security/Scripting",
        ] {
            assert!(
                SAFE_PROFILE.contains(&format!("oor:path=\"{}\"", path)),
                "{} is missing",
                path
            );
        }

        for scheme in ["HTTP", "HTTPS", "FTP"] {
            assert!(
                SAFE_PROFILE.contains(&format!(
                    "<prop oor:name=\"ooInet{}ProxyName\" oor:op=\"fuse\"><value>127.0.0.1</value></prop>",
                    scheme
                )),
                "{} is not proxied to the loopback",
                scheme
            );
            assert!(
                SAFE_PROFILE.contains(&format!(
                    "<prop oor:name=\"ooInet{}ProxyPort\" oor:op=\"fuse\"><value>1</value></prop>",
                    scheme
                )),
                "{} is not proxied to a closed port",
                scheme
            );
        }

        // Manual proxying — the mode where the addresses above are the ones used. A `2`
        // here means "whatever the system says", which in a container is a direct route.
        assert!(SAFE_PROFILE.contains(
            "<prop oor:name=\"ooInetProxyType\" oor:op=\"fuse\"><value>1</value></prop>"
        ));

        // One host left out of the proxy list is one host an uploaded document can reach
        assert!(SAFE_PROFILE
            .contains("<prop oor:name=\"ooInetNoProxy\" oor:op=\"fuse\"><value></value></prop>"));
    }

    #[test]
    fn the_profile_refuses_a_documents_own_references_and_its_macros() {
        // The layer that also covers file://, which no proxy setting can reach
        assert!(SAFE_PROFILE.contains(
            "<prop oor:name=\"BlockUntrustedRefererLinks\" oor:op=\"fuse\"><value>true</value></prop>"
        ));

        assert!(SAFE_PROFILE.contains(
            "<prop oor:name=\"DisableMacrosExecution\" oor:op=\"fuse\"><value>true</value></prop>"
        ));
        // 3 is the highest level LibreOffice defines: only trusted locations run
        assert!(SAFE_PROFILE.contains(
            "<prop oor:name=\"MacroSecurityLevel\" oor:op=\"fuse\"><value>3</value></prop>"
        ));

        // Writer and Calc each own their "update links on load" switch
        assert_eq!(
            SAFE_PROFILE
                .matches("<prop oor:name=\"Link\" oor:op=\"fuse\"><value>0</value></prop>")
                .count(),
            2
        );
    }
}

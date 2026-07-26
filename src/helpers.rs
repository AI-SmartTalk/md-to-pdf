use crate::types::*;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::Builder;

/// Maximum length of a client_id / pdf_name path component
const MAX_PATH_COMPONENT_LEN: usize = 128;

// ------------ Input sanitizing ------------

/// Validate a single path component (client_id or pdf_name) used to build a
/// filesystem path. Rejects anything that could escape `public/pdf`.
pub fn sanitize_path_component(value: &str, field: &str) -> Result<String, AppError> {
    if value.is_empty() {
        return Err(AppError::BadRequest(format!(
            "\"{}\" must not be empty",
            field
        )));
    }

    if value.len() > MAX_PATH_COMPONENT_LEN {
        return Err(AppError::BadRequest(format!(
            "\"{}\" must be at most {} characters",
            field, MAX_PATH_COMPONENT_LEN
        )));
    }

    if value.starts_with('.') {
        return Err(AppError::BadRequest(format!(
            "\"{}\" must not start with a dot",
            field
        )));
    }

    let valid = value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');

    if !valid {
        return Err(AppError::BadRequest(format!(
            "\"{}\" may only contain letters, digits, '-', '_' and '.'",
            field
        )));
    }

    Ok(value.to_string())
}

/// Escape a value that will be embedded inside a CSS string literal (content: "...")
pub fn escape_css_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' | '\r' => out.push(' '),
            _ => out.push(c),
        }
    }
    out
}

/// Escape a value that will be embedded inside HTML text content
pub fn escape_html(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// `page_number_format` is injected raw into the CSS (it is a CSS `content` value such as
/// `counter(page) " / " counter(pages)`), so it cannot be quoted. Reject anything that could
/// break out of the declaration or pull a remote resource.
fn validate_css_content_value(value: &str) -> Result<(), AppError> {
    const FORBIDDEN: [&str; 8] = ["{", "}", ";", "@", "<", ">", "url(", "expression("];

    let lowered = value.to_ascii_lowercase();
    for needle in FORBIDDEN {
        if lowered.contains(needle) {
            return Err(AppError::BadRequest(format!(
                "\"page_number_format\" must not contain `{}`",
                needle
            )));
        }
    }

    Ok(())
}

/// Margins land unquoted in the stylesheet, so they get the same treatment as
/// `page_number_format`: no way out of the declaration.
fn validate_css_length(value: &str, field: &str) -> Result<(), AppError> {
    let valid = !value.is_empty()
        && value.len() <= 32
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '%' || c == '-');

    if !valid {
        return Err(AppError::BadRequest(format!(
            "\"{}\" must be a CSS length such as \"2cm\"",
            field
        )));
    }

    Ok(())
}

// ------------ Process execution ------------

thread_local! {
    /// Deadline of the job running on this thread, if it declared one
    static JOB_DEADLINE: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
}

/// Bound a whole job — every process it spawns, however many passes it makes — by one
/// wall-clock deadline.
///
/// Per-process timeouts do not compose: a render that runs pandoc, then weasyprint, then
/// pdftotext and pdfinfo once per corrective pass can outlive the proxy timeout while each
/// individual process stayed inside its own limit. The proxy then answers 503 and the
/// render keeps holding its slot. The deadline is a thread-local because a job owns its
/// blocking thread from end to end (see `exec::offload`).
pub struct Budget;

impl Budget {
    pub fn start(total: Duration) -> Budget {
        JOB_DEADLINE.set(Some(Instant::now() + total));
        Budget
    }

    /// What is left of the deadline, or `None` when no job declared one
    pub fn remaining() -> Option<Duration> {
        JOB_DEADLINE.get().map(|deadline| {
            deadline
                .checked_duration_since(Instant::now())
                .unwrap_or_default()
        })
    }
}

impl Drop for Budget {
    fn drop(&mut self) {
        JOB_DEADLINE.set(None);
    }
}

/// The single wall-clock limit every external process runs under
pub fn process_timeout() -> Duration {
    let configured = crate::config::config().process_timeout;
    match Budget::remaining() {
        Some(remaining) => configured.min(remaining),
        None => configured,
    }
}

/// Wait for a child process, draining stdout/stderr on dedicated threads (so a large
/// output can never deadlock the writer) and killing it once the timeout expires.
fn wait_with_timeout(
    mut child: Child,
    stdin_data: Option<Vec<u8>>,
    label: &str,
) -> Result<Output, AppError> {
    let timeout = process_timeout();

    let stdin_thread = child.stdin.take().map(|mut stdin| {
        let data = stdin_data.unwrap_or_default();
        thread::spawn(move || {
            // A failing write means the child died early; the exit status reports it.
            let _ = stdin.write_all(&data);
            let _ = stdin.flush();
        })
    });

    let stdout_thread = child.stdout.take().map(|mut pipe| {
        thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            buf
        })
    });

    let stderr_thread = child.stderr.take().map(|mut pipe| {
        thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            buf
        })
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait()? {
            Some(status) => break status,
            None => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    error!("{} timed out after {}s", label, timeout.as_secs());
                    return Err(AppError::Timeout(format!(
                        "{} exceeded the {}s time limit",
                        label,
                        timeout.as_secs()
                    )));
                }
                thread::sleep(Duration::from_millis(25));
            }
        }
    };

    if let Some(handle) = stdin_thread {
        let _ = handle.join();
    }
    let stdout = stdout_thread
        .and_then(|h| h.join().ok())
        .unwrap_or_default();
    let stderr = stderr_thread
        .and_then(|h| h.join().ok())
        .unwrap_or_default();

    // A successful run still has things to say: the urlguard wrapper refuses a remote asset
    // and lets the render succeed without it, so this is the only place an operator can see
    // that a document silently lost an image.
    if status.success() && !stderr.is_empty() {
        let text = String::from_utf8_lossy(&stderr);
        let text = text.trim();
        if !text.is_empty() {
            warn!(
                "{} succeeded but wrote to stderr: {}",
                label,
                text.chars().take(MAX_LOGGED_STDERR).collect::<String>()
            );
        }
    }

    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

/// A renderer can produce pages of warnings; the log only needs enough to act on
const MAX_LOGGED_STDERR: usize = 2000;

/// Hand the weasyprint urlguard wrapper the policy this process resolved, instead of
/// letting it re-read the raw environment: a value the config validated or defaulted must
/// not be understood differently by the two halves of the same guard.
///
/// `stylesheet` is the temporary file pandoc turns into a `<link href>`, which the wrapper
/// then sees as a `file://` fetch.
fn apply_urlguard_env(cmd: &mut Command, stylesheet: Option<&str>) {
    let cfg = crate::config::config();

    cmd.env("PDF_ALLOWED_URL_HOSTS", cfg.allowed_url_hosts.join(","))
        .env(
            "PDF_URL_STRICT_HOSTS",
            if cfg.url_strict_hosts {
                "true"
            } else {
                "false"
            },
        )
        .env(
            "PDF_ALLOW_LOCAL_ASSETS",
            if cfg.allow_local_assets {
                "true"
            } else {
                "false"
            },
        );

    if let Some(path) = stylesheet {
        let mut value = OsString::from(path);
        if let Some(existing) = env::var_os("PDF_URLGUARD_ALLOW_FILES") {
            if !existing.is_empty() {
                value.push(":");
                value.push(existing);
            }
        }
        cmd.env("PDF_URLGUARD_ALLOW_FILES", value);
    }
}

/// A job that has already spent its wall-clock budget must not start one more process
fn budget_check(label: &str) -> Result<(), AppError> {
    if Budget::remaining() == Some(Duration::ZERO) {
        return Err(AppError::Timeout(format!(
            "the job exceeded its {}s budget before {} could run",
            crate::config::config().render_deadline.as_secs(),
            label
        )));
    }
    Ok(())
}

/// Shut the file-access primitives of TeX.
///
/// `\input{/etc/passwd}` is a perfectly ordinary LaTeX command, and the URL guard cannot
/// see it: it scans for URLs, not for TeX. kpathsea reads these three variables from the
/// environment, so paranoid mode is enforced on the engine itself rather than on a
/// blocklist of commands that would always miss one (`\openin`, `\InputIfFileExists`,
/// `\includegraphics`, ...).
fn apply_latex_sandbox(cmd: &mut Command) {
    cmd.env("openin_any", "p")
        .env("openout_any", "p")
        .env("shell_escape", "f")
        .arg("--pdf-engine-opt=-no-shell-escape");
}

/// Spawn a command with piped stdio and wait for it under the global timeout
fn run_command(cmd: &mut Command, label: &str) -> Result<Output, AppError> {
    budget_check(label)?;

    let child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            error!("Failed to spawn {}: {}", label, e);
            AppError::Io(e)
        })?;

    wait_with_timeout(child, None, label)
}

/// Turn a failed process into an AppError carrying its stderr
fn process_error(output: &Output, message: &str) -> AppError {
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    error!("{}: {}", message, stderr);
    AppError::ProcessFailed {
        message: message.to_string(),
        stderr,
    }
}

/// Run a command that only needs its exit status checked (qpdf, pdfunite, ...)
pub fn run_tool(cmd: &mut Command, label: &str, failure_message: &str) -> Result<(), AppError> {
    run_capture(cmd, label, failure_message).map(|_| ())
}

/// Run a command and hand its output back (pdfinfo, pdftotext, ...), under the same
/// timeout and the same error shape as every other external process
pub fn run_capture(
    cmd: &mut Command,
    label: &str,
    failure_message: &str,
) -> Result<Output, AppError> {
    let output = run_command(cmd, label)?;

    if !output.status.success() {
        return Err(process_error(&output, failure_message));
    }

    Ok(output)
}

// ------------ CSS ------------

/// Assemble the stylesheet in cascade order. The order is the contract: the client CSS
/// always outranks the theme and the options, and the corrective CSS the Layout Doctor
/// produces comes last because it exists precisely to override what broke the layout.
pub fn build_css_layers(
    theme_css: Option<&str>,
    options: Option<&PdfOptions>,
    custom_css: Option<&str>,
    corrective_css: Option<&str>,
) -> Result<tempfile::TempPath, AppError> {
    let default_css = fs::read_to_string("templates/default.css").map_err(AppError::Io)?;

    let options_css = match options {
        Some(opts) => options_to_css(opts)?,
        None => String::new(),
    };

    let layers = [
        Some(default_css.as_str()),
        theme_css,
        Some(options_css.as_str()),
        custom_css,
        corrective_css,
    ];

    let css_content = layers
        .into_iter()
        .flatten()
        .filter(|layer| !layer.is_empty())
        .collect::<Vec<&str>>()
        .join("\n");

    let mut css_file = Builder::new().suffix(".css").tempfile()?;
    css_file.write_all(css_content.as_bytes())?;
    Ok(css_file.into_temp_path())
}

/// Convert PdfOptions into CSS @page rules
pub fn options_to_css(opts: &PdfOptions) -> Result<String, AppError> {
    let mut rules = Vec::new();

    if let Some(ref size) = opts.paper_size {
        let size_str = size.to_string();
        let orientation_str = match opts.orientation {
            Some(Orientation::Landscape) => " landscape",
            _ => "",
        };
        rules.push(format!("size: {}{};", size_str, orientation_str));
    } else if let Some(Orientation::Landscape) = opts.orientation {
        rules.push("size: A4 landscape;".to_string());
    }

    if let Some(ref margins) = opts.margins {
        let top = margins.top.as_deref().unwrap_or("2cm");
        let right = margins.right.as_deref().unwrap_or("2cm");
        let bottom = margins.bottom.as_deref().unwrap_or("2cm");
        let left = margins.left.as_deref().unwrap_or("2cm");
        for (field, value) in [
            ("margins.top", top),
            ("margins.right", right),
            ("margins.bottom", bottom),
            ("margins.left", left),
        ] {
            validate_css_length(value, field)?;
        }
        rules.push(format!("margin: {} {} {} {};", top, right, bottom, left));
    }

    if opts.page_numbers.unwrap_or(false) {
        let format = opts
            .page_number_format
            .as_deref()
            .unwrap_or("counter(page)");
        validate_css_content_value(format)?;
        rules.push(format!(
            "@bottom-center {{ content: {}; font-size: 10pt; color: #666; }}",
            format
        ));
    }

    let mut css = String::new();

    if !rules.is_empty() {
        // Build the @page block; the @bottom-center must be nested inside @page
        let mut page_rules = Vec::new();
        let mut nested_rules = Vec::new();

        for rule in &rules {
            if rule.starts_with('@') {
                nested_rules.push(rule.as_str());
            } else {
                page_rules.push(rule.as_str());
            }
        }

        css.push_str("@page {\n");
        for r in &page_rules {
            css.push_str(&format!("  {}\n", r));
        }
        for r in &nested_rules {
            css.push_str(&format!("  {}\n", r));
        }
        css.push_str("}\n");
    }

    // Watermark via body::after
    if let Some(ref watermark) = opts.watermark {
        css.push_str(&format!(
            r#"body::after {{
  content: "{}";
  position: fixed;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%) rotate(-45deg);
  font-size: 80pt;
  color: rgba(0, 0, 0, 0.06);
  z-index: 9999;
  pointer-events: none;
  white-space: nowrap;
}}
"#,
            escape_css_string(watermark)
        ));
    }

    Ok(css)
}

/// Resolve header/footer: inline HTML takes priority over template file. The content is
/// returned rather than a file, because the cache key must depend on what a header/footer
/// contains and not on the name of the file it came from.
pub fn resolve_header_footer_content(
    inline_html: Option<&str>,
    template_name: Option<&str>,
) -> Result<Option<String>, AppError> {
    // Inline HTML takes priority
    if let Some(html) = inline_html {
        if !html.is_empty() {
            return Ok(Some(html.to_string()));
        }
    }

    // Fall back to template file
    if let Some(name) = template_name {
        if !name.is_empty() {
            // The name is used to build a path: keep it to a single, safe component.
            let name = sanitize_path_component(name, "header/footer template")?;
            let current_dir = env::current_dir()?;
            let path = current_dir.join("templates").join(&name);
            if path.exists() {
                return Ok(Some(fs::read_to_string(&path)?));
            } else {
                return Err(AppError::NotFound(format!(
                    "Template file not found: {}",
                    name
                )));
            }
        }
    }

    Ok(None)
}

/// Spill an HTML fragment into a temp file for a tool that only reads from disk
pub fn write_temp_html(content: &str) -> Result<tempfile::TempPath, AppError> {
    let mut file = Builder::new().suffix(".html").tempfile()?;
    file.write_all(content.as_bytes())?;
    Ok(file.into_temp_path())
}

// ------------ PDF generation ------------

/// Run pandoc to convert markdown to PDF
pub fn run_pandoc(
    markdown: &str,
    css_path: &str,
    engine: &PdfEngine,
    options: Option<&PdfOptions>,
    header_path: Option<&str>,
    footer_path: Option<&str>,
) -> Result<tempfile::TempPath, AppError> {
    let pdf_temp = Builder::new().suffix(".pdf").tempfile()?;
    let pdf_path = pdf_temp.path().to_str().ok_or_else(non_utf8_path)?;

    // pdflatex consumes LaTeX, not HTML: the intermediate format, the stylesheet and the
    // HTML header/footer includes only make sense for the HTML-based engines.
    let html_pipeline = !matches!(engine, PdfEngine::Pdflatex);

    let mut cmd = Command::new("pandoc");
    cmd.arg("--from=markdown+raw_html")
        .arg("--standalone")
        .arg("--variable=geometry:margin=1.5cm")
        .arg("--variable=papersize=a4")
        .arg(format!("--output={}", pdf_path))
        .arg(format!("--pdf-engine={}", engine));

    if html_pipeline {
        cmd.arg("--to=html5").arg(format!("--css={}", css_path));
        apply_urlguard_env(&mut cmd, Some(css_path));
    } else {
        cmd.arg("--to=latex");
        apply_latex_sandbox(&mut cmd);
    }

    // TOC support
    if let Some(opts) = options {
        if opts.toc.unwrap_or(false) {
            cmd.arg("--toc");
            if let Some(depth) = opts.toc_depth {
                cmd.arg(format!("--toc-depth={}", depth));
            }
        }
    }

    if html_pipeline {
        if let Some(header) = header_path {
            cmd.arg(format!("--include-in-header={}", header));
        }
        if let Some(footer) = footer_path {
            cmd.arg(format!("--include-after-body={}", footer));
        }
    } else if header_path.is_some() || footer_path.is_some() {
        warn!("HTML header/footer are ignored with the pdflatex engine");
    }

    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    budget_check("pandoc")?;

    let child = cmd.spawn().map_err(|e| {
        error!("Failed to spawn pandoc: {}", e);
        AppError::Io(e)
    })?;

    let output = wait_with_timeout(child, Some(markdown.as_bytes().to_vec()), "pandoc")?;

    if !output.status.success() {
        return Err(process_error(&output, "Pandoc conversion failed"));
    }

    Ok(pdf_temp.into_temp_path())
}

/// Run weasyprint to convert HTML to PDF directly (no pandoc)
pub fn run_weasyprint(html: &str, css_path: &str) -> Result<tempfile::TempPath, AppError> {
    weasyprint(html, Some(css_path))
}

/// Same, for HTML that carries its own `<style>` and needs no stylesheet
pub fn run_weasyprint_plain(html: &str) -> Result<tempfile::TempPath, AppError> {
    weasyprint(html, None)
}

fn weasyprint(html: &str, css_path: Option<&str>) -> Result<tempfile::TempPath, AppError> {
    let html_path = write_temp_html(html)?;
    let html_path_str = html_path.to_str().ok_or_else(non_utf8_path)?;

    let pdf_temp = Builder::new().suffix(".pdf").tempfile()?;
    let pdf_path = pdf_temp
        .path()
        .to_str()
        .ok_or_else(non_utf8_path)?
        .to_string();

    // The HTML lives in a temp dir, so relative assets (static/blured.png, custom images)
    // would resolve against /tmp. Anchor them on the working directory instead.
    let mut base_url = env::current_dir()?.to_string_lossy().to_string();
    if !base_url.ends_with('/') {
        base_url.push('/');
    }

    let mut cmd = Command::new("weasyprint");
    cmd.arg(html_path_str).arg(&pdf_path);
    if let Some(css_path) = css_path {
        cmd.arg("--stylesheet").arg(css_path);
    }
    cmd.arg("--base-url").arg(&base_url);
    apply_urlguard_env(&mut cmd, css_path);

    run_capture(&mut cmd, "weasyprint", "Weasyprint conversion failed")?;

    Ok(pdf_temp.into_temp_path())
}

// ------------ Storage ------------

/// Root directory holding every generated PDF
pub fn pdf_root() -> PathBuf {
    Path::new("public").join("pdf")
}

/// Save the PDF under public/pdf/<client_id>/<pdf_name> and return its download URL
pub fn save_pdf(pdf_path: &Path, client_id: &str, pdf_name: &str) -> Result<String, AppError> {
    let client_id = sanitize_path_component(client_id, "client_id")?;
    let pdf_name = sanitize_path_component(pdf_name, "pdf_name")?;

    let client_dir = pdf_root().join(&client_id);
    fs::create_dir_all(&client_dir)?;

    let final_pdf_name = if pdf_name.ends_with(".pdf") {
        pdf_name
    } else {
        format!("{}.pdf", pdf_name)
    };

    let out_path = client_dir.join(&final_pdf_name);
    fs::copy(pdf_path, &out_path)?;

    Ok(format!("/download/{}/{}", client_id, final_pdf_name))
}

/// Hand a produced PDF back the way every route does: the file itself, or the JSON
/// `download_url` when the caller asked for it to be saved.
pub async fn deliver(
    pdf: tempfile::TempPath,
    download_url: Option<String>,
) -> Result<
    rocket::Either<rocket::fs::NamedFile, rocket::serde::json::Json<ConvertResponse>>,
    AppError,
> {
    match download_url {
        Some(url) => Ok(rocket::Either::Right(rocket::serde::json::Json(
            ConvertResponse::new(url),
        ))),
        None => Ok(rocket::Either::Left(
            rocket::fs::NamedFile::open(&pdf)
                .await
                .map_err(AppError::Io)?,
        )),
    }
}

/// Save the PDF when the caller named a destination. Filesystem work like the tool run it
/// follows, so it belongs on the same blocking thread.
pub fn save_if_requested(
    pdf: &Path,
    client_id: Option<String>,
    pdf_name: Option<String>,
) -> Result<Option<String>, AppError> {
    match (client_id, pdf_name) {
        (Some(client_id), Some(pdf_name)) => Ok(Some(save_pdf(pdf, &client_id, &pdf_name)?)),
        _ => Ok(None),
    }
}

/// Resolution the preview has always been rendered at
pub const PREVIEW_DPI: u32 = 150;

/// Resolve a /download/... path to the actual filesystem path with validation
pub fn resolve_pdf_path(url: &str) -> Result<PathBuf, AppError> {
    // Accept paths like /download/client_id/file.pdf
    let stripped = url.trim_start_matches('/');
    let stripped = stripped.strip_prefix("download/").unwrap_or(stripped);

    let mut segments = stripped.split('/');
    let client_id = segments.next().unwrap_or_default();
    let pdf_name = segments.next().unwrap_or_default();

    if segments.next().is_some() {
        return Err(AppError::BadRequest(format!(
            "Invalid PDF path: {} (expected /download/<client_id>/<pdf_name>)",
            url
        )));
    }

    let client_id = sanitize_path_component(client_id, "client_id")?;
    let pdf_name = sanitize_path_component(pdf_name, "pdf_name")?;

    let root = pdf_root();
    // A fresh container has no public/pdf yet: create it so canonicalize() can succeed
    fs::create_dir_all(&root)?;

    let base = root
        .canonicalize()
        .map_err(|_| AppError::NotFound("PDF directory not found".to_string()))?;

    let canonical = base
        .join(&client_id)
        .join(&pdf_name)
        .canonicalize()
        .map_err(|_| AppError::NotFound(format!("PDF not found: {}", url)))?;

    // Defense in depth: a symlink inside public/pdf must not escape it either
    if !canonical.starts_with(&base) {
        return Err(AppError::BadRequest("Invalid PDF path".to_string()));
    }

    Ok(canonical)
}

fn non_utf8_path() -> AppError {
    AppError::BadRequest("Non UTF-8 path".to_string())
}

/// Borrow a path as &str, turning a non UTF-8 path into a proper API error
pub fn path_to_str(path: &Path) -> Result<&str, AppError> {
    path.to_str().ok_or_else(non_utf8_path)
}

/// Is an external tool actually installed in this image?
pub fn binary_available(name: &str) -> bool {
    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}

/// Base64 for `data:` URIs. A dedicated crate for twenty lines of table lookup would be
/// one more dependency to audit in the production image.
pub fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let bytes = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let packed = (bytes[0] as u32) << 16 | (bytes[1] as u32) << 8 | (bytes[2] as u32);

        out.push(ALPHABET[(packed >> 18 & 63) as usize] as char);
        out.push(ALPHABET[(packed >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(packed >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(packed & 63) as usize] as char
        } else {
            '='
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Without the job deadline a render could chain passes until the proxy gave up on it
    #[test]
    fn the_job_deadline_shortens_every_process_timeout() {
        let configured = crate::config::config().process_timeout;
        assert_eq!(process_timeout(), configured);

        {
            let _budget = Budget::start(Duration::from_millis(1));
            assert!(process_timeout() < configured);
            std::thread::sleep(Duration::from_millis(5));
            assert_eq!(process_timeout(), Duration::ZERO);
            assert!(budget_check("pandoc").is_err());
        }

        assert_eq!(process_timeout(), configured);
        assert!(budget_check("pandoc").is_ok());
    }

    #[test]
    fn encodes_base64_with_padding() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
    }
}

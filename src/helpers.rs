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

    /// Bound one stretch of a job more tightly than the job itself, until the guard drops.
    ///
    /// For work that is worth having and not worth the request: a verification pass, a
    /// second opinion, an optional enrichment. Without a cap, one of those can spend the
    /// whole deadline and leave nothing for the part the caller actually asked for — which
    /// is exactly what `/api/pdf-to-office` did, every single time, for sixty seconds.
    ///
    /// A cap only ever shortens. Handing it an hour inside a two-minute job changes nothing.
    pub fn cap(most: Duration) -> Cap {
        let previous = JOB_DEADLINE.get();
        let capped = Instant::now() + most;

        JOB_DEADLINE.set(Some(match previous {
            Some(existing) => existing.min(capped),
            None => capped,
        }));

        Cap(previous)
    }
}

/// Restores the deadline a `Budget::cap` shortened. Holds the previous value rather than
/// clearing it: a cap sits *inside* a job, and clearing would hand the rest of that job an
/// unlimited budget.
pub struct Cap(Option<Instant>);

impl Drop for Cap {
    fn drop(&mut self) {
        JOB_DEADLINE.set(self.0);
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

/// Start a child in a process group of its own, so the timeout can reach its descendants.
///
/// `Child::kill` signals one pid, and half the converters this service drives are not the
/// process that does the work: `/usr/bin/soffice` is a shell script that execs `oosplash`,
/// which forks `soffice.bin` and waits. Killing the pid we spawned leaves `soffice.bin`
/// running — measured at 99.9% of a core, for hours, holding the stderr pipe open so the
/// reader thread never returns either. One runaway conversion per timeout, and the host
/// eventually has nothing left to render with.
///
/// A group of its own also means the signal cannot travel *up*: the group id is the child's
/// own pid, so nothing this service needs is ever in range.
pub fn own_process_group(cmd: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
}

/// SIGKILL a timed-out child and everything it forked.
///
/// Safe to call on a child that has already exited: its group is then empty and `killpg`
/// answers ESRCH, which is exactly the outcome wanted.
pub fn kill_process_group(child: &mut Child) {
    #[cfg(unix)]
    {
        // `own_process_group` made the child a group leader, so its pid is the group id.
        // Nothing else can be in that group.
        let group = child.id() as libc::pid_t;
        unsafe { libc::killpg(group, libc::SIGKILL) };
    }

    // The direct child is signalled again — harmless — and, crucially, reaped: without a
    // `wait` it stays a zombie for the life of the process.
    let _ = child.kill();
    let _ = child.wait();
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
                    kill_process_group(&mut child);
                    // The reader threads are deliberately not joined: killing the group
                    // closes the pipes, so they finish on their own, and a descendant that
                    // somehow survived must not be able to hold this render slot hostage.
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

/// Variables a child process genuinely needs. Everything else is left behind.
const INHERITED_ENV: [&str; 6] = ["PATH", "HOME", "LANG", "LC_ALL", "TMPDIR", "TERM"];

/// Hand a child only the environment it needs.
///
/// Until this service accepted uploads, every process it spawned worked on content it had
/// written itself. Now Ghostscript, LibreOffice and Tesseract parse bytes chosen by
/// strangers — and they inherited the whole environment of the service, `API_KEY`,
/// `ATTESTATION_SECRET` and the log420 token included. Ghostscript has a history of sandbox
/// escapes and LibreOffice runs macros; making sure whatever runs in there finds nothing
/// worth stealing costs a dozen lines.
///
/// Variables the caller set on the command survive, and win: that is how the urlguard
/// policy and the TeX lockdown reach the child.
fn sanitize_env(cmd: &mut Command) {
    let explicit: Vec<(OsString, Option<OsString>)> = cmd
        .get_envs()
        .map(|(key, value)| (key.to_os_string(), value.map(|value| value.to_os_string())))
        .collect();

    cmd.env_clear();

    for key in INHERITED_ENV {
        if let Some(value) = env::var_os(key) {
            cmd.env(key, value);
        }
    }

    for (key, value) in explicit {
        match value {
            Some(value) => cmd.env(key, value),
            None => cmd.env_remove(key),
        };
    }
}

/// Spawn a command with piped stdio and wait for it under the global timeout
fn run_command(cmd: &mut Command, label: &str) -> Result<Output, AppError> {
    spawn_and_wait(cmd, None, label)
}

/// The one place in this service where an external program starts.
///
/// Both spawn sites — `run_command` and `run_pandoc`, which differs only by feeding stdin —
/// come through here, and that is what made the sandbox possible without touching thirteen
/// routes: when `SANDBOX_SPOOL` is set the child runs in a container with no network, and
/// the caller cannot tell the difference. Unset, it runs here exactly as it always has,
/// which is what development and every existing deployment get.
fn spawn_and_wait(
    cmd: &mut Command,
    stdin_data: Option<Vec<u8>>,
    label: &str,
) -> Result<Output, AppError> {
    budget_check(label)?;
    sanitize_env(cmd);

    if crate::sandbox::enabled() {
        // The remaining budget travels with the job: the worker holds the process, so it is
        // the only side that can enforce a deadline on it.
        return crate::sandbox::run(cmd, stdin_data, label, process_timeout());
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    if stdin_data.is_some() {
        cmd.stdin(Stdio::piped());
    }
    own_process_group(cmd);

    let child = cmd.spawn().map_err(|e| {
        error!("Failed to spawn {}: {}", label, e);
        AppError::Io(e)
    })?;

    wait_with_timeout(child, stdin_data, label)
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

    let output = spawn_and_wait(&mut cmd, Some(markdown.as_bytes().to_vec()), "pandoc")?;

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

/// The invocation, built apart from the running so it can be asserted on.
fn weasyprint_command(html: &str, pdf: &str, css: Option<&str>, base_url: &str) -> Command {
    let mut cmd = Command::new("weasyprint");
    cmd.arg(html).arg(pdf);

    if let Some(css) = css {
        cmd.arg("--stylesheet").arg(css);
    }

    cmd.arg("--base-url").arg(base_url);

    // Stating a fact, not making a guess: the document reaching this function came out of a
    // Rust `String`, which is UTF-8 by construction, and `write_temp_html` wrote those exact
    // bytes. Without the flag WeasyPrint applies the HTML5 default for a document that
    // declares nothing — windows-1252 — and every accented character in a caller's HTML came
    // back as mojibake: `Modèle` rendered as `ModÃ¨le`, silently, with a 200. Markdown never
    // showed it because pandoc emits a `<meta charset>` of its own; `/api/html-to-pdf` and
    // `/api/render` had no such luck.
    cmd.arg("--encoding").arg("utf-8");

    cmd
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

    let mut cmd = weasyprint_command(html_path_str, &pdf_path, css_path, &base_url);
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

/// Decide what a tool hands back, from inside the blocking closure that produced it.
///
/// The three destinations are not exclusive by accident: a caller can want the file saved
/// under a stable download URL *and* an asset handle to feed the next tool. What is
/// exclusive is streaming the binary, which only happens when nothing else was asked for.
///
/// `save_if_requested` runs first because `assets::store` moves the file away.
pub fn finish_tool(
    produced: &Path,
    client_id: Option<String>,
    pdf_name: Option<String>,
    output: ToolOutput,
    name_hint: &str,
) -> Result<ToolResponse, AppError> {
    let download_url = save_if_requested(produced, client_id, pdf_name)?;

    let asset = match output {
        ToolOutput::Asset => Some(crate::assets::store(produced, name_hint)?),
        ToolOutput::Binary => None,
    };

    // Who this work belongs to can only be read here, on the worker thread that carries the
    // owner scope. The record itself is written later, in `deliver_tool`, because the verdict
    // — the only reason the record is worth keeping — is not attached to the response until
    // after this function has returned.
    let account = crate::accounts::account_for_owner(&crate::assets::current_owner_name());

    Ok(ToolResponse {
        download_url,
        asset,
        account,
        source_name: crate::assets::current_source_name(),
        ..Default::default()
    })
}

/// Hand a tool result back: JSON when the caller asked for a URL or an asset, the file
/// itself otherwise. Mirrors `deliver`, which the older routes keep using.
pub async fn deliver_tool(
    produced: tempfile::TempPath,
    response: ToolResponse,
    tool: &str,
) -> Result<rocket::Either<rocket::fs::NamedFile, rocket::serde::json::Json<ToolResponse>>, AppError>
{
    // The quality record is written here because this is the one place that sees the whole
    // operation: the file, the page count, and the verdict the route attached after
    // `finish_tool` returned. Writing it earlier is what made every entry verdictless.
    crate::history::record(&response, tool);

    let wants_json =
        response.download_url.is_some() || response.asset.is_some() || response.assets.is_some();

    if wants_json {
        return Ok(rocket::Either::Right(rocket::serde::json::Json(response)));
    }

    Ok(rocket::Either::Left(
        rocket::fs::NamedFile::open(&produced)
            .await
            .map_err(AppError::Io)?,
    ))
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

/// Resolve any reference a caller may hand us to a file on disk.
///
/// Two forms are accepted, and the order matters only for readability: `asset://as_…` for
/// something that was uploaded, and `/download/<client_id>/<name>.pdf` for something this
/// service produced. The second form is `resolve_pdf_path` untouched — every route that
/// existed before uploads did keeps working byte for byte, which is the whole point of
/// adding a function rather than changing one.
pub fn resolve_source(reference: &str) -> Result<PathBuf, AppError> {
    match crate::assets::strip_scheme(reference) {
        Some(id) => {
            // The one place every tool passes through with the *caller's* name still in
            // hand — see `assets::note_source_name` for why the record needs it.
            if let Ok(meta) = crate::assets::meta(id) {
                crate::assets::note_source_name(&meta.name);
            }
            crate::assets::path(id)
        }
        None => resolve_pdf_path(reference),
    }
}

/// Same, plus the guarantee that what came back is a PDF.
///
/// An asset can be a spreadsheet or a photograph. Handing one of those to qpdf produces a
/// confusing parser error three layers down; saying so here produces a 400 the caller can
/// act on.
pub fn resolve_pdf_source(reference: &str) -> Result<PathBuf, AppError> {
    if let Some(id) = crate::assets::strip_scheme(reference) {
        let meta = crate::assets::meta(id)?;
        if meta.kind != crate::assets::AssetKind::Pdf {
            return Err(AppError::BadRequest(format!(
                "Asset {} is a {} file, this endpoint needs a PDF",
                meta.id,
                meta.kind.as_str()
            )));
        }
    }

    resolve_source(reference)
}

/// The wordings the tools of this image use for that one refusal — poppler says
/// `Command Line Error: Incorrect password`, qpdf `invalid password`, Ghostscript
/// `This file requires a password for access`. Reading the sentence rather than the exit
/// code is what keeps a genuine breakdown a 500: nothing else in these streams says
/// "password".
pub fn refused_for_password(stderr: &str) -> bool {
    const REFUSALS: [&str; 4] = [
        "incorrect password",
        "invalid password",
        "requires a password",
        "password required",
    ];

    let stderr = stderr.to_lowercase();
    REFUSALS.iter().any(|refusal| stderr.contains(refusal))
}

/// Same, plus the guarantee that the PDF can actually be opened.
///
/// A document a stranger uploaded and poppler cannot parse is not a fault of this service,
/// and answering 500 with `pdfinfo failed` and three lines of poppler's stderr says the
/// opposite: it tells the caller to retry, tells the operator to investigate, and tells
/// nobody what to do about it. Measured across the toolbelt, that was fourteen endpoints
/// answering 500 to the same five malformed files.
///
/// The check is free in the ordinary case. The asset store already counts a PDF's pages when
/// it takes the file in, so a stored PDF with no page count is one nothing will be able to
/// read; only that already-broken path pays for a diagnosis.
///
/// `/api/repair` and `/api/unlock` deliberately do **not** use this: accepting a document
/// nothing else can open is their entire purpose.
pub fn resolve_readable_pdf(reference: &str) -> Result<PathBuf, AppError> {
    let path = resolve_pdf_source(reference)?;

    if !counted_at_rest(reference) {
        return Err(why_unreadable(&path));
    }

    Ok(path)
}

/// Did the store manage to read this document when it came in?
///
/// Only asset references can answer: a `/download/…` path names a file this service produced
/// itself, and one of those that cannot be read is a bug here, not a bad upload — it must
/// stay the 500 it is.
fn counted_at_rest(reference: &str) -> bool {
    let Some(id) = crate::assets::strip_scheme(reference) else {
        return true;
    };

    match crate::assets::meta(id) {
        Ok(meta) => meta.kind != crate::assets::AssetKind::Pdf || meta.pages.is_some(),
        // The asset is gone or unreadable; `resolve_pdf_source` above already failed on it
        Err(_) => true,
    }
}

/// Ask why, once, on the path where the answer is already bad news.
///
/// Two causes, two remedies, and the difference matters to the caller: a document that asks
/// for a password is opened with `/api/unlock`, one whose structure is damaged with
/// `/api/repair`. Telling someone to repair a file that merely needed its password is how a
/// service earns a support ticket.
fn why_unreadable(pdf: &Path) -> AppError {
    let refusal = match run_capture(
        Command::new("pdfinfo").arg(match path_to_str(pdf) {
            Ok(path) => path,
            Err(err) => return err,
        }),
        "pdfinfo",
        "pdfinfo failed",
    ) {
        // pdfinfo read it after all — the store's failure was transient. Nothing to refuse.
        Ok(_) => {
            return AppError::BadRequest(
                "This PDF could not be read when it was uploaded. Upload it again.".to_string(),
            )
        }
        Err(err) => err,
    };

    if let AppError::ProcessFailed { stderr, .. } = &refusal {
        if refused_for_password(stderr) {
            return AppError::BadRequest(
                "This PDF is encrypted: it asks for a password before anything can be read \
                 from it. Remove the protection with POST /api/unlock, which takes the \
                 password, then send the file it hands back to this route."
                    .to_string(),
            );
        }
    }

    // A timeout is a timeout: the document may be fine and the machine busy.
    if matches!(refusal, AppError::Timeout(_)) {
        return refusal;
    }

    AppError::BadRequest(
        "This file is not a readable PDF: its structure is damaged, or it is not a PDF at \
         all. POST /api/repair rebuilds what can be rebuilt and tells you what it recovered."
            .to_string(),
    )
}

fn non_utf8_path() -> AppError {
    AppError::BadRequest("Non UTF-8 path".to_string())
}

/// Borrow a path as &str, turning a non UTF-8 path into a proper API error
pub fn path_to_str(path: &Path) -> Result<&str, AppError> {
    path.to_str().ok_or_else(non_utf8_path)
}

/// `<prefix>_` followed by 32 hexadecimal characters drawn from the kernel.
///
/// Used for asset and job identifiers. No `uuid` and no `rand` crate: the production image
/// is audited dependency by dependency, and this is sixteen bytes read from a file.
pub fn random_id(prefix: &str) -> Result<String, AppError> {
    const ID_BYTES: usize = 16;

    let mut buf = [0u8; ID_BYTES];
    fs::File::open("/dev/urandom")?.read_exact(&mut buf)?;

    let mut id = String::with_capacity(prefix.len() + 1 + ID_BYTES * 2);
    id.push_str(prefix);
    id.push('_');
    for byte in buf {
        id.push_str(&format!("{:02x}", byte));
    }
    Ok(id)
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

    /// A verification pass that spends the whole job's deadline leaves nothing for the job
    #[test]
    fn a_cap_shortens_the_deadline_for_its_own_scope_and_gives_it_back() {
        let _budget = Budget::start(Duration::from_secs(60));

        {
            let _cap = Budget::cap(Duration::from_secs(1));
            assert!(Budget::remaining().unwrap() <= Duration::from_secs(1));
        }

        assert!(Budget::remaining().unwrap() > Duration::from_secs(30));
    }

    /// A cap is a ceiling, never a grant: it must not resurrect a budget already spent
    #[test]
    fn a_cap_never_extends_the_deadline_it_sits_inside() {
        let _budget = Budget::start(Duration::from_millis(1));
        thread::sleep(Duration::from_millis(5));

        let _cap = Budget::cap(Duration::from_secs(600));
        assert_eq!(Budget::remaining(), Some(Duration::ZERO));
    }

    /// Outside a job there is no deadline to shorten, and the cap becomes the only one
    #[test]
    fn a_cap_with_no_job_around_it_still_bounds_the_work() {
        {
            let _cap = Budget::cap(Duration::from_secs(2));
            assert!(Budget::remaining().unwrap() <= Duration::from_secs(2));
        }

        assert_eq!(Budget::remaining(), None);
    }

    /// The regression that made every `soffice` timeout cost a core, permanently.
    ///
    /// The launcher stands in for `/usr/bin/soffice`: it forks the process that does the
    /// work and then waits. `Child::kill` reaches the launcher only, so before the group
    /// kill the forked half kept running — and kept writing.
    #[cfg(unix)]
    #[test]
    fn a_timeout_takes_the_processes_the_child_forked_with_it() {
        let marker = Builder::new()
            .suffix(".alive")
            .tempfile()
            .unwrap()
            .into_temp_path();

        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(format!(
            "sh -c 'while :; do echo . >> {} ; sleep 0.02; done' & wait",
            marker.display()
        ));

        // Short enough to keep the suite quick, long enough for the grandchild to have
        // written something — an assertion on a file nothing ever touched proves nothing.
        let _budget = Budget::start(Duration::from_millis(400));
        let error = run_command(&mut cmd, "launcher").unwrap_err();
        assert!(
            matches!(error, AppError::Timeout(_)),
            "expected a timeout, got {:?}",
            error
        );

        let written = fs::metadata(&marker).unwrap().len();
        assert!(
            written > 0,
            "the grandchild never ran, the test proves nothing"
        );

        thread::sleep(Duration::from_millis(300));
        assert_eq!(
            fs::metadata(&marker).unwrap().len(),
            written,
            "a process the timed-out child forked is still running"
        );
    }

    /// A child that finishes on its own must not be disturbed by any of the above
    #[cfg(unix)]
    #[test]
    fn a_child_in_its_own_group_still_reports_its_output_and_status() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("printf out; printf err >&2; exit 3");

        let output = run_command(&mut cmd, "shell").unwrap();

        assert_eq!(output.stdout, b"out");
        assert_eq!(output.stderr, b"err");
        assert_eq!(output.status.code(), Some(3));
    }

    /// The refusal these two routes hang a 400 on. Reading the sentence and not the exit
    /// code is what keeps a genuine breakdown a 500 — so the sentence has to be right.
    #[test]
    fn the_wordings_the_three_tools_use_for_a_password_are_all_recognised() {
        for stderr in [
            "Command Line Error: Incorrect password",
            "qpdf: in.pdf: invalid password",
            "This file requires a password for access",
            "GPL Ghostscript: password required",
        ] {
            assert!(refused_for_password(stderr), "{}", stderr);
        }

        for stderr in [
            "Syntax Error: Couldn't find trailer dictionary",
            "gs: out of memory",
            "",
        ] {
            assert!(!refused_for_password(stderr), "{}", stderr);
        }
    }

    /// A `/download/…` reference names a file this service produced. One of those that
    /// cannot be read is a bug here, and must keep the 500 that says so.
    #[test]
    fn only_an_uploaded_document_is_held_to_the_readability_check() {
        assert!(counted_at_rest("/download/client/report.pdf"));
        assert!(counted_at_rest("report.pdf"));
        // An asset the store cannot even name is `resolve_pdf_source`'s refusal, not this one
        assert!(counted_at_rest("asset://as_does_not_exist"));
    }

    /// Without this flag WeasyPrint reads a document that declares no charset as
    /// windows-1252, and `/api/html-to-pdf` returned `ModÃ¨le` for `Modèle` — with a 200.
    #[test]
    fn weasyprint_is_told_the_document_is_utf8_because_it_always_is() {
        let cmd = weasyprint_command("/tmp/in.html", "/tmp/out.pdf", None, "file:///work/");
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();

        let at = args
            .iter()
            .position(|a| a == "--encoding")
            .unwrap_or_else(|| panic!("no --encoding in {:?}", args));
        assert_eq!(args[at + 1], "utf-8");
    }

    #[test]
    fn a_stylesheet_is_passed_only_when_there_is_one() {
        let with = weasyprint_command("in", "out", Some("/tmp/a.css"), "base");
        let args: Vec<String> = with
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"--stylesheet".to_string()));
        assert!(args.contains(&"/tmp/a.css".to_string()));

        let without = weasyprint_command("in", "out", None, "base");
        let args: Vec<String> = without
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(!args.contains(&"--stylesheet".to_string()));
    }

    /// A parser fed hostile bytes must not find the service's secrets next to it
    #[test]
    fn a_child_process_does_not_inherit_the_service_secrets() {
        std::env::set_var("API_KEY", "s3cr3t-for-the-test");
        std::env::set_var("PATH", "/usr/bin:/bin");

        let mut cmd = Command::new("true");
        cmd.env("PDF_ALLOWED_URL_HOSTS", "example.com");
        sanitize_env(&mut cmd);

        let passed: Vec<(String, Option<String>)> = cmd
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect();

        let value_of = |name: &str| {
            passed
                .iter()
                .find(|(k, _)| k == name)
                .and_then(|(_, v)| v.clone())
        };

        assert_eq!(
            value_of("API_KEY"),
            None,
            "the API key must not reach a child"
        );
        // What the caller set explicitly still gets through, and still wins
        assert_eq!(
            value_of("PDF_ALLOWED_URL_HOSTS"),
            Some("example.com".to_string())
        );
        // …and the child keeps what it needs to run at all
        assert_eq!(value_of("PATH"), Some("/usr/bin:/bin".to_string()));

        std::env::remove_var("API_KEY");
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

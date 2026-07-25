use crate::types::*;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::Builder;

/// The censor replacement HTML block used to replace CENSOR tags in markdown
const CENSOR_REPLACEMENT: &str = "<div style=\"width:100% !important; max-width:100% !important; margin-top:0 !important; margin-bottom:0 !important; margin-left:0 !important; margin-right:0 !important; padding:0 !important; overflow:hidden !important; position:relative !important; left:0 !important; right:auto !important; box-sizing:border-box !important; text-align:left !important;\"><img src=\"static/blured.png\" alt=\"CONTENU PREMIUM - Achetez le rapport complet\" style=\"width:100% !important; height:auto !important; display:block !important; margin:0 !important; padding:0 !important; float:none !important;\"></div>";

/// Default wall-clock limit for every external process (pandoc, weasyprint, qpdf, ...).
/// Overridable with PDF_PROCESS_TIMEOUT_SECS.
const DEFAULT_PROCESS_TIMEOUT_SECS: u64 = 60;

/// Maximum length of a client_id / pdf_name path component
const MAX_PATH_COMPONENT_LEN: usize = 128;

/// Process CENSOR tags, replacing them with the blurred image block.
/// Applies to markdown as well as raw HTML input.
pub fn process_censor(input: &str) -> String {
    let mut result = input.to_string();
    result = result.replace("{{CENSOR}}", CENSOR_REPLACEMENT);
    result = result.replace("<CENSOR>", CENSOR_REPLACEMENT);
    result = result.replace("{{ CENSOR }}", CENSOR_REPLACEMENT);
    result = result.replace("{{CENSOR }}", CENSOR_REPLACEMENT);
    result = result.replace("{{ CENSOR}}", CENSOR_REPLACEMENT);
    result
}

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

fn process_timeout() -> Duration {
    let secs = env::var("PDF_PROCESS_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_PROCESS_TIMEOUT_SECS);
    Duration::from_secs(secs)
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

    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

/// Spawn a command with piped stdio and wait for it under the global timeout
fn run_command(cmd: &mut Command, label: &str) -> Result<Output, AppError> {
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
    let output = run_command(cmd, label)?;

    if !output.status.success() {
        return Err(process_error(&output, failure_message));
    }

    Ok(())
}

// ------------ CSS ------------

/// Build a combined CSS file from default.css + custom CSS + options-generated CSS
pub fn build_css(
    custom_css: Option<&str>,
    options: Option<&PdfOptions>,
) -> Result<tempfile::TempPath, AppError> {
    let default_css = fs::read_to_string("templates/default.css").map_err(AppError::Io)?;

    let options_css = match options {
        Some(opts) => options_to_css(opts)?,
        None => String::new(),
    };

    let css_content = match custom_css {
        Some(css) => format!("{}\n{}\n{}", default_css, options_css, css),
        None => format!("{}\n{}", default_css, options_css),
    };

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

/// Resolve header/footer: inline HTML takes priority over template file
pub fn resolve_header_footer(
    inline_html: Option<&str>,
    template_name: Option<&str>,
) -> Result<Option<tempfile::TempPath>, AppError> {
    // Inline HTML takes priority
    if let Some(html) = inline_html {
        if !html.is_empty() {
            let mut file = Builder::new().suffix(".html").tempfile()?;
            file.write_all(html.as_bytes())?;
            return Ok(Some(file.into_temp_path()));
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
                let content = fs::read_to_string(&path)?;
                let mut file = Builder::new().suffix(".html").tempfile()?;
                file.write_all(content.as_bytes())?;
                return Ok(Some(file.into_temp_path()));
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
    } else {
        cmd.arg("--to=latex");
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
    // Write HTML to temp file
    let mut html_file = Builder::new().suffix(".html").tempfile()?;
    html_file.write_all(html.as_bytes())?;
    let html_path = html_file.into_temp_path();
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

    let output = run_command(
        Command::new("weasyprint")
            .arg(html_path_str)
            .arg(&pdf_path)
            .arg("--stylesheet")
            .arg(css_path)
            .arg("--base-url")
            .arg(&base_url),
        "weasyprint",
    )?;

    if !output.status.success() {
        return Err(process_error(&output, "Weasyprint conversion failed"));
    }

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

/// Convert the first page of a PDF to a PNG using pdftoppm
pub fn pdf_to_png(pdf_path: &Path) -> Result<Vec<u8>, AppError> {
    let png_prefix = Builder::new().prefix("preview-").tempfile()?;
    let prefix_path = png_prefix
        .path()
        .to_str()
        .ok_or_else(non_utf8_path)?
        .to_string();

    // pdftoppm creates files like <prefix>-1.png; some versions pad the index
    let candidates = [
        format!("{}-1.png", prefix_path),
        format!("{}-01.png", prefix_path),
    ];

    // Whatever happens below, never leave a stray PNG behind
    let cleanup = || {
        for candidate in &candidates {
            let _ = fs::remove_file(candidate);
        }
    };

    let output = run_command(
        Command::new("pdftoppm")
            .arg("-png")
            .arg("-f")
            .arg("1")
            .arg("-l")
            .arg("1")
            .arg("-r")
            .arg("150")
            .arg(pdf_path.to_str().ok_or_else(non_utf8_path)?)
            .arg(&prefix_path),
        "pdftoppm",
    );

    let output = match output {
        Ok(output) => output,
        Err(e) => {
            cleanup();
            return Err(e);
        }
    };

    if !output.status.success() {
        cleanup();
        return Err(process_error(&output, "pdftoppm failed"));
    }

    let actual_path = match candidates.iter().find(|p| Path::new(p).exists()) {
        Some(path) => path.clone(),
        None => {
            cleanup();
            return Err(AppError::ProcessFailed {
                message: "PNG file not found after pdftoppm".to_string(),
                stderr: String::new(),
            });
        }
    };

    let png_data = fs::read(&actual_path);
    cleanup();
    Ok(png_data?)
}

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

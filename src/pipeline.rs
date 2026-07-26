//! The single rendering path.
//!
//! Every route that produces a PDF goes through here, so the cache, the block expansion,
//! the themes, the Layout Doctor and the anti-SSRF check are decided in exactly one place
//! and cannot drift between endpoints.

use crate::cache;
use crate::censor::{self, CensorConfig};
use crate::config::config;
use crate::exec;
use crate::helpers;
use crate::layout;
use crate::obs;
use crate::themes::{self, Theme};
use crate::types::*;
use crate::urlguard;
use crate::{blocks, pdfops};
use rocket::fs::NamedFile;
use rocket::serde::json::Json;
use rocket::Either;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Instant;
use tempfile::{Builder, TempPath};
use tera::{Context, Tera};

/// What the caller asked to be turned into a PDF
pub enum Source {
    Markdown(String),
    Html(String),
    Template { template: String, data: Value },
}

/// What happens to a document that references a URL the renderer must not fetch
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum UrlPolicy {
    /// 400 before a single process is spawned
    #[default]
    Enforce,
    /// Report it and render anyway. `POST /` has always produced a PDF with the asset
    /// silently missing, and the fetcher of `deploy/weasyprint-safe.py` still refuses the
    /// fetch: turning that into a hard failure would break documents that render today.
    Warn,
}

/// Everything that shapes the output PDF. Two specs that compare equal here must produce
/// the same bytes: this is what makes the cache correct.
pub struct RenderSpec {
    pub source: Source,
    pub css: Option<String>,
    pub engine: PdfEngine,
    pub options: PdfOptions,
    pub header_html: Option<String>,
    pub footer_html: Option<String>,
    pub header_template: Option<String>,
    pub footer_template: Option<String>,
    pub url_policy: UrlPolicy,
}

impl RenderSpec {
    /// A spec with nothing but a source: every other knob is opt-in
    pub fn new(source: Source) -> RenderSpec {
        RenderSpec {
            source,
            css: None,
            engine: PdfEngine::default(),
            options: PdfOptions::default(),
            header_html: None,
            footer_html: None,
            header_template: None,
            footer_template: None,
            url_policy: UrlPolicy::default(),
        }
    }
}

pub struct RenderOutcome {
    /// The PDF, always a temp file: it is deleted when this outcome is dropped, so a cache
    /// entry is copied out rather than handed over (dropping it would evict the entry).
    pub pdf: TempPath,
    pub cache_hit: bool,
    pub layout: Option<LayoutReport>,
    pub warnings: Vec<BlockWarning>,
}

/// Which renderer a source goes to, and how it is identified in the cache key
#[derive(Debug, Clone, Copy, PartialEq)]
enum Flavor {
    Markdown,
    Html,
    Template,
}

impl Flavor {
    fn tag(&self) -> &'static str {
        match self {
            Flavor::Markdown => "markdown",
            Flavor::Html => "html",
            Flavor::Template => "template",
        }
    }
}

/// Render off the async executor, under the global concurrency limit
pub async fn render(spec: RenderSpec) -> Result<RenderOutcome, AppError> {
    exec::offload(move || render_inner(spec, None)).await
}

/// Same, carrying the request id so the render event can be correlated with the access log
pub async fn render_traced(spec: RenderSpec, trace: String) -> Result<RenderOutcome, AppError> {
    exec::offload(move || render_inner(spec, Some(trace))).await
}

/// The blocking entry point, for callers that already own a blocking thread
pub fn render_blocking(spec: RenderSpec) -> Result<RenderOutcome, AppError> {
    render_inner(spec, None)
}

/// Count what fails, so `mdtopdf_render_failures_total` means what its HELP text promises.
/// The wall-clock deadline is `exec::offload`'s: it covers this and everything after it.
fn render_inner(spec: RenderSpec, trace: Option<String>) -> Result<RenderOutcome, AppError> {
    render_job(spec, trace).inspect_err(|err| obs::record_render_failure(err.kind()))
}

fn render_job(spec: RenderSpec, trace: Option<String>) -> Result<RenderOutcome, AppError> {
    let started = Instant::now();

    let RenderSpec {
        source,
        css,
        engine,
        options,
        header_html,
        footer_html,
        header_template,
        footer_template,
        url_policy,
    } = spec;

    let mut warnings = Vec::new();

    // 1. Reject a document that would make the renderer fetch what it must not, before a
    //    single process is spawned. The client stylesheet gets the same treatment: a
    //    `url(http://10.0.0.5/x)` never appears in the document itself. Only Markdown gets
    //    the code-fence heuristic: in HTML a stray backtick run is not a code block.
    let checked = match source {
        Source::Markdown(_) => urlguard::check_document(source.text()),
        _ => urlguard::check_markup(source.text()),
    };
    guard_urls(checked, url_policy, &mut warnings)?;
    if let Some(ref css) = css {
        guard_urls(urlguard::check_markup(css), url_policy, &mut warnings)?;
    }

    // 2. CENSOR tags are expanded before Tera runs: `{{CENSOR}}` would otherwise be read as
    //    an undefined template variable.
    let censored = censor::process(source.text(), &CensorConfig::from_options(&options));

    // 3. Charts and diagrams. A block that cannot be rendered is reported, never fatal.
    let (expanded, block_warnings) = blocks::expand_with_options(&censored, &options);
    warnings.extend(block_warnings);

    // 4. Templates only: apply the data.
    let (flavor, body) = match source {
        Source::Markdown(_) => (Flavor::Markdown, expanded),
        Source::Html(_) => (Flavor::Html, expanded),
        Source::Template { data, .. } => {
            if !data.is_object() {
                return Err(AppError::BadRequest(
                    "\"data\" must be a JSON object".to_string(),
                ));
            }
            let context = Context::from_value(data)?;
            let html = Tera::one_off(&expanded, &context, true)?;
            // The data can inject markup of its own: check the result too
            guard_urls(urlguard::check_markup(&html), url_policy, &mut warnings)?;
            (Flavor::Template, html)
        }
    };

    let theme = themes::resolve(options.theme.as_deref())?;

    // Header and footer only reach the HTML pipeline of pandoc, so they are resolved for
    // markdown alone — the same as before the pipeline existed. A theme that ships one is
    // the fallback, never the override: what the caller sent wins.
    let (header, footer) = match flavor {
        Flavor::Markdown => (
            helpers::resolve_header_footer_content(
                header_html.as_deref(),
                header_template.as_deref(),
            )?
            .or_else(|| theme.and_then(|theme| theme.header.clone())),
            helpers::resolve_header_footer_content(
                footer_html.as_deref(),
                footer_template.as_deref(),
            )?
            .or_else(|| theme.and_then(|theme| theme.footer.clone())),
        ),
        _ => (None, None),
    };

    // 5. The cover is part of the document, so it must exist before the key is computed
    let body = themes::apply_cover(body, theme, &options)?;

    // 6. Everything above influences the bytes of the PDF, so everything above is in the key
    let engine_tag = engine.to_string();
    let options_tag = format!("{:?}", options);
    let theme_tag = theme.map(Theme::cache_tag).unwrap_or_default();
    let key = cache::key_for(&[
        flavor.tag(),
        &body,
        css.as_deref().unwrap_or_default(),
        &engine_tag,
        &options_tag,
        header.as_deref().unwrap_or_default(),
        footer.as_deref().unwrap_or_default(),
        &theme_tag,
    ]);

    // A sweep on another thread can unlink the entry between the lookup and the copy: a
    // cache hit that evaporates is a miss, never a 500.
    if let Some(cached) = cache::get(&key) {
        match copy_to_temp(&cached) {
            Ok(pdf) => {
                let bytes = std::fs::metadata(&pdf).map(|meta| meta.len()).unwrap_or(0);
                emit_event(
                    &trace,
                    flavor,
                    &engine_tag,
                    &theme_tag,
                    started,
                    bytes,
                    true,
                    None,
                );
                return Ok(RenderOutcome {
                    pdf,
                    cache_hit: true,
                    layout: cache::layout_for(&key),
                    warnings,
                });
            }
            Err(e) => debug!("Cache entry disappeared before it could be read: {e:?}"),
        }
    }

    // 7. Cascade order: default, theme, options, client. The client keeps the last word.
    let theme_css = theme.map(|theme| theme.css);
    let header_file = header
        .as_deref()
        .map(helpers::write_temp_html)
        .transpose()?;
    let footer_file = footer
        .as_deref()
        .map(helpers::write_temp_html)
        .transpose()?;

    // 8. Markdown goes through pandoc, ready-made HTML straight to weasyprint
    let mut pdf = render_once(
        &body,
        flavor,
        &engine,
        &options,
        theme_css,
        css.as_deref(),
        None,
        header_file.as_deref(),
        footer_file.as_deref(),
    )
    // "Which kit broke the renderer" is the first question a 500 raises, and the render
    // event that carries the theme is only emitted when the render succeeded.
    .inspect_err(|e| emit_failure(&trace, flavor, &engine_tag, &theme_tag, e))?;

    // 9. Layout Doctor, opt-in. A corrective pass that does not improve the score is
    //    thrown away: the client never gets a PDF worse than the one it would have had.
    let mut report = None;
    if options.autolayout == Some(true) {
        let (fixed, best) = autolayout(
            pdf,
            &body,
            flavor,
            &engine,
            &options,
            theme_css,
            css.as_deref(),
            header_file.as_deref(),
            footer_file.as_deref(),
        )?;
        pdf = fixed;
        report = Some(best);
    }

    // 10. Publish the result: cache, metrics, event
    let bytes = std::fs::metadata(&pdf).map(|meta| meta.len()).unwrap_or(0);
    if let Err(e) = cache::put(&key, &pdf) {
        // A cache that cannot be written is a performance problem, not a failed request
        warn!("Could not cache the rendered PDF: {e:?}");
    } else if let Some(ref report) = report {
        cache::put_layout(&key, report);
    }

    obs::observe_render(started.elapsed());
    emit_event(
        &trace,
        flavor,
        &engine_tag,
        &theme_tag,
        started,
        bytes,
        false,
        report.as_ref(),
    );

    Ok(RenderOutcome {
        pdf,
        cache_hit: false,
        layout: report,
        warnings,
    })
}

/// Re-render with corrective CSS while it pays off, and keep the best PDF.
/// Returns the PDF to serve and the report that describes it.
#[allow(clippy::too_many_arguments)]
fn autolayout(
    first_pdf: TempPath,
    body: &str,
    flavor: Flavor,
    engine: &PdfEngine,
    options: &PdfOptions,
    theme_css: Option<&str>,
    custom_css: Option<&str>,
    header: Option<&Path>,
    footer: Option<&Path>,
) -> Result<(TempPath, LayoutReport), AppError> {
    let mut best_pdf = first_pdf;
    let mut best = layout::analyze(&best_pdf)?;
    let mut applied = 0u8;
    let mut corrections = String::new();

    while applied < config().layout_max_passes {
        // A pass the deadline would cut short costs a render and returns nothing
        if helpers::Budget::remaining().is_some_and(|left| left < config().process_timeout) {
            warn!("Not enough budget left for another corrective layout pass");
            break;
        }

        let corrective = layout::corrective_css(&best);
        if corrective.is_empty() {
            break;
        }
        corrections.push('\n');
        corrections.push_str(&corrective);

        // A failing corrective render must not lose the perfectly valid PDF we already have
        let candidate = match render_once(
            body,
            flavor,
            engine,
            options,
            theme_css,
            custom_css,
            Some(&corrections),
            header,
            footer,
        ) {
            Ok(candidate) => candidate,
            Err(e) => {
                warn!("Corrective layout pass failed, keeping the previous render: {e:?}");
                break;
            }
        };

        let report = match layout::analyze(&candidate) {
            Ok(report) => report,
            Err(e) => {
                warn!("Could not analyze the corrective render: {e:?}");
                break;
            }
        };

        if report.score <= best.score {
            break;
        }

        // Counted only now: `passes` describes the PDF that is served, and a rejected
        // candidate shaped nothing.
        applied += 1;
        best_pdf = candidate;
        best = report;
    }

    best.passes = Some(applied);
    Ok((best_pdf, best))
}

#[allow(clippy::too_many_arguments)]
fn render_once(
    body: &str,
    flavor: Flavor,
    engine: &PdfEngine,
    options: &PdfOptions,
    theme_css: Option<&str>,
    custom_css: Option<&str>,
    corrective_css: Option<&str>,
    header: Option<&Path>,
    footer: Option<&Path>,
) -> Result<TempPath, AppError> {
    let css_path = helpers::build_css_layers(theme_css, Some(options), custom_css, corrective_css)?;
    let css_path_str = helpers::path_to_str(&css_path)?;

    match flavor {
        Flavor::Markdown => helpers::run_pandoc(
            body,
            css_path_str,
            engine,
            Some(options),
            header.map(helpers::path_to_str).transpose()?,
            footer.map(helpers::path_to_str).transpose()?,
        ),
        Flavor::Html | Flavor::Template => helpers::run_weasyprint(body, css_path_str),
    }
}

/// A cache entry must outlive the response, so it is copied instead of being wrapped in a
/// `TempPath` that would delete it on drop.
fn copy_to_temp(source: &Path) -> Result<TempPath, AppError> {
    let temp = Builder::new().suffix(".pdf").tempfile()?;
    std::fs::copy(source, temp.path())?;
    Ok(temp.into_temp_path())
}

/// A blocked URL is a hard error on `/api/*` and a warning on the legacy endpoint
fn guard_urls(
    checked: Result<(), AppError>,
    policy: UrlPolicy,
    warnings: &mut Vec<BlockWarning>,
) -> Result<(), AppError> {
    match checked {
        Ok(()) => Ok(()),
        Err(e) => match policy {
            UrlPolicy::Enforce => Err(e),
            UrlPolicy::Warn => {
                warnings.push(BlockWarning::new("url", blocks::reason(&e), None));
                Ok(())
            }
        },
    }
}

fn emit_failure(
    trace: &Option<String>,
    flavor: Flavor,
    engine: &str,
    theme: &str,
    error: &AppError,
) {
    let mut fields = vec![
        ("source", json!(flavor.tag())),
        ("engine", json!(engine)),
        obs::err_field(error.kind(), &format!("{:?}", error)),
    ];

    if let Some(trace) = trace {
        fields.push(("trace_id", json!(trace)));
    }
    if !theme.is_empty() {
        fields.push(("theme", json!(theme)));
    }

    obs::event(obs::Level::Error, "render failed", fields);
}

#[allow(clippy::too_many_arguments)]
fn emit_event(
    trace: &Option<String>,
    flavor: Flavor,
    engine: &str,
    theme: &str,
    started: Instant,
    bytes: u64,
    cache_hit: bool,
    report: Option<&LayoutReport>,
) {
    let mut fields = vec![
        ("source", json!(flavor.tag())),
        ("engine", json!(engine)),
        ("duration_ms", json!(started.elapsed().as_millis())),
        ("bytes", json!(bytes)),
        ("cache", json!(if cache_hit { "hit" } else { "miss" })),
    ];

    // Which kit is failing is the first question asked when renders start returning 500
    if !theme.is_empty() {
        fields.push(("theme", json!(theme)));
    }

    if let Some(trace) = trace {
        fields.push(("trace_id", json!(trace)));
    }

    if let Some(report) = report {
        fields.push(("pages", json!(report.pages)));
        fields.push(("layout_score", json!(report.score)));
        fields.push(("layout_passes", json!(report.passes.unwrap_or(0))));
    }

    obs::event(obs::Level::Info, "render", fields);
}

/// The response contract shared by every rendering route: a PDF body, or a JSON
/// `download_url` when the caller asked for the file to be saved.
pub async fn respond(
    outcome: RenderOutcome,
    client_id: Option<String>,
    pdf_name: Option<String>,
) -> Result<Either<NamedFile, Json<ConvertResponse>>, AppError> {
    if let (Some(client_id), Some(pdf_name)) = (client_id, pdf_name) {
        let download_url = helpers::save_pdf(&outcome.pdf, &client_id, &pdf_name)?;
        Ok(Either::Right(Json(response_for(&outcome, download_url))))
    } else {
        Ok(Either::Left(
            NamedFile::open(&outcome.pdf).await.map_err(AppError::Io)?,
        ))
    }
}

/// Additive by construction: a request that asked for nothing new gets the historical
/// `{"download_url": "..."}` and nothing else.
fn response_for(outcome: &RenderOutcome, download_url: String) -> ConvertResponse {
    ConvertResponse {
        download_url,
        cached: if outcome.cache_hit { Some(true) } else { None },
        layout: outcome.layout.clone(),
        warnings: if outcome.warnings.is_empty() {
            None
        } else {
            Some(outcome.warnings.clone())
        },
    }
}

/// Render, then rasterize: one render slot for the whole job instead of two
pub async fn render_to_png(spec: RenderSpec, page: usize, dpi: u32) -> Result<Vec<u8>, AppError> {
    exec::offload(move || {
        let outcome = render_inner(spec, None)?;
        let pages = pdfops::rasterize(&outcome.pdf, page, page, dpi)?;
        match pages.into_iter().next() {
            Some(raster) => Ok(raster.png),
            None => Err(AppError::ProcessFailed {
                message: "PNG file not found after pdftoppm".to_string(),
                stderr: String::new(),
            }),
        }
    })
    .await
}

impl Source {
    /// The document text, before templating
    fn text(&self) -> &str {
        match self {
            Source::Markdown(text) | Source::Html(text) => text,
            Source::Template { template, .. } => template,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `POST /` produced a PDF for these documents long before the guard existed, and the
    /// fetcher still refuses the fetch itself: the caller loses the asset, not the report.
    #[test]
    fn the_legacy_endpoint_reports_a_blocked_url_instead_of_refusing_the_document() {
        let blocked = r#"<img src="http://nas.local/logo.png">"#;
        let mut warnings = Vec::new();

        assert!(guard_urls(
            urlguard::check_document(blocked),
            UrlPolicy::Warn,
            &mut warnings
        )
        .is_ok());
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].kind, "url");

        let mut warnings = Vec::new();
        assert!(guard_urls(
            urlguard::check_document(blocked),
            UrlPolicy::Enforce,
            &mut warnings
        )
        .is_err());
        assert!(warnings.is_empty());
    }
}

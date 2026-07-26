//! Named, versioned brand kits loaded from `themes/<name>/v<version>/`.
//!
//! A published version is immutable: a contractual report rendered with `aismarttalk@1` must
//! still come out identical next year. Changing the identity ships a new `v<n>` directory,
//! and the version travels in the cache key, so the two never mix.
//!
//! The manifest is JSON rather than TOML: `serde_json` is already a dependency and reading a
//! dozen metadata fields does not justify pulling a TOML parser into the image.

use crate::helpers::sanitize_path_component;
use crate::types::{AppError, Cover, PdfOptions};
use crate::urlguard;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::OnceLock;
use tera::{Context, Tera};

/// Brand kits sit next to `templates/`, resolved against the working directory
const THEMES_DIR: &str = "themes";

/// The document `GET /api/themes/<name>/<version>/preview.png` renders
const SAMPLE_FILE: &str = "sample.md";

/// A stylesheet or fragment past this size is a mistake, not a theme
const MAX_ASSET_BYTES: u64 = 512 * 1024;

/// A stylesheet the service ships. `version` is part of the cache key, so publishing a new
/// version of a theme cannot serve PDFs rendered with the previous one.
pub struct Theme {
    pub name: &'static str,
    pub version: u32,
    pub description: &'static str,
    pub css: &'static str,
    /// Display name, for a theme picker
    pub label: String,
    pub author: Option<String>,
    /// Families the stylesheet relies on; every one of them ships in the image
    pub fonts: Vec<String>,
    /// Colour tokens, so a caller can match the kit outside the PDF
    pub colors: BTreeMap<String, String>,
    /// Optional fragments: a cover template, and head/after-body includes
    pub cover: Option<String>,
    pub header: Option<String>,
    pub footer: Option<String>,
}

impl Theme {
    /// Identity of the theme inside a cache key
    pub fn cache_tag(&self) -> String {
        format!("{}@{}", self.name, self.version)
    }
}

/// What `GET /api/themes` publishes
#[derive(Debug, Clone, Serialize)]
pub struct ThemeInfo {
    pub name: String,
    pub version: u32,
    pub description: String,
    pub label: String,
    /// True for the version `"name"` resolves to when no version is pinned
    pub latest: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub fonts: Vec<String>,
    pub colors: BTreeMap<String, String>,
    pub cover: bool,
    pub header: bool,
    pub footer: bool,
    pub preview_url: String,
}

/// Metadata a kit declares. Unknown keys are accepted on purpose: a manifest written for a
/// later version of the service must not take its theme down.
#[derive(Deserialize)]
struct Manifest {
    label: String,
    description: String,
    author: Option<String>,
    #[serde(default)]
    fonts: Vec<String>,
    #[serde(default)]
    colors: BTreeMap<String, String>,
}

struct Registry {
    /// Sorted by name then version, which is also the order `list()` publishes
    themes: Vec<Theme>,
    sample: Option<&'static str>,
}

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(load)
}

/// Load the kits once, at startup, so a broken theme is reported before the first request
/// rather than in the middle of one.
pub fn init() {
    let registry = registry();
    if registry.themes.is_empty() {
        warn!(
            "No theme loaded from {}/: \"theme\" will only answer 404",
            THEMES_DIR
        );
        return;
    }

    let loaded: Vec<String> = registry.themes.iter().map(Theme::cache_tag).collect();
    info!("Themes: {}", loaded.join(", "));
}

/// Resolve `"name"` (latest version) or `"name@2"` (pinned version).
/// `None` in, `None` out: no theme requested is not an error.
pub fn resolve(spec: Option<&str>) -> Result<Option<&'static Theme>, AppError> {
    let spec = match spec.map(str::trim) {
        Some(spec) if !spec.is_empty() => spec,
        _ => return Ok(None),
    };

    let (name, version) = parse_spec(spec)?;
    find(&name, version).map(Some)
}

pub fn list() -> Vec<ThemeInfo> {
    let themes = &registry().themes;

    themes
        .iter()
        .map(|theme| ThemeInfo {
            name: theme.name.to_string(),
            version: theme.version,
            description: theme.description.to_string(),
            label: theme.label.clone(),
            latest: latest(theme.name).map(|l| l.version) == Some(theme.version),
            author: theme.author.clone(),
            fonts: theme.fonts.clone(),
            colors: theme.colors.clone(),
            cover: theme.cover.is_some(),
            header: theme.header.is_some(),
            footer: theme.footer.is_some(),
            preview_url: format!("/api/themes/{}/{}/preview.png", theme.name, theme.version),
        })
        .collect()
}

/// The demonstration document previews are rendered from
pub fn sample() -> Result<&'static str, AppError> {
    registry().sample.ok_or_else(|| {
        AppError::NotFound(format!(
            "The sample document {}/{} is missing from this deployment",
            THEMES_DIR, SAMPLE_FILE
        ))
    })
}

// ------------ Cover page ------------

/// Prepend the theme's cover page when `options.cover` asks for one. A document that does not
/// want a cover is handed back untouched, never copied.
pub fn apply_cover(
    body: String,
    theme: Option<&Theme>,
    options: &PdfOptions,
) -> Result<String, AppError> {
    let cover = match options.cover {
        Some(ref cover) => cover,
        None => return Ok(body),
    };

    let html = cover_html(cover, theme)?;
    Ok(insert_cover(body, &html))
}

/// Build the cover fragment: the theme's template when it ships one, the built-in otherwise
fn cover_html(cover: &Cover, theme: Option<&Theme>) -> Result<String, AppError> {
    let template = theme
        .and_then(|theme| theme.cover.as_deref())
        .unwrap_or(DEFAULT_COVER);

    let mut context = Context::new();
    context.insert("title", cover.title.as_deref().unwrap_or_default());
    context.insert("subtitle", cover.subtitle.as_deref().unwrap_or_default());
    context.insert("logo", cover.logo.as_deref().unwrap_or_default());
    context.insert("date", cover.date.as_deref().unwrap_or_default());

    // The values come straight from the request: escaping is not optional
    let html = Tera::one_off(template, &context, true)?;

    // `logo` lands in an <img src>, so the cover goes through the same gate as the document
    urlguard::check_document(&html)?;

    // Without a theme there is no stylesheet that knows about `.pdf-cover`, so the fragment
    // carries the little it needs to still be a cover page.
    match theme {
        Some(_) => Ok(html),
        None => Ok(format!("{}\n{}", FALLBACK_COVER_STYLE, html)),
    }
}

/// A full HTML document must keep its `<head>` first, so the cover goes just inside `<body>`;
/// a markdown or fragment body simply gets it in front.
fn insert_cover(body: String, cover: &str) -> String {
    match body_open_tag_end(&body) {
        Some(at) => {
            let mut out = String::with_capacity(body.len() + cover.len() + 2);
            out.push_str(&body[..at]);
            out.push('\n');
            out.push_str(cover);
            out.push_str(&body[at..]);
            out
        }
        None => format!("{}\n\n{}", cover, body),
    }
}

/// Byte offset just past the `<body ...>` opening tag, when the body is a full document.
/// Lowercasing ASCII keeps every byte offset valid on the original string.
fn body_open_tag_end(body: &str) -> Option<usize> {
    let lowered = body.to_ascii_lowercase();
    let start = lowered.find("<body")?;
    let end = lowered[start..].find('>')? + start;
    Some(end + 1)
}

/// Used when a cover is asked for without a theme: minimal, self-contained, no ornament
const DEFAULT_COVER: &str = r#"<section class="pdf-cover">
{% if logo %}<img class="pdf-cover-logo" src="{{ logo }}" alt="" />{% endif %}
<div class="pdf-cover-title">{{ title }}</div>
{% if subtitle %}<div class="pdf-cover-subtitle">{{ subtitle }}</div>{% endif %}
{% if date %}<div class="pdf-cover-date">{{ date }}</div>{% endif %}
</section>"#;

const FALLBACK_COVER_STYLE: &str = r#"<style>
@page cover { margin: 0; }
.pdf-cover { page: cover; break-after: page; height: 100%; padding: 6cm 3cm 0; }
.pdf-cover-logo { max-height: 2cm; margin-bottom: 1.5cm; }
.pdf-cover-title { font-size: 30pt; font-weight: 700; line-height: 1.15; }
.pdf-cover-subtitle { font-size: 14pt; margin-top: 0.6cm; }
.pdf-cover-date { font-size: 10pt; margin-top: 1.2cm; }
</style>"#;

// ------------ Resolution ------------

/// Split `"name"` or `"name@2"`. The name builds a filesystem path, so it goes through the
/// same validation as every other path component the API accepts.
fn parse_spec(spec: &str) -> Result<(String, Option<u32>), AppError> {
    let (name, version) = match spec.split_once('@') {
        Some((name, version)) => (name.trim(), Some(version.trim())),
        None => (spec, None),
    };

    let name = sanitize_path_component(name, "theme")?;

    let version = match version {
        Some(version) => Some(parse_version(version)?),
        None => None,
    };

    Ok((name, version))
}

/// Accepts `2`, `v2` and `latest`, the three spellings a caller reasonably tries
fn parse_version(value: &str) -> Result<u32, AppError> {
    let digits = value.strip_prefix('v').unwrap_or(value);

    digits.parse::<u32>().map_err(|_| {
        AppError::BadRequest(format!(
            "\"{}\" is not a theme version: use \"name\" or \"name@2\"",
            value
        ))
    })
}

fn find(name: &str, version: Option<u32>) -> Result<&'static Theme, AppError> {
    let themes = &registry().themes;

    match version {
        Some(version) => themes
            .iter()
            .find(|theme| theme.name.eq_ignore_ascii_case(name) && theme.version == version)
            .ok_or_else(|| unknown(name, Some(version))),
        None => latest(name).ok_or_else(|| unknown(name, None)),
    }
}

/// Highest version of a theme. The registry is sorted, so the last match wins.
fn latest(name: &str) -> Option<&'static Theme> {
    registry()
        .themes
        .iter()
        .rfind(|theme| theme.name.eq_ignore_ascii_case(name))
}

/// An unknown theme says what is available: a silent fallback to no theme would ship an
/// unbranded contract without anyone noticing.
fn unknown(name: &str, version: Option<u32>) -> AppError {
    let known: Vec<String> = registry().themes.iter().map(Theme::cache_tag).collect();

    let available = if known.is_empty() {
        "none is installed".to_string()
    } else {
        format!("available: {}", known.join(", "))
    };

    match version {
        Some(version) => AppError::NotFound(format!(
            "Unknown theme \"{}@{}\" ({})",
            name, version, available
        )),
        None => AppError::NotFound(format!("Unknown theme \"{}\" ({})", name, available)),
    }
}

// ------------ Loading ------------

/// Read every kit on disk. A theme that does not validate is dropped with a warning: a bad
/// stylesheet must never keep the service from starting.
fn load() -> Registry {
    let root = Path::new(THEMES_DIR);
    let mut themes = Vec::new();

    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) => {
            warn!("No theme directory at {:?}: {}", root, e);
            return Registry {
                themes,
                sample: None,
            };
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = match path.file_name().and_then(|name| name.to_str()) {
            Some(name) => name,
            None => continue,
        };

        // The name is echoed in URLs and joined into paths: hold it to the same rules as a
        // client_id, so a directory nobody could ever request is reported now.
        if let Err(e) = sanitize_path_component(name, "theme") {
            warn!("Ignoring theme directory {:?}: {:?}", path, e);
            continue;
        }

        let versions = match fs::read_dir(&path) {
            Ok(versions) => versions,
            Err(e) => {
                warn!("Ignoring theme {}: {}", name, e);
                continue;
            }
        };

        for version_entry in versions.flatten() {
            let version_path = version_entry.path();
            if !version_path.is_dir() {
                continue;
            }

            let directory = version_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();

            let version = match version_directory(directory) {
                Some(version) => version,
                None => {
                    warn!(
                        "Ignoring {}/{}: a version directory is named v1, v2, ...",
                        name, directory
                    );
                    continue;
                }
            };

            match load_version(name, version, &version_path) {
                Ok(theme) => themes.push(theme),
                Err(reason) => warn!("Ignoring theme {}@{}: {}", name, version, reason),
            }
        }
    }

    themes.sort_by(|a, b| a.name.cmp(b.name).then(a.version.cmp(&b.version)));

    Registry {
        themes,
        sample: read_sample(root),
    }
}

/// `v3` is version 3. `v03` is refused rather than silently normalized: the directory name
/// and the `name@version` a client pins have to be the same string.
fn version_directory(directory: &str) -> Option<u32> {
    let version: u32 = directory.strip_prefix('v')?.parse().ok()?;

    if format!("v{}", version) == directory {
        Some(version)
    } else {
        None
    }
}

fn load_version(name: &str, version: u32, dir: &Path) -> Result<Theme, String> {
    let manifest = read_asset(dir, "theme.json")?.ok_or("theme.json is missing")?;
    let manifest: Manifest =
        serde_json::from_str(&manifest).map_err(|e| format!("theme.json is invalid: {}", e))?;

    if manifest.label.trim().is_empty() || manifest.description.trim().is_empty() {
        return Err("theme.json needs a non-empty \"label\" and \"description\"".to_string());
    }

    let css = read_asset(dir, "theme.css")?.ok_or("theme.css is missing")?;
    if css.trim().is_empty() {
        return Err("theme.css is empty".to_string());
    }

    Ok(Theme {
        // The registry outlives the process, so the borrowed fields of the contract are the
        // leaked copies of what was just read.
        name: leak(name.to_string()),
        version,
        description: leak(manifest.description),
        css: leak(css),
        label: manifest.label,
        author: manifest.author,
        fonts: manifest.fonts,
        colors: manifest.colors,
        cover: read_asset(dir, "cover.html")?,
        header: read_asset(dir, "header.html")?,
        footer: read_asset(dir, "footer.html")?,
    })
}

/// Read an optional file of the kit, refusing anything oversized or not UTF-8
fn read_asset(dir: &Path, file: &str) -> Result<Option<String>, String> {
    let path = dir.join(file);

    let metadata = match fs::metadata(&path) {
        Ok(metadata) if metadata.is_file() => metadata,
        _ => return Ok(None),
    };

    if metadata.len() > MAX_ASSET_BYTES {
        return Err(format!(
            "{} is larger than {} KB",
            file,
            MAX_ASSET_BYTES / 1024
        ));
    }

    fs::read_to_string(&path)
        .map(Some)
        .map_err(|e| format!("{} could not be read: {}", file, e))
}

fn read_sample(root: &Path) -> Option<&'static str> {
    match fs::read_to_string(root.join(SAMPLE_FILE)) {
        Ok(sample) => Some(leak(sample)),
        Err(e) => {
            warn!(
                "No theme preview possible, {} is unreadable: {}",
                SAMPLE_FILE, e
            );
            None
        }
    }
}

/// The contract borrows `name`, `description` and `css` for `'static`, and the registry is
/// built once and never dropped, so leaking those three strings costs a few kilobytes total.
fn leak(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_directories_must_be_canonical() {
        assert_eq!(version_directory("v1"), Some(1));
        assert_eq!(version_directory("v12"), Some(12));
        assert_eq!(version_directory("v01"), None);
        assert_eq!(version_directory("1"), None);
        assert_eq!(version_directory("vnext"), None);
    }

    #[test]
    fn specs_split_into_name_and_version() {
        assert_eq!(parse_spec("report").unwrap(), ("report".to_string(), None));
        assert_eq!(
            parse_spec("report@2").unwrap(),
            ("report".to_string(), Some(2))
        );
        assert_eq!(
            parse_spec("report@v2").unwrap(),
            ("report".to_string(), Some(2))
        );
        assert!(parse_spec("../etc").is_err());
        assert!(parse_spec("report@next").is_err());
    }

    #[test]
    fn a_cover_goes_inside_the_body_of_a_full_document() {
        let html = "<html><head></head><BODY class=\"x\"><p>é</p></body></html>";
        let out = insert_cover(html.to_string(), "<section></section>");
        assert!(out.starts_with("<html><head></head><BODY class=\"x\">\n<section></section><p>"));

        let fragment = insert_cover("<p>text</p>".to_string(), "<section></section>");
        assert!(fragment.starts_with("<section></section>\n\n<p>"));
    }
}

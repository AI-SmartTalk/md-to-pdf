use crate::auth::ApiKey;
use crate::cache;
use crate::helpers::PREVIEW_DPI;
use crate::pipeline::{self, RenderSpec, Source};
use crate::themes::{self, ThemeInfo};
use crate::types::*;
use rocket::http::ContentType;
use rocket::serde::json::Json;
use serde::Serialize;
use std::fs;
use std::io::Write;

/// Wrapped in an object rather than returned as a bare array: adding a field later must not
/// break the clients parsing it.
#[derive(Serialize)]
pub struct ThemesResponse {
    pub themes: Vec<ThemeInfo>,
}

#[get("/themes")]
pub fn list_themes(_key: ApiKey) -> Json<ThemesResponse> {
    Json(ThemesResponse {
        themes: themes::list(),
    })
}

/// First page of the sample document rendered with one theme.
///
/// `?cover=true` previews the cover page instead of the body, which is the only way to see
/// what `options.cover` produces without rendering a document of one's own.
#[get("/themes/<name>/<version>/preview.png?<cover>")]
pub async fn theme_preview(
    _key: ApiKey,
    name: &str,
    version: &str,
    cover: Option<bool>,
) -> Result<(ContentType, Vec<u8>), AppError> {
    let spec = match version {
        "latest" => name.to_string(),
        version => format!("{}@{}", name, version),
    };

    let theme = themes::resolve(Some(&spec))?
        .ok_or_else(|| AppError::NotFound(format!("Unknown theme \"{}\"", spec)))?;

    let sample = themes::sample()?;
    let with_cover = cover.unwrap_or(false);

    // Rendering is worth about a second, so a preview is served from the PDF cache. The
    // sample text is part of the key: editing it must not keep serving the old picture.
    let key = cache::key_for(&[
        "theme-preview",
        &theme.cache_tag(),
        sample,
        if with_cover { "cover" } else { "body" },
    ]);

    // A sweep on another thread can unlink the entry between the lookup and the read: a
    // cache hit that evaporates is a miss, so fall through and render it again.
    if let Some(path) = cache::get(&key) {
        match fs::read(&path) {
            Ok(png) => return Ok((ContentType::PNG, png)),
            Err(e) => debug!("Cached preview disappeared before it could be read: {e}"),
        }
    }

    let mut options = PdfOptions {
        theme: Some(theme.cache_tag()),
        ..Default::default()
    };

    let body = if with_cover {
        // The cover is expanded here and `options.cover` left unset: whatever the pipeline
        // ends up doing with that option, this preview keeps showing exactly one cover.
        options.cover = Some(demo_cover());
        let body = themes::apply_cover(sample.to_string(), Some(theme), &options)?;
        options.cover = None;
        body
    } else {
        sample.to_string()
    };

    let png = pipeline::render_to_png(
        RenderSpec {
            options,
            ..RenderSpec::new(Source::Markdown(body))
        },
        1,
        PREVIEW_DPI,
    )
    .await?;

    // A preview that cannot be cached is still a valid preview
    if let Err(e) = store(&key, &png) {
        warn!(
            "Could not cache the preview of {}: {:?}",
            theme.cache_tag(),
            e
        );
    }

    Ok((ContentType::PNG, png))
}

/// The cache stores files, so the PNG is spilled next to the rendered PDFs. It is keyed
/// under its own prefix and never served as a PDF, so the `.pdf` name the cache gives it is
/// an implementation detail of the store.
fn store(key: &cache::CacheKey, png: &[u8]) -> Result<(), AppError> {
    let mut file = tempfile::Builder::new().suffix(".png").tempfile()?;
    file.write_all(png)?;
    cache::put(key, file.path())?;
    Ok(())
}

/// What the cover preview fills the template with
fn demo_cover() -> Cover {
    Cover {
        title: Some("Rapport d'activité".to_string()),
        subtitle: Some("Démonstration du thème — AI SmartTalk".to_string()),
        logo: None,
        date: Some("Janvier 2026".to_string()),
    }
}

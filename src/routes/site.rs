//! The public pages: `/outils` in French, `/tools` in English.
//!
//! Unauthenticated by design — this is the shop window, and a search engine has no API
//! key. The tools themselves are still behind whatever `API_KEY` says: the page renders,
//! the browser calls the API, and a closed deployment simply asks the visitor for a token.

use crate::site::{self, Lang};
use crate::types::AppError;
use rocket::response::content::RawHtml;

#[get("/outils")]
pub fn index_fr() -> Result<RawHtml<String>, AppError> {
    index(Lang::Fr)
}

#[get("/outils/<slug>")]
pub fn tool_fr(slug: &str) -> Result<RawHtml<String>, AppError> {
    tool(Lang::Fr, slug)
}

#[get("/tools")]
pub fn index_en() -> Result<RawHtml<String>, AppError> {
    index(Lang::En)
}

#[get("/tools/<slug>")]
pub fn tool_en(slug: &str) -> Result<RawHtml<String>, AppError> {
    tool(Lang::En, slug)
}

/// `sitemap.xml` is not decoration on a site whose whole distribution strategy is thirty
/// search intents: it is how the pages get found in the first place.
#[get("/sitemap.xml")]
pub fn sitemap() -> Result<rocket::response::content::RawXml<String>, AppError> {
    let base = std::env::var("PUBLIC_BASE_URL")
        .unwrap_or_else(|_| "https://pdf.aismarttalk.tech".to_string());
    let base = base.trim_end_matches('/');

    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n",
    );

    for lang in [Lang::Fr, Lang::En] {
        out.push_str(&format!(
            "  <url><loc>{}{}</loc><changefreq>weekly</changefreq></url>\n",
            base,
            lang.root()
        ));
        for tool in &site::site().catalog(lang).tools {
            out.push_str(&format!(
                "  <url><loc>{}{}/{}</loc><changefreq>monthly</changefreq></url>\n",
                base,
                lang.root(),
                escape_xml(&tool.slug)
            ));
        }
    }

    out.push_str("</urlset>\n");
    Ok(rocket::response::content::RawXml(out))
}

#[get("/robots.txt")]
pub fn robots() -> rocket::response::content::RawText<String> {
    let base = std::env::var("PUBLIC_BASE_URL")
        .unwrap_or_else(|_| "https://pdf.aismarttalk.tech".to_string());

    // `/download/` holds documents produced for identified callers: indexing them would
    // publish other people's files, which is the one mistake this whole design exists to
    // avoid. `/api/` is refused for the ordinary reason that it is not a page.
    rocket::response::content::RawText(format!(
        "User-agent: *\n\
         Disallow: /api/\n\
         Disallow: /download/\n\
         Allow: /\n\n\
         Sitemap: {}/sitemap.xml\n",
        base.trim_end_matches('/')
    ))
}

fn index(lang: Lang) -> Result<RawHtml<String>, AppError> {
    let catalog = site::site().catalog(lang);

    if catalog.tools.is_empty() {
        return Err(AppError::NotFound(
            "The public tool catalogue is not installed on this deployment".to_string(),
        ));
    }

    let mut context = site::base_context(lang);
    context.insert("tools", &catalog.tools);
    context.insert("index", &catalog.index.clone().unwrap_or_default());

    Ok(RawHtml(site::site().render("index.html", &context)?))
}

fn tool(lang: Lang, slug: &str) -> Result<RawHtml<String>, AppError> {
    let site = site::site();

    let tool = site
        .tool(lang, slug)
        .ok_or_else(|| AppError::NotFound(format!("No tool page for {:?}", slug)))?;

    // Related tools are named by slug; resolving them here keeps the template free of
    // lookups, and silently drops a slug that no longer exists rather than rendering a
    // dead link.
    let related: Vec<_> = tool
        .related
        .iter()
        .filter_map(|slug| site.tool(lang, slug))
        .collect();

    let mut context = site::base_context(lang);
    context.insert("tool", tool);
    context.insert("related", &related);
    context.insert(
        "index",
        &site.catalog(lang).index.clone().unwrap_or_default(),
    );

    Ok(RawHtml(site.render("tool.html", &context)?))
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_what_would_break_the_sitemap() {
        assert_eq!(escape_xml("a&b<c>"), "a&amp;b&lt;c&gt;");
        assert_eq!(escape_xml("compresser-pdf"), "compresser-pdf");
    }
}

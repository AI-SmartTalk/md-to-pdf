//! The public pages.
//!
//! The root belongs to the visitor who typed the domain, not to the integrator: `/` is the
//! French home, `/en` the English one, and the developer console — which used to occupy the
//! root and greeted everyone with `RUST · PANDOC · WEASYPRINT` — now lives at `/console`,
//! untouched, linked from the navigation and the footer.
//!
//! Unauthenticated by design: this is the shop window, and a search engine has no API key.
//! The tools themselves stay behind whatever `API_KEY` says.

use crate::site::{self, Lang};
use crate::types::AppError;
use rocket::fs::NamedFile;
use rocket::http::Status;
use rocket::response::content::RawHtml;
use rocket::response::Redirect;

// ------------ Home ------------

#[get("/")]
pub fn home_fr() -> Result<RawHtml<String>, AppError> {
    index(Lang::Fr)
}

#[get("/en")]
pub fn home_en() -> Result<RawHtml<String>, AppError> {
    index(Lang::En)
}

/// `/outils` and `/tools` were the home pages before the root was handed to the public.
/// They are kept as permanent redirects rather than as second copies: two URLs serving the
/// same page split its authority between them, which is the cheapest SEO mistake to make
/// and the most annoying to undo later.
#[get("/outils")]
pub fn index_fr_moved() -> Redirect {
    Redirect::permanent("/")
}

#[get("/tools")]
pub fn index_en_moved() -> Redirect {
    Redirect::permanent("/en")
}

// ------------ Tool pages ------------

#[get("/outils/<slug>")]
pub fn tool_fr(slug: &str) -> Result<(Status, RawHtml<String>), AppError> {
    tool(Lang::Fr, slug)
}

#[get("/tools/<slug>")]
pub fn tool_en(slug: &str) -> Result<(Status, RawHtml<String>), AppError> {
    tool(Lang::En, slug)
}

// ------------ Pricing ------------

#[get("/tarifs")]
pub fn pricing_fr() -> Result<RawHtml<String>, AppError> {
    pricing(Lang::Fr)
}

#[get("/pricing")]
pub fn pricing_en() -> Result<RawHtml<String>, AppError> {
    pricing(Lang::En)
}

// ------------ The console ------------

/// The integrator console, exactly as it was when it lived at the root. It is a
/// hash-routed single page, so serving its file here is enough for every one of its views.
#[get("/console")]
pub async fn console() -> Result<NamedFile, AppError> {
    NamedFile::open("static/index.html")
        .await
        .map_err(AppError::Io)
}

// ------------ The social card ------------

/// The image that shows up when someone shares a link, rendered by this service's own
/// engine.
///
/// A missing `og:image` costs a share; a hardcoded PNG in the repository is a binary nobody
/// can review. So the card is HTML, laid out at 1200×630 by WeasyPrint and rasterised by
/// the same path `/api/preview` uses — which also means it can never drift from the brand
/// the rest of the site is built with.
#[get("/og.png")]
pub async fn og_image() -> Result<(rocket::http::ContentType, Vec<u8>), AppError> {
    static CARD: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();

    if let Some(cached) = CARD.get() {
        return Ok((rocket::http::ContentType::PNG, cached.clone()));
    }

    let html = social_card_html();
    let mut spec = crate::pipeline::RenderSpec::new(crate::pipeline::Source::Html(html));
    // The card carries no remote asset; refusing one outright is cheaper than explaining it
    spec.url_policy = crate::pipeline::UrlPolicy::Enforce;

    let png = crate::pipeline::render_to_png(spec, 1, 96).await?;
    let _ = CARD.set(png.clone());

    Ok((rocket::http::ContentType::PNG, png))
}

/// 1200×630 at 96 dpi is 12.5 × 6.56 inches — the size every social preview expects.
fn social_card_html() -> String {
    format!(
        r#"<!DOCTYPE html><html><head><meta charset="utf-8"><style>
@page {{ size: 12.5in 6.5625in; margin: 0; }}
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{
  width: 12.5in; height: 6.5625in;
  background: linear-gradient(135deg, #0b1220 0%, #101c2e 55%, #12283a 100%);
  color: #eaf1fb;
  font-family: -apple-system, "Segoe UI", Inter, Roboto, Helvetica, Arial, sans-serif;
  padding: 0.95in 1.05in;
  display: flex; flex-direction: column; justify-content: space-between;
}}
.kicker {{ font-size: 22pt; letter-spacing: .22em; text-transform: uppercase; color: #4fd1e0; }}
h1 {{ font-size: 62pt; line-height: 1.05; letter-spacing: -.02em; max-width: 9.6in; }}
h1 em {{ font-style: normal; color: #4fd1e0; }}
p {{ font-size: 25pt; color: #9fb2cc; max-width: 9.2in; margin-top: .22in; }}
.foot {{ display: flex; gap: .55in; font-size: 20pt; color: #7f92ad; }}
.foot b {{ color: #eaf1fb; font-weight: 600; }}
</style></head><body>
<div class="kicker">AI SmartTalk Documents</div>
<div>
  <h1>Vos PDF, et <em>ce que l'opération a coûté</em>.</h1>
  <p>{tools} outils gratuits, sans filigrane. Le seul service qui vous dit ce qu'il a préservé.</p>
</div>
<div class="foot"><span><b>Sans filigrane</b></span><span><b>Effacé après 2 h</b></span><span><b>Hébergé en France</b></span></div>
</body></html>"#,
        tools = site::site().catalog(Lang::Fr).tools.len()
    )
}

// ------------ Crawling ------------

/// `sitemap.xml` is not decoration on a site whose whole distribution strategy is a few
/// dozen search intents: it is how the pages get found in the first place.
#[get("/sitemap.xml")]
pub fn sitemap() -> rocket::response::content::RawXml<String> {
    let base = site::public_base();
    let site = site::site();

    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\" \
         xmlns:xhtml=\"http://www.w3.org/1999/xhtml\">\n",
    );

    for lang in [Lang::Fr, Lang::En] {
        let other = lang.other();

        push_url(
            &mut out,
            &base,
            lang.home(),
            "weekly",
            "1.0",
            Some((lang, other, other.home().to_string())),
        );
        push_url(&mut out, &base, lang.pricing(), "monthly", "0.6", None);

        for tool in &site.catalog(lang).tools {
            let path = format!("{}/{}", lang.tools(), tool.slug);
            // Each tool declares its counterpart, so the alternate link points at the same
            // tool in the other language rather than at that language's home page — the
            // difference between a useful hreflang and a decorative one.
            let alternate = tool
                .alt_slug
                .as_ref()
                .map(|slug| format!("{}/{}", other.tools(), slug));
            push_url(
                &mut out,
                &base,
                &path,
                "monthly",
                "0.9",
                alternate.map(|href| (lang, other, href)),
            );
        }
    }

    out.push_str("</urlset>\n");
    rocket::response::content::RawXml(out)
}

fn push_url(
    out: &mut String,
    base: &str,
    path: &str,
    changefreq: &str,
    priority: &str,
    alternate: Option<(Lang, Lang, String)>,
) {
    out.push_str(&format!("  <url><loc>{}{}</loc>", base, escape_xml(path)));
    if let Some((lang, other, href)) = alternate {
        out.push_str(&format!(
            "<xhtml:link rel=\"alternate\" hreflang=\"{}\" href=\"{}{}\"/>\
             <xhtml:link rel=\"alternate\" hreflang=\"{}\" href=\"{}{}\"/>",
            lang.code(),
            base,
            escape_xml(path),
            other.code(),
            base,
            escape_xml(&href),
        ));
    }
    out.push_str(&format!(
        "<lastmod>{}</lastmod><changefreq>{}</changefreq><priority>{}</priority></url>\n",
        site::site().last_modified,
        changefreq,
        priority
    ));
}

#[get("/robots.txt")]
pub fn robots() -> rocket::response::content::RawText<String> {
    let base = site::public_base();

    // `/download/` holds documents produced for identified callers: indexing them would
    // publish other people's files, which is the one mistake this whole design exists to
    // avoid. `/api/` and `/console` are refused for the ordinary reason that neither is a
    // page a searcher should land on.
    rocket::response::content::RawText(format!(
        "User-agent: *\n\
         Disallow: /api/\n\
         Disallow: /download/\n\
         Disallow: /console\n\
         Allow: /\n\n\
         Sitemap: {}/sitemap.xml\n",
        base
    ))
}

// ------------ Rendering ------------

fn index(lang: Lang) -> Result<RawHtml<String>, AppError> {
    let catalog = site::site().catalog(lang);

    if catalog.tools.is_empty() {
        return Err(AppError::NotFound(
            "The public tool catalogue is not installed on this deployment".to_string(),
        ));
    }

    let copy = catalog.index.clone().unwrap_or_default();
    let mut context = site::base_context(lang);
    context.insert("tools", &catalog.tools);
    context.insert("page_title", &copy.title);
    context.insert("page_description", &copy.meta_description);
    context.insert("index", &copy);
    context.insert(
        "canonical",
        &format!("{}{}", site::public_base(), lang.home()),
    );
    context.insert(
        "alternate",
        &format!("{}{}", site::public_base(), lang.other().home()),
    );

    Ok(RawHtml(site::site().render("index.html", &context)?))
}

fn tool(lang: Lang, slug: &str) -> Result<(Status, RawHtml<String>), AppError> {
    let site = site::site();

    // A slug that does not exist is matched by this route, so the 404 catcher never sees
    // it: without this the visitor of a dead link — a renamed tool, a mistyped URL, an old
    // search result — would get a JSON error where a page was expected.
    let Some(tool) = site.tool(lang, slug) else {
        let path = format!("{}/{}", lang.tools(), slug);
        return Ok((Status::NotFound, RawHtml(site::not_found_page(&path)?)));
    };

    // Related tools are named by slug; resolving them here keeps the template free of
    // lookups, and silently drops a slug that no longer exists rather than rendering a
    // dead link.
    let related: Vec<_> = tool
        .related
        .iter()
        .filter_map(|slug| site.tool(lang, slug))
        .collect();

    let mut context = site::base_context(lang);
    let base = site::public_base();
    context.insert("page_title", &tool.title);
    context.insert("page_description", &tool.meta_description);
    context.insert("tool", tool);
    context.insert("related", &related);
    context.insert(
        "index",
        &site.catalog(lang).index.clone().unwrap_or_default(),
    );
    context.insert(
        "canonical",
        &format!("{}{}/{}", base, lang.tools(), tool.slug),
    );
    if let Some(alt) = tool.alt_slug.as_ref() {
        context.insert(
            "alternate",
            &format!("{}{}/{}", base, lang.other().tools(), alt),
        );
    }

    Ok((Status::Ok, RawHtml(site.render("tool.html", &context)?)))
}

fn pricing(lang: Lang) -> Result<RawHtml<String>, AppError> {
    let mut context = site::base_context(lang);
    let french = lang == Lang::Fr;
    context.insert(
        "page_title",
        if french {
            "Tarifs — gratuit, Pro, Équipe et API | AI SmartTalk"
        } else {
            "Pricing — free, Pro, Team and API | AI SmartTalk"
        },
    );
    context.insert(
        "page_description",
        if french {
            "Tous les outils PDF gratuitement, sans filigrane et sans compte. \
             Plans Pro, Équipe et API pour les chartes, l'OCR en volume et la preuve de provenance."
        } else {
            "Every PDF tool for free, no watermark and no account. Pro, Team and API plans \
             for brand kits, OCR at volume and signed provenance."
        },
    );
    context.insert(
        "canonical",
        &format!("{}{}", site::public_base(), lang.pricing()),
    );
    context.insert(
        "alternate",
        &format!("{}{}", site::public_base(), lang.other().pricing()),
    );
    context.insert("plans", &crate::site::plans(lang));

    Ok(RawHtml(site::site().render("pricing.html", &context)?))
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
        assert_eq!(
            escape_xml("/outils/compresser-pdf"),
            "/outils/compresser-pdf"
        );
    }

    /// A sitemap that lists a page under two URLs is worse than one that lists it once
    #[test]
    fn a_url_entry_carries_its_alternates_and_its_priority() {
        let mut out = String::new();
        push_url(
            &mut out,
            "https://x.test",
            "/outils/compresser-pdf",
            "monthly",
            "0.9",
            Some((Lang::Fr, Lang::En, "/tools/compress-pdf".to_string())),
        );

        assert!(out.contains("<loc>https://x.test/outils/compresser-pdf</loc>"));
        assert!(out.contains("hreflang=\"fr\" href=\"https://x.test/outils/compresser-pdf\""));
        assert!(out.contains("hreflang=\"en\" href=\"https://x.test/tools/compress-pdf\""));
        assert!(out.contains("<priority>0.9</priority>"));
    }

    #[test]
    fn a_page_without_a_counterpart_lists_no_alternate() {
        let mut out = String::new();
        push_url(
            &mut out,
            "https://x.test",
            "/tarifs",
            "monthly",
            "0.6",
            None,
        );
        assert!(!out.contains("hreflang"));
    }
}

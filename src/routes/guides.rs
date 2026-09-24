//! The guides, as pages that can be found.
//!
//! The twelve-odd guides are the only body of writing this product owns: what the engine
//! does, what censoring guarantees, why a theme is immutable. They lived inside the
//! hash-routed console, reachable from one footer link, which means no search engine ever
//! saw a word of them. Technical SEO is done; written content is the lever that is left,
//! and this one was already written.
//!
//! So the same content is served here as ordinary HTML under `/guides/<slug>`, in the same
//! shell as the rest of the public site — same header, same footer, same brand. The console
//! keeps reading `static/guides.js`; these pages read a JSON catalogue extracted from it,
//! one file per language, already resolved so a template never has to pick a translation.

use crate::site::{self, Lang};
use crate::types::AppError;
use rocket::http::Status;
use rocket::response::content::RawHtml;
use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::OnceLock;

/// One guide, in one language.
///
/// `key` is the identifier the console has always used (`overview`, `censor`); `slug` is
/// what goes in the URL, written in the language of the reader. They are kept apart on
/// purpose: renaming a slug for search must never break the console, and a French URL that
/// reads `censoring-a-document` costs trust on the page where trust decides.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Guide {
    pub key: String,
    pub slug: String,
    /// Slug of the same guide in the other language, for `hreflang` and the language switch
    #[serde(default)]
    pub alt_slug: Option<String>,
    #[serde(default)]
    pub group: String,
    pub title: String,
    #[serde(default)]
    pub lede: String,
    #[serde(default)]
    pub meta_description: String,
    #[serde(default)]
    pub icon: Option<String>,
    /// Slugs of the tool pages this guide talks about, resolved against the catalogue at
    /// render time so a renamed tool drops out of the page instead of becoming a dead link.
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub blocks: Vec<Block>,
}

/// One content block, already resolved in this file's language.
///
/// A single flat struct rather than an enum: the catalogue is editorial data, and a writer
/// who adds a field to one block should not make the whole file unreadable to serde. An
/// unknown `kind` renders as nothing rather than as a 500.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    /// Read from `type`, handed to the template as `kind`: `block.type` is not something a
    /// Tera expression can address, and renaming it here is cheaper than renaming it in a
    /// file writers read.
    #[serde(rename(deserialize = "type", serialize = "kind"))]
    pub kind: String,
    #[serde(default)]
    pub text: Option<String>,
    /// Authored HTML — `<code>`, `<b>`, links to other guides. Rendered with `| safe`.
    #[serde(default)]
    pub html: Option<String>,
    /// Language of a code block, as the console labelled it: `json`, `shell`, `markdown`
    #[serde(default)]
    pub lang: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
    /// `info` or `warn`
    #[serde(default)]
    pub level: Option<String>,
    #[serde(default)]
    pub head: Vec<String>,
    #[serde(default)]
    pub rows: Vec<Vec<String>>,
    #[serde(default)]
    pub cards: Vec<Card>,
    /// Console endpoint a "try it" block points at
    #[serde(default)]
    pub target: Option<String>,
    /// Public tool page that runs that endpoint, when there is one
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Card {
    pub title: String,
    #[serde(default)]
    pub html: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Catalog {
    #[serde(default)]
    guides: Vec<Guide>,
}

pub struct Guides {
    fr: Vec<Guide>,
    en: Vec<Guide>,
    /// Date the guides last changed, `YYYY-MM-DD`. Taken from the files rather than from
    /// the process start, for the reason the tool catalogue takes it from its own files: a
    /// `lastmod` that moves on every deployment teaches a crawler to ignore the field.
    last_modified: String,
}

static GUIDES: OnceLock<Guides> = OnceLock::new();

pub fn guides() -> &'static Guides {
    GUIDES.get_or_init(Guides::load)
}

impl Guides {
    fn load() -> Guides {
        Guides {
            fr: read_catalog("static/outils/guides.fr.json"),
            en: read_catalog("static/outils/guides.en.json"),
            last_modified: catalogue_date(),
        }
    }

    pub fn all(&self, lang: Lang) -> &[Guide] {
        match lang {
            Lang::Fr => &self.fr,
            Lang::En => &self.en,
        }
    }

    pub fn get(&self, lang: Lang, slug: &str) -> Option<&Guide> {
        self.all(lang).iter().find(|guide| guide.slug == slug)
    }

    pub fn last_modified(&self) -> &str {
        &self.last_modified
    }
}

/// A missing catalogue is not fatal: the API and the tool pages have to keep serving. The
/// guides answer 404 until the file is there.
fn read_catalog(path: &str) -> Vec<Guide> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) => {
            warn!("Guides: {} could not be read ({})", path, e);
            return Vec::new();
        }
    };

    // Written as `{"guides": [...]}` or as a bare array; both are natural things to hand a
    // writer, and refusing one of them would be pedantry.
    match serde_json::from_str::<Catalog>(&raw) {
        Ok(catalog) if !catalog.guides.is_empty() => catalog.guides,
        _ => match serde_json::from_str::<Vec<Guide>>(&raw) {
            Ok(guides) => guides,
            Err(e) => {
                error!("Guides: {} is not a valid catalogue: {}", path, e);
                Vec::new()
            }
        },
    }
}

fn catalogue_date() -> String {
    let newest = [
        "static/outils/guides.fr.json",
        "static/outils/guides.en.json",
    ]
    .iter()
    .filter_map(|path| fs::metadata(path).ok())
    .filter_map(|meta| meta.modified().ok())
    .filter_map(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
    .map(|elapsed| elapsed.as_secs())
    .max()
    .unwrap_or_else(crate::assets::now_unix);

    crate::assets::rfc3339(newest).chars().take(10).collect()
}

/// Where a language's guides live. English sits under `/en` like its home page does, so the
/// two languages keep the shape the rest of the site already taught a visitor.
pub fn root(lang: Lang) -> &'static str {
    match lang {
        Lang::Fr => "/guides",
        Lang::En => "/en/guides",
    }
}

// ------------ Routes ------------

#[get("/guides")]
pub fn index_fr() -> Result<RawHtml<String>, AppError> {
    index(Lang::Fr)
}

#[get("/en/guides")]
pub fn index_en() -> Result<RawHtml<String>, AppError> {
    index(Lang::En)
}

#[get("/guides/<slug>")]
pub fn guide_fr(slug: &str) -> Result<(Status, RawHtml<String>), AppError> {
    guide(Lang::Fr, slug)
}

#[get("/en/guides/<slug>")]
pub fn guide_en(slug: &str) -> Result<(Status, RawHtml<String>), AppError> {
    guide(Lang::En, slug)
}

// ------------ Rendering ------------

/// Guides of one language, grouped in declaration order.
///
/// The order is a reading order — from "what is this" to "how do I operate it" — so it is
/// kept rather than sorted, exactly as the console keeps it.
fn grouped(lang: Lang) -> Vec<serde_json::Value> {
    let mut groups: Vec<(String, Vec<&Guide>)> = Vec::new();

    for guide in guides().all(lang) {
        match groups.iter_mut().find(|(name, _)| *name == guide.group) {
            Some((_, list)) => list.push(guide),
            None => groups.push((guide.group.clone(), vec![guide])),
        }
    }

    groups
        .into_iter()
        .map(|(name, list)| serde_json::json!({ "name": name, "guides": list }))
        .collect()
}

fn index(lang: Lang) -> Result<RawHtml<String>, AppError> {
    let all = guides().all(lang);

    if all.is_empty() {
        return Err(AppError::NotFound(
            "The guides are not installed on this deployment".to_string(),
        ));
    }

    let french = lang == Lang::Fr;
    let mut context = site::base_context(lang);
    context.insert(
        "page_title",
        if french {
            "Guides — comprendre le moteur documentaire | AI SmartTalk"
        } else {
            "Guides — understanding the document engine | AI SmartTalk"
        },
    );
    context.insert(
        "page_description",
        if french {
            "Les guides du moteur documentaire : rendu Markdown et HTML, thèmes, censure, \
             graphiques, contrôle de mise en page, caviardage, preuve de provenance."
        } else {
            "The document engine guides: Markdown and HTML rendering, themes, censoring, \
             charts, layout audit, redaction and signed provenance."
        },
    );
    context.insert("groups", &grouped(lang));
    context.insert("guides_count", &all.len());
    context.insert("guides_root", root(lang));
    context.insert("other_guides_root", root(lang.other()));
    context.insert(
        "canonical",
        &format!("{}{}", site::public_base(), root(lang)),
    );
    context.insert(
        "alternate",
        &format!("{}{}", site::public_base(), root(lang.other())),
    );

    Ok(RawHtml(site::site().render("guides.html", &context)?))
}

fn guide(lang: Lang, slug: &str) -> Result<(Status, RawHtml<String>), AppError> {
    // A slug that does not exist is matched by this route, so the 404 catcher never sees
    // it: without this, a dead link — a renamed guide, an old search result — would answer
    // with a JSON error where a page was expected.
    let Some(guide) = guides().get(lang, slug) else {
        let path = format!("{}/{}", root(lang), slug);
        return Ok((Status::NotFound, RawHtml(site::not_found_page(&path)?)));
    };

    let site = site::site();
    let base = site::public_base();

    // The link a guide owes its subject: the tool page that runs what it explains. Slugs
    // that no longer exist are dropped rather than rendered as dead links.
    let tools: Vec<_> = guide
        .tools
        .iter()
        .filter_map(|slug| site.tool(lang, slug))
        .collect();

    // Reading order is the guides' own: a visitor who landed here from a search result has
    // no sidebar to fall back on, and one next step is worth more than twenty.
    let all = guides().all(lang);
    let position = all.iter().position(|other| other.slug == guide.slug);

    let mut context = site::base_context(lang);
    // "guide" is the same word in both languages, which is the only reason this title is
    // not built from a conditional like every other string on the site.
    context.insert(
        "page_title",
        &format!("{} — guide | AI SmartTalk Documents", guide.title),
    );
    context.insert("page_description", &guide.meta_description);
    context.insert("guide", guide);
    context.insert("tools", &tools);
    context.insert("guides_root", root(lang));
    context.insert("other_guides_root", root(lang.other()));
    context.insert("last_modified", guides().last_modified());
    // Always inserted, `null` when there is none: a template that tests a variable which
    // was never inserted is a Tera error waiting for the first and last guide of the list.
    context.insert(
        "previous",
        &position
            .filter(|index| *index > 0)
            .map(|index| &all[index - 1]),
    );
    context.insert(
        "next",
        &position
            .filter(|index| index + 1 < all.len())
            .map(|index| &all[index + 1]),
    );
    context.insert(
        "canonical",
        &format!("{}{}/{}", base, root(lang), guide.slug),
    );
    if let Some(alt) = guide.alt_slug.as_ref() {
        context.insert(
            "alternate",
            &format!("{}{}/{}", base, root(lang.other()), alt),
        );
    }

    Ok((Status::Ok, RawHtml(site.render("guide.html", &context)?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_language_serves_its_guides_from_its_own_prefix() {
        assert_eq!(root(Lang::Fr), "/guides");
        assert_eq!(root(Lang::En), "/en/guides");
    }

    #[test]
    fn a_catalogue_that_is_not_there_yields_no_guides() {
        assert!(read_catalog("static/outils/guides.klingon.json").is_empty());
    }

    /// The two files must describe the same guides, or `hreflang` sends a reader to a page
    /// that is not the one they were reading
    #[test]
    fn the_two_languages_declare_the_same_guides_and_point_at_each_other() {
        let fr = read_catalog("static/outils/guides.fr.json");
        let en = read_catalog("static/outils/guides.en.json");

        assert!(!fr.is_empty(), "the French guides should be installed");
        assert_eq!(fr.len(), en.len());

        for guide in &fr {
            let counterpart = en
                .iter()
                .find(|other| other.key == guide.key)
                .unwrap_or_else(|| panic!("no English guide for {}", guide.key));
            assert_eq!(guide.alt_slug.as_deref(), Some(counterpart.slug.as_str()));
            assert_eq!(counterpart.alt_slug.as_deref(), Some(guide.slug.as_str()));
        }
    }

    /// A slug is what a search engine indexes: two guides sharing one would serve the same
    /// URL twice, and a slug with a slash or an accent is a different URL from the one the
    /// sitemap declares
    #[test]
    fn slugs_are_unique_and_url_safe() {
        for lang in [Lang::Fr, Lang::En] {
            let all = read_catalog(match lang {
                Lang::Fr => "static/outils/guides.fr.json",
                Lang::En => "static/outils/guides.en.json",
            });
            let mut seen: Vec<&str> = Vec::new();
            for guide in &all {
                assert!(
                    !seen.contains(&guide.slug.as_str()),
                    "duplicate slug {}",
                    guide.slug
                );
                assert!(
                    guide
                        .slug
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                    "slug {} is not url-safe",
                    guide.slug
                );
                assert!(!guide.meta_description.is_empty(), "{}", guide.slug);
                seen.push(&guide.slug);
            }
        }
    }

    #[test]
    fn a_block_keeps_its_kind_and_its_content() {
        let raw = r#"{"type":"note","level":"warn","html":"<b>Careful.</b>"}"#;
        let block: Block = serde_json::from_str(raw).unwrap();
        assert_eq!(block.kind, "note");
        assert_eq!(block.level.as_deref(), Some("warn"));
        assert!(block.head.is_empty());
        // Serialised for the template under a name a Tera expression can address
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("\"kind\":\"note\""));
    }

    /// The templates are the only place a typo stays silent until a visitor finds it, so
    /// both are rendered here for real, against the catalogue that ships.
    #[test]
    fn the_index_renders_every_guide_of_both_languages() {
        for lang in [Lang::Fr, Lang::En] {
            let html = index(lang).expect("the guides index should render").0;
            for guide in guides().all(lang) {
                assert!(
                    html.contains(&format!("{}/{}", root(lang), guide.slug)),
                    "{} is missing from the {} index",
                    guide.slug,
                    lang.code()
                );
            }
        }
    }

    #[test]
    fn a_guide_renders_with_its_canonical_its_alternate_and_its_structured_data() {
        let slug = &guides().all(Lang::Fr)[0].slug.clone();
        let (status, html) = guide(Lang::Fr, slug).expect("the guide should render");

        assert_eq!(status, Status::Ok);
        let html = html.0;
        assert!(html.contains(&format!(
            "rel=\"canonical\" href=\"{}/guides/{}\"",
            site::public_base(),
            slug
        )));
        assert!(html.contains("hreflang=\"en\""));
        assert!(html.contains("\"@type\":\"Article\""));
        assert!(html.contains("\"@type\":\"BreadcrumbList\""));
        // The shell of the public site, not a second application
        assert!(html.contains("AI SmartTalk <strong>Documents</strong>"));
    }

    /// A dead link answers with a page, not with the JSON a caller of the API would get
    #[test]
    fn an_unknown_slug_answers_with_the_404_page() {
        let (status, html) = guide(Lang::En, "no-such-guide").expect("the 404 page renders");
        assert_eq!(status, Status::NotFound);
        // The path is echoed by the page; Tera escapes its slashes on the way out
        assert!(html.0.contains("no-such-guide"));
        assert!(html.0.contains("<html lang=\"en\""));
    }

    #[test]
    fn a_table_block_keeps_its_head_and_its_rows() {
        let raw = r#"{"type":"table","head":["A","B"],"rows":[["1","2"],["3","4"]]}"#;
        let block: Block = serde_json::from_str(raw).unwrap();
        assert_eq!(block.head.len(), 2);
        assert_eq!(block.rows[1][0], "3");
    }
}

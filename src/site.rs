//! The public tool pages, rendered by the service itself.
//!
//! A hash-routed single page cannot be indexed, and this market is won on exactly thirty
//! search intents — "compresser un pdf", "pdf en word". So each tool gets a real URL, real
//! HTML, and a real `<title>`, served from a catalogue that non-developers can edit without
//! touching a template.
//!
//! Rendering happens through Tera, which is already a dependency: no build step, no
//! generated files to keep in sync with the catalogue, and a page that cannot drift from
//! the endpoint it documents because both are read from the same JSON.

use crate::types::AppError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::OnceLock;
use tera::Tera;

/// One tool page. Every field but `slug` and `endpoint` is editorial: the catalogue is
/// meant to be edited by whoever writes the copy, not only by whoever wrote the code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub slug: String,
    pub title: String,
    pub h1: String,
    #[serde(default)]
    pub meta_description: String,
    #[serde(default)]
    pub lede: String,
    pub endpoint: String,
    #[serde(default)]
    pub accept: String,
    #[serde(default)]
    pub multiple: bool,
    /// JSON field the uploaded reference goes into. `pdf` for most, `file` for the Office
    /// conversion, `images` for the image collator.
    #[serde(default)]
    pub file_param: Option<String>,
    /// `asset` for the tools added with the ingestion socle, `binary` for the older routes
    /// that never learned to answer with one.
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub params: Vec<Param>,
    #[serde(default)]
    pub steps: Vec<String>,
    /// The sentence that sets this service apart: what the verdict will tell you.
    #[serde(default)]
    pub verdict_says: String,
    #[serde(default)]
    pub faq: Vec<Faq>,
    #[serde(default)]
    pub related: Vec<String>,
    #[serde(default)]
    pub group: Option<String>,
    /// Short name for the card in the grid. `h1` keeps the phrase people search for —
    /// « Océriser un PDF scanné » — while the card shows what a reader scans in a list.
    /// Both are needed, and they are not the same string.
    #[serde(default)]
    pub label: Option<String>,
    /// Icon key, resolved against the sprite in `templates/site/icons.html`
    #[serde(default)]
    pub icon: Option<String>,
    /// Slug of the same tool in the other language, for `hreflang` and the language switch
    #[serde(default)]
    pub alt_slug: Option<String>,
    /// File kinds this tool accepts, as `AssetKind::as_str` names them. Used by the home
    /// page to propose the right tools once it has read a dropped file's bytes.
    #[serde(default)]
    pub accepts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    /// `string`, `number`, `boolean`, `select`, `numbers`, `strings`
    #[serde(rename = "type")]
    pub kind: String,
    pub label: String,
    #[serde(default)]
    pub options: Vec<Vec<String>>,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    #[serde(default)]
    pub help: Option<String>,
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Faq {
    pub q: String,
    pub a: String,
}

/// A catalogue in one language, plus what the page chrome needs to say
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    #[serde(default)]
    pub tools: Vec<Tool>,
    #[serde(default)]
    pub index: Option<IndexCopy>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexCopy {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub meta_description: String,
    #[serde(default)]
    pub h1: String,
    #[serde(default)]
    pub lede: String,
    #[serde(default)]
    pub claims: Vec<String>,
}

/// Everything the site needs, resolved once at startup
pub struct Site {
    pub fr: Catalog,
    pub en: Catalog,
    tera: Option<Tera>,
    /// Date the catalogue last changed, `YYYY-MM-DD`, for the sitemap.
    ///
    /// Taken from the file rather than from the process start: a `lastmod` that moves on
    /// every deployment teaches a crawler to ignore the field, which is worse than not
    /// sending it at all.
    pub last_modified: String,
}

static SITE: OnceLock<Site> = OnceLock::new();

pub fn site() -> &'static Site {
    SITE.get_or_init(Site::load)
}

/// Load the catalogues and compile the templates. Called at startup so a malformed
/// catalogue is a line in the boot log rather than a 500 on the first visitor.
pub fn init() {
    let site = site();
    info!(
        "Public site: {} tools in French, {} in English, templates {}",
        site.fr.tools.len(),
        site.en.tools.len(),
        if site.tera.is_some() {
            "compiled"
        } else {
            "MISSING"
        }
    );
}

impl Site {
    fn load() -> Site {
        Site {
            fr: read_catalog("static/outils/catalog.fr.json"),
            en: read_catalog("static/outils/catalog.en.json"),
            tera: compile_templates(),
            last_modified: catalogue_date(),
        }
    }

    pub fn catalog(&self, lang: Lang) -> &Catalog {
        match lang {
            Lang::Fr => &self.fr,
            Lang::En => &self.en,
        }
    }

    pub fn tool(&self, lang: Lang, slug: &str) -> Option<&Tool> {
        self.catalog(lang)
            .tools
            .iter()
            .find(|tool| tool.slug == slug)
    }

    /// Render a template with a context, turning every Tera failure into an API error
    /// rather than a panic in a request handler.
    pub fn render(&self, template: &str, context: &tera::Context) -> Result<String, AppError> {
        let tera = self.tera.as_ref().ok_or_else(|| {
            AppError::NotFound("The public site templates are not installed".to_string())
        })?;

        tera.render(template, context)
            .map_err(|e| AppError::TemplateError(format!("{}: {}", template, describe(&e))))
    }
}

/// A missing catalogue is not fatal: the rest of the API has to keep serving. The pages
/// answer 404 until the file is there, and the startup log says how many tools were read.
fn read_catalog(path: &str) -> Catalog {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) => {
            warn!("Public site: {} could not be read ({})", path, e);
            return Catalog {
                tools: Vec::new(),
                index: None,
            };
        }
    };

    // The catalogue may be written as `{"tools": [...]}` or as a bare array; both are
    // natural things to hand a writer, and refusing one of them would be pedantry.
    match serde_json::from_str::<Catalog>(&raw) {
        Ok(catalog) if !catalog.tools.is_empty() => catalog,
        _ => match serde_json::from_str::<Vec<Tool>>(&raw) {
            Ok(tools) => Catalog { tools, index: None },
            Err(e) => {
                error!("Public site: {} is not a valid catalogue: {}", path, e);
                Catalog {
                    tools: Vec::new(),
                    index: None,
                }
            }
        },
    }
}

/// Newest modification date among the catalogues, as a date alone: the hour a file was
/// written says nothing useful to a crawler, and pretending to that precision invites it
/// to come back for nothing.
fn catalogue_date() -> String {
    let newest = [
        "static/outils/catalog.fr.json",
        "static/outils/catalog.en.json",
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

fn compile_templates() -> Option<Tera> {
    match Tera::new("templates/site/**/*.html") {
        Ok(tera) => Some(tera),
        Err(e) => {
            // Tera puts the template name in the top-level error and the actual syntax
            // problem three causes down. Printing only the first line says "failed to
            // parse tool.html" and nothing else, which is how a typo costs an afternoon.
            error!("Public site templates failed to compile: {}", describe(&e));
            None
        }
    }
}

/// Tera nests its causes, and only the innermost one names the actual problem
fn describe(error: &tera::Error) -> String {
    let mut message = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        message.push_str(" — ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Fr,
    En,
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::Fr => "fr",
            Lang::En => "en",
        }
    }

    /// Where this language's home page lives.
    ///
    /// French is at the root because it is the first market and the root is the address a
    /// visitor types; giving it to a language rather than to a redirect saves a hop on the
    /// page that matters most.
    pub fn home(self) -> &'static str {
        match self {
            Lang::Fr => "/",
            Lang::En => "/en",
        }
    }

    /// Path prefix the tool pages live under. Kept in the language of the reader: a French
    /// URL that reads `/tools/compress-pdf` looks machine-translated, and that costs trust
    /// on the one page where trust decides.
    pub fn tools(self) -> &'static str {
        match self {
            Lang::Fr => "/outils",
            Lang::En => "/tools",
        }
    }

    pub fn pricing(self) -> &'static str {
        match self {
            Lang::Fr => "/tarifs",
            Lang::En => "/pricing",
        }
    }

    /// The account pages, in this language.
    ///
    /// The English half of the site used to link to `/connexion`, `/inscription` and the
    /// French workspace: an English visitor who clicked "Sign in" landed on a page written
    /// entirely in French, at the exact moment we ask them for a password.
    pub fn signin(self) -> &'static str {
        match self {
            Lang::Fr => "/connexion",
            Lang::En => "/signin",
        }
    }

    pub fn signup(self) -> &'static str {
        match self {
            Lang::Fr => "/inscription",
            Lang::En => "/signup",
        }
    }

    pub fn workspace(self) -> &'static str {
        match self {
            Lang::Fr => "/app",
            Lang::En => "/en/app",
        }
    }

    pub fn other(self) -> Lang {
        match self {
            Lang::Fr => Lang::En,
            Lang::En => Lang::Fr,
        }
    }
}

/// Absolute origin the canonical links and the sitemap are built from.
///
/// Search engines treat `https://host/x` and `http://host/x` as two pages; a canonical that
/// is not absolute leaves that ambiguity in place, which is the classic way to split one
/// page's authority in half.
pub fn public_base() -> String {
    std::env::var("PUBLIC_BASE_URL")
        .unwrap_or_else(|_| "https://pdf.aismarttalk.tech".to_string())
        .trim_end_matches('/')
        .to_string()
}

// ------------ Pricing ------------

/// One tier of the offer. Written here rather than in a catalogue because a price is not
/// editorial copy: it is a commitment, and it should change with a code review.
#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub name: String,
    pub price: String,
    pub period: String,
    pub pitch: String,
    pub features: Vec<String>,
    pub cta: String,
    pub cta_href: String,
    /// The one tier the eye should land on
    pub featured: bool,
}

/// The four tiers of `PLAN-METAMORPHOSE.md` §7.
///
/// Two things are stated rather than implied, because they are the offer's whole argument:
/// no watermark on any tier, and the quality verdict included everywhere. Charging for the
/// verdict would turn our differentiator into a paywall nobody would ever meet.
pub fn plans(lang: Lang) -> Vec<Plan> {
    let fr = lang == Lang::Fr;

    vec![
        Plan {
            name: if fr { "Gratuit" } else { "Free" }.to_string(),
            price: "0 €".to_string(),
            period: if fr { "pour toujours" } else { "forever" }.to_string(),
            pitch: if fr {
                "Tous les outils, sans compte et sans filigrane."
            } else {
                "Every tool, no account and no watermark."
            }
            .to_string(),
            features: strings(if fr {
                &[
                    "Les 21 outils, sans limite de tâches",
                    "Fichiers jusqu'à 50 Mo",
                    "3 fichiers par traitement",
                    "Le verdict de qualité, toujours",
                    "Aucun filigrane, jamais",
                    "Fichiers supprimés au bout d'une heure",
                ]
            } else {
                &[
                    "All 21 tools, no task limit",
                    "Files up to 50 MB",
                    "3 files per operation",
                    "The quality verdict, always",
                    "No watermark, ever",
                    "Files deleted after one hour",
                ]
            }),
            cta: if fr { "Commencer" } else { "Start now" }.to_string(),
            cta_href: lang.home().to_string(),
            featured: false,
        },
        Plan {
            name: "Pro".to_string(),
            price: "5 €".to_string(),
            period: if fr {
                "par mois, en annuel"
            } else {
                "per month, billed yearly"
            }
            .to_string(),
            pitch: if fr {
                "Pour qui traite des documents toutes les semaines."
            } else {
                "For anyone handling documents every week."
            }
            .to_string(),
            features: strings(if fr {
                &[
                    "Taille de fichier illimitée",
                    "Traitement par lots illimité",
                    "OCR : 2 000 pages par mois",
                    "Rapport de qualité détaillé",
                    "Une charte documentaire",
                    "Signature électronique",
                    "Fichiers gardés 7 jours",
                ]
            } else {
                &[
                    "Unlimited file size",
                    "Unlimited batch processing",
                    "OCR: 2,000 pages a month",
                    "Detailed quality report",
                    "One document brand kit",
                    "Electronic signature",
                    "Files kept for 7 days",
                ]
            }),
            cta: if fr {
                "Demander un accès"
            } else {
                "Request access"
            }
            .to_string(),
            cta_href: "/dev#/acces".to_string(),
            featured: true,
        },
        Plan {
            name: if fr { "Équipe" } else { "Team" }.to_string(),
            price: "12 €".to_string(),
            period: if fr {
                "par utilisateur et par mois"
            } else {
                "per user, per month"
            }
            .to_string(),
            pitch: if fr {
                "Vos documents à votre charte, et la preuve qu'ils viennent de vous."
            } else {
                "Your documents in your brand, and the proof they came from you."
            }
            .to_string(),
            features: strings(if fr {
                &[
                    "Tout le plan Pro",
                    "OCR et IA sans limite raisonnable",
                    "Chartes documentaires illimitées",
                    "Attestation de provenance signée",
                    "Le contrat de document (pages, mise en page)",
                    "Hébergement en France, accord de traitement",
                    "Fichiers gardés 30 jours",
                ]
            } else {
                &[
                    "Everything in Pro",
                    "OCR and AI without a practical limit",
                    "Unlimited document brand kits",
                    "Signed provenance attestation",
                    "The document contract (pages, layout)",
                    "Hosted in France, data processing agreement",
                    "Files kept for 30 days",
                ]
            }),
            cta: if fr { "Nous contacter" } else { "Talk to us" }.to_string(),
            cta_href: "/dev#/acces".to_string(),
            featured: false,
        },
        Plan {
            name: "API".to_string(),
            price: if fr { "sur mesure" } else { "custom" }.to_string(),
            period: if fr {
                "à la page, au fichier"
            } else {
                "per page, per file"
            }
            .to_string(),
            pitch: if fr {
                "Le moteur, ses verdicts et sa preuve, dans vos produits."
            } else {
                "The engine, its verdicts and its proof, inside your products."
            }
            .to_string(),
            features: strings(if fr {
                &[
                    "43 endpoints, un contrat OpenAPI",
                    "Serveur MCP natif pour vos agents",
                    "Travaux asynchrones et rappels signés",
                    "Clé nommée par intégration, quotas, métriques",
                    "Attestation et vérification de provenance",
                    "Rétention configurable",
                ]
            } else {
                &[
                    "43 endpoints, one OpenAPI contract",
                    "Native MCP server for your agents",
                    "Asynchronous jobs and signed callbacks",
                    "One named key per integration, quotas, metrics",
                    "Provenance attestation and verification",
                    "Configurable retention",
                ]
            }),
            cta: if fr {
                "Voir la documentation"
            } else {
                "Read the docs"
            }
            .to_string(),
            cta_href: "/dev#/api".to_string(),
            featured: false,
        },
    ]
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

/// The page a visitor gets instead of a dead end.
///
/// The language is guessed from the path rather than from `Accept-Language`: someone who
/// landed on `/tools/whatever` was reading English a second ago, and the twenty-one links
/// this page offers should be in that language.
pub fn not_found_page(path: &str) -> Result<String, AppError> {
    let lang = if path.starts_with("/tools") || path.starts_with("/en") || path == "/pricing" {
        Lang::En
    } else {
        Lang::Fr
    };

    let mut context = base_context(lang);
    context.insert("page_title", "404");
    context.insert("page_description", "");
    context.insert("missing", path);
    site().render("404.html", &context)
}

/// Tools of one language, grouped in catalogue order.
///
/// The navigation needs them on every page, not just on the home page: a visitor who lands
/// on a tool page from a search result — which is most of them — must be able to reach the
/// other twenty without going back to an index they never saw.
fn nav_groups(lang: Lang) -> Vec<(String, Vec<Tool>)> {
    let mut groups: Vec<(String, Vec<Tool>)> = Vec::new();

    for tool in &site().catalog(lang).tools {
        let name = tool.group.clone().unwrap_or_default();
        match groups.iter_mut().find(|(group, _)| *group == name) {
            Some((_, tools)) => tools.push(tool.clone()),
            None => groups.push((name, vec![tool.clone()])),
        }
    }

    groups
}

/// Context every page shares
pub fn base_context(lang: Lang) -> tera::Context {
    let mut context = tera::Context::new();
    let groups = nav_groups(lang);
    context.insert(
        "nav_groups",
        &groups
            .into_iter()
            .map(|(name, tools)| serde_json::json!({ "name": name, "tools": tools }))
            .collect::<Vec<_>>(),
    );
    context.insert("lang", lang.code());
    context.insert("home", lang.home());
    context.insert("root", lang.tools());
    context.insert("pricing", lang.pricing());
    context.insert("signin", lang.signin());
    context.insert("signup", lang.signup());
    context.insert("workspace", lang.workspace());
    context.insert("other_lang", lang.other().code());
    context.insert("other_home", lang.other().home());
    context.insert("other_root", lang.other().tools());
    context.insert("base", &public_base());
    context.insert("version", env!("CARGO_PKG_VERSION"));
    context.insert(
        "retention_hours",
        &(crate::config::config().asset_ttl.as_secs() / 3600),
    );
    // Both numbers come from the config so that a page can never quote a retention the
    // service does not actually apply — which it did, for a while, on the sign-up page.
    context.insert(
        "member_retention_hours",
        &(crate::config::config().asset_ttl_member.as_secs() / 3600),
    );
    context.insert("max_mb", &crate::config::config().asset_max_mb);
    context
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_language_knows_its_counterpart_and_its_paths() {
        assert_eq!(Lang::Fr.home(), "/");
        assert_eq!(Lang::En.home(), "/en");
        assert_eq!(Lang::Fr.tools(), "/outils");
        assert_eq!(Lang::En.tools(), "/tools");
        assert_eq!(Lang::Fr.pricing(), "/tarifs");
        assert_eq!(Lang::Fr.other(), Lang::En);
        assert_eq!(Lang::En.other().code(), "fr");
    }

    /// A trailing slash in the configured base would double up in every canonical
    #[test]
    fn the_public_base_never_ends_with_a_slash() {
        std::env::set_var("PUBLIC_BASE_URL", "https://example.test/");
        assert_eq!(public_base(), "https://example.test");
        std::env::remove_var("PUBLIC_BASE_URL");
    }

    #[test]
    fn reads_a_catalogue_written_as_an_object() {
        let raw = r#"{"tools":[{"slug":"compresser-pdf","title":"T","h1":"H",
                     "endpoint":"/api/compress"}]}"#;
        let catalog: Catalog = serde_json::from_str(raw).unwrap();
        assert_eq!(catalog.tools.len(), 1);
        assert_eq!(catalog.tools[0].slug, "compresser-pdf");
        // Everything editorial is optional, so a half-written entry still loads
        assert!(catalog.tools[0].faq.is_empty());
        assert!(!catalog.tools[0].multiple);
    }

    #[test]
    fn reads_a_catalogue_written_as_a_bare_array() {
        let raw = r#"[{"slug":"merge-pdf","title":"T","h1":"H","endpoint":"/api/merge"}]"#;
        let tools: Vec<Tool> = serde_json::from_str(raw).unwrap();
        assert_eq!(tools[0].slug, "merge-pdf");
    }

    /// A catalogue that will not parse must not take the API down with it
    #[test]
    fn a_broken_catalogue_yields_an_empty_one() {
        let catalog = read_catalog("static/outils/does-not-exist.json");
        assert!(catalog.tools.is_empty());
    }

    #[test]
    fn a_parameter_keeps_its_select_options() {
        let raw = r#"{"name":"level","type":"select","label":"Niveau",
                      "options":[["ebook","Équilibré"],["screen","Maximum"]],
                      "default":"ebook"}"#;
        let param: Param = serde_json::from_str(raw).unwrap();
        assert_eq!(param.kind, "select");
        assert_eq!(param.options.len(), 2);
        assert_eq!(param.options[0][1], "Équilibré");
        assert!(!param.required);
    }
}

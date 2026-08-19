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

    /// Path prefix this language's pages live under
    pub fn root(self) -> &'static str {
        match self {
            Lang::Fr => "/outils",
            Lang::En => "/tools",
        }
    }

    pub fn other(self) -> Lang {
        match self {
            Lang::Fr => Lang::En,
            Lang::En => Lang::Fr,
        }
    }
}

/// Context every page shares
pub fn base_context(lang: Lang) -> tera::Context {
    let mut context = tera::Context::new();
    context.insert("lang", lang.code());
    context.insert("root", lang.root());
    context.insert("other_lang", lang.other().code());
    context.insert("other_root", lang.other().root());
    context.insert("version", env!("CARGO_PKG_VERSION"));
    context.insert(
        "retention_hours",
        &(crate::config::config().asset_ttl.as_secs() / 3600),
    );
    context.insert("max_mb", &crate::config::config().asset_max_mb);
    context
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_language_knows_its_counterpart_and_its_root() {
        assert_eq!(Lang::Fr.root(), "/outils");
        assert_eq!(Lang::En.root(), "/tools");
        assert_eq!(Lang::Fr.other(), Lang::En);
        assert_eq!(Lang::En.other().code(), "fr");
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

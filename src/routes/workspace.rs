//! The workspace — a working folder, not an admin panel.
//!
//! `/app` and `/en/app` serve the shell of a page and nothing else. Everything that appears
//! on it belongs to somebody: the quality record, the keys, the plan. None of it is rendered
//! here, because rendering it here would mean this route deciding who is signed in, and a
//! page that answers differently depending on a cookie is a page a cache will eventually
//! hand to the wrong person.
//!
//! So the template is the same for everyone, and `static/outils/workspace.js` fills it from
//! `/api/auth/me`, `/api/history`, `/api/keys` and `/api/usage` — each with the session
//! cookie, each answering 401 to a stranger. A visitor who is not signed in gets the sign-in
//! page, not an empty workspace.
//!
//! The reason this page exists at all is `GET /api/history`. Every competitor's account
//! shows a list of files; this one shows what each operation cost — text preserved, pages
//! kept, score out of a hundred. A verdict shown once is a card; kept in order, they are a
//! record, and a record is the only thing in this product that compounds.

use crate::site::{self, Lang};
use crate::types::AppError;
use rocket::response::content::RawHtml;

#[get("/app")]
pub fn workspace_fr() -> Result<RawHtml<String>, AppError> {
    workspace(Lang::Fr)
}

#[get("/en/app")]
pub fn workspace_en() -> Result<RawHtml<String>, AppError> {
    workspace(Lang::En)
}

fn workspace(lang: Lang) -> Result<RawHtml<String>, AppError> {
    let french = lang == Lang::Fr;
    let mut context = site::base_context(lang);

    context.insert(
        "page_title",
        if french {
            "Votre espace — vos documents et leur registre | AI SmartTalk Documents"
        } else {
            "Your workspace — your documents and their record | AI SmartTalk Documents"
        },
    );
    context.insert(
        "page_description",
        if french {
            "Le registre de qualité de vos documents : chaque opération avec son verdict, \
             vos clés d'API en libre-service, et les limites de votre palier."
        } else {
            "The quality record of your documents: every operation with its verdict, your \
             self-service API keys, and the limits of your plan."
        },
    );

    // No canonical and no alternate: this page is `noindex`, and declaring a canonical for a
    // page nobody should index is at best noise and at worst an invitation.
    Ok(RawHtml(site::site().render("app.html", &context)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The template is the one place a typo stays silent until a visitor finds it, so both
    /// languages are rendered here for real.
    #[test]
    fn both_languages_render_the_shell_of_the_workspace() {
        for lang in [Lang::Fr, Lang::En] {
            let html = workspace(lang).expect("the workspace should render").0;

            // The shell of the public site, not a second application
            assert!(html.contains("AI SmartTalk <strong>Documents</strong>"));
            assert!(html.contains(&format!("<html lang=\"{}\"", lang.code())));

            // The three sections, in the order they matter
            let registry = html.find("id=\"registre\"").expect("the quality record");
            let keys = html.find("id=\"cles\"").expect("the keys");
            let plan = html.find("id=\"palier\"").expect("the plan");
            assert!(registry < keys && keys < plan);

            // The page is client-rendered, and its script is what renders it
            assert!(html.contains("/static/outils/workspace.js"));
        }
    }

    /// A personal space that ends up in a search index is a leak of the worst kind: the URL
    /// alone tells a stranger the page exists, and the title tells them whose it is.
    #[test]
    fn the_workspace_is_never_indexed() {
        let html = workspace(Lang::Fr).expect("the workspace should render").0;
        assert!(html.contains("name=\"robots\" content=\"noindex"));
        assert!(!html.contains("rel=\"canonical\""));
    }

    /// The sign-in address is handed to the script by the template rather than hardcoded in
    /// it, and each language points at its own.
    #[test]
    fn each_language_knows_where_to_send_a_stranger() {
        assert!(workspace(Lang::Fr)
            .unwrap()
            .0
            .contains("data-signin=\"/connexion\""));
        assert!(workspace(Lang::En)
            .unwrap()
            .0
            .contains("data-signin=\"/signin\""));
    }

    /// An empty record must offer a way out of itself, in the language of the reader
    #[test]
    fn the_tools_the_record_points_at_are_in_the_readers_language() {
        assert!(workspace(Lang::Fr)
            .unwrap()
            .0
            .contains("data-tools=\"/outils\""));
        assert!(workspace(Lang::En)
            .unwrap()
            .0
            .contains("data-tools=\"/tools\""));
    }
}

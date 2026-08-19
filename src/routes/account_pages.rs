//! Signing up and signing in, as pages of the site.
//!
//! These are not an application bolted onto the side: they extend `base.html` like every
//! other public page, so the header, the brand, the language switch and the theme are the
//! ones the visitor was already looking at. Someone who signs up after compressing a file
//! should never feel they left the product to do it.
//!
//! The forms themselves post to `/api/auth/*` from `static/outils/account.js`. Nothing is
//! rendered here that depends on the session: the pages are identical for everyone, which
//! keeps them cacheable and keeps this module free of any coupling to the session guard.
//!
//! They carry `noindex, follow`: a sign-in form has nothing to offer a search result, but
//! the links it points at — the tools, the pricing — deserve to be followed.

use crate::site::{self, Lang};
use crate::types::AppError;
use rocket::response::content::RawHtml;

// ------------ Sign in ------------

#[get("/connexion")]
pub fn signin_fr() -> Result<RawHtml<String>, AppError> {
    render(Lang::Fr, Page::SignIn)
}

#[get("/signin")]
pub fn signin_en() -> Result<RawHtml<String>, AppError> {
    render(Lang::En, Page::SignIn)
}

// ------------ Sign up ------------

#[get("/inscription")]
pub fn signup_fr() -> Result<RawHtml<String>, AppError> {
    render(Lang::Fr, Page::SignUp)
}

#[get("/signup")]
pub fn signup_en() -> Result<RawHtml<String>, AppError> {
    render(Lang::En, Page::SignUp)
}

// ------------ Rendering ------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    SignIn,
    SignUp,
}

impl Page {
    fn template(self) -> &'static str {
        match self {
            Page::SignIn => "signin.html",
            Page::SignUp => "signup.html",
        }
    }

    /// Where this page lives in a given language. Kept in one place because the canonical,
    /// the alternate and the cross-link between the two forms all read from it, and three
    /// copies of the same table is three chances to leave one behind.
    fn path(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Page::SignIn, Lang::Fr) => "/connexion",
            (Page::SignIn, Lang::En) => "/signin",
            (Page::SignUp, Lang::Fr) => "/inscription",
            (Page::SignUp, Lang::En) => "/signup",
        }
    }

    fn other(self) -> Page {
        match self {
            Page::SignIn => Page::SignUp,
            Page::SignUp => Page::SignIn,
        }
    }
}

/// The workspace this language sends a newcomer to once the account exists.
fn workspace(lang: Lang) -> &'static str {
    match lang {
        Lang::Fr => "/app",
        Lang::En => "/en/app",
    }
}

fn render(lang: Lang, page: Page) -> Result<RawHtml<String>, AppError> {
    let french = lang == Lang::Fr;
    let base = site::public_base();
    let mut context = site::base_context(lang);

    context.insert(
        "page_title",
        &match (page, french) {
            (Page::SignIn, true) => "Se connecter | AI SmartTalk Documents",
            (Page::SignIn, false) => "Sign in | AI SmartTalk Documents",
            (Page::SignUp, true) => "Créer un compte gratuit | AI SmartTalk Documents",
            (Page::SignUp, false) => "Create a free account | AI SmartTalk Documents",
        },
    );
    context.insert(
        "page_description",
        &match (page, french) {
            (Page::SignIn, true) => {
                "Connectez-vous pour retrouver vos documents, leurs verdicts de qualité et \
                 vos clés d'API."
            }
            (Page::SignIn, false) => {
                "Sign in to find your documents, their quality verdicts and your API keys."
            }
            (Page::SignUp, true) => {
                "Un compte gratuit : l'historique de vos documents avec le verdict de chaque \
                 opération, vos clés d'API en libre-service, et des fichiers gardés plus \
                 longtemps. Sans carte bancaire."
            }
            (Page::SignUp, false) => {
                "A free account: the history of your documents with the verdict of every \
                 operation, self-service API keys, and files kept for longer. No card."
            }
        },
    );

    context.insert("canonical", &format!("{}{}", base, page.path(lang)));
    context.insert("alternate", &format!("{}{}", base, page.path(lang.other())));

    // The template needs the paths as paths, not as absolute URLs: a link that leaves the
    // origin for no reason drops the session cookie on a domain change nobody intended.
    context.insert("this_page", page.path(lang));
    context.insert("other_page", page.other().path(lang));
    context.insert("workspace", workspace(lang));

    Ok(RawHtml(site::site().render(page.template(), &context)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_form_knows_its_counterpart_in_both_languages() {
        assert_eq!(Page::SignIn.path(Lang::Fr), "/connexion");
        assert_eq!(Page::SignIn.path(Lang::En), "/signin");
        assert_eq!(Page::SignUp.path(Lang::Fr), "/inscription");
        assert_eq!(Page::SignUp.path(Lang::En), "/signup");
        assert_eq!(Page::SignIn.other().path(Lang::Fr), "/inscription");
        assert_eq!(Page::SignUp.other().path(Lang::En), "/signin");
    }

    /// A French sign-up that landed on the English workspace would switch language on the
    /// one screen where the visitor has just trusted us with a password
    #[test]
    fn each_language_lands_in_its_own_workspace() {
        assert_eq!(workspace(Lang::Fr), "/app");
        assert_eq!(workspace(Lang::En), "/en/app");
    }

    #[test]
    fn the_pages_render_with_their_canonical_and_their_noindex() {
        for (lang, page, path) in [
            (Lang::Fr, Page::SignUp, "/inscription"),
            (Lang::En, Page::SignIn, "/signin"),
        ] {
            let html = render(lang, page).expect("the account pages render").0;
            assert!(html.contains("name=\"robots\" content=\"noindex, follow\""));
            assert!(html.contains(&format!(
                "rel=\"canonical\" href=\"{}{}\"",
                site::public_base(),
                path
            )));
            // The form is useless without the script that submits it
            assert!(html.contains("/static/outils/account.js"));
        }
    }
}

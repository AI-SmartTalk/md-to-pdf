#[macro_use]
extern crate rocket;

#[macro_use]
extern crate log;

mod accounts;
mod assets;
mod attest;
mod auth;
mod blocks;
mod cache;
mod catchers;
mod censor;
mod charts;
mod config;
mod exec;
mod helpers;
mod history;
mod jobs;
mod layout;
mod mermaid;
mod obs;
mod pdfops;
mod pipeline;
mod routes;
mod sign;
mod site;
mod themes;
mod types;
mod urlguard;

use rocket::fs::FileServer;
use rocket::http::Method;
use rocket_cors::{AllowedOrigins, CorsOptions};

#[launch]
fn rocket() -> _ {
    env_logger::init();

    // Configuration first: everything below, including the log shipper, reads from it
    info!("Configuration: {}", config::config().summary());
    obs::init();
    themes::init();

    // Generated PDFs live here; create it up front so the very first request cannot fail
    if let Err(e) = std::fs::create_dir_all(helpers::pdf_root()) {
        error!("Could not create {:?}: {}", helpers::pdf_root(), e);
    }

    // Uploaded files live here, and whatever a previous run left behind is dropped now
    assets::init();

    // Accounts, sessions and the key index. First durable state this service has ever
    // held — see the note at the top of `accounts.rs` for why it is files and not a base.
    accounts::init();

    // Catalogues and templates of the public tool pages: a malformed catalogue must show
    // up in the boot log, not on the first visitor's screen
    site::init();

    if auth::is_open() {
        warn!("API_KEY is not set: the /api endpoints are open to anyone who can reach them");
    } else {
        info!(
            "API keys configured: {}",
            auth::configured_names().join(", ")
        );
    }

    let cors = CorsOptions::default()
        .allowed_origins(AllowedOrigins::all())
        .allowed_methods(
            vec![Method::Get, Method::Post, Method::Delete, Method::Options]
                .into_iter()
                .map(From::from)
                .collect(),
        )
        // The API authenticates with a header, never with cookies: credentialed
        // cross-origin requests must stay off while the origin is a wildcard.
        .allow_credentials(false)
        .to_cors()
        .expect("Error creating CORS fairing");

    rocket::build()
        .attach(cors)
        .attach(obs::Observer)
        // The sweeper spawns a tokio task, so it cannot start while the instance is only
        // being built: liftoff is the first moment there is a runtime to spawn into.
        .attach(rocket::fairing::AdHoc::on_liftoff("Asset sweeper", |_| {
            Box::pin(async { assets::start_sweeper() })
        }))
        // Legacy FormData endpoint (backward compatible), and the public tool pages.
        // Both are declared routes, so they outrank the static file server below.
        .mount(
            "/",
            routes![
                routes::legacy::convert,
                // The root belongs to the visitor who typed the domain
                routes::site::home_fr,
                routes::site::home_en,
                routes::site::tool_fr,
                routes::site::tool_en,
                routes::site::pricing_fr,
                routes::site::pricing_en,
                // Where the home pages used to live
                routes::site::index_fr_moved,
                routes::site::index_en_moved,
                // The integrator console, unchanged, at its own address
                routes::site::console,
                routes::site::console_moved,
                // Le compte : s'inscrire, se connecter, et son espace de travail
                routes::account_pages::signin_fr,
                routes::account_pages::signin_en,
                routes::account_pages::signup_fr,
                routes::account_pages::signup_en,
                routes::workspace::workspace_fr,
                routes::workspace::workspace_en,
                // Les douze guides, enfin indexables : c'est le seul contenu de fond du
                // produit, et il vivait derrière un lien de pied de page.
                routes::guides::index_fr,
                routes::guides::index_en,
                routes::guides::guide_fr,
                routes::guides::guide_en,
                routes::site::sitemap,
                routes::site::robots,
                routes::site::og_image,
            ],
        )
        // Static files
        .mount("/static", FileServer::from("static"))
        // Assets the console and the public site reference from the root (favicon, …).
        // Ranked below every declared route so /, /api and /download always win.
        .mount("/", FileServer::from("static").rank(20))
        // Download saved PDFs
        .mount("/download", routes![routes::download::download_pdf])
        // Model Context Protocol: the agent surface. Authenticated by the same keys as the
        // rest of the API, so an agent's consumption is attributed and quota-ed like any
        // other integration.
        .mount("/mcp", routes![routes::mcp::mcp_post, routes::mcp::mcp_get])
        // New JSON API endpoints
        .mount(
            "/api",
            routes![
                routes::health::health,
                routes::convert::convert,
                routes::render::render,
                routes::html_to_pdf::html_to_pdf,
                routes::preview::preview,
                routes::merge::merge,
                routes::watermark::watermark,
                routes::protect::protect,
                routes::redact::redact,
                routes::diff::diff,
                routes::layout::analyze_layout,
                routes::metrics::metrics,
                routes::themes::list_themes,
                routes::themes::theme_preview,
                // Ingestion: the way a caller's own file gets in
                routes::files::upload,
                routes::files::fetch,
                routes::files::describe,
                routes::files::forget,
                routes::jobs::status,
                // The toolbelt. Every one of these accepts an uploaded asset or a PDF this
                // service produced, and answers with a binary, a download URL or an asset.
                routes::pages::pages,
                routes::numbering::number_pages,
                routes::crop::crop,
                routes::compress::compress,
                routes::repair::repair,
                routes::repair::unlock,
                routes::rasterize::rasterize,
                routes::images_to_pdf::images_to_pdf,
                routes::office::office_to_pdf,
                routes::office::pdf_to_office,
                routes::ocr::ocr,
                routes::extract::extract,
                routes::pdfa::to_pdfa,
                // The contract and its proof: what no competitor answers
                routes::compose::compose,
                routes::attest::attest,
                routes::attest::verify,
                routes::jobs::submit,
                // Accounts: signing up, signing in, and minting your own keys
                routes::auth::signup,
                routes::auth::login,
                routes::auth::logout,
                routes::auth::me,
                routes::auth::create_key,
                routes::auth::list_keys,
                routes::auth::revoke_key,
                routes::auth::usage,
                routes::auth::history,
                routes::auth::clear_history,
            ],
        )
        .register(
            "/",
            catchers![
                catchers::bad_request,
                catchers::unauthorized,
                catchers::not_found,
                catchers::payload_too_large,
                catchers::unsupported_media_type,
                catchers::unprocessable_entity,
                catchers::too_many_requests,
                catchers::internal_error,
                catchers::bad_gateway,
                catchers::gateway_timeout,
            ],
        )
}

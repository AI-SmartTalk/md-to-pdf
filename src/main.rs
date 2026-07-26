#[macro_use]
extern crate rocket;

#[macro_use]
extern crate log;

mod auth;
mod blocks;
mod cache;
mod catchers;
mod censor;
mod charts;
mod config;
mod exec;
mod helpers;
mod layout;
mod mermaid;
mod obs;
mod pdfops;
mod pipeline;
mod routes;
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

    if std::env::var("API_KEY")
        .map(|k| k.is_empty())
        .unwrap_or(true)
    {
        warn!("API_KEY is not set: the /api endpoints are open to anyone who can reach them");
    }

    let cors = CorsOptions::default()
        .allowed_origins(AllowedOrigins::all())
        .allowed_methods(
            vec![Method::Get, Method::Post, Method::Options]
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
        // Legacy FormData endpoint (backward compatible)
        .mount("/", routes![routes::legacy::convert])
        // Static files
        .mount("/static", FileServer::from("static"))
        // Landing page, API reference and test console at the service root.
        // Ranked below every declared route so /api and /download always win.
        .mount("/", FileServer::from("static").rank(20))
        // Download saved PDFs
        .mount("/download", routes![routes::download::download_pdf])
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

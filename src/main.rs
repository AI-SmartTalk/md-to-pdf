#[macro_use]
extern crate rocket;

#[macro_use]
extern crate log;

mod auth;
mod catchers;
mod helpers;
mod routes;
mod types;

use rocket::fs::FileServer;
use rocket::http::Method;
use rocket_cors::{AllowedOrigins, CorsOptions};

#[launch]
fn rocket() -> _ {
    env_logger::init();

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
        // Legacy FormData endpoint (backward compatible)
        .mount("/", routes![routes::legacy::convert])
        // Static files
        .mount("/static", FileServer::from("static"))
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
                catchers::internal_error,
                catchers::gateway_timeout,
            ],
        )
}

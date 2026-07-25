use crate::helpers;
use crate::types::HealthResponse;
use rocket::serde::json::Json;

/// Unauthenticated on purpose: this is what the container healthcheck polls.
#[get("/health")]
pub fn health() -> Json<HealthResponse> {
    // Report what is actually installed instead of a hardcoded list
    let engines: Vec<String> = ["weasyprint", "wkhtmltopdf", "pdflatex"]
        .iter()
        .filter(|bin| helpers::binary_available(bin))
        .map(|bin| bin.to_string())
        .collect();

    let core_tools_present =
        helpers::binary_available("pandoc") && helpers::binary_available("weasyprint");

    Json(HealthResponse {
        status: if core_tools_present { "ok" } else { "degraded" }.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        engines,
    })
}

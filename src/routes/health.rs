use crate::helpers;
use crate::types::HealthResponse;
use rocket::http::Status;
use rocket::serde::json::Json;

/// External binaries a whole family of endpoints depends on. Reporting them is not
/// decoration: an image built without Ghostscript answers 200 on every probe and 500 on
/// every compression, and the only way to tell the two deployments apart is to ask.
const CAPABILITIES: [(&str, &str); 6] = [
    ("compress", "gs"),
    ("pdfa", "gs"),
    ("ocr", "ocrmypdf"),
    ("office", "soffice"),
    ("images-to-pdf", "img2pdf"),
    ("pages", "qpdf"),
];

/// Unauthenticated on purpose: this is what the container healthcheck polls.
///
/// A degraded service answers `503`, not `200` with a sad word in the body. Everything that
/// watches this endpoint — the compose healthcheck, `deploy/md-to-pdf-watchdog.sh` and the
/// rollback in `deploy/bootstrap.sh` — is a `curl -fsS`, which reads the status line and
/// nothing else. Reporting `"degraded"` under a `200` therefore told all three that a
/// service answering 500 to every conversion was fine: no restart, and no rollback of the
/// deployment that broke it.
#[get("/health")]
pub fn health() -> (Status, Json<HealthResponse>) {
    // Report what is actually installed instead of a hardcoded list
    let engines: Vec<String> = ["weasyprint", "wkhtmltopdf", "pdflatex"]
        .iter()
        .filter(|bin| helpers::binary_available(bin))
        .map(|bin| bin.to_string())
        .collect();

    let capabilities: Vec<String> = CAPABILITIES
        .iter()
        .filter(|(_, binary)| helpers::binary_available(binary))
        .map(|(name, _)| name.to_string())
        .collect();

    let core_tools_present =
        helpers::binary_available("pandoc") && helpers::binary_available("weasyprint");

    // When the converters live in the isolated container, this service is only as healthy as
    // that container is: if the worker is gone, every probe still answers 200 while every
    // conversion returns 500, and the watchdog restarts nothing because nothing looks wrong.
    let sandbox = crate::sandbox::enabled().then(|| {
        if crate::sandbox::responds() {
            "ok".to_string()
        } else {
            "unreachable".to_string()
        }
    });

    let healthy = core_tools_present && sandbox.as_deref() != Some("unreachable");

    (
        if healthy {
            Status::Ok
        } else {
            Status::ServiceUnavailable
        },
        Json(HealthResponse {
            status: if healthy { "ok" } else { "degraded" }.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            engines,
            capabilities: Some(capabilities),
            sandbox,
        }),
    )
}

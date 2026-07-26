use crate::auth::ApiKey;
use crate::obs;
use rocket::http::ContentType;

/// Prometheus scrape endpoint.
///
/// Behind the API key on purpose: the per-route counters expose client traffic volumes,
/// which is not something to publish on the open internet.
#[get("/metrics")]
pub fn metrics(_key: ApiKey) -> (ContentType, String) {
    let content_type = ContentType::new("text", "plain").with_params(("version", "0.0.4"));
    (content_type, obs::metrics_text())
}

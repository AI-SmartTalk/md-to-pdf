use crate::types::AppError;
use rocket::http::Status;
use rocket::request::{FromRequest, Outcome, Request};
use std::env;

/// Request guard protecting the `/api` routes.
///
/// The key is read from the `API_KEY` environment variable. When it is unset or empty the
/// guard is a no-op, so existing deployments keep working unchanged; set it in production
/// to close the API. Accepted headers: `X-API-Key: <key>` or `Authorization: Bearer <key>`.
pub struct ApiKey;

#[rocket::async_trait]
impl<'r> FromRequest<'r> for ApiKey {
    type Error = AppError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let expected = match env::var("API_KEY") {
            Ok(key) if !key.is_empty() => key,
            _ => return Outcome::Success(ApiKey),
        };

        let provided = req
            .headers()
            .get_one("X-API-Key")
            .or_else(|| {
                req.headers()
                    .get_one("Authorization")
                    .and_then(|value| value.strip_prefix("Bearer "))
            })
            .unwrap_or_default();

        if constant_time_eq(provided.as_bytes(), expected.as_bytes()) {
            Outcome::Success(ApiKey)
        } else {
            Outcome::Error((
                Status::Unauthorized,
                AppError::Unauthorized(
                    "Missing or invalid API key (X-API-Key or Authorization: Bearer)".to_string(),
                ),
            ))
        }
    }
}

/// Compare two secrets without leaking their length or content through timing
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

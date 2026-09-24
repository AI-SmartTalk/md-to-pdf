use crate::types::ErrorResponse;
use rocket::response::content::RawHtml;
use rocket::serde::json::Json;
use rocket::Either;
use rocket::Request;

/// Every error leaving the service is a JSON body with the same shape as AppError's,
/// including the ones Rocket itself produces (bad JSON, unknown route, guard failure).
fn error(error: &str, details: &str) -> Json<ErrorResponse> {
    Json(ErrorResponse {
        error: error.to_string(),
        details: details.to_string(),
    })
}

#[catch(400)]
pub fn bad_request(_: &Request) -> Json<ErrorResponse> {
    error("Bad request", "The request could not be understood")
}

#[catch(401)]
pub fn unauthorized(_: &Request) -> Json<ErrorResponse> {
    error(
        "Unauthorized",
        "Missing or invalid API key (X-API-Key or Authorization: Bearer)",
    )
}

/// A visitor and an integration are not owed the same 404.
///
/// A dead link from a search result is a person who wanted a tool and found nothing: they
/// get a page that offers the twenty-one others. An API call gets the JSON body every other
/// error on this service returns. The `Accept` header is what tells the two apart, and a
/// crawler that asks for HTML gets HTML — which is also what keeps a mistyped URL from
/// being indexed as a broken JSON document.
#[catch(404)]
pub fn not_found(req: &Request) -> Either<RawHtml<String>, Json<ErrorResponse>> {
    let path = req.uri().path().as_str();
    let wants_html = req
        .headers()
        .get_one("Accept")
        .is_some_and(|accept| accept.contains("text/html"));

    // The API answers JSON whatever the browser asked for: a 404 on /api/files is a
    // programming answer, not a page.
    let is_api =
        path.starts_with("/api") || path.starts_with("/download") || path.starts_with("/mcp");

    if wants_html && !is_api {
        if let Ok(page) = crate::site::not_found_page(path) {
            return Either::Left(RawHtml(page));
        }
    }

    Either::Right(error("Not found", &format!("No resource at {}", req.uri())))
}

#[catch(413)]
pub fn payload_too_large(_: &Request) -> Json<ErrorResponse> {
    error(
        "Payload too large",
        "The request body exceeds the configured limit",
    )
}

#[catch(415)]
pub fn unsupported_media_type(_: &Request) -> Json<ErrorResponse> {
    error(
        "Unsupported media type",
        "This endpoint expects Content-Type: application/json",
    )
}

#[catch(422)]
pub fn unprocessable_entity(_: &Request) -> Json<ErrorResponse> {
    error(
        "Unprocessable entity",
        "The JSON body is malformed or is missing required fields",
    )
}

#[catch(429)]
pub fn too_many_requests(_: &Request) -> Json<ErrorResponse> {
    error(
        "Too many requests",
        "The render queue is full; retry in a few seconds",
    )
}

#[catch(500)]
pub fn internal_error(_: &Request) -> Json<ErrorResponse> {
    error("Internal error", "The server failed to handle the request")
}

#[catch(502)]
pub fn bad_gateway(_: &Request) -> Json<ErrorResponse> {
    error(
        "Upstream failure",
        "A service this request depends on did not answer",
    )
}

#[catch(504)]
pub fn gateway_timeout(_: &Request) -> Json<ErrorResponse> {
    error("Timeout", "PDF generation exceeded the time limit")
}

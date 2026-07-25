use crate::types::ErrorResponse;
use rocket::serde::json::Json;
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

#[catch(404)]
pub fn not_found(req: &Request) -> Json<ErrorResponse> {
    error("Not found", &format!("No resource at {}", req.uri()))
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

#[catch(500)]
pub fn internal_error(_: &Request) -> Json<ErrorResponse> {
    error("Internal error", "The server failed to handle the request")
}

#[catch(504)]
pub fn gateway_timeout(_: &Request) -> Json<ErrorResponse> {
    error("Timeout", "PDF generation exceeded the time limit")
}

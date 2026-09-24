//! Asynchronous work: submitting it, and polling it.
//!
//! Rather than growing an `"async": true` flag on fifteen endpoints — fifteen places to get
//! the return type wrong — the dispatcher takes the endpoint's name and its ordinary body.
//! It can do that because every tool exposes a `run()` that is pure blocking work and knows
//! nothing about Rocket. Adding a tool to the asynchronous surface is one match arm.
//!
//! The poll URL is the source of truth. A callback, when one was asked for, is a courtesy
//! sent once and never retried — a webhook that must arrive exactly once is a message
//! queue, and this service is not one.

use crate::auth::PublicOrKey;
use crate::jobs::{self, JobView};
use crate::types::{AppError, ToolOutput, ToolResponse};
use rocket::http::Status;
use rocket::serde::json::Json;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
pub struct SubmitRequest {
    /// The endpoint to run, exactly as it appears in the API: `/api/ocr`, `ocr`, both work
    pub endpoint: String,
    /// The body that endpoint would have received
    pub body: Value,
    /// Optional http(s) URL notified once, signed with `X-Signature`
    pub callback_url: Option<String>,
}

#[get("/jobs/<id>")]
pub fn status(_key: PublicOrKey, id: &str) -> Result<Json<JobView>, AppError> {
    Ok(Json(jobs::get(id)?))
}

/// Turn a body into the request type an endpoint expects, then hand it to the job runner.
///
/// The output destination is forced to `asset` unless the caller named a download
/// destination: an asynchronous job has no socket to stream a file down, so a result that
/// went nowhere would be a job that reports `done` and hands back nothing.
macro_rules! dispatch {
    ($body:expr, $kind:expr, $callback:expr, $owner:expr, $ty:ty, $run:path) => {{
        let mut request: $ty = decode($body, $kind)?;
        if request.client_id.is_none() || request.pdf_name.is_none() {
            request.output = Some(ToolOutput::Asset);
        }
        jobs::submit($owner, $kind, $callback, move || {
            // The temp path is dropped here on purpose: `run` has already saved the result
            // to its destination, and holding the file open past that would only keep a
            // copy nobody can reach.
            $run(request).map(|(_, response): (tempfile::TempPath, ToolResponse)| response)
        })
    }};
}

#[post("/jobs", format = "json", data = "<req>")]
pub fn submit(
    key: PublicOrKey,
    req: Json<SubmitRequest>,
) -> Result<(Status, Json<JobView>), AppError> {
    let SubmitRequest {
        endpoint,
        body,
        callback_url,
    } = req.into_inner();

    let kind = normalise(&endpoint);
    // The key's name, so the per-key ceiling counts the integration that is looping rather
    // than punishing everyone at once. The public tier is a name like any other here.
    let owner = key.0;

    let job = match kind {
        "compress" => dispatch!(
            body,
            "compress",
            callback_url,
            owner,
            super::compress::CompressRequest,
            super::compress::run
        ),
        "ocr" => dispatch!(
            body,
            "ocr",
            callback_url,
            owner,
            super::ocr::OcrRequest,
            super::ocr::run
        ),
        "pdfa" => dispatch!(
            body,
            "pdfa",
            callback_url,
            owner,
            super::pdfa::PdfaRequest,
            super::pdfa::run
        ),
        "office-to-pdf" => dispatch!(
            body,
            "office-to-pdf",
            callback_url,
            owner,
            super::office::OfficeToPdfRequest,
            super::office::run
        ),
        "pdf-to-office" => dispatch!(
            body,
            "pdf-to-office",
            callback_url,
            owner,
            super::office::PdfToOfficeRequest,
            super::office::run_pdf_to_office
        ),
        "pages" => dispatch!(
            body,
            "pages",
            callback_url,
            owner,
            super::pages::PagesRequest,
            super::pages::run
        ),
        "pages/number" => dispatch!(
            body,
            "pages/number",
            callback_url,
            owner,
            super::numbering::NumberPagesRequest,
            super::numbering::run
        ),
        "crop" => dispatch!(
            body,
            "crop",
            callback_url,
            owner,
            super::crop::CropRequest,
            super::crop::run
        ),
        "unlock" => dispatch!(
            body,
            "unlock",
            callback_url,
            owner,
            super::repair::UnlockRequest,
            super::repair::run_unlock
        ),
        "rasterize" => dispatch!(
            body,
            "rasterize",
            callback_url,
            owner,
            super::rasterize::RasterizeRequest,
            super::rasterize::run
        ),
        "repair" => dispatch!(
            body,
            "repair",
            callback_url,
            owner,
            super::repair::RepairRequest,
            super::repair::run
        ),
        "images-to-pdf" => dispatch!(
            body,
            "images-to-pdf",
            callback_url,
            owner,
            super::images_to_pdf::ImagesToPdfRequest,
            super::images_to_pdf::run
        ),
        // `/api/extract` stays synchronous, and not by oversight: its result is a body of
        // text, not a file. A job hands back a `ToolResponse`, which carries an asset or a
        // download URL and has nowhere to put four hundred thousand characters — so queueing
        // it would either drop the extraction or invent a file the caller never asked for.
        // It is also the cheapest route of the set: one `pdftotext` over a page range.
        other => {
            return Err(AppError::BadRequest(format!(
                "\"{}\" cannot be run asynchronously. Supported: {}",
                other,
                SUPPORTED.join(", ")
            )))
        }
    }?;

    Ok((Status::Accepted, Json(job)))
}

/// Every endpoint that produces a file. The ones that spawn Ghostscript, LibreOffice or
/// Tesseract are the reason this surface exists; the cheaper ones are here because a caller
/// chaining tools should not have to remember which half of them takes a job handle, and
/// because "cheap" is a statement about the average document, not about the one someone is
/// holding. The routes that hand back JSON rather than a file stay synchronous — see the
/// note on `/api/extract` in the dispatcher.
const SUPPORTED: [&str; 12] = [
    "compress",
    "ocr",
    "pdfa",
    "office-to-pdf",
    "pdf-to-office",
    "pages",
    "pages/number",
    "crop",
    "unlock",
    "rasterize",
    "repair",
    "images-to-pdf",
];

/// `/api/ocr`, `api/ocr` and `ocr` all name the same thing; a caller copying a path out of
/// the documentation should not get a 400 for the leading slash.
fn normalise(endpoint: &str) -> &str {
    endpoint
        .trim()
        .trim_start_matches('/')
        .strip_prefix("api/")
        .unwrap_or_else(|| endpoint.trim().trim_start_matches('/'))
        .trim_end_matches('/')
}

fn decode<T: serde::de::DeserializeOwned>(body: Value, kind: &str) -> Result<T, AppError> {
    serde_json::from_value(body).map_err(|e| {
        AppError::BadRequest(format!("\"body\" is not a valid {} request: {}", kind, e))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_an_endpoint_written_any_of_the_usual_ways() {
        for form in ["/api/ocr", "api/ocr", "ocr", " /api/ocr ", "/api/ocr/"] {
            assert_eq!(normalise(form), "ocr", "{:?}", form);
        }
    }

    #[test]
    fn keeps_a_name_that_is_not_an_api_path() {
        assert_eq!(normalise("office-to-pdf"), "office-to-pdf");
        assert_eq!(normalise("/office-to-pdf"), "office-to-pdf");
    }

    /// `/api/pages/number` is the one supported endpoint whose name has a slash in it; the
    /// prefix stripping must leave it whole, or the dispatcher never sees it.
    #[test]
    fn every_supported_name_survives_being_written_as_an_api_path() {
        for name in SUPPORTED {
            assert_eq!(normalise(&format!("/api/{}", name)), name);
            assert_eq!(normalise(name), name);
        }
    }

    /// The error must list what *is* possible: a caller who guessed wrong needs the menu,
    /// not a refusal.
    #[test]
    fn the_supported_list_is_not_empty_and_holds_no_api_prefixes() {
        assert!(!SUPPORTED.is_empty());
        assert!(SUPPORTED
            .iter()
            .all(|name| !name.starts_with('/') && !name.contains("api/")));
    }

    /// M9: `crop`, `pages/number` and `unlock` expose a `run()` and a route, so refusing
    /// them as "cannot be run asynchronously" was an accident of the table, not a decision.
    ///
    /// An empty body is deliberate: decoding fails before anything is spawned, so the error
    /// tells us which arm was reached without needing a tokio runtime.
    #[test]
    fn the_routes_that_produce_a_file_are_all_reachable_asynchronously() {
        for name in SUPPORTED {
            let outcome = submit(
                PublicOrKey("test"),
                Json(SubmitRequest {
                    endpoint: format!("/api/{}", name),
                    body: serde_json::json!({}),
                    callback_url: None,
                }),
            );

            match outcome {
                Err(AppError::BadRequest(message)) => assert!(
                    message.contains("is not a valid"),
                    "{} was not dispatched: {}",
                    name,
                    message
                ),
                Err(other) => panic!("{} failed unexpectedly: {:?}", name, other),
                Ok(_) => panic!("{} accepted an empty body", name),
            }
        }
    }

    /// It stays out on purpose, and the dispatcher says why: its result is text, and a
    /// `ToolResponse` has nowhere to carry text.
    #[test]
    fn extract_is_refused_with_the_menu_of_what_is_possible() {
        let outcome = submit(
            PublicOrKey("test"),
            Json(SubmitRequest {
                endpoint: "/api/extract".to_string(),
                body: serde_json::json!({}),
                callback_url: None,
            }),
        );

        match outcome {
            Err(AppError::BadRequest(message)) => {
                assert!(message.contains("cannot be run asynchronously"));
                assert!(message.contains("compress"));
            }
            other => panic!(
                "expected a refusal naming the alternatives, got {:?}",
                other
            ),
        }
    }
}

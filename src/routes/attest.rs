//! Issue and check the proof that a document came from here.
//!
//! Two routes, one promise: *prove that this PDF is the file this service issued, and that
//! nobody edited it since.* No competitor answers that question, and the cost is a hash we
//! were already computing.

use crate::attest::{Attestation, Verification};
use crate::auth::PublicOrKey;
use crate::exec;
use crate::helpers;
use crate::types::AppError;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct AttestRequest {
    /// `asset://as_…` or `/download/<client_id>/<name>.pdf`
    pub pdf: String,
    pub engine: Option<String>,
    pub theme: Option<String>,
    pub pdf_variant: Option<String>,
    /// SHA-256 of the source document, when the caller kept it
    pub source_sha256: Option<String>,
    pub operations: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct AttestResponse {
    /// The single header-safe line: `v1.<payload>.<signature>`
    pub attestation: String,
    /// The same record, readable, so a caller does not have to decode it to show it
    pub claims: Attestation,
}

#[post("/attest", format = "json", data = "<req>")]
pub async fn attest(
    _key: PublicOrKey,
    req: Json<AttestRequest>,
) -> Result<Json<AttestResponse>, AppError> {
    let req = req.into_inner();

    if let Some(ref hash) = req.source_sha256 {
        // It is copied verbatim into a signed record; a malformed value would be sealed
        // and then puzzle whoever reads it back.
        if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(AppError::BadRequest(
                "\"source_sha256\" must be 64 hexadecimal characters".to_string(),
            ));
        }
    }

    let response = exec::offload(move || build(req)).await?;
    Ok(Json(response))
}

fn build(req: AttestRequest) -> Result<AttestResponse, AppError> {
    let path = helpers::resolve_readable_pdf(&req.pdf)?;
    let pages = crate::pdfops::page_count(&path)?;

    let mut claims = Attestation::of(&path, pages)?;
    claims.engine = req.engine;
    claims.theme = req.theme;
    claims.pdf_variant = req.pdf_variant;
    claims.source_sha256 = req.source_sha256;
    if let Some(operations) = req.operations {
        // Bounded and cleaned: these end up in a signed record and in the logs
        claims.operations = operations
            .into_iter()
            .take(16)
            .map(|op| {
                op.chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                    .take(32)
                    .collect()
            })
            .filter(|op: &String| !op.is_empty())
            .collect();
    }

    Ok(AttestResponse {
        attestation: claims.seal()?,
        claims,
    })
}

#[derive(Deserialize)]
pub struct VerifyRequest {
    pub pdf: String,
    pub attestation: String,
}

#[post("/verify", format = "json", data = "<req>")]
pub async fn verify(
    _key: PublicOrKey,
    req: Json<VerifyRequest>,
) -> Result<Json<Verification>, AppError> {
    let req = req.into_inner();

    let verification = exec::offload(move || {
        let path = helpers::resolve_readable_pdf(&req.pdf)?;
        crate::attest::verify(&req.attestation, &path)
    })
    .await?;

    Ok(Json(verification))
}

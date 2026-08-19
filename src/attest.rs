//! Proof that a document is the one this service issued.
//!
//! Every PDF tool on the market hands back a file and stops there. Ask any of them
//! *"prove this contract came from that source, with that brand, unaltered"* and there is
//! no answer — the file is just bytes, and bytes are editable.
//!
//! An attestation is a small signed record of what was rendered, from what, by which
//! engine, with which theme, and what the result weighed. It costs almost nothing to
//! produce: the source hash is already computed, because it is the render cache key.
//!
//! What it proves is narrow and worth stating plainly: that a given file is byte-for-byte
//! the one this deployment issued, and that the metadata travelling with it has not been
//! edited. It says nothing about who asked for it, and it is not a legal signature — that
//! is `POST /api/sign` and a certificate authority.

use crate::config::config;
use crate::sign;
use crate::types::AppError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Bumped when the payload gains or loses a field, so an old attestation is rejected as
/// unreadable rather than silently verified against a different meaning.
const VERSION: u8 = 1;

/// The signed record. Field order is the canonical order: `serde_json` preserves
/// declaration order, which is what makes the signature reproducible without a
/// canonicalisation library.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Attestation {
    pub version: u8,
    pub service: String,
    pub issued_at: String,
    /// SHA-256 of the produced file
    pub output_sha256: String,
    pub bytes: u64,
    pub pages: usize,
    /// SHA-256 of the source document, when the caller produced it through this service
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pdf_variant: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout_score: Option<u8>,
    /// Operations applied, in order: `convert`, `redact`, `compress`, ...
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub operations: Vec<String>,
}

impl Attestation {
    /// Describe a produced file. Reads it once to hash it, so it belongs on a blocking
    /// thread like everything else that touches a PDF.
    pub fn of(path: &Path, pages: usize) -> Result<Attestation, AppError> {
        let bytes = fs::read(path)?;

        Ok(Attestation {
            version: VERSION,
            service: config().service_name.clone(),
            issued_at: crate::assets::rfc3339(crate::assets::now_unix()),
            output_sha256: sign::sha256_hex(&bytes),
            bytes: bytes.len() as u64,
            pages,
            source_sha256: None,
            engine: None,
            theme: None,
            pdf_variant: None,
            layout_score: None,
            operations: Vec::new(),
        })
    }

    pub fn with_operation(mut self, operation: &str) -> Attestation {
        self.operations.push(operation.to_string());
        self
    }

    /// The exact bytes that get signed
    pub fn payload(&self) -> Result<String, AppError> {
        serde_json::to_string(self).map_err(|e| AppError::ProcessFailed {
            message: "Could not serialise the attestation".to_string(),
            stderr: e.to_string(),
        })
    }

    /// `v1.<base64 payload>.<hex signature>` — one header-safe line.
    ///
    /// Self-contained on purpose: verifying it needs the file and this string, never a
    /// lookup in a database we would then have to keep, back up and eventually leak.
    pub fn seal(&self) -> Result<String, AppError> {
        let payload = self.payload()?;
        Ok(format!(
            "v{}.{}.{}",
            VERSION,
            base64url(payload.as_bytes()),
            sign::sign(payload.as_bytes())
        ))
    }
}

/// What a verification concluded
#[derive(Debug, Clone, Serialize)]
pub struct Verification {
    /// `valid`, `altered`, `forged` or `unreadable`
    pub verdict: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attestation: Option<Attestation>,
}

/// Check a sealed attestation against the file it claims to describe.
///
/// The two failures are told apart deliberately. A bad signature means the record itself
/// was tampered with or came from another deployment; a good signature over a file whose
/// hash no longer matches means the record is genuine and the *document* was edited. Those
/// are different incidents and a single "invalid" would hide which one happened.
pub fn verify(sealed: &str, file: &Path) -> Result<Verification, AppError> {
    let mut parts = sealed.trim().splitn(3, '.');
    let (Some(version), Some(payload_b64), Some(signature)) =
        (parts.next(), parts.next(), parts.next())
    else {
        return Ok(unreadable(
            "the attestation is not in the v1.<payload>.<signature> form",
        ));
    };

    if version != format!("v{}", VERSION) {
        return Ok(unreadable(&format!(
            "this deployment reads attestation {} only, and this one is {}",
            format_args!("v{}", VERSION),
            version
        )));
    }

    let Some(payload) = decode_base64url(payload_b64) else {
        return Ok(unreadable("the payload is not valid base64url"));
    };

    if !sign::verify(&payload, signature) {
        return Ok(Verification {
            verdict: "forged".to_string(),
            detail: "The signature does not match this deployment's key: the attestation was \
                     altered, or it was issued by another service."
                .to_string(),
            attestation: None,
        });
    }

    let attestation: Attestation = match serde_json::from_slice(&payload) {
        Ok(value) => value,
        Err(e) => {
            return Ok(unreadable(&format!(
                "the payload is not an attestation: {}",
                e
            )))
        }
    };

    let actual = sign::sha256_hex(&fs::read(file)?);
    if actual != attestation.output_sha256 {
        return Ok(Verification {
            verdict: "altered".to_string(),
            detail: format!(
                "The attestation is genuine but describes a different file: it expects \
                 {}, this document hashes to {}.",
                short(&attestation.output_sha256),
                short(&actual)
            ),
            attestation: Some(attestation),
        });
    }

    Ok(Verification {
        verdict: "valid".to_string(),
        detail: format!(
            "This document is byte-for-byte the one {} issued on {}.",
            attestation.service, attestation.issued_at
        ),
        attestation: Some(attestation),
    })
}

fn unreadable(detail: &str) -> Verification {
    Verification {
        verdict: "unreadable".to_string(),
        detail: detail.to_string(),
        attestation: None,
    }
}

fn short(hash: &str) -> String {
    hash.chars().take(12).collect()
}

// ------------ base64url ------------

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Unpadded base64url: the sealed attestation travels in an HTTP header, where `+`, `/`
/// and `=` all need escaping by someone, somewhere, and eventually one of them forgets.
pub fn base64url(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);

    for chunk in data.chunks(3) {
        let bytes = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let packed = (bytes[0] as u32) << 16 | (bytes[1] as u32) << 8 | (bytes[2] as u32);

        out.push(ALPHABET[(packed >> 18 & 63) as usize] as char);
        out.push(ALPHABET[(packed >> 12 & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(packed >> 6 & 63) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(packed & 63) as usize] as char);
        }
    }

    out
}

pub fn decode_base64url(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0u32;

    for c in text.chars() {
        let value = ALPHABET.iter().position(|&a| a as char == c)? as u32;
        buffer = (buffer << 6) | value;
        bits += 6;

        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }

    // Leftover bits must be zero padding; anything else is a corrupted string rather than
    // a short one, and decoding it would invent bytes nobody sent.
    if bits > 0 && (buffer & ((1 << bits) - 1)) != 0 {
        return None;
    }

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn sample() -> Attestation {
        Attestation {
            version: VERSION,
            service: "md-to-pdf".to_string(),
            issued_at: "2026-08-19T10:00:00Z".to_string(),
            output_sha256: "a".repeat(64),
            bytes: 1234,
            pages: 4,
            source_sha256: Some("b".repeat(64)),
            engine: Some("weasyprint".to_string()),
            theme: Some("aismarttalk@1".to_string()),
            pdf_variant: None,
            layout_score: Some(96),
            operations: vec!["convert".to_string()],
        }
    }

    #[test]
    fn round_trips_base64url() {
        for case in [
            b"".as_slice(),
            b"f",
            b"fo",
            b"foo",
            b"foobar",
            &[0xff, 0xfe, 0xfd, 0x00, 0x01],
        ] {
            let encoded = base64url(case);
            assert!(!encoded.contains('+') && !encoded.contains('/') && !encoded.contains('='));
            assert_eq!(decode_base64url(&encoded).as_deref(), Some(case));
        }
    }

    #[test]
    fn refuses_a_corrupted_encoding() {
        assert!(decode_base64url("not base64!").is_none());
        assert!(decode_base64url("aa=").is_none());
    }

    #[test]
    fn a_seal_is_three_dotted_parts() {
        let sealed = sample().seal().unwrap();
        let parts: Vec<&str> = sealed.split('.').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "v1");
        assert_eq!(parts[2].len(), 64);
    }

    /// The whole point: a document that was edited after the fact must not verify
    #[test]
    fn tells_a_genuine_document_from_an_edited_one() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"%PDF-1.7 original").unwrap();

        let attestation = Attestation::of(file.path(), 1).unwrap();
        let sealed = attestation.seal().unwrap();

        assert_eq!(verify(&sealed, file.path()).unwrap().verdict, "valid");

        let mut edited = tempfile::NamedTempFile::new().unwrap();
        edited.write_all(b"%PDF-1.7 tampered").unwrap();
        let result = verify(&sealed, edited.path()).unwrap();
        assert_eq!(result.verdict, "altered");
        // A genuine record over the wrong file still hands back what it described
        assert!(result.attestation.is_some());
    }

    /// A record whose payload was rewritten must fail as forged, not as altered
    #[test]
    fn detects_a_rewritten_payload() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"%PDF-1.7").unwrap();

        let sealed = Attestation::of(file.path(), 1).unwrap().seal().unwrap();
        let parts: Vec<&str> = sealed.split('.').collect();

        let mut forged = sample();
        forged.pages = 999;
        let payload = forged.payload().unwrap();
        let tampered = format!("v1.{}.{}", base64url(payload.as_bytes()), parts[2]);

        assert_eq!(verify(&tampered, file.path()).unwrap().verdict, "forged");
    }

    #[test]
    fn refuses_what_it_cannot_read() {
        let file = tempfile::NamedTempFile::new().unwrap();

        for bad in ["", "garbage", "v9.aaa.bbb", "v1.!!!.bbb"] {
            assert_eq!(
                verify(bad, file.path()).unwrap().verdict,
                "unreadable",
                "{} should be unreadable",
                bad
            );
        }
    }

    #[test]
    fn the_payload_keeps_a_stable_field_order() {
        let payload = sample().payload().unwrap();
        let version_at = payload.find("\"version\"").unwrap();
        let service_at = payload.find("\"service\"").unwrap();
        let output_at = payload.find("\"output_sha256\"").unwrap();
        assert!(version_at < service_at && service_at < output_at);
    }

    /// Absent fields must not appear at all: a signature over `"theme":null` and one over
    /// nothing are different bytes, and the reader would have to guess which it holds.
    #[test]
    fn omits_the_fields_that_do_not_apply() {
        let mut attestation = sample();
        attestation.theme = None;
        attestation.pdf_variant = None;
        attestation.operations.clear();

        let payload = attestation.payload().unwrap();
        assert!(!payload.contains("theme"));
        assert!(!payload.contains("pdf_variant"));
        assert!(!payload.contains("operations"));
    }
}

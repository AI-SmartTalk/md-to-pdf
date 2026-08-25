//! HMAC-SHA256, and the secret it is keyed with.
//!
//! Two features need to prove that a piece of JSON came from this service and was not
//! edited on the way: the render attestation, and the signed callback an asynchronous job
//! posts when it finishes. Both are the same primitive, so it lives here once.
//!
//! `sha2` is already a dependency (the render cache is content-addressed with it); HMAC is
//! thirty lines on top of it, which is cheaper than auditing another crate in an image we
//! read by hand.

use sha2::{Digest, Sha256};
use std::env;
use std::sync::OnceLock;

/// SHA-256 block size, in bytes
const BLOCK: usize = 64;

/// RFC 2104 HMAC over SHA-256
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut block = [0u8; BLOCK];

    // A key longer than the block is hashed first; a shorter one is zero-padded
    if key.len() > BLOCK {
        let digest = Sha256::digest(key);
        block[..32].copy_from_slice(&digest);
    } else {
        block[..key.len()].copy_from_slice(key);
    }

    let mut inner_pad = [0u8; BLOCK];
    let mut outer_pad = [0u8; BLOCK];
    for i in 0..BLOCK {
        inner_pad[i] = block[i] ^ 0x36;
        outer_pad[i] = block[i] ^ 0x5c;
    }

    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update(message);
    let inner_digest = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner_digest);

    let mut out = [0u8; 32];
    out.copy_from_slice(&outer.finalize());
    out
}

/// Lowercase hexadecimal, the form every signature travels in
pub fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{:02x}", byte));
    }
    out
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

/// Compare two signatures without leaking where they first differ
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// The key every signature of this process is made with.
///
/// `ATTESTATION_SECRET` when set. Otherwise the deployment gets a secret drawn at startup:
/// signatures stay valid for the life of the process and stop verifying after a restart.
/// That is the honest failure — an attestation nobody can check is better than one anybody
/// can forge, and the warning says which situation an operator is in.
pub fn secret() -> &'static [u8] {
    static SECRET: OnceLock<Vec<u8>> = OnceLock::new();

    SECRET.get_or_init(|| match env::var("ATTESTATION_SECRET") {
        Ok(value) if value.len() >= 16 => value.into_bytes(),
        Ok(value) if !value.is_empty() => {
            warn!("ATTESTATION_SECRET is shorter than 16 characters: ignoring it");
            ephemeral()
        }
        _ => {
            warn!(
                "ATTESTATION_SECRET is not set: attestations are signed with a key drawn \
                     at startup and stop verifying after a restart"
            );
            ephemeral()
        }
    })
}

fn ephemeral() -> Vec<u8> {
    crate::helpers::random_id("eph")
        .unwrap_or_else(|_| format!("eph_{}", crate::assets::now_unix()))
        .into_bytes()
}

/// Sign a payload with this deployment's secret, hex-encoded
pub fn sign(payload: &[u8]) -> String {
    hex(&hmac_sha256(secret(), payload))
}

/// Does this signature belong to this payload?
pub fn verify(payload: &[u8], signature: &str) -> bool {
    constant_time_eq(sign(payload).as_bytes(), signature.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 4231 test case 1, so a rewrite of the padding cannot pass unnoticed
    #[test]
    fn matches_the_rfc_4231_vectors() {
        let mac = hmac_sha256(&[0x0b; 20], b"Hi There");
        assert_eq!(
            hex(&mac),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );

        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(
            hex(&mac),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    /// A key longer than the block is hashed before use; getting this wrong silently
    /// produces signatures that nobody else can reproduce.
    #[test]
    fn hashes_keys_longer_than_the_block() {
        let mac = hmac_sha256(
            &[0xaa; 131],
            b"Test Using Larger Than Block-Size Key - Hash Key First",
        );
        assert_eq!(
            hex(&mac),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    #[test]
    fn a_signature_belongs_to_exactly_one_payload() {
        let signature = sign(b"{\"pages\":4}");
        assert!(verify(b"{\"pages\":4}", &signature));
        assert!(!verify(b"{\"pages\":5}", &signature));
        assert!(!verify(b"{\"pages\":4}", "deadbeef"));
    }

    #[test]
    fn compares_without_leaking_the_prefix() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }
}

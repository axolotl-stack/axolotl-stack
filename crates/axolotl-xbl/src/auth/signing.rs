//! ECDSA request signing for Xbox Live APIs.
//!
//! Xbox Live requires signed requests using ECDSA P-256 keys.
//! The signature includes the timestamp, HTTP method, path, authorization header, and body.

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use p256::ecdsa::{Signature, SigningKey, signature::DigestSigner};
use p256::elliptic_curve::rand_core::OsRng;
use sha2::{Digest, Sha256};
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// Global server time offset, updated from response Date headers.
/// This is critical because Xbox Live will reject signatures if the timestamp is off.
static SERVER_TIME_OFFSET: RwLock<Option<i64>> = RwLock::new(None);

/// Update the server time offset from an HTTP Date header.
/// Call this after each response to keep time in sync.
pub fn update_server_time_from_header(date_header: &str) {
    // Parse RFC 1123 date format: "Mon, 02 Jan 2006 15:04:05 GMT"
    if let Ok(parsed) = chrono::DateTime::parse_from_rfc2822(date_header) {
        let server_unix = parsed.timestamp();
        let local_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let offset = server_unix - local_unix;
        if let Ok(mut guard) = SERVER_TIME_OFFSET.write() {
            *guard = Some(offset);
        }
    }
}

/// ECDSA P-256 key pair for signing Xbox Live requests.
#[derive(Debug)]
pub struct SigningKeyPair {
    signing_key: SigningKey,
}

impl SigningKeyPair {
    /// Generate a new random ECDSA P-256 key pair.
    pub fn generate() -> Self {
        Self {
            signing_key: SigningKey::random(&mut OsRng),
        }
    }

    /// Get the ProofKey JSON object for Xbox Live requests.
    pub fn proof_key(&self) -> serde_json::Value {
        let verifying_key = self.signing_key.verifying_key();
        let point = verifying_key.to_encoded_point(false);

        // Keys sorted alphabetically to match gophertunnel/Broadcaster behavior
        serde_json::json!({
            "alg": "ES256",
            "crv": "P-256",
            "kty": "EC",
            "use": "sig",
            "x": base64_url_encode(point.x().expect("x coordinate")),
            "y": base64_url_encode(point.y().expect("y coordinate")),
        })
    }

    /// Sign an HTTP request for Xbox Live.
    ///
    /// Returns the base64-encoded signature to use in the `Signature` header.
    pub fn sign_request(
        &self,
        method: &str,
        path: &str,
        authorization: &str,
        body: &[u8],
    ) -> String {
        let timestamp = windows_timestamp();

        let mut hasher = Sha256::new();

        // Signature policy version (0, 0, 0, 1) + 0 byte + timestamp + 0 byte
        let mut prefix = Vec::with_capacity(13);
        prefix.extend_from_slice(&[0, 0, 0, 1, 0]);
        prefix.extend_from_slice(&timestamp.to_be_bytes());
        prefix.push(0);
        hasher.update(&prefix);

        // HTTP method + 0 byte
        hasher.update(method.as_bytes());
        hasher.update(&[0]);

        // Path (with query string) + 0 byte
        hasher.update(path.as_bytes());
        hasher.update(&[0]);

        // Authorization header + 0 byte
        hasher.update(authorization.as_bytes());
        hasher.update(&[0]);

        // Body + 0 byte
        hasher.update(body);
        hasher.update(&[0]);

        // Sign the hash
        let signature: Signature = self.signing_key.sign_digest(hasher);

        // Build final signature: [version (4 bytes)][timestamp (8 bytes)][signature (64 bytes)]
        let mut result = Vec::with_capacity(76);
        result.extend_from_slice(&[0, 0, 0, 1]);
        result.extend_from_slice(&timestamp.to_be_bytes());
        result.extend_from_slice(&signature.to_bytes());

        BASE64.encode(&result)
    }
}

impl Default for SigningKeyPair {
    fn default() -> Self {
        Self::generate()
    }
}

/// Get Windows-style timestamp (100-nanosecond intervals since 1601-01-01).
/// Uses server time offset if available, otherwise falls back to local time.
fn windows_timestamp() -> i64 {
    let local_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_secs() as i64;

    // Apply server time offset if we have one
    let offset = SERVER_TIME_OFFSET.read().ok().and_then(|g| *g).unwrap_or(0);
    let adjusted_unix = local_unix + offset;

    // Windows epoch offset: seconds between 1601-01-01 and 1970-01-01
    const WINDOWS_EPOCH_OFFSET: i64 = 11644473600;

    (adjusted_unix + WINDOWS_EPOCH_OFFSET) * 10_000_000
}

/// Base64 URL-safe encoding without padding (for ProofKey).
fn base64_url_encode(bytes: &[u8]) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proof_key_format() {
        let key = SigningKeyPair::generate();
        let proof = key.proof_key();

        assert_eq!(proof["crv"], "P-256");
        assert_eq!(proof["alg"], "ES256");
        assert_eq!(proof["kty"], "EC");
        assert!(proof["x"].is_string());
        assert!(proof["y"].is_string());
    }

    #[test]
    fn test_sign_request() {
        let key = SigningKeyPair::generate();
        let sig = key.sign_request("POST", "/test", "", b"{}");

        // Signature should be base64-encoded and non-empty
        assert!(!sig.is_empty());
        assert!(BASE64.decode(&sig).is_ok());
    }
}

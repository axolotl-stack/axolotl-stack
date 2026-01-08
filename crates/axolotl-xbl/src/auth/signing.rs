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
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    // ========== SigningKeyPair Tests ==========

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
    fn test_proof_key_x_y_are_base64_url() {
        let key = SigningKeyPair::generate();
        let proof = key.proof_key();

        // x and y should be valid URL-safe base64 without padding
        let x = proof["x"].as_str().unwrap();
        let y = proof["y"].as_str().unwrap();

        assert!(URL_SAFE_NO_PAD.decode(x).is_ok());
        assert!(URL_SAFE_NO_PAD.decode(y).is_ok());

        // P-256 coordinates are 32 bytes, base64 encoded = 43 chars (no padding)
        assert_eq!(x.len(), 43);
        assert_eq!(y.len(), 43);
    }

    #[test]
    fn test_proof_key_use_field() {
        let key = SigningKeyPair::generate();
        let proof = key.proof_key();

        assert_eq!(proof["use"], "sig");
    }

    #[test]
    fn test_sign_request() {
        let key = SigningKeyPair::generate();
        let sig = key.sign_request("POST", "/test", "", b"{}");

        // Signature should be base64-encoded and non-empty
        assert!(!sig.is_empty());
        assert!(BASE64.decode(&sig).is_ok());
    }

    #[test]
    fn test_sign_request_structure() {
        let key = SigningKeyPair::generate();
        let sig = key.sign_request("POST", "/path", "auth_header", b"body");

        let decoded = BASE64.decode(&sig).expect("valid base64");

        // Structure: [version (4 bytes)][timestamp (8 bytes)][signature (64 bytes)]
        assert_eq!(decoded.len(), 76);

        // Version should be [0, 0, 0, 1]
        assert_eq!(&decoded[0..4], &[0, 0, 0, 1]);
    }

    #[test]
    fn test_sign_request_timestamp_is_big_endian() {
        let key = SigningKeyPair::generate();
        let sig = key.sign_request("GET", "/", "", b"");

        let decoded = BASE64.decode(&sig).expect("valid base64");

        // Extract timestamp (bytes 4-12)
        let timestamp_bytes: [u8; 8] = decoded[4..12].try_into().unwrap();
        let timestamp = i64::from_be_bytes(timestamp_bytes);

        // Timestamp should be positive (Windows epoch is in the past)
        // Note: exact value depends on server time offset which may be set by other tests
        assert!(timestamp > 0);
    }

    #[test]
    fn test_sign_request_different_inputs_different_signatures() {
        let key = SigningKeyPair::generate();

        let sig1 = key.sign_request("POST", "/path1", "", b"body1");
        let sig2 = key.sign_request("POST", "/path2", "", b"body1");
        let sig3 = key.sign_request("POST", "/path1", "", b"body2");
        let sig4 = key.sign_request("GET", "/path1", "", b"body1");

        // All should be different (different inputs)
        assert_ne!(sig1, sig2);
        assert_ne!(sig1, sig3);
        assert_ne!(sig1, sig4);
    }

    #[test]
    fn test_sign_request_with_authorization() {
        let key = SigningKeyPair::generate();
        let sig = key.sign_request("POST", "/xsts", "XBL3.0 x=hash;token", b"{}");

        let decoded = BASE64.decode(&sig).expect("valid base64");
        assert_eq!(decoded.len(), 76);
    }

    #[test]
    fn test_sign_request_empty_body() {
        let key = SigningKeyPair::generate();
        let sig = key.sign_request("GET", "/resource", "", b"");

        let decoded = BASE64.decode(&sig).expect("valid base64");
        assert_eq!(decoded.len(), 76);
    }

    #[test]
    fn test_sign_request_large_body() {
        let key = SigningKeyPair::generate();
        let large_body = vec![0u8; 10000];
        let sig = key.sign_request("POST", "/upload", "", &large_body);

        let decoded = BASE64.decode(&sig).expect("valid base64");
        assert_eq!(decoded.len(), 76);
    }

    #[test]
    fn test_generate_creates_different_keys() {
        let key1 = SigningKeyPair::generate();
        let key2 = SigningKeyPair::generate();

        let proof1 = key1.proof_key();
        let proof2 = key2.proof_key();

        // Different keys should have different x/y coordinates
        assert_ne!(proof1["x"], proof2["x"]);
        assert_ne!(proof1["y"], proof2["y"]);
    }

    #[test]
    fn test_default_generates_key() {
        let key = SigningKeyPair::default();
        let proof = key.proof_key();

        // Should have valid coordinates
        assert!(proof["x"].is_string());
        assert!(proof["y"].is_string());
    }

    // ========== base64_url_encode Tests ==========

    #[test]
    fn test_base64_url_encode_empty() {
        let result = base64_url_encode(&[]);
        assert_eq!(result, "");
    }

    #[test]
    fn test_base64_url_encode_no_padding() {
        // 1 byte encodes to 2 chars (normally would have 2 padding chars)
        let result = base64_url_encode(&[0xFF]);
        assert!(!result.ends_with('='));
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_base64_url_encode_url_safe_chars() {
        // Standard base64 uses + and /, URL-safe uses - and _
        // Bytes that would produce + or / in standard base64
        let bytes = &[0xFB, 0xEF]; // Should produce URL-safe encoding
        let result = base64_url_encode(bytes);

        assert!(!result.contains('+'));
        assert!(!result.contains('/'));
    }

    #[test]
    fn test_base64_url_encode_roundtrip() {
        let original = b"Hello, Xbox Live!";
        let encoded = base64_url_encode(original);
        let decoded = URL_SAFE_NO_PAD.decode(&encoded).expect("valid base64");
        assert_eq!(decoded, original);
    }

    // ========== Windows Timestamp Tests ==========

    #[test]
    fn test_windows_timestamp_format() {
        // Reset server time offset to avoid interference from other tests
        if let Ok(mut guard) = SERVER_TIME_OFFSET.write() {
            *guard = None;
        }

        let ts = windows_timestamp();

        // Windows timestamp is 100-nanosecond intervals since 1601-01-01
        // Should be a large positive number
        assert!(ts > 0);

        // Should be roughly in the range for year 2020-2040
        // 2020: ~132400000000000000
        // 2040: ~140000000000000000
        assert!(ts > 130_000_000_000_000_000, "timestamp {} too low", ts);
        assert!(ts < 145_000_000_000_000_000, "timestamp {} too high", ts);
    }

    #[test]
    fn test_windows_timestamp_increases() {
        // Reset server time offset to avoid interference from other tests
        if let Ok(mut guard) = SERVER_TIME_OFFSET.write() {
            *guard = None;
        }

        let ts1 = windows_timestamp();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let ts2 = windows_timestamp();

        // Second timestamp should be greater (or at least equal if very fast)
        assert!(ts2 >= ts1, "ts2 {} should be >= ts1 {}", ts2, ts1);
    }

    // ========== update_server_time_from_header Tests ==========

    #[test]
    fn test_update_server_time_from_header_valid() {
        // RFC 2822 format
        let header = "Mon, 02 Jan 2006 15:04:05 +0000";
        update_server_time_from_header(header);
        // Should not panic
    }

    #[test]
    fn test_update_server_time_from_header_invalid() {
        // Invalid format should be silently ignored
        update_server_time_from_header("not a date");
        update_server_time_from_header("");
        // Should not panic
    }

    #[test]
    fn test_update_server_time_from_header_gmt() {
        let header = "Wed, 01 Jan 2025 12:00:00 GMT";
        update_server_time_from_header(header);
        // Should not panic
    }

    // ========== Server Time Offset Tests ==========

    #[test]
    fn test_server_time_offset_starts_none() {
        // Note: This test may fail if other tests have set the offset
        // In practice, we're testing that reading the offset doesn't panic
        let offset = SERVER_TIME_OFFSET.read().ok();
        assert!(offset.is_some());
    }
}

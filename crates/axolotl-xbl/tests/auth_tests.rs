//! Integration tests for Xbox Live authentication.
//!
//! Note: Most tests require network access and a real Microsoft account.
//! Unit tests for signing and structure are included.

use axolotl_xbl::auth::SigningKeyPair;

/// Test that SigningKeyPair generates valid ProofKey format.
#[test]
fn test_proof_key_format() {
    let key = SigningKeyPair::generate();
    let proof = key.proof_key();

    // Must have all required JWK fields
    assert_eq!(proof["crv"], "P-256", "Wrong curve");
    assert_eq!(proof["alg"], "ES256", "Wrong algorithm");
    assert_eq!(proof["use"], "sig", "Wrong use");
    assert_eq!(proof["kty"], "EC", "Wrong key type");

    // x and y must be strings (base64 URL-safe encoded)
    assert!(proof["x"].is_string(), "x must be string");
    assert!(proof["y"].is_string(), "y must be string");

    // x and y should be 43 characters (32 bytes base64 URL-safe no padding)
    let x = proof["x"].as_str().unwrap();
    let y = proof["y"].as_str().unwrap();
    assert_eq!(x.len(), 43, "x should be 43 chars (32 bytes base64)");
    assert_eq!(y.len(), 43, "y should be 43 chars (32 bytes base64)");
}

/// Test that signing produces valid base64 output.
#[test]
fn test_signing_produces_valid_base64() {
    use base64::{Engine, engine::general_purpose::STANDARD};

    let key = SigningKeyPair::generate();
    let signature = key.sign_request("POST", "/device/authenticate", "", b"{}");

    // Must be valid base64
    let decoded = STANDARD.decode(&signature).expect("Invalid base64");

    // Structure: [version 4B][timestamp 8B][signature 64B] = 76 bytes
    assert_eq!(decoded.len(), 76, "Signature should be 76 bytes");

    // Version must be [0, 0, 0, 1]
    assert_eq!(
        &decoded[0..4],
        &[0, 0, 0, 1],
        "Wrong signature policy version"
    );
}

/// Test that multiple keys generate different signatures.
#[test]
fn test_different_keys_different_signatures() {
    let key1 = SigningKeyPair::generate();
    let key2 = SigningKeyPair::generate();

    let sig1 = key1.sign_request("POST", "/test", "", b"body");
    let sig2 = key2.sign_request("POST", "/test", "", b"body");

    // Signatures should differ (different keys)
    assert_ne!(
        sig1, sig2,
        "Different keys should produce different signatures"
    );
}

/// Test that same key with different bodies produces different signatures.
#[test]
fn test_different_bodies_different_signatures() {
    let key = SigningKeyPair::generate();

    let sig1 = key.sign_request("POST", "/test", "", b"body1");
    let sig2 = key.sign_request("POST", "/test", "", b"body2");

    // Signatures should differ (different bodies)
    assert_ne!(
        sig1, sig2,
        "Different bodies should produce different signatures"
    );
}

#[test]
fn credentials_are_redacted_from_debug() {
    use axolotl_xbl::auth::{OAuthToken, XblToken};
    let oauth = OAuthToken {
        access_token: "secret-access".into(),
        refresh_token: "secret-refresh".into(),
        token_type: "Bearer".into(),
        expires_in: 3600,
        user_id: None,
    };
    let xbl = XblToken {
        token: "secret-xsts".into(),
        user_hash: "hash".into(),
        gamertag: "name".into(),
        xuid: "1".into(),
        expires_at: 1,
    };
    for value in [format!("{oauth:?}"), format!("{xbl:?}")] {
        assert!(!value.contains("secret-"), "{value}");
    }
}

#[test]
fn malformed_response_error_does_not_disclose_credentials() {
    let error = serde_json::from_str::<u64>("\"secret-token\"").unwrap_err();
    let error = axolotl_xbl::XblError::from(error);
    assert!(!format!("{error} {error:?}").contains("secret-token"));
    assert!(std::error::Error::source(&error).is_none());
}

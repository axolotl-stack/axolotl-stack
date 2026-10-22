//! The ECDH encryption handshake shared by both roles.
//!
//! The server signs `{"salt": base64(16 random bytes)}` as an ES384 JWT with
//! its public key in the `x5u` header. Both sides derive
//! `key = SHA256(salt || ECDH(P-384) shared x)` and use `key[..12]` as the IV.

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE_NO_PAD};
use p384::{PublicKey, SecretKey, pkcs8::DecodePublicKey};
use sha2::{Digest, Sha256};

use crate::error::{JolyneError, ProtocolError};

/// Derives the AES key and IV from our secret, the peer's key and the salt.
pub fn derive_key(secret: &SecretKey, peer: &PublicKey, salt: &[u8]) -> ([u8; 32], [u8; 12]) {
    let shared = p384::ecdh::diffie_hellman(secret.to_nonzero_scalar(), peer.as_affine());
    let mut digest = Sha256::new();
    digest.update(salt);
    digest.update(shared.raw_secret_bytes());
    let key: [u8; 32] = digest.finalize().into();
    let mut iv = [0u8; 12];
    iv.copy_from_slice(&key[..12]);
    (key, iv)
}

fn invalid(message: impl Into<String>) -> JolyneError {
    ProtocolError::InvalidHandshake(message.into()).into()
}

/// Parses a base64 (standard alphabet) DER SubjectPublicKeyInfo.
pub fn parse_public_key(spki_b64: &str) -> Result<PublicKey, JolyneError> {
    let der = STANDARD
        .decode(spki_b64)
        .map_err(|error| invalid(format!("public key base64: {error}")))?;
    PublicKey::from_public_key_der(&der).map_err(|error| invalid(format!("public key: {error}")))
}

/// Builds the server's `ServerToClientHandshake` token for `salt`.
#[cfg(feature = "server")]
pub fn server_token(server_key: &SecretKey, salt: &[u8; 16]) -> Result<String, JolyneError> {
    use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
    use p384::pkcs8::{EncodePrivateKey, EncodePublicKey};

    #[derive(serde::Serialize)]
    struct SaltClaims {
        salt: String,
    }

    let public = server_key
        .public_key()
        .to_public_key_der()
        .map_err(|error| ProtocolError::HandshakeToken(error.to_string()))?;
    let private = server_key
        .to_pkcs8_der()
        .map_err(|error| ProtocolError::HandshakeToken(error.to_string()))?;
    let mut header = Header::new(Algorithm::ES384);
    header.x5u = Some(STANDARD.encode(public.as_bytes()));
    encode(
        &header,
        &SaltClaims {
            salt: STANDARD_NO_PAD.encode(salt),
        },
        &EncodingKey::from_ec_der(private.as_bytes()),
    )
    .map_err(|error| ProtocolError::HandshakeToken(error.to_string()).into())
}

/// Verifies the server's handshake token and returns `(server key, salt)`.
///
/// The algorithm is pinned to ES384 (gophertunnel `jwt.ParseSigned(…, ES384)`)
/// and the signature is checked against the token's own `x5u` key: this
/// proves possession of that key, it does not establish server trust.
pub fn verify_server_token(token: &str) -> Result<(PublicKey, Vec<u8>), JolyneError> {
    use p384::ecdsa::{Signature, VerifyingKey, signature::Verifier};

    #[derive(serde::Deserialize)]
    struct Header {
        alg: String,
        x5u: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct Claims {
        salt: String,
    }

    let mut parts = token.split('.');
    let (Some(protected), Some(claims), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(invalid("token is not a three-part JWS"));
    };
    let header: Header = URL_SAFE_NO_PAD
        .decode(protected)
        .ok()
        .and_then(|json| serde_json::from_slice(&json).ok())
        .ok_or_else(|| invalid("malformed JWT header"))?;
    if header.alg != "ES384" {
        return Err(invalid(format!("algorithm {} is not ES384", header.alg)));
    }
    let server_key = parse_public_key(
        header
            .x5u
            .as_deref()
            .ok_or_else(|| invalid("missing x5u"))?,
    )?;
    let signature = URL_SAFE_NO_PAD
        .decode(signature)
        .ok()
        .and_then(|bytes| Signature::try_from(bytes.as_slice()).ok())
        .ok_or_else(|| invalid("malformed signature"))?;
    let signed = &token[..protected.len() + 1 + claims.len()];
    VerifyingKey::from(&server_key)
        .verify(signed.as_bytes(), &signature)
        .map_err(|_| invalid("signature does not verify against x5u"))?;
    let claims: Claims = URL_SAFE_NO_PAD
        .decode(claims)
        .ok()
        .and_then(|json| serde_json::from_slice(&json).ok())
        .ok_or_else(|| invalid("malformed JWT claims"))?;
    let salt = STANDARD_NO_PAD
        .decode(claims.salt.trim_end_matches('='))
        .map_err(|error| invalid(format!("salt base64: {error}")))?;
    Ok((server_key, salt))
}

#[cfg(all(test, feature = "server"))]
mod tests {
    use super::*;

    #[test]
    fn token_roundtrip_derives_matching_keys() {
        let server = SecretKey::random(&mut rand::thread_rng());
        let client = SecretKey::random(&mut rand::thread_rng());
        let salt = [5u8; 16];
        let token = server_token(&server, &salt).unwrap();
        let (server_public, parsed_salt) = verify_server_token(&token).unwrap();
        assert_eq!(parsed_salt, salt);
        assert_eq!(
            derive_key(&client, &server_public, &parsed_salt),
            derive_key(&server, &client.public_key(), &salt)
        );
    }

    #[test]
    fn rejects_non_es384_and_tampered_tokens() {
        let server = SecretKey::random(&mut rand::thread_rng());
        let token = server_token(&server, &[1; 16]).unwrap();
        let mut parts: Vec<String> = token.split('.').map(str::to_owned).collect();

        let header: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(&parts[0]).unwrap()).unwrap();
        let mut downgraded = header.clone();
        downgraded["alg"] = "none".into();
        let forged = format!(
            "{}.{}.{}",
            URL_SAFE_NO_PAD.encode(downgraded.to_string()),
            parts[1],
            parts[2]
        );
        assert!(
            verify_server_token(&forged)
                .unwrap_err()
                .to_string()
                .contains("not ES384")
        );

        parts[1] = URL_SAFE_NO_PAD.encode(r#"{"salt":"AAAAAAAAAAAAAAAAAAAAAA"}"#);
        assert!(verify_server_token(&parts.join(".")).is_err());
    }
}

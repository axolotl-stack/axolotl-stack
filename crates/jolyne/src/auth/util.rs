use crate::error::{AuthError, JolyneError};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use jsonwebtoken::DecodingKey;
use p384::{PublicKey, elliptic_curve::sec1::ToEncodedPoint, pkcs8::DecodePublicKey};
use serde::de::DeserializeOwned;

pub fn decode_unverified_claims<T: DeserializeOwned>(token: &str) -> Option<T> {
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload_b64 = parts.next()?;
    let payload = URL_SAFE_NO_PAD.decode(payload_b64).ok()?;
    serde_json::from_slice(&payload).ok()
}

pub fn key_from_base64(b64: &str) -> Result<DecodingKey, JolyneError> {
    let der = STANDARD
        .decode(b64)
        .map_err(|e| AuthError::BadSignature(format!("Invalid base64 key: {e}")))?;
    let public = PublicKey::from_public_key_der(&der)
        .map_err(|e| AuthError::BadSignature(format!("Invalid P-384 identity key: {e}")))?;
    Ok(DecodingKey::from_ec_der(
        public.to_encoded_point(false).as_bytes(),
    ))
}

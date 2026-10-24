//! Legacy (pre-OpenID) login chains, accepted only for offline logins.
//!
//! Online mode does not accept Mojang-rooted chains: Bedrock Dedicated Server
//! 1.26.51 no longer carries the Mojang root key, so vanilla servers only
//! authenticate the OpenID multiplayer `Token`. See the auth section of the
//! crate README.

use crate::auth::types::{ChainClaims, Trust, ValidatedIdentity};
use crate::error::AuthError;
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use jsonwebtoken::decode_header;
use serde_json::Value;

const MAX_CHAIN_TOKENS: usize = 8;
const MAX_TOKEN_BYTES: usize = 8 * 1024;

pub(crate) fn chain_from_value(v: &Value) -> Option<Vec<String>> {
    // 0. Value IS the chain array.
    if let Some(arr) = v.as_array() {
        let mut out = Vec::with_capacity(arr.len());
        let mut all_strings = true;
        for item in arr {
            if let Some(s) = item.as_str() {
                out.push(s.to_string());
            } else {
                all_strings = false;
                break;
            }
        }
        if all_strings {
            return Some(out);
        }
    }

    // 0.5 Value is a JSON string (recurse).
    if let Some(s) = v.as_str()
        && let Ok(nested_val) = serde_json::from_str::<Value>(s)
        // Avoid infinite recursion if the string parses back to a string that is identical.
        // (serde_json::from_str of "\"foo\"" is Value::String("foo")).
        // But usually we expect an Object or Array inside.
        && !nested_val.is_string()
        && let Some(chain) = chain_from_value(&nested_val)
    {
        return Some(chain);
    }

    // Direct chain array.
    if let Some(arr) = v.get("chain").and_then(|c| c.as_array()) {
        let mut out = Vec::with_capacity(arr.len());
        for item in arr {
            out.push(item.as_str()?.to_string());
        }
        return Some(out);
    }

    // Certificate object/string with chain (handle both lower/upper keys).
    for key in ["certificate", "Certificate"] {
        if let Some(cert_obj) = v.get(key) {
            // certificate as object
            if let Some(arr) = cert_obj.get("chain").and_then(|c| c.as_array()) {
                let mut out = Vec::with_capacity(arr.len());
                for item in arr {
                    out.push(item.as_str()?.to_string());
                }
                return Some(out);
            }
            // certificate as escaped JSON string
            if let Some(s) = cert_obj.as_str()
                && let Ok(cert_val) = serde_json::from_str::<Value>(s)
                && let Some(chain) = chain_from_value(&cert_val)
            {
                return Some(chain);
            }
        }
    }
    None
}

/// Extracts the unauthenticated identity of an offline legacy chain.
///
/// Signatures are not checked: offline identities are client-chosen anyway,
/// and the caller binds the client data to the returned identity key.
pub(crate) fn offline_chain_identity(chain: &[String]) -> Result<ValidatedIdentity, AuthError> {
    if chain.len() > MAX_CHAIN_TOKENS {
        return Err(AuthError::ChainTooLong(MAX_CHAIN_TOKENS));
    }
    let last = chain.last().ok_or(AuthError::EmptyChain)?;
    if last.len() > MAX_TOKEN_BYTES {
        return Err(AuthError::TokenTooLarge(MAX_TOKEN_BYTES));
    }
    let header_key = decode_header(last)
        .ok()
        .and_then(|h| h.x5u.or_else(|| h.x5c.and_then(|v| v.into_iter().next())));
    let payload = last
        .split('.')
        .nth(1)
        .ok_or_else(|| AuthError::BadSignature("Invalid JWT format".into()))?;
    let payload = URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| STANDARD.decode(payload))
        .map_err(|e| AuthError::BadSignature(format!("Invalid offline JWT b64: {e}")))?;
    let claims: ChainClaims =
        serde_json::from_slice(&payload).map_err(|_| AuthError::InvalidJson)?;
    let key = claims
        .identity_public_key
        .filter(|key| !key.is_empty())
        .or(header_key)
        .ok_or(AuthError::MissingIdentityKey)?;
    let extra = claims.extra_data.ok_or(AuthError::MissingExtraData)?;
    ValidatedIdentity::new(extra.into(), key, Trust::Unauthenticated)
}

#[cfg(all(test, feature = "server"))]
mod tests {
    use super::*;

    #[test]
    fn chain_from_value_handles_direct_array() {
        let json = serde_json::json!(["jwt1", "jwt2"]);
        let chain = chain_from_value(&json).expect("should extract chain from array");
        assert_eq!(chain, vec!["jwt1", "jwt2"]);
    }

    #[test]
    fn chain_from_value_handles_json_string() {
        // Simulates `certificate: "{\"chain\":[\"jwt1\"]}"`
        let inner_json = r#"{"chain":["jwt1"]}"#;
        let val = Value::String(inner_json.to_string());
        let chain = chain_from_value(&val).expect("should recurse into string");
        assert_eq!(chain, vec!["jwt1"]);
    }

    #[test]
    fn offline_chain_accepts_padded_jwt_and_is_never_authenticated() {
        let payload = serde_json::json!({
            "identityPublicKey": "key",
            "extraData": {
                "XUID": "123",
                "displayName": "User",
                "identity": "e401efb1-4838-328e-9d95-f0821e340749"
            }
        });
        // Encode with standard padding.
        let payload_b64 = STANDARD.encode(serde_json::to_vec(&payload).unwrap());
        let jwt = format!("header.{}.sig", payload_b64);

        let identity = offline_chain_identity(&[jwt]).expect("should accept padded offline jwt");
        assert_eq!(identity.display_name(), "User");
        assert!(!identity.is_authenticated());
        assert_eq!(identity.xuid(), None);
    }
}

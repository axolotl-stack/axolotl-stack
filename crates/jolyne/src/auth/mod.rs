//! Login authentication.
//!
//! Server side: [`decode_login_request`] splits `LoginPacket.connection_request`
//! and [`authenticate_login`] verifies it, returning the identity and the
//! typed, validated [`ClientData`].
//!
//! | Mode | Accepted | Identity |
//! |---|---|---|
//! | online | `AuthenticationType` 0 with a provider-signed `Token` | authenticated; `xuid()`, `playfab_id()`, `title_id()` set |
//! | offline | type 0 with any `Token`; with `allow_legacy`, type 1/2 with a self-signed `Token` or a legacy chain | unauthenticated; `xuid()` is `None` |
//!
//! Client side: see [`client`].

#[cfg(feature = "client")]
pub mod client;

#[cfg(any(feature = "client", feature = "server"))]
pub mod client_data;

#[cfg(feature = "server")]
pub mod legacy;
#[cfg(feature = "server")]
pub mod openid;
#[cfg(feature = "server")]
pub mod types;
#[cfg(feature = "server")]
mod util;

#[cfg(any(feature = "client", feature = "server"))]
pub use client_data::ClientData;
#[cfg(feature = "server")]
pub use legacy::parse_login_chain;
#[cfg(feature = "server")]
pub use types::{AuthenticatedLogin, Trust, ValidatedIdentity};

#[cfg(feature = "server")]
use crate::error::AuthError;
#[cfg(feature = "server")]
use tracing::{instrument, warn};
#[cfg(feature = "server")]
use types::{AuthInfo, AuthenticationType, OpenIdClaims};

/// Decodes the length-prefixed authentication payload carried by the
/// Bedrock 26.51 (`1.26.51`, protocol 2193) `LoginPacket.connection_request` field.
///
/// Layout: `[i32 LE len][auth JSON][i32 LE len][client data JWT]`. Negative
/// lengths, truncation and trailing bytes are rejected with checked arithmetic.
#[cfg(feature = "server")]
pub fn decode_login_request(payload: &[u8]) -> Result<(String, String), AuthError> {
    fn read_i32(payload: &[u8], offset: &mut usize) -> Result<i32, AuthError> {
        let end = offset.checked_add(4).ok_or(AuthError::InvalidJson)?;
        let bytes = payload.get(*offset..end).ok_or(AuthError::InvalidJson)?;
        *offset = end;
        Ok(i32::from_le_bytes(
            bytes.try_into().expect("four-byte slice"),
        ))
    }

    fn read_bytes<'a>(
        payload: &'a [u8],
        offset: &mut usize,
        length: i32,
    ) -> Result<&'a [u8], AuthError> {
        let length = usize::try_from(length).map_err(|_| AuthError::InvalidJson)?;
        let end = offset.checked_add(length).ok_or(AuthError::InvalidJson)?;
        let bytes = payload.get(*offset..end).ok_or(AuthError::InvalidJson)?;
        *offset = end;
        Ok(bytes)
    }

    let mut offset = 0;
    let auth_len = read_i32(payload, &mut offset)?;
    let auth_bytes = read_bytes(payload, &mut offset, auth_len)?;
    let client_len = read_i32(payload, &mut offset)?;
    let client_bytes = read_bytes(payload, &mut offset, client_len)?;
    if offset != payload.len() {
        return Err(AuthError::InvalidJson);
    }

    let auth_info = std::str::from_utf8(auth_bytes).map_err(|_| AuthError::InvalidUtf8)?;
    let client_data = std::str::from_utf8(client_bytes).map_err(|_| AuthError::InvalidUtf8)?;
    Ok((auth_info.to_owned(), client_data.to_owned()))
}

/// Authenticate the two decoded values from a Bedrock `LoginPacket` request
/// against the process-wide Minecraft OpenID provider
/// ([`openid::OpenIdProvider::minecraft`]).
///
/// `auth_info_json` is the AuthenticationInfo JSON and `client_data_jwt` is
/// the raw ClientData JWT from `connection_request`. `allow_legacy` admits
/// Guest/SelfSigned authentication types in offline mode only.
#[cfg(feature = "server")]
pub async fn authenticate_login(
    auth_info_json: &str,
    client_data_jwt: &str,
    online_mode: bool,
    allow_legacy: bool,
) -> Result<AuthenticatedLogin, AuthError> {
    authenticate_login_with(
        openid::OpenIdProvider::minecraft(),
        auth_info_json,
        client_data_jwt,
        online_mode,
        allow_legacy,
    )
    .await
}

/// [`authenticate_login`] with an explicit provider (custom trust source or
/// deterministic tests). The provider is only consulted in online mode.
#[cfg(feature = "server")]
#[instrument(skip_all, level = "trace")]
pub async fn authenticate_login_with(
    provider: &openid::OpenIdProvider,
    auth_info_json: &str,
    client_data_jwt: &str,
    online_mode: bool,
    allow_legacy: bool,
) -> Result<AuthenticatedLogin, AuthError> {
    let auth_info: AuthInfo =
        serde_json::from_str(auth_info_json).map_err(|_| AuthError::InvalidJson)?;
    let auth_type = AuthenticationType::try_from(
        auth_info
            .authentication_type
            .ok_or(AuthError::MissingAuthType)?,
    )?;
    let token = auth_info.token.as_deref().filter(|token| !token.is_empty());
    match auth_type {
        AuthenticationType::Full => {
            let token = token.ok_or(AuthError::MissingToken)?;
            if online_mode {
                provider.validate(token, client_data_jwt).await
            } else {
                offline_token_login(token, client_data_jwt)
            }
        }
        AuthenticationType::SelfSigned | AuthenticationType::Guest => {
            // Online mode accepts only provider-signed tokens; see `legacy`
            // for why Mojang-rooted chains are not accepted.
            if online_mode || !allow_legacy {
                warn!(
                    online_mode,
                    allow_legacy, "Client attempted legacy/self-signed auth but it is not allowed"
                );
                return Err(AuthError::LegacyAuthDisabled);
            }
            // Current offline clients send a self-signed multiplayer token.
            if let Some(token) = token {
                return offline_token_login(token, client_data_jwt);
            }
            let chain = auth_info
                .certificate
                .as_ref()
                .and_then(legacy::chain_from_value)
                .or_else(|| auth_info.chain.as_ref().and_then(legacy::chain_from_value))
                .ok_or_else(|| {
                    warn!(?auth_type, "failed to extract chain from auth info");
                    AuthError::MissingCertificate
                })?;
            let identity = legacy::offline_chain_identity(&chain)?;
            let client_data =
                openid::verify_client_data(identity.identity_public_key(), client_data_jwt)?;
            Ok(AuthenticatedLogin {
                identity,
                client_data,
            })
        }
    }
}

/// Offline multiplayer token: claims are read without signature or expiry
/// checks and the identity is always [`Trust::Unauthenticated`]. The client
/// data must still be signed by the claimed `cpk`.
#[cfg(feature = "server")]
fn offline_token_login(
    token: &str,
    client_data_jwt: &str,
) -> Result<AuthenticatedLogin, AuthError> {
    let claims: OpenIdClaims =
        util::decode_unverified_claims(token).ok_or(AuthError::InvalidJson)?;
    openid::login_from_claims(claims, client_data_jwt, Trust::Unauthenticated)
}

#[cfg(all(test, feature = "server"))]
mod security_tests;

#[cfg(all(test, feature = "server"))]
mod provider_tests;

#[cfg(all(test, feature = "server"))]
mod tests {
    use super::*;
    use crate::error::JolyneError;

    #[test]
    fn parse_login_chain_accepts_valid_payload() {
        let payload = br#"{"chain":["token1","token2"]}"#;
        let parsed = parse_login_chain(payload).expect("should parse");
        assert_eq!(parsed, vec!["token1".to_string(), "token2".to_string()]);
    }

    #[test]
    fn parse_login_chain_rejects_invalid_utf8() {
        let payload = &[0xff, 0xfe, 0xfd];
        let err = parse_login_chain(payload).expect_err("should fail");
        assert!(matches!(err, JolyneError::Auth(AuthError::InvalidUtf8)));
    }

    #[test]
    fn parse_login_chain_rejects_invalid_json() {
        let payload = br#"not a json object"#;
        let err = parse_login_chain(payload).expect_err("should fail");
        assert!(matches!(err, JolyneError::Auth(AuthError::InvalidJson)));
    }

    #[test]
    fn parse_login_chain_rejects_missing_chain() {
        let payload = br#"{"nope":["token"]}"#;
        let err = parse_login_chain(payload).expect_err("should fail");
        assert!(matches!(err, JolyneError::Auth(AuthError::MissingChain)));
    }

    #[cfg(feature = "client")]
    #[test]
    fn login_request_round_trip_preserves_length_prefixed_values() {
        let request = client::encode_login_request(r#"{"chain":["identity"]}"#, "client.jwt");
        let (auth_info, client_data) = decode_login_request(&request).expect("valid request");
        assert_eq!(auth_info, r#"{"chain":["identity"]}"#);
        assert_eq!(client_data, "client.jwt");
    }

    #[tokio::test]
    async fn authenticate_login_rejects_legacy_in_online_mode_even_when_legacy_allowed() {
        let auth_info = r#"{"AuthenticationType":2,"Certificate":{"chain":["token"]}}"#;
        let err = authenticate_login(auth_info, "ignored", true, true)
            .await
            .expect_err("online mode must not accept self-signed auth");
        assert_eq!(err, AuthError::LegacyAuthDisabled);
    }

    #[tokio::test]
    async fn authenticate_login_rejects_legacy_when_disabled() {
        let auth_info = r#"{"AuthenticationType":2,"Certificate":{"chain":["token"]}}"#;
        let err = authenticate_login(auth_info, "ignored", false, false)
            .await
            .expect_err("legacy disabled");
        assert_eq!(err, AuthError::LegacyAuthDisabled);
    }

    #[tokio::test]
    async fn authenticate_login_requires_token_for_full() {
        let auth_info = r#"{"AuthenticationType":0}"#;
        let err = authenticate_login(auth_info, "ignored", true, true)
            .await
            .expect_err("should require token");
        assert_eq!(err, AuthError::MissingToken);
    }

    #[tokio::test]
    async fn authenticate_login_requires_certificate_for_self_signed() {
        let auth_info = r#"{"AuthenticationType":2}"#;
        let err = authenticate_login(auth_info, "ignored", false, true)
            .await
            .expect_err("should require cert");
        assert_eq!(err, AuthError::MissingCertificate);
    }

    #[tokio::test]
    async fn authenticate_login_rejects_unknown_auth_type() {
        let auth_info = r#"{"AuthenticationType":99}"#;
        let err = authenticate_login(auth_info, "ignored", true, true)
            .await
            .expect_err("should reject unknown type");
        assert_eq!(err, AuthError::UnsupportedAuthType(99));
    }

    #[tokio::test]
    async fn authenticate_login_requires_auth_type() {
        let auth_info = r#"{"Token":"ignored"}"#;
        let err = authenticate_login(auth_info, "ignored", true, true)
            .await
            .expect_err("should require auth type");
        assert_eq!(err, AuthError::MissingAuthType);
    }
}

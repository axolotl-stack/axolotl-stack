pub mod legacy;
pub mod openid;
pub mod types;
mod util;

pub use legacy::{MOJANG_PUBLIC_KEY_BASE64, parse_login_chain, validate_chain};
pub use types::ValidatedIdentity;

use crate::error::{AuthError, JolyneError};
use openid::fill_identity_from_client_data;
use tracing::warn;
use types::{AuthInfo, AuthenticationType};

/// Parse the Bedrock `LoginPacket` authentication fields.
/// `auth_info_json` is the LoginPacket.identity field (AuthenticationInfo JSON).
/// `client_data_jwt` is the LoginPacket.client field.
pub async fn authenticate_login(
    auth_info_json: &str,
    client_data_jwt: &str,
    online_mode: bool,
    allow_legacy: bool,
) -> Result<ValidatedIdentity, JolyneError> {
    // Try to parse structured AuthenticationInfo
    let auth_info: AuthInfo =
        serde_json::from_str(auth_info_json).map_err(|_| AuthError::InvalidJson)?;

    let auth_type_raw = auth_info
        .authentication_type
        .ok_or(AuthError::MissingAuthType)?;
    let auth_type = AuthenticationType::try_from(auth_type_raw)?;
    match auth_type {
        AuthenticationType::Full => {
            let token = auth_info.token.as_ref().ok_or(AuthError::MissingToken)?;
            let identity = openid::validate_open_id(token, client_data_jwt, online_mode).await?;
            Ok(identity)
        }
        AuthenticationType::SelfSigned | AuthenticationType::Guest => {
            if !allow_legacy {
                return Err(AuthError::LegacyAuthDisabled.into());
            }

            let chain_opt = auth_info
                .certificate
                .as_ref()
                .and_then(legacy::chain_from_value)
                .or_else(|| auth_info.chain.as_ref().and_then(legacy::chain_from_value));

            if let Some(chain) = chain_opt {
                let identity = validate_chain(chain, online_mode)?;
                let identity = fill_identity_from_client_data(identity, client_data_jwt);
                Ok(identity)
            } else {
                warn!(?auth_info, "failed to extract chain from auth info");
                Err(AuthError::MissingCertificate.into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use serde_json::json;

    fn make_jwt(header: serde_json::Value, payload: serde_json::Value) -> String {
        let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).expect("header json"));
        let payload_b64 =
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).expect("payload json"));
        format!("{header_b64}.{payload_b64}.sig")
    }

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

    #[tokio::test]
    async fn authenticate_login_rejects_legacy_when_disabled() {
        let auth_info = r#"{"AuthenticationType":2,"Certificate":{"chain":["token"]}}"#;
        let err = authenticate_login(auth_info, "ignored", true, false)
            .await
            .expect_err("legacy disabled");
        assert!(matches!(
            err,
            JolyneError::Auth(AuthError::LegacyAuthDisabled)
        ));
    }

    #[tokio::test]
    async fn authenticate_login_requires_token_for_full() {
        let auth_info = r#"{"AuthenticationType":0}"#;
        let err = authenticate_login(auth_info, "ignored", true, true)
            .await
            .expect_err("should require token");
        assert!(matches!(err, JolyneError::Auth(AuthError::MissingToken)));
    }

    #[tokio::test]
    async fn authenticate_login_requires_certificate_for_self_signed() {
        let auth_info = r#"{"AuthenticationType":2}"#;
        let err = authenticate_login(auth_info, "ignored", true, true)
            .await
            .expect_err("should require cert");
        assert!(matches!(
            err,
            JolyneError::Auth(AuthError::MissingCertificate)
        ));
    }

    #[tokio::test]
    async fn authenticate_login_rejects_unknown_auth_type() {
        let auth_info = r#"{"AuthenticationType":99}"#;
        let err = authenticate_login(auth_info, "ignored", true, true)
            .await
            .expect_err("should reject unknown type");
        assert!(matches!(
            err,
            JolyneError::Auth(AuthError::UnsupportedAuthType(99))
        ));
    }

    #[tokio::test]
    async fn validate_open_id_prefers_client_identity_key_when_missing_in_token() {
        let token = make_jwt(
            json!({"alg":"ES256"}),
            json!({
                "extraData": {
                    "displayName": "TokenName",
                    "identity": "uuid-token",
                    "XUID": "12345"
                }
            }),
        );

        let client = make_jwt(
            json!({"alg":"ES256"}),
            json!({
                "IdentityPublicKey": "CLIENT_KEY",
                "displayName": "ClientName",
                "identity": "uuid-client",
                "XUID": "12345"
            }),
        );

        let id = openid::validate_open_id(&token, &client, false)
            .await
            .expect("should accept");
        assert_eq!(id.identity_public_key, "CLIENT_KEY");
        assert_eq!(id.display_name.as_deref(), Some("TokenName"));
        assert_eq!(id.xuid.as_deref(), Some("12345"));
        assert_eq!(id.uuid.as_deref(), Some("uuid-token"));
    }

    #[tokio::test]
    async fn authenticate_login_requires_auth_type() {
        let auth_info = r#"{"Token":"ignored"}"#;
        let err = authenticate_login(auth_info, "ignored", true, true)
            .await
            .expect_err("should require auth type");
        assert!(matches!(err, JolyneError::Auth(AuthError::MissingAuthType)));
    }
}

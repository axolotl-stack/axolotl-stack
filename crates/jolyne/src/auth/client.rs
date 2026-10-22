//! Client-side Login request construction.
//!
//! [`build_login`] produces the AuthenticationInfo JSON and the client-data JWT
//! for [`LoginCredentials`]:
//!
//! - [`LoginCredentials::Online`] sends `AuthenticationType: 0` with a
//!   provider-signed multiplayer token whose `cpk` is this connection's key,
//!   as vanilla clients and gophertunnel (`minecraft/dial.go`) do. Jolyne
//!   online servers and BDS 1.26.51 authenticate only this token.
//! - [`LoginCredentials::Offline`] sends a self-signed multiplayer token
//!   (`AuthenticationType: 2`), accepted only by offline-mode servers.
//!
//! Token acquisition is behind [`MultiplayerTokenSource`] so this crate does
//! not depend on an Xbox Live client. With `axolotl-xbl` the chain is
//! PlayFab-scoped XBL token → `PlayFabClient::login` → `service_token` →
//! `multiplayer_token` (see `examples/simple_client.rs`).

use crate::auth::client_data::ClientData;
use crate::error::AuthError;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use futures::future::BoxFuture;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use p384::SecretKey;
use p384::pkcs8::{EncodePrivateKey, EncodePublicKey};
use serde::Serialize;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Issues provider-signed multiplayer tokens (gophertunnel
/// `minecraft.MultiplayerTokenSource`).
pub trait MultiplayerTokenSource: Send + Sync {
    /// Returns a token whose `cpk` claim is `public_key_der` (PKIX/SPKI DER of
    /// the connection's P-384 key). A fresh token is needed per connection key.
    fn multiplayer_token<'a>(
        &'a self,
        public_key_der: &'a [u8],
    ) -> BoxFuture<'a, Result<String, AuthError>>;
}

/// How the client proves its identity in the Login packet.
#[derive(Clone, Default)]
pub enum LoginCredentials {
    /// Self-signed identity for offline-mode servers.
    #[default]
    Offline,
    /// Provider-signed multiplayer token (`AuthenticationType: 0`).
    Online(Arc<dyn MultiplayerTokenSource>),
}

impl std::fmt::Debug for LoginCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Offline => "Offline",
            Self::Online(_) => "Online(..)",
        })
    }
}

/// The two values carried by `LoginPacket.connection_request`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginTokens {
    pub auth_info_json: String,
    pub client_data_jwt: String,
}

/// Builds the Login authentication for `credentials`, signing the client data
/// with `key`. Retain `key` for the encryption handshake.
pub async fn build_login(
    key: &SecretKey,
    display_name: &str,
    uuid: Uuid,
    credentials: &LoginCredentials,
) -> Result<LoginTokens, AuthError> {
    let (auth_info_json, client_data_jwt) = match credentials {
        LoginCredentials::Offline => generate_self_signed_chain(key, display_name, uuid)?,
        LoginCredentials::Online(source) => {
            let token = source.multiplayer_token(&public_key_der(key)?).await?;
            if token.is_empty() {
                return Err(AuthError::TokenAcquisition(
                    "empty multiplayer token".into(),
                ));
            }
            encode_with_multiplayer_token(key, display_name, uuid, &token)?
        }
    };
    Ok(LoginTokens {
        auth_info_json,
        client_data_jwt,
    })
}

#[derive(Serialize)]
struct IdentityClaims<'a> {
    aud: &'static str,
    cpk: &'a str,
    xname: &'a str,
    leguuid: String,
    xid: &'static str,
    ipt: &'static str,
    mid: &'static str,
    tid: &'static str,
    nbf: u64,
    exp: u64,
}

fn signing_error(err: impl std::fmt::Display) -> AuthError {
    AuthError::TokenAcquisition(err.to_string())
}

fn public_key_der(key: &SecretKey) -> Result<Vec<u8>, AuthError> {
    Ok(key
        .public_key()
        .to_public_key_der()
        .map_err(signing_error)?
        .into_vec())
}

fn encoding_key(key: &SecretKey) -> Result<EncodingKey, AuthError> {
    Ok(EncodingKey::from_ec_der(
        key.to_pkcs8_der().map_err(signing_error)?.as_bytes(),
    ))
}

/// ES384 JWT signed by `key` with the base64 SPKI key in `x5u`, as vanilla.
fn sign_es384(key: &SecretKey, claims: &impl Serialize) -> Result<String, AuthError> {
    let mut header = Header::new(Algorithm::ES384);
    header.x5u = Some(STANDARD.encode(public_key_der(key)?));
    encode(&header, claims, &encoding_key(key)?).map_err(signing_error)
}

/// Client data with jolyne's defaults: an Android device (retail BDS 1.26.51
/// rejects UWP before authentication) with an opaque 64x64 Steve-shaped skin.
pub fn default_client_data(display_name: &str, uuid: Uuid) -> ClientData {
    ClientData {
        arm_size: "wide".into(),
        client_random_id: (rand::random::<u64>() & 0x7FFF_FFFF_FFFF_FFFF) as i64,
        current_input_mode: 1, // Mouse/Keyboard
        default_input_mode: 1,
        device_id: Uuid::new_v4().to_string(),
        device_model: "JolyneClient".into(),
        device_os: 1, // Android
        game_version: crate::valentine::GAME_VERSION.into(),
        language_code: "en_US".into(),
        max_view_distance: 32,
        memory_tier: 5,
        self_signed_id: uuid.to_string(),
        skin_colour: "#b37b62".into(),
        skin_data: STANDARD.encode([255u8; 64 * 64 * 4]),
        skin_geometry: String::new(),
        skin_id: format!("{uuid}.Custom"),
        skin_image_height: 64,
        skin_image_width: 64,
        skin_resource_patch: STANDARD
            .encode(r#"{"geometry":{"default":"geometry.humanoid.custom"}}"#),
        third_party_name: display_name.into(),
        third_party_name_only: Some(false),
        ..ClientData::default()
    }
}

/// Signs `client_data` as the client-data JWT. The server verifies it with the
/// identity key (`cpk`), so `key` must be the connection key.
pub fn sign_client_data(key: &SecretKey, client_data: &ClientData) -> Result<String, AuthError> {
    sign_es384(key, client_data)
}

/// Generates a modern self-signed multiplayer identity and a ClientData JWT.
/// Returns `(authentication_json, client_data_jwt)` for offline mode.
///
/// The XUID claim is empty: self-signed tokens never carry account authority.
pub fn generate_self_signed_chain(
    key: &SecretKey,
    display_name: &str,
    uuid: Uuid,
) -> Result<(String, String), AuthError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(signing_error)?
        .as_secs();
    let cpk = STANDARD.encode(public_key_der(key)?);
    let identity_jwt = sign_es384(
        key,
        &IdentityClaims {
            aud: "api://auth-minecraft-services/multiplayer",
            cpk: &cpk,
            xname: display_name,
            leguuid: uuid.to_string(),
            xid: "",
            ipt: "",
            mid: "",
            tid: "",
            nbf: now - 1,
            exp: now + 24 * 60 * 60,
        },
    )?;
    // BDS 26.51 reads the multiplayer Token, not a top-level legacy chain.
    let auth = serde_json::json!({
        "AuthenticationType": 2,
        "Certificate": r#"{"chain":[""]}"#,
        "Token": identity_jwt
    })
    .to_string();
    let client = sign_client_data(key, &default_client_data(display_name, uuid))?;
    Ok((auth, client))
}

/// Formats modern online authentication using an already acquired multiplayer token.
///
/// The caller must acquire the provider token for this exact key; this function
/// neither acquires nor verifies account credentials. Pass both returned strings
/// to `send_login_with_tokens`, and retain `key` for the encryption handshake.
pub fn encode_with_multiplayer_token(
    key: &SecretKey,
    display_name: &str,
    uuid: Uuid,
    multiplayer_token: &str,
) -> Result<(String, String), AuthError> {
    let auth = serde_json::json!({
        "AuthenticationType": 0,
        "Token": multiplayer_token,
    })
    .to_string();
    let client = sign_client_data(key, &default_client_data(display_name, uuid))?;
    Ok((auth, client))
}

/// Encodes the authentication JSON and client-data JWT into the raw
/// `LoginPacket.connection_request` payload used by Bedrock 26.51 (protocol 2193).
///
/// The protocol prefixes both values with little-endian signed 32-bit byte
/// lengths. The authentication JSON bytes are preserved exactly, matching
/// the pinned gophertunnel login encoder.
pub fn encode_login_request(auth_info_json: &str, client_data_jwt: &str) -> Vec<u8> {
    let auth_info = auth_info_json.as_bytes();

    let auth_len = i32::try_from(auth_info.len()).expect("login auth JSON exceeds i32 length");
    let client_len =
        i32::try_from(client_data_jwt.len()).expect("login client token exceeds i32 length");

    let mut request = Vec::with_capacity(8 + auth_info.len() + client_data_jwt.len());
    request.extend_from_slice(&auth_len.to_le_bytes());
    request.extend_from_slice(auth_info);
    request.extend_from_slice(&client_len.to_le_bytes());
    request.extend_from_slice(client_data_jwt.as_bytes());
    request
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use parking_lot::Mutex;

    #[test]
    fn offline_login_encodes_a_retail_supported_device_os() {
        let key = SecretKey::from_slice(&[1; 48]).unwrap();
        let (auth, client) = generate_self_signed_chain(&key, "ClientDiff", Uuid::nil()).unwrap();
        let request = encode_login_request(&auth, &client);
        let auth_len = u32::from_le_bytes(request[..4].try_into().unwrap()) as usize;
        let client_len_offset = 4 + auth_len;
        let client_len = u32::from_le_bytes(
            request[client_len_offset..client_len_offset + 4]
                .try_into()
                .unwrap(),
        ) as usize;
        assert_eq!(request.len(), client_len_offset + 4 + client_len);
        let token = std::str::from_utf8(&request[client_len_offset + 4..]).unwrap();
        let claims: serde_json::Value = serde_json::from_slice(
            &URL_SAFE_NO_PAD
                .decode(token.split('.').nth(1).unwrap())
                .unwrap(),
        )
        .unwrap();
        // BDS 1.26.51's isWellFormed uses bit (DeviceOS + 1) of 0x1763d.
        // UWP (7) fails this check before authentication; Android (1) is supported.
        let os = claims["DeviceOS"].as_u64().unwrap();
        assert!(os < 16 && (0x1763d_u32 & (1 << (os + 1))) != 0);
        assert_eq!(os, 1);
    }

    #[test]
    fn offline_login_identity_token_binds_the_client_signing_key() {
        use p384::ecdsa::{Signature, VerifyingKey, signature::Verifier};
        let key = SecretKey::from_slice(&[1; 48]).unwrap();
        let uuid = Uuid::from_u128(123);
        let (auth, client) = generate_self_signed_chain(&key, "ClientDiff", uuid).unwrap();
        let request = encode_login_request(&auth, &client);
        let auth_len = u32::from_le_bytes(request[..4].try_into().unwrap()) as usize;
        let envelope: serde_json::Value =
            serde_json::from_slice(&request[4..4 + auth_len]).unwrap();
        assert_eq!(envelope["AuthenticationType"], 2);
        let token = envelope["Token"].as_str().expect("modern identity token");
        let parts: Vec<_> = token.split('.').collect();
        let claims: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(claims["aud"], "api://auth-minecraft-services/multiplayer");
        assert_eq!(claims["xname"], "ClientDiff");
        assert_eq!(claims["leguuid"], uuid.to_string());
        assert_eq!(claims["xid"], "");
        assert_eq!(
            claims["cpk"],
            STANDARD.encode(key.public_key().to_public_key_der().unwrap())
        );
        let verify = |token: &str| {
            let parts: Vec<_> = token.split('.').collect();
            let signature =
                Signature::from_slice(&URL_SAFE_NO_PAD.decode(parts[2]).unwrap()).unwrap();
            VerifyingKey::from(key.public_key())
                .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &signature)
                .unwrap();
        };
        verify(token);
        verify(std::str::from_utf8(&request[4 + auth_len + 4..]).unwrap());
    }

    #[test]
    fn authenticated_login_uses_full_auth_and_preserves_provider_token() {
        use p384::ecdsa::{Signature, VerifyingKey, signature::Verifier};
        let key = SecretKey::from_slice(&[1; 48]).unwrap();
        let provider_token = "opaque.provider.token";
        let (auth, client) =
            encode_with_multiplayer_token(&key, "ClientDiff", Uuid::nil(), provider_token).unwrap();
        let request = encode_login_request(&auth, &client);
        let auth_len = u32::from_le_bytes(request[..4].try_into().unwrap()) as usize;
        let envelope: serde_json::Value =
            serde_json::from_slice(&request[4..4 + auth_len]).unwrap();
        assert_eq!(envelope["AuthenticationType"], 0);
        assert_eq!(envelope["Token"], provider_token);
        let token = std::str::from_utf8(&request[4 + auth_len + 4..]).unwrap();
        let parts: Vec<_> = token.split('.').collect();
        let signature = Signature::from_slice(&URL_SAFE_NO_PAD.decode(parts[2]).unwrap()).unwrap();
        VerifyingKey::from(key.public_key())
            .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &signature)
            .unwrap();
    }

    #[test]
    fn client_without_a_chunk_generator_does_not_advertise_one() {
        let key = SecretKey::from_slice(&[1; 48]).unwrap();
        for (_, client) in [
            generate_self_signed_chain(&key, "ClientDiff", Uuid::nil()).unwrap(),
            encode_with_multiplayer_token(&key, "ClientDiff", Uuid::nil(), "provider-token")
                .unwrap(),
        ] {
            let claims: serde_json::Value = serde_json::from_slice(
                &URL_SAFE_NO_PAD
                    .decode(client.split('.').nth(1).unwrap())
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(claims["CompatibleWithClientSideChunkGen"], false);
        }
    }

    #[test]
    fn default_client_data_passes_server_validation() {
        default_client_data("ClientDiff", Uuid::from_u128(7))
            .validate()
            .unwrap();
    }

    struct RecordingSource {
        token: &'static str,
        keys: Mutex<Vec<Vec<u8>>>,
    }

    impl MultiplayerTokenSource for RecordingSource {
        fn multiplayer_token<'a>(
            &'a self,
            public_key_der: &'a [u8],
        ) -> BoxFuture<'a, Result<String, AuthError>> {
            self.keys.lock().push(public_key_der.to_vec());
            Box::pin(async move { Ok(self.token.to_owned()) })
        }
    }

    #[tokio::test]
    async fn online_credentials_send_the_modern_token_for_the_connection_key() {
        let key = SecretKey::from_slice(&[1; 48]).unwrap();
        let source = Arc::new(RecordingSource {
            token: "provider.signed.token",
            keys: Mutex::default(),
        });
        let login = build_login(
            &key,
            "ClientDiff",
            Uuid::nil(),
            &LoginCredentials::Online(source.clone()),
        )
        .await
        .unwrap();
        let envelope: serde_json::Value = serde_json::from_str(&login.auth_info_json).unwrap();
        assert_eq!(envelope["AuthenticationType"], 0);
        assert_eq!(envelope["Token"], "provider.signed.token");
        assert_eq!(
            *source.keys.lock(),
            vec![key.public_key().to_public_key_der().unwrap().into_vec()]
        );
    }

    #[tokio::test]
    async fn empty_provider_token_is_rejected_before_sending() {
        let key = SecretKey::from_slice(&[1; 48]).unwrap();
        let source = Arc::new(RecordingSource {
            token: "",
            keys: Mutex::default(),
        });
        let err = build_login(&key, "A", Uuid::nil(), &LoginCredentials::Online(source))
            .await
            .unwrap_err();
        assert!(matches!(err, AuthError::TokenAcquisition(_)));
    }
}

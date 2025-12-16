use crate::error::JolyneError;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use p384::SecretKey;
use p384::pkcs8::{EncodePrivateKey, EncodePublicKey};
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Serialize)]
struct IdentityClaims {
    #[serde(rename = "extraData")]
    extra_data: IdentityExtraData,
    #[serde(rename = "identityPublicKey")]
    identity_public_key: String,
    nbf: u64,
    exp: u64,
    iat: u64,
    iss: String,
}

#[derive(Serialize)]
struct IdentityExtraData {
    #[serde(rename = "displayName")]
    display_name: String,
    identity: String, // UUID
    #[serde(rename = "XUID")]
    xuid: String,
    #[serde(rename = "titleId")]
    title_id: String,
}

#[derive(Serialize)]
struct ClientDataPayload {
    #[serde(rename = "ClientRandomId")]
    client_random_id: u64,
    #[serde(rename = "CurrentInputMode")]
    current_input_mode: u32,
    #[serde(rename = "DefaultInputMode")]
    default_input_mode: u32,
    #[serde(rename = "DeviceModel")]
    device_model: String,
    #[serde(rename = "DeviceOS")]
    device_os: u32,
    #[serde(rename = "GameVersion")]
    game_version: String,
    #[serde(rename = "GuiScale")]
    gui_scale: i32,
    #[serde(rename = "LanguageCode")]
    language_code: String,
    #[serde(rename = "SkinData")]
    skin_data: String,
    #[serde(rename = "SkinGeometry")]
    skin_geometry: String,
    #[serde(rename = "SkinId")]
    skin_id: String,
    #[serde(rename = "UIProfile")]
    ui_profile: u32,
    nbf: u64,
    exp: u64,
    iat: u64,
    iss: String,
}

/// Generates a self-signed chain (for Offline Mode) and a ClientData JWT.
/// Returns (identity_chain_json_string, client_data_jwt).
pub fn generate_self_signed_chain(
    key: &SecretKey,
    display_name: &str,
    uuid: Uuid,
) -> Result<(String, String), JolyneError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let exp = now + 24 * 60 * 60; // 24h

    let public_key_der = key
        .public_key()
        .to_public_key_der()
        .map_err(|e| JolyneError::Auth(crate::error::AuthError::BadSignature(e.to_string())))?;
    let public_key_b64 = STANDARD.encode(public_key_der.as_bytes());

    let private_der = key
        .to_pkcs8_der()
        .map_err(|e| JolyneError::Auth(crate::error::AuthError::BadSignature(e.to_string())))?;
    let encoding_key = EncodingKey::from_ec_der(private_der.as_bytes());

    // 1. Identity Token
    let identity_claims = IdentityClaims {
        extra_data: IdentityExtraData {
            display_name: display_name.to_string(),
            identity: uuid.to_string(),
            xuid: "".to_string(),
            title_id: "896928775".to_string(),
        },
        identity_public_key: public_key_b64.clone(),
        nbf: now - 1,
        exp,
        iat: now,
        iss: "self".to_string(),
    };

    let mut header = Header::new(Algorithm::ES384);
    header.x5u = Some(public_key_b64); // Self-signed: x5u is self

    let identity_jwt = encode(&header, &identity_claims, &encoding_key)
        .map_err(|e| JolyneError::Auth(crate::error::AuthError::BadSignature(e.to_string())))?;

    // Chain JSON
    let chain_json = serde_json::json!({
        "chain": [identity_jwt],
        "AuthenticationType": 2
    })
    .to_string();

    // 2. ClientData Token
    // Bedrock clients usually include a large base64 SkinData. We use a minimal one here.
    // A 64x32 RGBA skin is 8192 bytes.
    // We'll provide a dummy empty string for now, as most simple servers might ignore it.
    // If strict validation is required, we'd need a real skin.
    let client_claims = ClientDataPayload {
        client_random_id: rand::random(),
        current_input_mode: 1, // Mouse/Keyboard
        default_input_mode: 1,
        device_model: "JolyneClient".into(),
        device_os: 7, // Win10
        game_version: crate::protocol::GAME_VERSION.into(),
        gui_scale: 0,
        language_code: "en_US".into(),
        skin_data: "".into(), // TODO: Real skin
        skin_geometry: "".into(),
        skin_id: "Custom".into(),
        ui_profile: 0,
        nbf: now - 1,
        exp,
        iat: now,
        iss: "self".into(),
    };

    // ClientData JWT usually doesn't need x5u, it's signed by the Identity Key.
    // The server verifies it against the key in the Identity Chain.
    let client_header = Header::new(Algorithm::ES384);
    let client_jwt = encode(&client_header, &client_claims, &encoding_key)
        .map_err(|e| JolyneError::Auth(crate::error::AuthError::BadSignature(e.to_string())))?;

    Ok((chain_json, client_jwt))
}

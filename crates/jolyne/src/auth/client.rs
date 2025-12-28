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

/// Complete ClientData payload with all fields required by BDS.
/// Based on gophertunnel's minecraft/protocol/login/data.go
#[derive(Serialize)]
struct ClientDataPayload {
    // Animation data (empty for simple skins)
    #[serde(rename = "AnimatedImageData")]
    animated_image_data: Vec<()>,
    #[serde(rename = "ArmSize")]
    arm_size: String,
    #[serde(rename = "CapeData")]
    cape_data: String,
    #[serde(rename = "CapeId")]
    cape_id: String,
    #[serde(rename = "CapeImageHeight")]
    cape_image_height: u32,
    #[serde(rename = "CapeImageWidth")]
    cape_image_width: u32,
    #[serde(rename = "CapeOnClassicSkin")]
    cape_on_classic_skin: bool,
    #[serde(rename = "ClientRandomId")]
    client_random_id: i64,
    #[serde(rename = "CompatibleWithClientSideChunkGen")]
    compatible_with_client_side_chunk_gen: bool,
    #[serde(rename = "CurrentInputMode")]
    current_input_mode: u32,
    #[serde(rename = "DefaultInputMode")]
    default_input_mode: u32,
    #[serde(rename = "DeviceId")]
    device_id: String,
    #[serde(rename = "DeviceModel")]
    device_model: String,
    #[serde(rename = "DeviceOS")]
    device_os: u32,
    #[serde(rename = "GameVersion")]
    game_version: String,
    #[serde(rename = "GraphicsMode")]
    graphics_mode: u32,
    #[serde(rename = "GuiScale")]
    gui_scale: i32,
    #[serde(rename = "IsEditorMode")]
    is_editor_mode: bool,
    #[serde(rename = "LanguageCode")]
    language_code: String,
    #[serde(rename = "MaxViewDistance")]
    max_view_distance: u32,
    #[serde(rename = "MemoryTier")]
    memory_tier: u32,
    #[serde(rename = "OverrideSkin")]
    override_skin: bool,
    #[serde(rename = "PersonaPieces")]
    persona_pieces: Vec<()>,
    #[serde(rename = "PersonaSkin")]
    persona_skin: bool,
    #[serde(rename = "PieceTintColors")]
    piece_tint_colors: Vec<()>,
    #[serde(rename = "PlatformOfflineId")]
    platform_offline_id: String,
    #[serde(rename = "PlatformOnlineId")]
    platform_online_id: String,
    #[serde(rename = "PlatformType")]
    platform_type: u32,
    #[serde(rename = "PlayFabId")]
    play_fab_id: String,
    #[serde(rename = "PremiumSkin")]
    premium_skin: bool,
    #[serde(rename = "SelfSignedId")]
    self_signed_id: String,
    #[serde(rename = "ServerAddress")]
    server_address: String,
    #[serde(rename = "SkinAnimationData")]
    skin_animation_data: String,
    #[serde(rename = "SkinColor")]
    skin_color: String,
    #[serde(rename = "SkinData")]
    skin_data: String,
    #[serde(rename = "SkinGeometryData")]
    skin_geometry_data: String,
    #[serde(rename = "SkinGeometryDataEngineVersion")]
    skin_geometry_data_engine_version: String,
    #[serde(rename = "SkinId")]
    skin_id: String,
    #[serde(rename = "SkinImageHeight")]
    skin_image_height: u32,
    #[serde(rename = "SkinImageWidth")]
    skin_image_width: u32,
    #[serde(rename = "SkinResourcePatch")]
    skin_resource_patch: String,
    #[serde(rename = "ThirdPartyName")]
    third_party_name: String,
    #[serde(rename = "ThirdPartyNameOnly")]
    third_party_name_only: bool,
    #[serde(rename = "TrustedSkin")]
    trusted_skin: bool,
    #[serde(rename = "UIProfile")]
    ui_profile: u32,
}

/// Generates a minimal valid skin resource patch JSON.
fn generate_skin_resource_patch() -> String {
    let json = serde_json::json!({
        "geometry": {
            "default": "geometry.humanoid.custom"
        }
    });
    STANDARD.encode(json.to_string().as_bytes())
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
            title_id: "896928775".to_string(), // Win10
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
    // A minimal 64x64 RGBA skin is 16384 bytes (64*64*4).
    // Create a simple solid-color skin (all white/opaque)
    let skin_pixels = vec![255u8; 64 * 64 * 4];
    let skin_data_b64 = STANDARD.encode(&skin_pixels);

    // Device ID is a UUID
    let device_id = Uuid::new_v4().to_string();

    let client_claims = ClientDataPayload {
        animated_image_data: vec![],
        arm_size: "wide".into(), // Standard Steve arm size
        cape_data: "".into(),
        cape_id: "".into(),
        cape_image_height: 0,
        cape_image_width: 0,
        cape_on_classic_skin: false,
        client_random_id: (rand::random::<u64>() & 0x7FFFFFFFFFFFFFFF) as i64,
        compatible_with_client_side_chunk_gen: true,
        current_input_mode: 1, // Mouse/Keyboard
        default_input_mode: 1,
        device_id,
        device_model: "JolyneClient".into(),
        device_os: 7, // Win10
        game_version: crate::valentine::GAME_VERSION.into(),
        graphics_mode: 0,
        gui_scale: 0,
        is_editor_mode: false,
        language_code: "en_US".into(),
        max_view_distance: 32,
        memory_tier: 5, // Super High
        override_skin: false,
        persona_pieces: vec![],
        persona_skin: false,
        piece_tint_colors: vec![],
        platform_offline_id: "".into(),
        platform_online_id: "".into(),
        platform_type: 0,
        play_fab_id: "".into(),
        premium_skin: false,
        self_signed_id: uuid.to_string(),
        server_address: "".into(),
        skin_animation_data: "".into(),
        skin_color: "#b37b62".into(), // Default Steve skin color
        skin_data: skin_data_b64,
        skin_geometry_data: STANDARD.encode(""), // Empty = use default
        skin_geometry_data_engine_version: "".into(),
        skin_id: format!("{}.Custom", uuid),
        skin_image_height: 64,
        skin_image_width: 64,
        skin_resource_patch: generate_skin_resource_patch(),
        third_party_name: display_name.into(),
        third_party_name_only: false,
        trusted_skin: false,
        ui_profile: 0,
    };

    // ClientData JWT - uses x5u to specify the signing key
    let mut client_header = Header::new(Algorithm::ES384);
    client_header.x5u = Some(
        STANDARD.encode(
            key.public_key()
                .to_public_key_der()
                .map_err(|e| {
                    JolyneError::Auth(crate::error::AuthError::BadSignature(e.to_string()))
                })?
                .as_bytes(),
        ),
    );

    let client_jwt = encode(&client_header, &client_claims, &encoding_key)
        .map_err(|e| JolyneError::Auth(crate::error::AuthError::BadSignature(e.to_string())))?;

    Ok((chain_json, client_jwt))
}

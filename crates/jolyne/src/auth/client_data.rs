//! Typed Login client data (the client-signed ClientData JWT payload).
//!
//! Field names and validation follow gophertunnel
//! `minecraft/protocol/login/data.go` (`ClientData`, `ClientData.Validate`).
//! Every value is client-controlled: the server only knows that the holder of
//! the identity key signed it.
//!
//! Deliberate differences from gophertunnel:
//! - `ServerAddress` is validated syntactically and never resolved through DNS
//!   (gophertunnel calls `net.ResolveUDPAddr`, a DNS lookup on attacker input).
//! - `LanguageCode` must be a well-formed BCP-47 tag; the IANA registry lookup
//!   of `golang.org/x/text/language.Parse` is not reproduced.
//! - `GameVersion` must consist only of digits and dots (gophertunnel's regex
//!   is unanchored and accepts any string containing one digit or dot).
//! - Image dimensions are unsigned and their byte size uses checked arithmetic.

use crate::error::AuthError;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};

/// Login client data. Unknown JSON fields are ignored and missing fields take
/// their zero value, matching Go `encoding/json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClientData {
    #[serde(rename = "AnimatedImageData")]
    pub animated_image_data: Vec<SkinAnimation>,
    #[serde(rename = "ArmSize")]
    pub arm_size: String,
    #[serde(rename = "CapeData")]
    pub cape_data: String,
    #[serde(rename = "CapeId")]
    pub cape_id: String,
    #[serde(rename = "CapeImageHeight")]
    pub cape_image_height: u32,
    #[serde(rename = "CapeImageWidth")]
    pub cape_image_width: u32,
    #[serde(rename = "CapeOnClassicSkin")]
    pub cape_on_classic_skin: bool,
    #[serde(rename = "ClientRandomId")]
    pub client_random_id: i64,
    #[serde(rename = "ClientEditorConnectionIntent")]
    pub client_editor_connection_intent: i32,
    #[serde(rename = "ClientIsEditorCapable")]
    pub client_is_editor_capable: bool,
    #[serde(rename = "CompatibleWithClientSideChunkGen")]
    pub compatible_with_client_side_chunk_gen: bool,
    #[serde(rename = "CurrentInputMode")]
    pub current_input_mode: i32,
    #[serde(rename = "DefaultInputMode")]
    pub default_input_mode: i32,
    /// Device-specific ID; its format varies per OS and is not validated.
    #[serde(rename = "DeviceId")]
    pub device_id: String,
    #[serde(rename = "DeviceModel")]
    pub device_model: String,
    /// `1..=15`: Android, iOS, OSX, FireOS, GearVR, Hololens, UWP, Win32,
    /// Dedicated, TVOS, Orbis, NX, Xbox, Windows Phone, Linux.
    #[serde(rename = "DeviceOS")]
    pub device_os: i32,
    #[serde(rename = "FilterProfanity")]
    pub filter_profanity: bool,
    #[serde(rename = "GameVersion")]
    pub game_version: String,
    #[serde(rename = "GraphicsMode")]
    pub graphics_mode: i32,
    #[serde(rename = "GuiScale")]
    pub gui_scale: i32,
    #[serde(rename = "IsEditorMode")]
    pub is_editor_mode: bool,
    /// Echo of the identity key. When present it must equal the key that
    /// signed the client data.
    #[serde(
        rename = "IdentityPublicKey",
        alias = "identityPublicKey",
        skip_serializing_if = "Option::is_none"
    )]
    pub identity_public_key: Option<String>,
    #[serde(rename = "IsPartyLeader")]
    pub party_leader: bool,
    #[serde(rename = "LanguageCode")]
    pub language_code: String,
    #[serde(rename = "MaxViewDistance")]
    pub max_view_distance: i32,
    #[serde(rename = "MemoryTier")]
    pub memory_tier: i32,
    #[serde(rename = "Nonce", skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    #[serde(rename = "OverrideSkin")]
    pub override_skin: bool,
    #[serde(rename = "PartyId")]
    pub party_id: String,
    #[serde(rename = "PersonaPieces")]
    pub persona_pieces: Vec<PersonaPiece>,
    #[serde(rename = "PersonaSkin")]
    pub persona_skin: bool,
    #[serde(rename = "PieceTintColors")]
    pub piece_tint_colours: Vec<PersonaPieceTintColour>,
    #[serde(rename = "PlatformOfflineId")]
    pub platform_offline_id: String,
    #[serde(rename = "PlatformOnlineId")]
    pub platform_online_id: String,
    #[serde(rename = "PlatformType")]
    pub platform_type: i32,
    #[serde(rename = "PlatformUserId", skip_serializing_if = "Option::is_none")]
    pub platform_user_id: Option<String>,
    /// Client-claimed PlayFab ID; use [`super::ValidatedIdentity::playfab_id`]
    /// for the provider-verified value.
    #[serde(rename = "PlayFabId")]
    pub playfab_id: String,
    #[serde(rename = "PremiumSkin")]
    pub premium_skin: bool,
    #[serde(rename = "ProfileHash")]
    pub profile_hash: String,
    #[serde(rename = "SelfSignedId")]
    pub self_signed_id: String,
    /// Address the player typed (`host:port`, or a NetherNet URL form).
    #[serde(rename = "ServerAddress")]
    pub server_address: String,
    #[serde(rename = "SkinAnimationData")]
    pub skin_animation_data: String,
    #[serde(rename = "SkinColor")]
    pub skin_colour: String,
    /// Base64 RGBA pixels, exactly `SkinImageWidth * SkinImageHeight * 4` bytes.
    #[serde(rename = "SkinData")]
    pub skin_data: String,
    #[serde(rename = "SkinGeometryData")]
    pub skin_geometry: String,
    #[serde(rename = "SkinGeometryDataEngineVersion")]
    pub skin_geometry_version: String,
    #[serde(rename = "SkinId")]
    pub skin_id: String,
    #[serde(rename = "SkinImageHeight")]
    pub skin_image_height: u32,
    #[serde(rename = "SkinImageWidth")]
    pub skin_image_width: u32,
    #[serde(rename = "SkinResourcePatch")]
    pub skin_resource_patch: String,
    /// Not Xbox-protected; prefer the identity display name.
    #[serde(rename = "ThirdPartyName")]
    pub third_party_name: String,
    #[serde(rename = "ThirdPartyNameOnly", skip_serializing_if = "Option::is_none")]
    pub third_party_name_only: Option<bool>,
    #[serde(rename = "TrustedSkin")]
    pub trusted_skin: bool,
    #[serde(rename = "UIProfile")]
    pub ui_profile: i32,
}

/// gophertunnel `login.SkinAnimation`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SkinAnimation {
    #[serde(rename = "Frames")]
    pub frames: f64,
    #[serde(rename = "Image")]
    pub image: String,
    #[serde(rename = "ImageHeight")]
    pub image_height: u32,
    #[serde(rename = "ImageWidth")]
    pub image_width: u32,
    /// 0 none, 1 face, 2 32x32 body, 3 128x128 body.
    #[serde(rename = "Type")]
    pub animation_type: i32,
    #[serde(rename = "AnimationExpression")]
    pub animation_expression: i32,
}

/// gophertunnel `login.PersonaPiece`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PersonaPiece {
    #[serde(rename = "IsDefault")]
    pub default: bool,
    #[serde(rename = "PackId")]
    pub pack_id: String,
    #[serde(rename = "PieceId")]
    pub piece_id: String,
    #[serde(rename = "PieceType")]
    pub piece_type: String,
    #[serde(rename = "ProductId")]
    pub product_id: String,
}

/// gophertunnel `login.PersonaPieceTintColour`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PersonaPieceTintColour {
    /// ARGB hex colours, normally four.
    #[serde(rename = "Colors")]
    pub colours: Vec<String>,
    #[serde(rename = "PieceType")]
    pub piece_type: String,
}

fn invalid(rule: &'static str) -> AuthError {
    AuthError::InvalidClientData(rule)
}

impl ClientData {
    /// Mirrors gophertunnel's `Parse`: Mojang omits brackets around IPv6
    /// addresses entered without them, so `a:b::c:port` becomes `[a:b::c]:port`.
    pub(crate) fn normalize_server_address(&mut self) {
        let address = &self.server_address;
        if !address.contains("://") && address.matches(':').count() > 1 && !address.starts_with('[')
        {
            let split = address.rfind(':').expect("contains ':'");
            self.server_address = format!("[{}]{}", &address[..split], &address[split..]);
        }
    }

    /// gophertunnel `ClientData.Validate`, without DNS (see module docs).
    pub fn validate(&self) -> Result<(), AuthError> {
        if !(1..=15).contains(&self.device_os) {
            return Err(invalid("DeviceOS must be between 1 and 15"));
        }
        if self.game_version.is_empty()
            || !self
                .game_version
                .bytes()
                .all(|b| b.is_ascii_digit() || b == b'.')
        {
            return Err(invalid("GameVersion must only contain digits and dots"));
        }
        if !is_well_formed_language_tag(&self.language_code) {
            return Err(invalid("LanguageCode must be a BCP-47 language tag"));
        }
        if !self.platform_offline_id.is_empty()
            && uuid::Uuid::parse_str(&self.platform_offline_id).is_err()
        {
            return Err(invalid("PlatformOfflineId must be a UUID or empty"));
        }
        if !self.platform_online_id.is_empty() && self.platform_online_id.parse::<u64>().is_err() {
            return Err(invalid("PlatformOnlineId must be a uint64 or empty"));
        }
        if !self.self_signed_id.is_empty() && uuid::Uuid::parse_str(&self.self_signed_id).is_err() {
            return Err(invalid("SelfSignedId must be a UUID or empty"));
        }
        if !is_valid_server_address(&self.server_address) {
            return Err(invalid(
                "ServerAddress must be a host:port or NetherNet URL",
            ));
        }
        check_image(
            &self.skin_data,
            self.skin_image_width,
            self.skin_image_height,
            "SkinData does not match SkinImageWidth * SkinImageHeight * 4",
        )?;
        check_image(
            &self.cape_data,
            self.cape_image_width,
            self.cape_image_height,
            "CapeData does not match CapeImageWidth * CapeImageHeight * 4",
        )?;
        if !self
            .playfab_id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid("PlayFabId must be lower-case hex"));
        }
        for animation in &self.animated_image_data {
            check_image(
                &animation.image,
                animation.image_width,
                animation.image_height,
                "AnimatedImageData image does not match its dimensions",
            )?;
            if !(0..=3).contains(&animation.animation_type) {
                return Err(invalid("AnimatedImageData type must be between 0 and 3"));
            }
        }
        let geometry = STANDARD
            .decode(&self.skin_geometry)
            .map_err(|_| invalid("SkinGeometryData must be base64"))?;
        if !geometry.is_empty() && !is_json_object(&geometry) {
            return Err(invalid("SkinGeometryData must be a JSON object"));
        }
        let patch = STANDARD
            .decode(&self.skin_resource_patch)
            .map_err(|_| invalid("SkinResourcePatch must be base64"))?;
        if !is_json_object(&patch) {
            return Err(invalid("SkinResourcePatch must be a JSON object"));
        }
        if self.skin_id.is_empty() {
            return Err(invalid("SkinId must not be empty"));
        }
        if !(0..=2).contains(&self.ui_profile) {
            return Err(invalid("UIProfile must be between 0 and 2"));
        }
        Ok(())
    }
}

/// gophertunnel `base64DecLength`: standard padded base64 whose decoded size
/// is exactly `width * height * 4`.
fn check_image(data: &str, width: u32, height: u32, rule: &'static str) -> Result<(), AuthError> {
    // u32 * u32 * 4 overflows u64; such sizes can never match a login payload.
    let expected = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| invalid(rule))?;
    // Reject before decoding: padded base64 encodes 3 bytes per 4 characters.
    if (data.len() as u64).div_ceil(4) * 3 < expected {
        return Err(invalid(rule));
    }
    let decoded = STANDARD.decode(data).map_err(|_| invalid(rule))?;
    if decoded.len() as u64 != expected {
        return Err(invalid(rule));
    }
    Ok(())
}

/// Go `json.Unmarshal` into `map[string]any`: a JSON object, or `null`.
fn is_json_object(bytes: &[u8]) -> bool {
    let trimmed = bytes.trim_ascii();
    (trimmed.first() == Some(&b'{') || trimmed == b"null")
        && serde_json::from_slice::<serde::de::IgnoredAny>(bytes).is_ok()
}

/// RFC 5646 well-formedness (case-insensitive; `_` and `-` separate subtags,
/// as accepted by `golang.org/x/text/language.Parse`).
fn is_well_formed_language_tag(tag: &str) -> bool {
    fn alpha(s: &str) -> bool {
        s.bytes().all(|b| b.is_ascii_alphabetic())
    }
    fn digit(s: &str) -> bool {
        s.bytes().all(|b| b.is_ascii_digit())
    }
    fn alnum(s: &str) -> bool {
        s.bytes().all(|b| b.is_ascii_alphanumeric())
    }
    let lower = tag.to_ascii_lowercase();
    if lower.is_empty() || lower.len() > 64 {
        return false;
    }
    // Irregular grandfathered tags that x/text accepts.
    if matches!(lower.replace('_', "-").as_str(), "i-klingon" | "i-default") {
        return true;
    }
    let mut parts = lower.split(['-', '_']).peekable();
    let first = parts.next().unwrap_or_default();
    if first == "x" {
        return parts.peek().is_some() && parts.all(|p| (1..=8).contains(&p.len()) && alnum(p));
    }
    if !alpha(first) || !matches!(first.len(), 2..=3 | 5..=8) {
        return false;
    }
    // extlang: up to three 3-letter subtags after a 2-3 letter language.
    let mut extlangs = 0;
    while first.len() <= 3 && extlangs < 3 && parts.peek().is_some_and(|p| p.len() == 3 && alpha(p))
    {
        parts.next();
        extlangs += 1;
    }
    if parts.peek().is_some_and(|p| p.len() == 4 && alpha(p)) {
        parts.next();
    }
    if parts
        .peek()
        .is_some_and(|p| (p.len() == 2 && alpha(p)) || (p.len() == 3 && digit(p)))
    {
        parts.next();
    }
    while parts.peek().is_some_and(|p| {
        (5..=8).contains(&p.len()) && alnum(p)
            || (p.len() == 4 && p.as_bytes()[0].is_ascii_digit() && alnum(p))
    }) {
        parts.next();
    }
    // Extensions (singleton + 2..8 subtags) and a final private-use section.
    while let Some(singleton) = parts.next() {
        if singleton.len() != 1 || !alnum(singleton) {
            return false;
        }
        if singleton == "x" {
            return parts.peek().is_some() && parts.all(|p| (1..=8).contains(&p.len()) && alnum(p));
        }
        let mut subtags = 0;
        while parts
            .peek()
            .is_some_and(|p| (2..=8).contains(&p.len()) && alnum(p))
        {
            parts.next();
            subtags += 1;
        }
        if subtags == 0 {
            return false;
        }
    }
    true
}

/// Syntactic version of gophertunnel's `ServerAddress` checks. Never performs
/// DNS: hosts must be IP literals or LDH host names.
fn is_valid_server_address(address: &str) -> bool {
    if let Some((scheme, rest)) = address.split_once("://") {
        // NetherNet form: `<http|https>://<host>:<port>:<port>`.
        if scheme != "http" && scheme != "https" {
            return false;
        }
        let Some((url, tail)) = rest.rsplit_once(':') else {
            return false;
        };
        let authority = url.split(['/', '?', '#']).next().unwrap_or_default();
        return match split_host_port(authority) {
            Some((host, port)) => {
                !host.is_empty()
                    && !port.is_empty()
                    && port == tail
                    && is_valid_port(port)
                    && is_valid_host(host)
            }
            None => false,
        };
    }
    // Go `net.ResolveUDPAddr` accepts the empty string and an empty host or port.
    if address.is_empty() {
        return true;
    }
    match split_host_port(address) {
        Some((host, port)) => {
            (port.is_empty() || is_valid_port(port)) && (host.is_empty() || is_valid_host(host))
        }
        None => false,
    }
}

/// Go `net.SplitHostPort`.
fn split_host_port(address: &str) -> Option<(&str, &str)> {
    if let Some(rest) = address.strip_prefix('[') {
        let (host, after) = rest.split_once(']')?;
        let port = after.strip_prefix(':')?;
        if port.contains([':', '[', ']']) {
            return None;
        }
        return host
            .parse::<std::net::Ipv6Addr>()
            .ok()
            .map(|_| (host, port));
    }
    let (host, port) = address.rsplit_once(':')?;
    if host.contains([':', '[', ']']) || port.contains(['[', ']']) {
        return None;
    }
    Some((host, port))
}

fn is_valid_port(port: &str) -> bool {
    !port.is_empty()
        && port.len() <= 5
        && port.bytes().all(|b| b.is_ascii_digit())
        && port.parse::<u16>().is_ok()
}

fn is_valid_host(host: &str) -> bool {
    if host.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }
    let name = host.strip_suffix('.').unwrap_or(host);
    !name.is_empty()
        && name.len() <= 253
        && name.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn valid() -> ClientData {
        ClientData {
            device_os: 1,
            game_version: "1.26.51".into(),
            language_code: "en_US".into(),
            server_address: "127.0.0.1:19132".into(),
            skin_data: STANDARD.encode(vec![255u8; 64 * 64 * 4]),
            skin_image_width: 64,
            skin_image_height: 64,
            skin_resource_patch: STANDARD
                .encode(r#"{"geometry":{"default":"geometry.humanoid.custom"}}"#),
            skin_id: "skin".into(),
            ..Default::default()
        }
    }

    #[test]
    fn baseline_client_data_is_valid() {
        valid().validate().unwrap();
    }

    #[test]
    fn each_gophertunnel_rule_rejects_its_field() {
        type Mutation = fn(&mut ClientData);
        let cases: Vec<(&str, Mutation)> = vec![
            ("DeviceOS 0", |c| c.device_os = 0),
            ("DeviceOS 16", |c| c.device_os = 16),
            ("GameVersion letters", |c| c.game_version = "1.26a".into()),
            ("GameVersion empty", |c| c.game_version.clear()),
            ("LanguageCode empty", |c| c.language_code.clear()),
            ("LanguageCode malformed", |c| c.language_code = "e".into()),
            ("PlatformOfflineId", |c| {
                c.platform_offline_id = "nope".into()
            }),
            ("PlatformOnlineId", |c| c.platform_online_id = "-1".into()),
            ("SelfSignedId", |c| c.self_signed_id = "nope".into()),
            ("ServerAddress no port", |c| {
                c.server_address = "1.2.3.4".into()
            }),
            ("ServerAddress bad port", |c| {
                c.server_address = "1.2.3.4:99999".into()
            }),
            ("ServerAddress bad host", |c| {
                c.server_address = "a b:1".into()
            }),
            ("ServerAddress bad url", |c| {
                c.server_address = "ftp://h:1:1".into()
            }),
            ("SkinData size", |c| c.skin_image_width = 32),
            ("SkinData base64", |c| c.skin_data = "!!!!".into()),
            ("CapeData size", |c| {
                c.cape_image_width = 1;
                c.cape_image_height = 1;
            }),
            ("PlayFabId", |c| c.playfab_id = "ABC".into()),
            ("animation image", |c| {
                c.animated_image_data.push(SkinAnimation {
                    image_width: 1,
                    image_height: 1,
                    ..Default::default()
                })
            }),
            ("animation type", |c| {
                c.animated_image_data.push(SkinAnimation {
                    animation_type: 4,
                    ..Default::default()
                })
            }),
            ("SkinGeometryData base64", |c| c.skin_geometry = "%".into()),
            ("SkinGeometryData json", |c| {
                c.skin_geometry = STANDARD.encode("[1]")
            }),
            ("SkinResourcePatch empty", |c| c.skin_resource_patch.clear()),
            ("SkinResourcePatch json", |c| {
                c.skin_resource_patch = STANDARD.encode("{")
            }),
            ("SkinId", |c| c.skin_id.clear()),
            ("UIProfile", |c| c.ui_profile = 3),
        ];
        for (name, mutate) in cases {
            let mut data = valid();
            mutate(&mut data);
            assert!(
                matches!(data.validate(), Err(AuthError::InvalidClientData(_))),
                "{name} should be rejected"
            );
        }
    }

    #[test]
    fn huge_declared_dimensions_do_not_overflow_or_allocate() {
        let mut data = valid();
        data.skin_image_width = u32::MAX;
        data.skin_image_height = u32::MAX;
        assert!(data.validate().is_err());
    }

    #[test]
    fn server_address_forms_follow_go_without_dns() {
        for (address, ok) in [
            ("", true),
            (":19132", true),
            ("1.2.3.4:", true),
            ("1.2.3.4:19132", true),
            ("[::1]:19132", true),
            ("play.example.com:19132", true),
            ("localhost:19132", true),
            ("https://example.com:443:443", true),
            ("https://example.com:443:444", false),
            ("https://:443:443", false),
            ("1.2.3.4:http", false),
            ("host_name:1", false),
            ("[::1:19132", false),
        ] {
            assert_eq!(is_valid_server_address(address), ok, "{address}");
        }
        let mut data = valid();
        data.server_address = "2001:db8::1:19132".into();
        data.normalize_server_address();
        assert_eq!(data.server_address, "[2001:db8::1]:19132");
        data.validate().unwrap();
    }

    #[test]
    fn language_tags_follow_bcp47_syntax() {
        for (tag, ok) in [
            ("en_US", true),
            ("en-US", true),
            ("EN_us", true),
            ("zh_Hans_CN", true),
            ("en_US_POSIX", true),
            ("und", true),
            ("i-klingon", true),
            ("x-private", true),
            ("es_419", true),
            ("", false),
            ("e", false),
            ("toolongtag", false),
            ("en__US", false),
            ("en_US_", false),
        ] {
            assert_eq!(is_well_formed_language_tag(tag), ok, "{tag}");
        }
    }

    #[test]
    fn go_zero_values_apply_to_missing_fields() {
        let data: ClientData = serde_json::from_str(r#"{"DeviceOS":1,"Unknown":true}"#).unwrap();
        assert_eq!(data.device_os, 1);
        assert!(data.skin_id.is_empty());
        assert!(data.validate().is_err());
    }
}

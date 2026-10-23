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
//! - Every list is length-bounded while it is deserialized (`bounded`):
//!   unbounded lists of small structs let a compressed few-KB login expand to
//!   100+ MB of retained memory. The caps are far above vanilla clients.

use crate::error::AuthError;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Deserializer, Serialize};

/// Caps for client-data lists. Vanilla 1.26.51 persona logins carry a handful
/// of animations, ~10-20 persona pieces with as many tint entries, and four
/// colours per tint; the caps leave ample headroom.
const MAX_ANIMATIONS: usize = 32;
const MAX_PERSONA_PIECES: usize = 128;
const MAX_PIECE_TINTS: usize = 128;
const MAX_TINT_COLOURS: usize = 16;

/// Deserializes a JSON array into a `Vec`, failing as soon as it holds more
/// than `MAX` elements so oversized input is rejected before it is retained.
/// `null` yields an empty list, as Go `encoding/json` yields a nil slice.
fn bounded<'de, D, T, const MAX: usize>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct BoundedVisitor<T, const MAX: usize>(std::marker::PhantomData<T>);

    impl<'de, T: Deserialize<'de>, const MAX: usize> serde::de::Visitor<'de>
        for BoundedVisitor<T, MAX>
    {
        type Value = Vec<T>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "an array of at most {MAX} elements")
        }

        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<T>, A::Error> {
            let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(MAX));
            while let Some(item) = seq.next_element()? {
                if out.len() == MAX {
                    return Err(serde::de::Error::invalid_length(MAX + 1, &self));
                }
                out.push(item);
            }
            Ok(out)
        }

        fn visit_unit<E: serde::de::Error>(self) -> Result<Vec<T>, E> {
            Ok(Vec::new())
        }
    }

    deserializer.deserialize_any(BoundedVisitor::<T, MAX>(std::marker::PhantomData))
}

/// Login client data. Unknown JSON fields are ignored and missing fields take
/// their zero value, matching Go `encoding/json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClientData {
    /// Persona skin animations layered over the skin and cycled client-side.
    #[serde(
        rename = "AnimatedImageData",
        deserialize_with = "bounded::<_, _, MAX_ANIMATIONS>"
    )]
    pub animated_image_data: Vec<SkinAnimation>,
    /// Model arm width: `wide` or `slim`.
    #[serde(rename = "ArmSize")]
    pub arm_size: String,
    /// Base64 RGBA cape pixels, exactly `CapeImageWidth * CapeImageHeight * 4` bytes;
    /// usually empty.
    #[serde(rename = "CapeData")]
    pub cape_data: String,
    /// Identifier of the equipped cape, usually containing a UUID; empty for none.
    #[serde(rename = "CapeId")]
    pub cape_id: String,
    /// Cape image height in pixels.
    #[serde(rename = "CapeImageHeight")]
    pub cape_image_height: u32,
    /// Cape image width in pixels.
    #[serde(rename = "CapeImageWidth")]
    pub cape_image_width: u32,
    /// Whether the equipped cape belongs to a classic skin (typically a MineCon cape).
    #[serde(rename = "CapeOnClassicSkin")]
    pub cape_on_classic_skin: bool,
    /// Random client ID, usually stable across sessions and game restarts.
    #[serde(rename = "ClientRandomId")]
    pub client_random_id: i64,
    /// How the client intends to connect to an editor world.
    #[serde(rename = "ClientEditorConnectionIntent")]
    pub client_editor_connection_intent: i32,
    /// Whether the client supports editor features.
    #[serde(rename = "ClientIsEditorCapable")]
    pub client_is_editor_capable: bool,
    /// Whether the client hardware supports client-side chunk generation.
    #[serde(rename = "CompatibleWithClientSideChunkGen")]
    pub compatible_with_client_side_chunk_gen: bool,
    /// Input mode the client is currently using.
    #[serde(rename = "CurrentInputMode")]
    pub current_input_mode: i32,
    /// Default input mode of the device.
    #[serde(rename = "DefaultInputMode")]
    pub default_input_mode: i32,
    /// Device-specific ID; its format varies per OS and is not validated.
    #[serde(rename = "DeviceId")]
    pub device_id: String,
    /// Device model name as reported by the client.
    #[serde(rename = "DeviceModel")]
    pub device_model: String,
    /// `1..=15`: Android, iOS, OSX, FireOS, GearVR, Hololens, UWP, Win32,
    /// Dedicated, TVOS, Orbis, NX, Xbox, Windows Phone, Linux.
    #[serde(rename = "DeviceOS")]
    pub device_os: i32,
    /// Whether the client has profanity filtering enabled.
    #[serde(rename = "FilterProfanity")]
    pub filter_profanity: bool,
    /// Client game version such as `1.26.51`; digits and dots only.
    #[serde(rename = "GameVersion")]
    pub game_version: String,
    /// Graphics mode the client is running.
    #[serde(rename = "GraphicsMode")]
    pub graphics_mode: i32,
    /// GUI scale setting: 0 by default, -1 or -2 for a smaller GUI.
    #[serde(rename = "GuiScale")]
    pub gui_scale: i32,
    /// Whether the client is running in editor mode.
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
    /// Whether the player leads the party identified by `PartyId`.
    #[serde(rename = "IsPartyLeader")]
    pub party_leader: bool,
    /// Client language such as `en_US`; a BCP-47 tag that may use `_` separators.
    #[serde(rename = "LanguageCode")]
    pub language_code: String,
    /// Highest render distance, in chunks, that the client hardware can handle.
    #[serde(rename = "MaxViewDistance")]
    pub max_view_distance: i32,
    /// Device memory tier, 0 (super low, under ~1.5 GB) to 4 (super high, over ~8 GB).
    #[serde(rename = "MemoryTier")]
    pub memory_tier: i32,
    /// Hex nonce assigned by a peer-to-peer world host through its Xbox Live session;
    /// absent for other connections.
    #[serde(rename = "Nonce", skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    /// Client-sent skin override flag; its meaning is unknown.
    #[serde(rename = "OverrideSkin")]
    pub override_skin: bool,
    /// Identifier of the player's party; empty when not in a party.
    #[serde(rename = "PartyId")]
    pub party_id: String,
    /// Persona pieces the skin is composed of.
    #[serde(
        rename = "PersonaPieces",
        deserialize_with = "bounded::<_, _, MAX_PERSONA_PIECES>"
    )]
    pub persona_pieces: Vec<PersonaPiece>,
    /// Whether the skin is a persona skin made in the in-game character creator.
    #[serde(rename = "PersonaSkin")]
    pub persona_skin: bool,
    /// Tint colours for some of the pieces in `PersonaPieces`.
    #[serde(
        rename = "PieceTintColors",
        deserialize_with = "bounded::<_, _, MAX_PIECE_TINTS>"
    )]
    pub piece_tint_colours: Vec<PersonaPieceTintColour>,
    /// Platform offline ID: a UUID or empty.
    #[serde(rename = "PlatformOfflineId")]
    pub platform_offline_id: String,
    /// Platform online ID: a decimal `u64` or empty.
    #[serde(rename = "PlatformOnlineId")]
    pub platform_online_id: String,
    /// Type of platform the client runs on.
    #[serde(rename = "PlatformType")]
    pub platform_type: i32,
    /// UUID sent only by Xbox clients; absent otherwise.
    #[serde(rename = "PlatformUserId", skip_serializing_if = "Option::is_none")]
    pub platform_user_id: Option<String>,
    /// Client-claimed PlayFab ID; use [`super::ValidatedIdentity::playfab_id`]
    /// for the provider-verified value.
    #[serde(rename = "PlayFabId")]
    pub playfab_id: String,
    /// Whether the skin was purchased (premium content).
    #[serde(rename = "PremiumSkin")]
    pub premium_skin: bool,
    /// Client-generated hash of the equipped persona skin.
    #[serde(rename = "ProfileHash")]
    pub profile_hash: String,
    /// UUID that stays stable across game restarts and sessions: a UUID or empty.
    #[serde(rename = "SelfSignedId")]
    pub self_signed_id: String,
    /// Address the player typed (`host:port`, or a NetherNet URL form).
    #[serde(rename = "ServerAddress")]
    pub server_address: String,
    /// Skin animation data string; its contents are undocumented.
    #[serde(rename = "SkinAnimationData")]
    pub skin_animation_data: String,
    /// Base skin colour as `#rrggbb` hex.
    #[serde(rename = "SkinColor")]
    pub skin_colour: String,
    /// Base64 RGBA pixels, exactly `SkinImageWidth * SkinImageHeight * 4` bytes.
    #[serde(rename = "SkinData")]
    pub skin_data: String,
    /// Base64 JSON skin geometry (bones, UVs, pivots); empty or a JSON object.
    #[serde(rename = "SkinGeometryData")]
    pub skin_geometry: String,
    /// Engine version of the skin geometry.
    #[serde(rename = "SkinGeometryDataEngineVersion")]
    pub skin_geometry_version: String,
    /// Unique skin ID, e.g. `<uuid>_Alex` for the default Alex skin; must be non-empty.
    #[serde(rename = "SkinId")]
    pub skin_id: String,
    /// Skin image height in pixels.
    #[serde(rename = "SkinImageHeight")]
    pub skin_image_height: u32,
    /// Skin image width in pixels.
    #[serde(rename = "SkinImageWidth")]
    pub skin_image_width: u32,
    /// Base64 JSON object naming the geometry the skin uses (e.g. `geometry.default`).
    #[serde(rename = "SkinResourcePatch")]
    pub skin_resource_patch: String,
    /// Not Xbox-protected; prefer the identity display name.
    #[serde(rename = "ThirdPartyName")]
    pub third_party_name: String,
    /// Whether the user has only a third-party name; must not be trusted.
    #[serde(rename = "ThirdPartyNameOnly", skip_serializing_if = "Option::is_none")]
    pub third_party_name_only: Option<bool>,
    /// Whether the client reports the skin as trusted.
    #[serde(rename = "TrustedSkin")]
    pub trusted_skin: bool,
    /// UI profile: 0 Classic, 1 Pocket; [`ClientData::validate`] accepts `0..=2`.
    #[serde(rename = "UIProfile")]
    pub ui_profile: i32,
}

/// gophertunnel `login.SkinAnimation`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SkinAnimation {
    /// Number of animation frames contained in `Image`.
    #[serde(rename = "Frames")]
    pub frames: f64,
    /// Base64 RGBA pixels, exactly `ImageWidth * ImageHeight * 4` bytes, holding all
    /// frames.
    #[serde(rename = "Image")]
    pub image: String,
    /// Animation image height in pixels.
    #[serde(rename = "ImageHeight")]
    pub image_height: u32,
    /// Animation image width in pixels.
    #[serde(rename = "ImageWidth")]
    pub image_width: u32,
    /// 0 none, 1 face, 2 32x32 body, 3 128x128 body.
    #[serde(rename = "Type")]
    pub animation_type: i32,
    /// Expression type: 0 linear, 1 blinking.
    #[serde(rename = "AnimationExpression")]
    pub animation_expression: i32,
}

/// gophertunnel `login.PersonaPiece`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PersonaPiece {
    /// Whether the piece is one of the default Steve/Alex pieces.
    #[serde(rename = "IsDefault")]
    pub default: bool,
    /// UUID of the pack the piece belongs to.
    #[serde(rename = "PackId")]
    pub pack_id: String,
    /// UUID of the piece itself.
    #[serde(rename = "PieceId")]
    pub piece_id: String,
    /// Piece type such as `persona_body`, `persona_hair` or `persona_eyes`.
    #[serde(rename = "PieceType")]
    pub piece_type: String,
    /// Purchase UUID of the piece; empty for default pieces.
    #[serde(rename = "ProductId")]
    pub product_id: String,
}

/// gophertunnel `login.PersonaPieceTintColour`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PersonaPieceTintColour {
    /// ARGB hex colours, normally four.
    #[serde(
        rename = "Colors",
        deserialize_with = "bounded::<_, _, MAX_TINT_COLOURS>"
    )]
    pub colours: Vec<String>,
    /// Persona piece type these colours tint; it appears in `PersonaPieces`.
    #[serde(rename = "PieceType")]
    pub piece_type: String,
}

fn invalid(rule: &'static str) -> AuthError {
    AuthError::InvalidClientData(rule)
}

impl ClientData {
    /// Mirrors gophertunnel's `Parse`: Mojang omits brackets around IPv6
    /// addresses entered without them, so `a:b::c:port` becomes `[a:b::c]:port`.
    #[cfg(feature = "server")]
    pub(crate) fn normalize_server_address(&mut self) {
        let address = &self.server_address;
        if !address.contains("://")
            && address.matches(':').count() > 1
            && !address.starts_with('[')
            && let Some(split) = address.rfind(':')
        {
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
    }

    #[cfg(feature = "server")]
    #[test]
    fn unbracketed_ipv6_server_address_is_normalized() {
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

    fn with_list(field: &str, item: &str, count: usize) -> String {
        let items = vec![item; count].join(",");
        format!(r#"{{"DeviceOS":1,"{field}":[{items}]}}"#)
    }

    #[test]
    fn oversized_lists_are_rejected_while_parsing() {
        for (field, item, max) in [
            ("AnimatedImageData", "{}", MAX_ANIMATIONS),
            ("PersonaPieces", "{}", MAX_PERSONA_PIECES),
            ("PieceTintColors", "{}", MAX_PIECE_TINTS),
        ] {
            let at_cap: ClientData = serde_json::from_str(&with_list(field, item, max)).unwrap();
            let _ = at_cap;
            assert!(
                serde_json::from_str::<ClientData>(&with_list(field, item, max + 1)).is_err(),
                "{field} accepted {} entries",
                max + 1
            );
        }
        // The attack shape from the review: ~1M empty persona pieces in a few
        // KB of compressed JSON must fail without materialising the list.
        let flood = with_list("PersonaPieces", "{}", 1_000_000);
        assert!(serde_json::from_str::<ClientData>(&flood).is_err());

        let colours = format!(
            r#"{{"PieceTintColors":[{{"Colors":[{}]}}]}}"#,
            vec![r##""#ffffffff""##; MAX_TINT_COLOURS + 1].join(",")
        );
        assert!(serde_json::from_str::<ClientData>(&colours).is_err());
    }

    #[test]
    fn null_lists_decode_as_empty_like_go() {
        let data: ClientData = serde_json::from_str(
            r#"{"AnimatedImageData":null,"PersonaPieces":null,"PieceTintColors":[{"Colors":null}]}"#,
        )
        .unwrap();
        assert!(data.animated_image_data.is_empty());
        assert!(data.persona_pieces.is_empty());
        assert!(data.piece_tint_colours[0].colours.is_empty());
    }
}

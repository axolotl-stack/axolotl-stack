//! The serialized skin BDS 1.26.51 forwards in `PlayerList` for a login.
//!
//! Measured with `tools/bds-sweep/protocol_probe.py` (`relay`, `skins`;
//! fixtures under `crates/bds-extractor/captures/1.26.51/protocol/`), every
//! login self-signed:
//!
//! - ids, images, animations, cape, resource patch, arm size (`slim`, else
//!   wide), premium/persona/cape-on-classic flags and the profile hash are
//!   echoed; the full id is the skin id followed by the cape id;
//! - the PlayFab id is always empty and the trusted flag the string `false`,
//!   whatever the login claims;
//! - the geometry engine version and the skin animation data are base64 in
//!   the login and forwarded decoded; animation data that is not base64
//!   becomes empty, an empty engine version `0.0.0`;
//! - the geometry is parsed and written again ([`geometry_json`]): keys
//!   sorted, no whitespace, floats in a fixed `printf` style, a newline at
//!   the end; an empty geometry becomes `null` and a newline;
//! - persona piece types and tint types are the login's names mapped to the
//!   protocol enum ([`piece_type`]); tint keys are the enum's short names
//!   ([`tint_key`]); colours are `#`-prefixed hex read as a 32-bit integer
//!   (`#7f4f3f` stays without alpha, an unparsable colour is 0).
//!
//! A login naming a piece type BDS does not know is refused before this
//! point (`jolyne::auth::ClientData::validate`).

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jolyne::auth::ClientData;
use jolyne::auth::client_data::PersonaPiece;
use jolyne::valentine::types::{
    AnimatedImageData, EnumsSharedTypespersonaArmSizeType, EnumsSharedTypespersonaPieceType,
    EnumspersonaAnimatedTextureType, EnumspersonaAnimationExpression, MceColor,
    SerializedPersonaPieceHandle, SerializedSkinRef, SerializedSkinRefPieceTintColorsItem,
    SkinImage, TintMapColor,
};

fn base64(value: &str) -> Vec<u8> {
    // The login validator has already checked every image and JSON field.
    STANDARD.decode(value).unwrap_or_default()
}

/// A base64 text field, decoded; empty when it is not base64 text.
fn base64_text(value: &str) -> String {
    STANDARD
        .decode(value)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_default()
}

fn image(data: &str, width: u32, height: u32) -> SkinImage {
    SkinImage {
        width,
        height,
        image_bytes: base64(data),
    }
}

/// `#`-prefixed hex as a 32-bit integer, as BDS reads skin and tint colours:
/// `#7f4f3f` is `0x7f4f3f` (no alpha added), `#ffb37b62` is `0xffb37b62`.
/// Anything else is 0.
pub fn colour(value: &str) -> i32 {
    value
        .strip_prefix('#')
        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
        .map_or(0, |colour| colour as i32)
}

/// The login names of persona piece types, the protocol enum value and the
/// tint key BDS forwards for each, in enum order. `persona_hands` is not a
/// name BDS accepts, although the enum has a hands value.
const PIECE_TYPES: [(&str, EnumsSharedTypespersonaPieceType, &str); 26] = {
    use EnumsSharedTypespersonaPieceType as P;
    [
        ("persona_skeleton", P::Skeleton, "skeleton"),
        ("persona_body", P::Body, "body"),
        ("persona_skin", P::Skin, "skin"),
        ("persona_bottom", P::Bottom, "bottom"),
        ("persona_feet", P::Feet, "feet"),
        ("persona_dress", P::Dress, "dress"),
        ("persona_top", P::Top, "top"),
        ("persona_high_pants", P::HighPants, "high_pants"),
        ("persona_outerwear", P::Outerwear, "outerwear"),
        ("persona_facial_hair", P::Facialhair, "facialhair"),
        ("persona_mouth", P::Mouth, "mouth"),
        ("persona_eyes", P::Eyes, "eyes"),
        ("persona_hair", P::Hair, "hair"),
        ("persona_hood", P::Hood, "hood"),
        ("persona_back", P::Back, "back"),
        ("persona_face_accessory", P::Faceaccessory, "faceaccessory"),
        ("persona_head", P::Head, "head"),
        ("persona_legs", P::Legs, "legs"),
        ("persona_left_leg", P::Leftleg, "leftleg"),
        ("persona_right_leg", P::Rightleg, "rightleg"),
        ("persona_arms", P::Arms, "arms"),
        ("persona_left_arm", P::Leftarm, "leftarm"),
        ("persona_right_arm", P::Rightarm, "rightarm"),
        ("persona_capes", P::Capes, "capes"),
        ("persona_classic_skin", P::Classicskin, "classicskin"),
        ("persona_emote", P::Emote, "emote"),
    ]
};

/// The protocol piece type of a login piece type name.
pub fn piece_type(name: &str) -> Option<EnumsSharedTypespersonaPieceType> {
    PIECE_TYPES
        .iter()
        .find(|(login, _, _)| *login == name)
        .map(|(_, piece, _)| *piece)
}

/// The tint key BDS forwards for a login piece type name.
pub fn tint_key(name: &str) -> Option<&'static str> {
    PIECE_TYPES
        .iter()
        .find(|(login, _, _)| *login == name)
        .map(|(_, _, key)| *key)
}

fn piece(piece: &PersonaPiece) -> SerializedPersonaPieceHandle {
    SerializedPersonaPieceHandle {
        piece_id: piece.piece_id.as_str().into(),
        // Validated at login: unknown names refuse the login.
        piece_type: piece_type(&piece.piece_type)
            .unwrap_or(EnumsSharedTypespersonaPieceType::Unknown),
        // [INFERENCE] Every probe pack id was a UUID; a pack id that is not
        // one is unmeasured and sent as the nil UUID.
        pack_id: piece.pack_id.parse().unwrap_or_default(),
        is_default_piece: piece.default,
        product_id: piece.product_id.as_str().into(),
    }
}

/// The serialized skin BDS forwards for a login (module docs).
pub fn skin(client: &ClientData) -> SerializedSkinRef {
    let geometry = String::from_utf8(base64(&client.skin_geometry)).unwrap_or_default();
    let geometry_version = base64_text(&client.skin_geometry_version);
    SerializedSkinRef {
        id: client.skin_id.as_str().into(),
        play_fab_id: "".into(),
        resource_patch: String::from_utf8(base64(&client.skin_resource_patch))
            .unwrap_or_default()
            .into(),
        image_data: image(
            &client.skin_data,
            client.skin_image_width,
            client.skin_image_height,
        ),
        animated_image_data: client
            .animated_image_data
            .iter()
            .map(|animation| AnimatedImageData {
                skin_image: image(
                    &animation.image,
                    animation.image_width,
                    animation.image_height,
                ),
                animated_texture_type: match animation.animation_type {
                    1 => EnumspersonaAnimatedTextureType::Face,
                    2 => EnumspersonaAnimatedTextureType::Body32X32,
                    3 => EnumspersonaAnimatedTextureType::Body128X128,
                    _ => EnumspersonaAnimatedTextureType::None,
                },
                frames: animation.frames as f32,
                animation_expression: match animation.animation_expression {
                    1 => EnumspersonaAnimationExpression::Blinking,
                    _ => EnumspersonaAnimationExpression::Linear,
                },
            })
            .collect(),
        cape_image_data: image(
            &client.cape_data,
            client.cape_image_width,
            client.cape_image_height,
        ),
        geometry_data: geometry_json(&geometry).into(),
        geometry_data_min_engine_version: if geometry_version.is_empty() {
            "0.0.0".into()
        } else {
            geometry_version.into()
        },
        animation_data: base64_text(&client.skin_animation_data).into(),
        cape_id: client.cape_id.as_str().into(),
        full_id: format!("{}{}", client.skin_id, client.cape_id).into(),
        arm_size: if client.arm_size == "slim" {
            EnumsSharedTypespersonaArmSizeType::Slim
        } else {
            EnumsSharedTypespersonaArmSizeType::Wide
        },
        skin_color: MceColor {
            color: colour(&client.skin_colour),
        },
        persona_pieces: client.persona_pieces.iter().map(piece).collect(),
        piece_tint_colors: client
            .piece_tint_colours
            .iter()
            .map(|tint| {
                let mut colors: [MceColor; 4] = Default::default();
                for (slot, value) in colors.iter_mut().zip(&tint.colours) {
                    slot.color = colour(value);
                }
                SerializedSkinRefPieceTintColorsItem {
                    key: tint_key(&tint.piece_type).unwrap_or_default().into(),
                    value: TintMapColor { colors },
                }
            })
            .collect(),
        is_premium: client.premium_skin,
        is_persona: client.persona_skin,
        is_persona_cape_on_classic_skin: client.cape_on_classic_skin,
        is_primary_user: false,
        overrides_player_appearance: client.override_skin,
        trusted_skin_flag: "false".into(),
        profile_hash: client.profile_hash.as_str().into(),
    }
}

/// The geometry JSON as BDS forwards it: parsed and written again with
/// sorted keys, no whitespace, integers as written and other numbers in
/// [`float_text`] style (out-of-range numbers become `inf`), followed by a
/// newline. An empty geometry is `null`.
/// [INFERENCE] Text that does not parse is forwarded as `null` too; the
/// login validator only admits JSON objects.
pub fn geometry_json(geometry: &str) -> String {
    let value = if geometry.trim().is_empty() {
        Json::Null
    } else {
        let mut parser = JsonParser {
            text: geometry.as_bytes(),
            at: 0,
        };
        parser.document().unwrap_or(Json::Null)
    };
    let mut out = String::new();
    value.write(&mut out);
    out.push('\n');
    out
}

/// A parsed JSON value that keeps each number's text, so integers of any
/// size and out-of-range floats are written the way BDS writes them.
enum Json {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            Json::Number(text) => out.push_str(&number_text(text)),
            // [INFERENCE] String escaping is not measured; identifiers are ASCII.
            Json::String(text) => out.push_str(&serde_json::Value::from(text.as_str()).to_string()),
            Json::Array(items) => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Json::Object(entries) => {
                let mut sorted: Vec<&(String, Json)> = entries.iter().collect();
                // Sorted by key. [INFERENCE] A later duplicate key replaces an
                // earlier one (not measured).
                sorted.reverse();
                sorted.sort_by(|a, b| a.0.cmp(&b.0));
                sorted.dedup_by(|a, b| a.0 == b.0);
                out.push('{');
                for (index, (key, item)) in sorted.into_iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::Value::from(key.as_str()).to_string());
                    out.push(':');
                    item.write(out);
                }
                out.push('}');
            }
        }
    }
}

/// A JSON number as BDS writes it: an integer that fits 64 bits (signed or
/// unsigned) as written, anything else as a double in [`float_text`] style.
fn number_text(text: &str) -> String {
    if !text.contains(['.', 'e', 'E'])
        && (text.parse::<i64>().is_ok() || text.parse::<u64>().is_ok())
    {
        return text.to_owned();
    }
    float_text(text.parse::<f64>().unwrap_or(f64::NAN))
}

/// The JSON number grammar: `-? (0 | [1-9][0-9]*) (. [0-9]+)? ([eE] [+-]? [0-9]+)?`.
fn is_json_number(text: &str) -> bool {
    let digits = |s: &str| -> usize { s.bytes().take_while(u8::is_ascii_digit).count() };
    let mut rest = text.strip_prefix('-').unwrap_or(text);
    match digits(rest) {
        0 => return false,
        count if count > 1 && rest.starts_with('0') => return false,
        count => rest = &rest[count..],
    }
    if let Some(fraction) = rest.strip_prefix('.') {
        let count = digits(fraction);
        if count == 0 {
            return false;
        }
        rest = &fraction[count..];
    }
    if let Some(exponent) = rest.strip_prefix(['e', 'E']) {
        let exponent = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
        let count = digits(exponent);
        if count == 0 {
            return false;
        }
        rest = &exponent[count..];
    }
    rest.is_empty()
}

struct JsonParser<'a> {
    text: &'a [u8],
    at: usize,
}

impl JsonParser<'_> {
    fn document(&mut self) -> Option<Json> {
        let value = self.value()?;
        self.space();
        (self.at == self.text.len()).then_some(value)
    }

    fn space(&mut self) {
        while self
            .text
            .get(self.at)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
        {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> bool {
        self.space();
        let found = self.text.get(self.at) == Some(&byte);
        if found {
            self.at += 1;
        }
        found
    }

    fn literal(&mut self, word: &str, value: Json) -> Option<Json> {
        self.text[self.at..].starts_with(word.as_bytes()).then(|| {
            self.at += word.len();
            value
        })
    }

    fn value(&mut self) -> Option<Json> {
        self.space();
        match *self.text.get(self.at)? {
            b'n' => self.literal("null", Json::Null),
            b't' => self.literal("true", Json::Bool(true)),
            b'f' => self.literal("false", Json::Bool(false)),
            b'"' => self.string().map(Json::String),
            b'[' => {
                self.at += 1;
                let mut items = Vec::new();
                if self.eat(b']') {
                    return Some(Json::Array(items));
                }
                loop {
                    items.push(self.value()?);
                    if self.eat(b']') {
                        return Some(Json::Array(items));
                    }
                    if !self.eat(b',') {
                        return None;
                    }
                }
            }
            b'{' => {
                self.at += 1;
                let mut entries = Vec::new();
                if self.eat(b'}') {
                    return Some(Json::Object(entries));
                }
                loop {
                    self.space();
                    let key = self.string()?;
                    if !self.eat(b':') {
                        return None;
                    }
                    entries.push((key, self.value()?));
                    if self.eat(b'}') {
                        return Some(Json::Object(entries));
                    }
                    if !self.eat(b',') {
                        return None;
                    }
                }
            }
            b'-' | b'0'..=b'9' => {
                let start = self.at;
                while self.text.get(self.at).is_some_and(|byte| {
                    matches!(byte, b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                }) {
                    self.at += 1;
                }
                let number = std::str::from_utf8(&self.text[start..self.at]).ok()?;
                is_json_number(number).then(|| Json::Number(number.to_owned()))
            }
            _ => None,
        }
    }

    /// A string literal at the cursor, decoded (serde_json decodes escapes).
    fn string(&mut self) -> Option<String> {
        if self.text.get(self.at) != Some(&b'"') {
            return None;
        }
        let start = self.at;
        self.at += 1;
        loop {
            match *self.text.get(self.at)? {
                b'\\' => self.at += 2,
                b'"' => break,
                _ => self.at += 1,
            }
        }
        self.at += 1;
        serde_json::from_slice(&self.text[start..self.at]).ok()
    }
}

/// A non-integer JSON number as BDS writes it (the 2026-11-11 `numbers`
/// skin case): C `%.16g` decides the digits; when that has no exponent, the
/// number is written with `%#.{n}g`, `n` being one more than its significant
/// digits (at most 16), so `0.1` is `0.10`, `100.0` is `100.0` and `1e15`
/// is `1000000000000000.`; with an exponent it is `%#.16g`
/// (`1.000000000000000e-05`).
pub fn float_text(value: f64) -> String {
    if value.is_infinite() {
        return if value > 0.0 { "inf" } else { "-inf" }.into();
    }
    if value.is_nan() {
        return "nan".into();
    }
    let shortest = printf_g(value, 16, false);
    if shortest.contains('e') {
        return printf_g(value, 16, true);
    }
    let significant = shortest
        .bytes()
        .filter(u8::is_ascii_digit)
        .skip_while(|digit| *digit == b'0')
        .count()
        .max(1);
    printf_g(value, (significant + 1).min(16), true)
}

/// C `printf("%.{precision}g")` (`alternate`: `%#.{precision}g`, keeping
/// trailing zeros and the decimal point) for a finite value.
pub fn printf_g(value: f64, precision: usize, alternate: bool) -> String {
    let precision = precision.max(1);
    // Scientific form with the requested significant digits decides the
    // exponent after rounding, as C does.
    let scientific = format!("{:.*e}", precision - 1, value);
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("exponent formatting always has an exponent");
    let exponent: i32 = exponent.parse().expect("an integer exponent");
    if exponent < -4 || exponent >= precision as i32 {
        let sign = if exponent < 0 { '-' } else { '+' };
        let mut mantissa = mantissa.to_owned();
        if !alternate {
            mantissa = strip_zeros(&mantissa);
        } else if !mantissa.contains('.') {
            mantissa.push('.');
        }
        format!("{mantissa}e{sign}{:02}", exponent.abs())
    } else {
        let decimals = (precision as i32 - 1 - exponent).max(0) as usize;
        let mut fixed = format!("{value:.decimals$}");
        if alternate {
            if !fixed.contains('.') {
                fixed.push('.');
            }
        } else {
            fixed = strip_zeros(&fixed);
        }
        fixed
    }
}

fn strip_zeros(text: &str) -> String {
    if !text.contains('.') {
        return text.to_owned();
    }
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printf_g_matches_c() {
        assert_eq!(printf_g(0.1, 16, false), "0.1");
        assert_eq!(printf_g(1e15, 16, false), "1000000000000000");
        assert_eq!(printf_g(1e16, 16, false), "1e+16");
        assert_eq!(printf_g(1e-5, 16, false), "1e-05");
        assert_eq!(printf_g(0.0001, 16, false), "0.0001");
        assert_eq!(printf_g(0.1, 2, true), "0.10");
        assert_eq!(printf_g(100.0, 4, true), "100.0");
        assert_eq!(printf_g(-0.0, 2, true), "-0.0");
        assert_eq!(printf_g(9.96, 2, true), "10.");
    }

    /// Every number of the `floats` and `numbers` skin cases, as BDS wrote it.
    #[test]
    #[allow(clippy::approx_constant)] // 3.14159 is the probe's literal, not pi
    fn floats_are_written_like_bds() {
        for (value, bds) in [
            (0.1, "0.10"),
            (0.5, "0.50"),
            (24.125, "24.1250"),
            (-0.0, "-0.0"),
            (1e-7, "1.000000000000000e-07"),
            (100.0, "100.0"),
            (3.14159, "3.141590"),
            (1.5, "1.50"),
            (2.25, "2.250"),
            (0.75, "0.750"),
            (-4.0, "-4.0"),
            (0.3, "0.30"),
            (0.333_333_333_333_333_3, "0.3333333333333333"),
            (0.0001, "0.00010"),
            (0.00001, "1.000000000000000e-05"),
            (0.000123, "0.0001230"),
            (2.5e-5, "2.500000000000000e-05"),
            (0.001, "0.0010"),
            (123_456_789.5, "123456789.50"),
            (1e15, "1000000000000000."),
            (1e16, "1.000000000000000e+16"),
            (1e17, "1.000000000000000e+17"),
            (1e20, "1.000000000000000e+20"),
            (1.5e300, "1.500000000000000e+300"),
            (-2.5, "-2.50"),
            (-0.001, "-0.0010"),
            (65536.0, "65536.0"),
            (4_294_967_296.0, "4294967296.0"),
            (1e21, "1.000000000000000e+21"),
            (1.000_000_000_000_000_2, "1.0"),
            (f64::INFINITY, "inf"),
        ] {
            assert_eq!(float_text(value), bds, "{value:e}");
        }
    }

    #[test]
    fn geometry_is_rewritten_with_sorted_keys() {
        assert_eq!(geometry_json(""), "null\n");
        assert_eq!(
            geometry_json(
                r#"{"b":[1,2.5,-9223372036854775808,18446744073709551615],"a":{"z":true,"y":null,"x":"s"}}"#
            ),
            "{\"a\":{\"x\":\"s\",\"y\":null,\"z\":true},\"b\":[1,2.50,-9223372036854775808,18446744073709551615]}\n"
        );
    }

    #[test]
    fn colours_and_tint_keys_follow_bds() {
        assert_eq!(colour("#7f4f3f"), 0x7f4f3f);
        assert_eq!(colour("#ffb37b62"), 0xffb37b62_u32 as i32);
        assert_eq!(colour("#0"), 0);
        assert_eq!(colour("not-a-colour"), 0);
        assert_eq!(tint_key("persona_high_pants"), Some("high_pants"));
        assert_eq!(tint_key("persona_left_arm"), Some("leftarm"));
        assert!(piece_type("persona_hands").is_none());
        // The names jolyne's login validation accepts are exactly the mapped ones.
        let names: Vec<&str> = PIECE_TYPES.iter().map(|(name, _, _)| *name).collect();
        assert_eq!(names, jolyne::auth::client_data::PERSONA_PIECE_TYPES);
    }
}

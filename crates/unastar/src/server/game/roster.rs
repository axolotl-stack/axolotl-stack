//! The player roster as BDS 1.26.51 announces it.
//!
//! Measured with `tools/bds-sweep/capture.py --pair` (fixtures under
//! `crates/bds-extractor/captures/1.26.51/roster/`):
//!
//! - A joining player gets the sleeping-player `LevelEventGeneric` and a
//!   `PlayerList` holding only itself first, and later in the join a
//!   `PlayerList` holding every online player, itself included.
//! - Every player already online gets, in this order, the sleeping-player
//!   event with the new count, a `PlayerList` adding the newcomer and an
//!   `UpdateAdventureSettings` (the same settings the join sends), all before
//!   the newcomer's `AddPlayer`.
//! - When a player leaves, everyone else gets the sleeping-player event with
//!   the new count and a `PlayerList` removing it.
//! - A player's colour is the entry of [`PLAYER_COLOURS`] the fewest online
//!   players hold, the earliest one on a tie; it is kept until the player
//!   leaves. Up to 13 players that is the first free colour; the 14th player
//!   starts over at the first.
//!
//! The order of a full `PlayerList` is not stable in BDS (two joins with the
//! same players online listed them differently); unastar lists players in
//! join order.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use bevy_ecs::prelude::{Component, Entity, World};
use bytes::{BufMut, Bytes, BytesMut};
use jolyne::auth::ClientData;
use jolyne::valentine::types::{
    ActorUniqueId, AdventureSettings, AnimatedImageData, EnumsBuildPlatform,
    EnumsPlayerListPacketType, EnumsSharedTypespersonaArmSizeType, EnumspersonaAnimatedTextureType,
    EnumspersonaAnimationExpression, MceColor, PlayerListPacketEntriesItem,
    PlayerListPacketPayloadAddEntry, PlayerListPacketPayloadAddEntryAction,
    PlayerListPacketPayloadRemoveEntry, SerializedSkinRef, SkinImage,
};
use jolyne::valentine::{LevelEventGenericPacket, PlayerListPacket, UpdateAdventureSettingsPacket};
use uuid::Uuid;
use valentine::bedrock::codec::{BedrockCodec, NbtCompoundBody};

/// Player colours (ARGB) in the order BDS 1.26.51 hands them out (21 players
/// online at once in the `twenty-one-players` roster fixture).
pub const PLAYER_COLOURS: [u32; 13] = [
    0xFFED_EDED,
    0xFF54_FFFF,
    0xFFFF_A900,
    0xFFBD_EC9F,
    0xFFFD_FDAE,
    0xFFFF_54FF,
    0xFFA9_A9A9,
    0xFFA6_C3FF,
    0xFF54_FF54,
    0xFFE6_7B78,
    0xFF81_A5FF,
    0xFFFF_FF54,
    0xFF19_BD9F,
];

/// `LevelEventGeneric` id of the sleeping-player count.
pub const LEVEL_EVENT_SLEEPING_PLAYERS: i32 = 9801;

/// A player's `PlayerList` entry, built once at join from its login.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct RosterEntry(pub Box<PlayerListPacketPayloadAddEntry>);

impl RosterEntry {
    /// The entry of a player from its identity and login client data.
    pub fn new(
        uuid: Uuid,
        unique_id: i64,
        name: &str,
        xuid: Option<&str>,
        client: &ClientData,
        colour: u32,
    ) -> Self {
        Self(Box::new(PlayerListPacketPayloadAddEntry {
            action: PlayerListPacketPayloadAddEntryAction::Add,
            uuid,
            actor_unique_id: ActorUniqueId {
                actor_unique_id: unique_id,
            },
            player_name: name.into(),
            xblxuid: xuid.unwrap_or_default().into(),
            platform_online_id: client.platform_online_id.as_str().into(),
            build_platform: build_platform(client.device_os),
            serialized_skin: skin(client),
            is_teacher: false,
            is_host: false,
            is_sub_client: false,
            player_color: MceColor {
                color: colour as i32,
            },
        }))
    }

    pub fn uuid(&self) -> Uuid {
        self.0.uuid
    }

    pub fn unique_id(&self) -> i64 {
        self.0.actor_unique_id.actor_unique_id
    }

    pub fn colour(&self) -> u32 {
        self.0.player_color.color as u32
    }
}

/// The colour a joining player gets while the online players hold `taken`:
/// the least-held colour, the earliest in [`PLAYER_COLOURS`] on a tie.
pub fn free_colour(taken: impl IntoIterator<Item = u32>) -> u32 {
    let taken: Vec<u32> = taken.into_iter().collect();
    PLAYER_COLOURS
        .into_iter()
        .min_by_key(|colour| taken.iter().filter(|held| *held == colour).count())
        .expect("the palette is not empty")
}

/// Every online player with its entry, in join order (unique ids are
/// handed out in session order).
pub fn online(world: &World) -> Vec<(Entity, &RosterEntry)> {
    let Some(mut query) = world.try_query::<(Entity, &RosterEntry)>() else {
        return Vec::new();
    };
    let mut online: Vec<(Entity, &RosterEntry)> = query.iter(world).collect();
    online.sort_by_key(|(entity, entry)| (entry.unique_id(), *entity));
    online
}

/// A `PlayerList` adding `entries`, in the given order.
pub fn add_packet<'a>(entries: impl IntoIterator<Item = &'a RosterEntry>) -> PlayerListPacket {
    PlayerListPacket {
        entries: entries
            .into_iter()
            .map(|entry| PlayerListPacketEntriesItem::AddEntry(entry.0.clone()))
            .collect(),
    }
}

/// A `PlayerList` removing the player `uuid`.
pub fn remove_packet(uuid: Uuid) -> PlayerListPacket {
    PlayerListPacket {
        entries: vec![PlayerListPacketEntriesItem::RemoveEntry(
            PlayerListPacketPayloadRemoveEntry {
                action: EnumsPlayerListPacketType::Remove,
                uuid,
            },
        )],
    }
}

/// The sleeping-player event: how many players are in the Overworld and how
/// many of them sleep. `ableToSleep` is 1 in every BDS capture, day or night.
pub fn sleeping_players(overworld_players: i32, sleeping_players: i32) -> LevelEventGenericPacket {
    let mut body = BytesMut::new();
    // Network NBT compound body: int tags in name order, then TAG_End.
    for (name, value) in [
        ("ableToSleep", 1),
        ("overworldPlayerCount", overworld_players),
        ("sleepingPlayerCount", sleeping_players),
    ] {
        body.put_u8(3);
        valentine::bedrock::codec::VarUInt(name.len() as u32)
            .encode(&mut body)
            .expect("writing to a buffer cannot fail");
        body.put_slice(name.as_bytes());
        valentine::bedrock::codec::ZigZag32(value)
            .encode(&mut body)
            .expect("writing to a buffer cannot fail");
    }
    body.put_u8(0);
    LevelEventGenericPacket {
        event_id: LEVEL_EVENT_SLEEPING_PLAYERS,
        __ctd__: NbtCompoundBody(Bytes::from(body)),
    }
}

/// The adventure settings BDS sends with every join, to the joining player
/// and to everyone already online. Name tags are off although the
/// `showTags` game rule is on, and changing `pvp` or `showTags` does not
/// resend them (docs/quirks/bds.md).
pub fn adventure_settings() -> UpdateAdventureSettingsPacket {
    UpdateAdventureSettingsPacket {
        adventure_settings: AdventureSettings {
            no_pv_m: false,
            no_mv_p: false,
            immutable_world: false,
            show_name_tags: false,
            auto_jump: true,
        },
    }
}

/// The build platform for a login's `DeviceOS`: both use the same numbers.
pub fn build_platform(device_os: i32) -> EnumsBuildPlatform {
    EnumsBuildPlatform::decode(&mut &device_os.to_le_bytes()[..], ())
        .expect("every i32 decodes to a build platform")
}

fn base64(value: &str) -> Vec<u8> {
    // The login validator has already checked every image and JSON field.
    STANDARD.decode(value).unwrap_or_default()
}

fn image(data: &str, width: u32, height: u32) -> SkinImage {
    SkinImage {
        width,
        height,
        image_bytes: base64(data),
    }
}

/// `#rrggbb` as an RGB integer without alpha, as BDS echoes `SkinColor`.
/// [INFERENCE] A malformed colour (the login validator does not check it) is 0.
fn skin_colour(value: &str) -> i32 {
    value
        .strip_prefix('#')
        .and_then(|hex| i32::from_str_radix(hex, 16).ok())
        .unwrap_or(0)
}

/// The serialized skin BDS forwards for a login. Measured for a plain custom
/// skin (`crates/bds-extractor/captures/1.26.51/member/join/00012-*`): ids,
/// images, resource patch and colour are echoed; an empty geometry becomes
/// `null` followed by a newline and an empty geometry engine version
/// `0.0.0`; the trusted flag is the string `false`. Persona pieces and tint
/// colours are not forwarded yet.
pub fn skin(client: &ClientData) -> SerializedSkinRef {
    let geometry = String::from_utf8(base64(&client.skin_geometry)).unwrap_or_default();
    SerializedSkinRef {
        id: client.skin_id.as_str().into(),
        play_fab_id: client.playfab_id.as_str().into(),
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
        // [INFERENCE] A non-empty geometry is forwarded as the client sent it.
        geometry_data: if geometry.is_empty() {
            "null\n".into()
        } else {
            geometry.into()
        },
        geometry_data_min_engine_version: if client.skin_geometry_version.is_empty() {
            "0.0.0".into()
        } else {
            client.skin_geometry_version.as_str().into()
        },
        animation_data: client.skin_animation_data.as_str().into(),
        cape_id: client.cape_id.as_str().into(),
        // [INFERENCE] Skin id then cape id; measured only without a cape.
        full_id: format!("{}{}", client.skin_id, client.cape_id).into(),
        arm_size: if client.arm_size == "slim" {
            EnumsSharedTypespersonaArmSizeType::Slim
        } else {
            EnumsSharedTypespersonaArmSizeType::Wide
        },
        skin_color: MceColor {
            color: skin_colour(&client.skin_colour),
        },
        persona_pieces: Vec::new(),
        piece_tint_colors: Vec::new(),
        is_premium: client.premium_skin,
        is_persona: client.persona_skin,
        is_persona_cape_on_classic_skin: client.cape_on_classic_skin,
        is_primary_user: false,
        overrides_player_appearance: client.override_skin,
        trusted_skin_flag: "false".into(),
        profile_hash: client.profile_hash.as_str().into(),
    }
}

#[cfg(test)]
mod tests;

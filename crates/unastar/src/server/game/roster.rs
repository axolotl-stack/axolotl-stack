//! The player roster as BDS 1.26.51 announces it.
//!
//! Measured with `tools/bds-sweep/capture.py --pair` (fixtures under
//! `crates/bds-extractor/captures/1.26.51/roster/`):
//!
//! - A joining player gets the sleeping-player `LevelEventGeneric`, a
//!   `PlayerList` holding only itself and the world clock state before
//!   `StartGame` (in its batch), and later in the join a `PlayerList`
//!   holding every online player, itself included. A player counts as online
//!   from that moment: [`OnlineRoster`] reserves its place and colour in the
//!   connection task, before the game loop has spawned it.
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

use std::collections::BTreeMap;
use std::sync::Arc;

use bevy_ecs::prelude::{Component, Entity, Resource, World};
use bytes::{BufMut, Bytes, BytesMut};
use jolyne::auth::ClientData;
use jolyne::valentine::types::{
    ActorUniqueId, AdventureSettings, EnumsBuildPlatform, EnumsPlayerListPacketType, MceColor,
    PlayerListPacketEntriesItem, PlayerListPacketPayloadAddEntry,
    PlayerListPacketPayloadAddEntryAction, PlayerListPacketPayloadRemoveEntry,
};
use jolyne::valentine::{
    LevelEventGenericPacket, McpePacket, PlayerListPacket, UpdateAdventureSettingsPacket,
};
use parking_lot::Mutex;
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

    #[cfg(test)]
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

/// Who counts as online, shared by the connection tasks and the game loop:
/// each session's colour, from the moment its join reaches `StartGame` until
/// the player leaves. BDS counts a joining player (and has given it a colour)
/// before `StartGame`, so the place is reserved there, off the game thread.
#[derive(Resource, Debug, Clone, Default)]
pub struct OnlineRoster(Arc<Mutex<BTreeMap<u64, u32>>>);

impl OnlineRoster {
    /// Reserves `session`'s place: the [`free_colour`] among everyone online
    /// and the number online, `session` included. Dropping the reservation
    /// before [`Reservation::keep`] gives the place back (a join that failed
    /// before the game loop took it over).
    pub fn reserve(&self, session: u64) -> Reservation {
        let (colour, online) = self.join(session);
        Reservation {
            roster: self.clone(),
            session,
            colour,
            online,
            kept: false,
        }
    }

    /// `session`'s colour and the number online, reserving its place when it
    /// has none yet.
    pub fn join(&self, session: u64) -> (u32, usize) {
        let mut online = self.0.lock();
        let colour = match online.get(&session) {
            Some(colour) => *colour,
            None => {
                let colour = free_colour(online.values().copied());
                online.insert(session, colour);
                colour
            }
        };
        (colour, online.len())
    }

    /// Gives `session`'s place and colour back (it left); returns how many
    /// stay online.
    pub fn leave(&self, session: u64) -> usize {
        let mut online = self.0.lock();
        online.remove(&session);
        online.len()
    }

    /// How many players are online, joining ones included.
    pub fn len(&self) -> usize {
        self.0.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A place in the [`OnlineRoster`] held by a join in progress.
#[derive(Debug)]
pub struct Reservation {
    roster: OnlineRoster,
    session: u64,
    colour: u32,
    online: usize,
    kept: bool,
}

impl Reservation {
    pub fn colour(&self) -> u32 {
        self.colour
    }

    /// How many were online when the place was reserved, this one included.
    pub fn online(&self) -> usize {
        self.online
    }

    /// The game loop has the player now; its leave gives the place back.
    pub fn keep(mut self) {
        self.kept = true;
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if !self.kept {
            self.roster.leave(self.session);
        }
    }
}

/// The packets BDS 1.26.51 sends a joining player before `StartGame`, in its
/// order (member join fixtures `00011`-`00013`): the sleeping-player count
/// with everyone online (the joining player included), the player's own
/// `PlayerList` entry and the world clock state.
pub fn before_start_game(
    online: usize,
    own: &RosterEntry,
    time: crate::world::rules::WorldTime,
) -> Vec<McpePacket> {
    vec![
        McpePacket::from(sleeping_players(online as i32, 0)),
        McpePacket::from(add_packet([own])),
        McpePacket::from(crate::world::clock::clock_state_packet(time)),
    ]
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

mod skin;
pub use skin::skin;

#[cfg(test)]
mod probe_tests;
#[cfg(test)]
mod tests;

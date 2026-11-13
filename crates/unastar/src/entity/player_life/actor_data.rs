//! A player's synched actor data as BDS 1.26.51 sends it: the whole set in
//! the player's own join `SetActorData` and in `AddPlayer` to other players
//! ([`full_actor_data`]), then `SetActorData` updates of what changed
//! (`testdata/bds-1.26.51-player-data.json`,
//! `testdata/bds-1.26.51-breath.json`,
//! `testdata/bds-1.26.51-movement.json`).
//!
//! The keys that change during play:
//! - 0, the flag word ([`player_flags`]): `CANCLIMB` and `HAS_GRAVITY`,
//!   `HAS_COLLISION` unless spectating, `BREATHING` while breathing, the
//!   movement flags, `MOVING` while its last input moved it
//!   ([`super::motion`]), `USINGITEM`, `ONFIRE` while burning, `INVISIBLE`
//!   while invisible, `FIRE_IMMUNE` under fire resistance, and
//!   `CAN_SHOW_NAME` once an invisibility has ended;
//! - 7, the air supply (`breath`);
//! - 53 and 54, the box: 0.6 × 1.8 standing, 1.49 tall sneaking, 0.6 tall
//!   swimming, gliding or crawling (a block's collision box is in the
//!   standing box but not in the crawling one) and 0.2 × 0.2 dead;
//! - 92, the second flag word: `HIDDEN_WHEN_INVISIBLE` and
//!   `PUSH_TOWARDS_CLOSEST_SPACE`, `CRAWLING`, `BLOCKING` while sneaking and
//!   `TRANSITION_BLOCKING` from the first sneak on;
//! - 127, 128 and 129: the block and dimension of the last death and whether
//!   the player ever died.
//!
//! Each change reaches the player and its viewers (the players that see it,
//! [`super::player_viewers`]) as one `SetActorData` of the changed keys, in
//! key order, with the player's input tick:
//! - right after the breath ([`sync_breath`], after the tick's attribute
//!   update): the breath, the pose, burning, and a death during the player
//!   tick (landing, environment and breath damage, [`PlayerTick`])
//!   together with its 0.2 × 0.2 box;
//! - at the end of every tick ([`sync_players`]): `MOVING` (as its own
//!   update, after the breath's), effects and whatever else changed since;
//! - at once, when an item use starts or stops, the game mode changes
//!   ([`send_changes`]) or a sprint, swim or glide is accepted
//!   ([`super::motion`]), when the player dies outside the player tick (the
//!   death keys; the box follows at the end of the tick), and when it
//!   respawns (the box; the breath follows at the end of the tick);
//! - a sneak start or stop, whichever update carries it, is two updates:
//!   the flag word with the box, then the second flag word;
//! - a refused movement toggle resends the flag words unchanged with key 139
//!   ([`resync`]), alone or with the update it comes with.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::types::{
    ActorRuntimeId, BlockPos, DataItemEntry, DataItemEntryPayload, DataItemPosPayload,
    DataItemVec3Payload, EnumsDataItemType, PropertySyncData, SynchedActorDataCopyableDataList,
    Vec3,
};
use jolyne::valentine::{McpePacket, SetActorDataPacket, actor_data_i_ds as ids, actor_flags};

use super::breath::PlayerBreath;
use super::environment::Burning;
use super::motion::PlayerMoves;
use super::{Dead, send_to_player_and_viewers};
use crate::entity::PLAYER_EYE_HEIGHT;
use crate::entity::components::{
    EffectType, Effects, GameMode, Player, PlayerState, Position, RuntimeEntityId, input_tick_of,
};
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::systems::BlockSource;
use crate::registry::collision::{Aabb, block_collision_aabbs, connected_state};
use crate::server::game::item_use::ItemUsing;
use crate::world::ecs::AwaitingSpawnStatus;

/// Width of a living player's box (keys 53 and 130).
const WIDTH: f32 = 0.6;
/// Height of a standing player's box (keys 54 and 130).
const STANDING_HEIGHT: f32 = 1.8;
/// Height of a sneaking player's box.
const SNEAKING_HEIGHT: f32 = 1.49;
/// Height of a crawling, swimming or gliding player's box.
const CRAWLING_HEIGHT: f32 = 0.6;
/// How far the crawl test's boxes are shrunk on every side: positions come
/// from the client as `f32`, so feet on a floor can sit a hair below its
/// top (eyes at 5.62f32 are 3.99999988 + 1.62).
const CRAWL_TOLERANCE: f64 = 1.0e-5;
/// Width and height of a dead player's box.
const DEAD_SIZE: f32 = 0.2;
/// The air supply a player's actor data starts with (keys 7 and 42).
pub(crate) const FULL_AIR: i16 = 300;

/// Keys a joining player's own `SetActorData` leaves out of the
/// [`full_actor_data`] that `AddPlayer` carries (BDS 1.26.51 join capture
/// `member/join/00062-SetActorDataPacket.bin`).
const NOT_IN_JOIN: [u32; 7] = [
    ids::RESERVED_010,
    ids::RESERVED_038,
    ids::RESERVED_053,
    ids::RESERVED_054,
    ids::RESERVED_056,
    ids::RESERVED_074,
    ids::RESERVED_139,
];

/// The values of the changing keys, as computed or as last sent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Synced {
    pub flags: i64,
    pub air: i16,
    pub width: f32,
    pub height: f32,
    pub flags2: i64,
    pub death_pos: IVec3,
    pub death_dimension: i32,
    pub has_died: bool,
}

impl Synced {
    /// What a joining player's actor data says: not yet breathing, full
    /// air, standing, never died (the client's defaults for the box).
    fn joined() -> Self {
        Self {
            flags: base_flags(false),
            air: FULL_AIR,
            width: WIDTH,
            height: STANDING_HEIGHT,
            flags2: base_flags2(),
            death_pos: IVec3::ZERO,
            death_dimension: 0,
            has_died: false,
        }
    }
}

/// A player's actor-data state that no other component holds, and what
/// its clients last received.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct PlayerActorData {
    /// Key 27: the lowest index no other player in the world has, from 1.
    pub index: i32,
    /// Keys 127 and 128: the block (its position floored) and dimension
    /// where the player last died; key 129, whether it ever died.
    pub death_pos: IVec3,
    pub death_dimension: i32,
    pub has_died: bool,
    /// The player was invisible: `CAN_SHOW_NAME` is set once it no longer is.
    was_invisible: bool,
    /// The player sneaked: `TRANSITION_BLOCKING` stays set.
    sneaked: bool,
    /// A movement toggle was refused: the next update resends the flag
    /// words and key 139.
    resync: bool,
    /// The `MOVING` flag as the end-of-tick update last took it from
    /// [`PlayerMoves`].
    moving: bool,
    /// The player had its first breath tick: updates start with it.
    ticked: bool,
    /// Key 37: 0 for a player's first join, -1 when its saved data was
    /// restored (BDS 1.26.51: every rejoin in the player-data captures).
    leash_holder: i64,
    sent: Synced,
    /// What the clients had at the start of this tick: a player joining in
    /// it finds the others in this state ([`announce_players`]).
    announced: Synced,
}

impl PlayerActorData {
    /// A joining player with player index `index`.
    pub fn joined(index: i32) -> Self {
        Self {
            index,
            death_pos: IVec3::ZERO,
            death_dimension: 0,
            has_died: false,
            was_invisible: false,
            sneaked: false,
            resync: false,
            moving: false,
            ticked: false,
            leash_holder: 0,
            sent: Synced::joined(),
            announced: Synced::joined(),
        }
    }

    /// A player with player index `index` whose saved data was restored.
    pub fn rejoined(index: i32) -> Self {
        Self {
            leash_holder: -1,
            ..Self::joined(index)
        }
    }

    /// The player's first breath tick ran: from now on its changes are
    /// sent (the first update carries that tick's flag and air, as BDS's
    /// does: a rejoining player's saved air arrives with `BREATHING`).
    pub fn ticked(&mut self) {
        self.ticked = true;
    }

    /// What the player's clients last received.
    pub fn sent(&self) -> &Synced {
        &self.sent
    }

    /// Record a death at `position` (the player's eye position) in
    /// `dimension`.
    pub fn died_at(&mut self, position: DVec3, dimension: i32) {
        self.death_pos = position.floor().as_ivec3();
        self.death_dimension = dimension;
        self.has_died = true;
    }
}

/// The player's game mode was set for it (`/gamemode`, or a saved mode
/// other than the world default): `AddPlayer` names that mode instead of
/// BDS's "default" (a player following the world default is `default`
/// there, one given a mode is that mode, even survival).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExplicitGameMode;

/// The player index a joining player gets: the lowest index from 1 that no
/// player in the world has (BDS 1.26.51: 1, 2, 3, then 4 while the second
/// player's connection was still open; a player rejoining an empty server
/// is 1 again).
pub fn next_index(world: &mut World) -> i32 {
    let mut taken: Vec<i32> = world
        .query::<&PlayerActorData>()
        .iter(world)
        .map(|data| data.index)
        .collect();
    taken.sort_unstable();
    let mut index = 1;
    for taken in taken {
        if taken == index {
            index += 1;
        } else if taken > index {
            break;
        }
    }
    index
}

/// `CANCLIMB` and `HAS_GRAVITY`, and `HAS_COLLISION` unless spectating.
fn base_flags(spectator: bool) -> i64 {
    let mut flags = (1u64 << actor_flags::CANCLIMB) | (1u64 << actor_flags::HAS_GRAVITY);
    if !spectator {
        flags |= 1u64 << actor_flags::HAS_COLLISION;
    }
    flags as i64
}

/// The second flag word's bits every player has (flags 64 and up).
fn base_flags2() -> i64 {
    flag2(actor_flags::HIDDEN_WHEN_INVISIBLE) | flag2(actor_flags::PUSH_TOWARDS_CLOSEST_SPACE)
}

/// A flag of the second word (key 92).
fn flag2(flag: u32) -> i64 {
    (1u64 << (flag - 64)) as i64
}

/// The flag word BDS sends for `player`, `using` an item or not (see the
/// module documentation; item use: capture `item-use-20261029`).
pub fn player_flags(world: &World, player: Entity, using: bool) -> i64 {
    let spectator = world.get::<GameMode>(player) == Some(&GameMode::Spectator);
    let mut flags = base_flags(spectator) as u64;
    let mut set = |on: bool, flag: u32| {
        if on {
            flags |= 1u64 << flag;
        }
    };
    set(
        world
            .get::<PlayerBreath>(player)
            .is_none_or(|breath| breath.breathing),
        actor_flags::BREATHING,
    );
    if let Some(state) = world.get::<PlayerState>(player) {
        set(state.sneaking, actor_flags::SNEAKING);
        set(state.sprinting, actor_flags::SPRINTING);
        set(state.swimming, actor_flags::SWIMMING);
        set(state.gliding, actor_flags::GLIDING);
    }
    set(
        world
            .get::<PlayerActorData>(player)
            .is_some_and(|data| data.moving),
        actor_flags::MOVING,
    );
    set(using, actor_flags::USINGITEM);
    set(world.get::<Burning>(player).is_some(), actor_flags::ONFIRE);
    let effects = world.get::<Effects>(player);
    let invisible = effects.is_some_and(|effects| effects.has(EffectType::INVISIBILITY));
    set(invisible, actor_flags::INVISIBLE);
    set(
        effects.is_some_and(|effects| effects.has(EffectType::FIRE_RESISTANCE)),
        actor_flags::FIRE_IMMUNE,
    );
    set(
        !invisible
            && world
                .get::<PlayerActorData>(player)
                .is_some_and(|data| data.was_invisible),
        actor_flags::CAN_SHOW_NAME,
    );
    flags as i64
}

/// The second flag word for `player`.
fn player_flags2(data: &PlayerActorData, sneaking: bool, crawling: bool) -> i64 {
    let mut flags = base_flags2();
    if crawling {
        flags |= flag2(actor_flags::CRAWLING);
    }
    if sneaking {
        flags |= flag2(actor_flags::BLOCKING);
    }
    if sneaking || data.sneaked {
        flags |= flag2(actor_flags::TRANSITION_BLOCKING);
    }
    flags
}

/// Whether a block's collision box overlaps `area`; `None` while a block
/// it covers is not loaded.
fn blocks_overlap(world: &World, area: Aabb) -> Option<bool> {
    let min = area.min.floor().as_ivec3();
    let max = area.max.floor().as_ivec3();
    let registry = world.registry();
    // One layer below: fences and walls reach into the block above.
    for y in (min.y - 1)..=max.y {
        for x in min.x..=max.x {
            for z in min.z..=max.z {
                let pos = IVec3::new(x, y, z);
                let state = world.block_at(pos)?;
                let state = connected_state(registry, pos, state, |p| world.block_at(p));
                let origin = pos.as_dvec3();
                if block_collision_aabbs(state).any(|aabb| aabb.offset(origin).intersects(&area)) {
                    return Some(true);
                }
            }
        }
    }
    Some(false)
}

/// Whether `player` crawls: a block is in its standing box but not in its
/// crawling box (BDS 1.26.51: stone, glass and oak leaves at eye height).
/// `None` while the blocks there are not loaded.
fn crawls(world: &World, player: Entity) -> Option<bool> {
    let feet = world.get::<Position>(player)?.0 - DVec3::Y * PLAYER_EYE_HEIGHT;
    let shrink = DVec3::splat(-CRAWL_TOLERANCE);
    let standing =
        Aabb::from_feet(feet, f64::from(WIDTH), f64::from(STANDING_HEIGHT)).inflate(shrink);
    if !blocks_overlap(world, standing)? {
        return Some(false);
    }
    let crawling =
        Aabb::from_feet(feet, f64::from(WIDTH), f64::from(CRAWLING_HEIGHT)).inflate(shrink);
    Some(!blocks_overlap(world, crawling)?)
}

/// The current values of `player`'s changing keys.
fn current(world: &World, player: Entity, data: &PlayerActorData) -> Synced {
    let using = world.get::<ItemUsing>(player).is_some();
    let flags = player_flags(world, player, using);
    let dead = world.get::<Dead>(player).is_some();
    let state = world
        .get::<PlayerState>(player)
        .copied()
        .unwrap_or_default();
    let sneaking = state.sneaking;
    let sent_crawling = data.sent.flags2 & flag2(actor_flags::CRAWLING) != 0;
    let crawling = !dead && crawls(world, player).unwrap_or(sent_crawling);
    let (width, height) = if dead {
        (DEAD_SIZE, DEAD_SIZE)
    } else if crawling || state.swimming || state.gliding {
        (WIDTH, CRAWLING_HEIGHT)
    } else if sneaking {
        (WIDTH, SNEAKING_HEIGHT)
    } else {
        (WIDTH, STANDING_HEIGHT)
    };
    Synced {
        flags,
        air: world
            .get::<PlayerBreath>(player)
            .map_or(FULL_AIR, |breath| breath.air),
        width,
        height,
        flags2: player_flags2(data, sneaking, crawling),
        death_pos: data.death_pos,
        death_dimension: data.death_dimension,
        has_died: data.has_died,
    }
}

/// Which changed keys one update may carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keys {
    /// Every changed key.
    All,
    /// All but the box (a death outside the player tick: the box follows
    /// at the end of the tick).
    AllButBox,
    /// Only the box (a respawn: the breath follows at the end of the tick).
    Box,
}

fn long(id: u32, value: i64) -> DataItemEntry {
    DataItemEntry {
        id,
        payload: DataItemEntryPayload::DataItemInt64Payload(
            jolyne::valentine::types::DataItemInt64Payload {
                type_: EnumsDataItemType::Int64,
                value,
            },
        ),
    }
}

fn float(id: u32, value: f32) -> DataItemEntry {
    DataItemEntry {
        id,
        payload: DataItemEntryPayload::DataItemFloatPayload(
            jolyne::valentine::types::DataItemFloatPayload {
                type_: EnumsDataItemType::Float,
                value,
            },
        ),
    }
}

fn short(id: u32, value: i16) -> DataItemEntry {
    DataItemEntry {
        id,
        payload: DataItemEntryPayload::DataItemShortPayload(
            jolyne::valentine::types::DataItemShortPayload {
                type_: EnumsDataItemType::Short,
                value,
            },
        ),
    }
}

fn int(id: u32, value: i32) -> DataItemEntry {
    DataItemEntry {
        id,
        payload: DataItemEntryPayload::DataItemIntPayload(
            jolyne::valentine::types::DataItemIntPayload {
                type_: EnumsDataItemType::Int,
                value,
            },
        ),
    }
}

fn byte(id: u32, value: i8) -> DataItemEntry {
    DataItemEntry {
        id,
        payload: DataItemEntryPayload::DataItemBytePayload(
            jolyne::valentine::types::DataItemBytePayload {
                type_: EnumsDataItemType::Byte,
                value,
            },
        ),
    }
}

fn pos(id: u32, value: IVec3) -> DataItemEntry {
    DataItemEntry {
        id,
        payload: DataItemEntryPayload::DataItemPosPayload(DataItemPosPayload {
            type_: EnumsDataItemType::Pos,
            value: BlockPos {
                x: value.x,
                y: value.y,
                z: value.z,
            },
        }),
    }
}

fn vec3(id: u32, x: f32, y: f32, z: f32) -> DataItemEntry {
    DataItemEntry {
        id,
        payload: DataItemEntryPayload::DataItemVec3Payload(DataItemVec3Payload {
            type_: EnumsDataItemType::Vec3,
            value: Vec3 { x, y, z },
        }),
    }
}

/// The rows of `now` that differ from `before` and `keys` lets through, in
/// key order.
fn changed_rows(before: &Synced, now: &Synced, keys: Keys) -> Vec<DataItemEntry> {
    let box_only = keys == Keys::Box;
    let with_box = keys != Keys::AllButBox;
    let mut rows = Vec::new();
    if !box_only && now.flags != before.flags {
        rows.push(long(ids::RESERVED_0, now.flags));
    }
    if !box_only && now.air != before.air {
        rows.push(short(ids::AIR_SUPPLY, now.air));
    }
    if with_box && now.width != before.width {
        rows.push(float(ids::RESERVED_053, now.width));
    }
    if with_box && now.height != before.height {
        rows.push(float(ids::RESERVED_054, now.height));
    }
    if !box_only && now.flags2 != before.flags2 {
        rows.push(long(ids::RESERVED_092, now.flags2));
    }
    if !box_only && now.death_pos != before.death_pos {
        rows.push(pos(ids::PLAYER_LAST_DEATH_POS, now.death_pos));
    }
    if !box_only && now.death_dimension != before.death_dimension {
        rows.push(int(ids::PLAYER_LAST_DEATH_DIMENSION, now.death_dimension));
    }
    if !box_only && now.has_died != before.has_died {
        rows.push(byte(ids::PLAYER_HAS_DIED, i8::from(now.has_died)));
    }
    rows
}

/// Mark `sent` as what the clients now have for the keys `keys` lets
/// through.
fn mark_sent(sent: &mut Synced, now: &Synced, keys: Keys) {
    let box_only = keys == Keys::Box;
    if keys != Keys::AllButBox {
        sent.width = now.width;
        sent.height = now.height;
    }
    if !box_only {
        let (width, height) = (sent.width, sent.height);
        *sent = *now;
        sent.width = width;
        sent.height = height;
    }
}

/// Send `rows` about `player` to it and its viewers.
fn send_rows(world: &World, player: Entity, rows: Vec<DataItemEntry>) {
    if rows.is_empty() {
        return;
    }
    let Some(runtime_id) = world.get::<RuntimeEntityId>(player).map(|id| id.0) else {
        return;
    };
    let packet = McpePacket::from(SetActorDataPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id as u64,
        },
        actor_data: SynchedActorDataCopyableDataList { data: rows },
        synched_properties: PropertySyncData::default(),
        tick: input_tick_of(world, player),
    });
    send_to_player_and_viewers(world, player, &packet);
}

/// Send `player` and its viewers the changed keys `keys` lets through.
/// Nothing before the player's first breath tick after the join handed it
/// over (`PlayStatus` spawn).
pub fn send_changes(world: &mut World, player: Entity, keys: Keys) {
    if world.get::<AwaitingSpawnStatus>(player).is_some() {
        return;
    }
    let Some(data) = world
        .get::<PlayerActorData>(player)
        .filter(|data| data.ticked)
    else {
        return;
    };
    let now = current(world, player, data);
    let flag = 1i64 << actor_flags::SNEAKING;
    if keys == Keys::All && now.flags & flag != data.sent.flags & flag {
        // A sneak started or stopped: its own updates first, whatever else
        // sends the flag word.
        sneak(world, player);
    }
    let Some(data) = world.get::<PlayerActorData>(player) else {
        return;
    };
    let now = current(world, player, data);
    let resync = keys == Keys::All && data.resync;
    let mut rows = changed_rows(&data.sent, &now, keys);
    if resync {
        // A refused toggle: both flag words as they are, then key 139.
        if !rows.iter().any(|row| row.id == ids::RESERVED_0) {
            rows.insert(0, long(ids::RESERVED_0, now.flags));
        }
        if !rows.iter().any(|row| row.id == ids::RESERVED_092) {
            let at = rows
                .iter()
                .position(|row| row.id > ids::RESERVED_092)
                .unwrap_or(rows.len());
            rows.insert(at, long(ids::RESERVED_092, now.flags2));
        }
        rows.push(long(ids::RESERVED_139, 0));
    }
    if rows.is_empty() {
        return;
    }
    send_rows(world, player, rows);
    if let Some(mut data) = world.get_mut::<PlayerActorData>(player) {
        mark_sent(&mut data.sent, &now, keys);
        if resync {
            data.resync = false;
        }
    }
}

/// A movement toggle of `player` was refused: its next update (the one sent
/// right after, if nothing else changed) resends the flag words unchanged
/// with key 139.
pub fn resync(world: &mut World, player: Entity) {
    if let Some(mut data) = world.get_mut::<PlayerActorData>(player) {
        data.resync = true;
    }
}

/// A sneak started or stopped since the last update: the flag word with the
/// box (BDS's first update), then the second flag word with the blocking
/// bits.
fn sneak(world: &mut World, player: Entity) {
    let Some(mut data) = world.get_mut::<PlayerActorData>(player) else {
        return;
    };
    // Remembered before the new values are computed: TRANSITION_BLOCKING
    // stays from the first sneak on.
    data.sneaked = true;
    let data = world.get::<PlayerActorData>(player).expect("read above");
    let before = data.sent;
    let now = current(world, player, data);
    let mut first = vec![long(ids::RESERVED_0, now.flags)];
    if now.width != before.width {
        first.push(float(ids::RESERVED_053, now.width));
    }
    if now.height != before.height {
        first.push(float(ids::RESERVED_054, now.height));
    }
    if let Some(mut data) = world.get_mut::<PlayerActorData>(player) {
        data.sent.flags = now.flags;
        data.sent.width = now.width;
        data.sent.height = now.height;
    }
    send_rows(world, player, first);
    if now.flags2 != before.flags2 {
        if let Some(mut data) = world.get_mut::<PlayerActorData>(player) {
            data.sent.flags2 = now.flags2;
        }
        send_rows(world, player, vec![long(ids::RESERVED_092, now.flags2)]);
    }
}

/// Players whose actor data is kept: their join handed over.
type Synchronized = QueryState<
    Entity,
    (
        With<Player>,
        With<PlayerActorData>,
        Without<AwaitingSpawnStatus>,
    ),
>;

/// System (exclusive, first in the tick): remember what every player's
/// clients have now. BDS 1.26.51 shows a player joining in this tick the
/// others as they were before the tick's updates (the late joiner's
/// `AddPlayer` in the player-data capture carries the air of the tick
/// before).
pub fn announce_players(world: &mut World, players: &mut QueryState<&mut PlayerActorData>) {
    for mut data in players.iter_mut(world) {
        data.announced = data.sent;
    }
}

/// System (exclusive, right after the breath): the update of what the
/// player tick changed (breath, pose, burning, a death and its box), before
/// `MOVING` changes.
pub fn sync_breath(world: &mut World, players: &mut Synchronized, mut due: Local<Vec<Entity>>) {
    due.extend(players.iter(world));
    for player in due.drain(..) {
        if world
            .get::<PlayerActorData>(player)
            .is_some_and(|data| data.ticked)
        {
            send_changes(world, player, Keys::All);
        }
    }
}

/// System (exclusive, network send): the end-of-tick actor-data update of
/// every player: `MOVING` as the last input left it, effects, and anything
/// else that changed after the breath.
pub fn sync_players(world: &mut World, players: &mut Synchronized, mut due: Local<Vec<Entity>>) {
    due.extend(players.iter(world));
    for player in due.drain(..) {
        if world
            .get::<PlayerActorData>(player)
            .is_none_or(|data| !data.ticked)
        {
            continue;
        }
        let invisible = world
            .get::<Effects>(player)
            .is_some_and(|effects| effects.has(EffectType::INVISIBILITY));
        let moving = world
            .get::<PlayerMoves>(player)
            .is_some_and(|moves| moves.moving);
        let mut data = world
            .get_mut::<PlayerActorData>(player)
            .expect("read above");
        data.was_invisible |= invisible;
        data.moving = moving;
        send_changes(world, player, Keys::All);
    }
}

/// Set while the player tick's own damage runs (landings, the environment
/// and breath, between [`enter_player_tick`] and [`leave_player_tick`] in
/// those exclusive systems, so no other system sees it): a death then waits
/// for [`sync_players`], which sends it with the dead box.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct PlayerTick(bool);

/// The player tick's own damage starts (see [`PlayerTick`]).
pub fn enter_player_tick(world: &mut World) {
    world.get_resource_or_init::<PlayerTick>().0 = true;
}

/// The player tick's own damage is done.
pub fn leave_player_tick(world: &mut World) {
    world.get_resource_or_init::<PlayerTick>().0 = false;
}

/// A player just died: record where and, outside the player tick, send the
/// death keys at once (the box follows at the end of the tick).
pub fn died(world: &mut World, player: Entity) {
    let dimension = world
        .get_resource::<crate::server::game::ServerConfigResource>()
        .map_or(0, |config| config.0.world.dimension);
    let Some(position) = world.get::<Position>(player).map(|position| position.0) else {
        return;
    };
    let Some(mut data) = world.get_mut::<PlayerActorData>(player) else {
        return;
    };
    data.died_at(position, dimension);
    if !world
        .get_resource::<PlayerTick>()
        .is_some_and(|tick| tick.0)
    {
        send_changes(world, player, Keys::AllButBox);
    }
}

/// A player just respawned: its box at once (the breath follows at the end
/// of the tick).
pub fn respawned(world: &mut World, player: Entity) {
    send_changes(world, player, Keys::Box);
}

/// Every row of a player's actor data, as `AddPlayer` carries it, from the
/// values its clients had at the start of the tick: the BDS 1.26.51 rows of a player (`AddPlayer` in the
/// player-data capture), in key order.
pub fn full_actor_data(name: &str, data: &PlayerActorData) -> Vec<DataItemEntry> {
    use jolyne::valentine::bedrock::codec::Nbt;
    use jolyne::valentine::types::DataItemCompoundTagPayload;
    let sent = &data.announced;
    let mut m = EntityMetadata::with_capacity(80);
    m.set_long(ids::RESERVED_0, sent.flags);
    m.set_int(ids::STRUCTURAL_INTEGRITY, 1);
    m.set_int(ids::VARIANT, 0);
    m.set_byte(ids::COLOR_INDEX, 0);
    m.set_string(ids::NAME, name);
    m.set_long(ids::OWNER, -1);
    m.set_long(ids::TARGET, 0);
    m.set_short(ids::AIR_SUPPLY, sent.air);
    m.set_byte(ids::RESERVED_010, 0);
    m.set_byte(ids::CHARGE_AMOUNT, 0);
    m.set_byte(ids::PLAYER_FLAGS, 0);
    m.set_int(ids::PLAYER_INDEX, data.index);
    m.set_payload(ids::BED_POSITION, pos(0, IVec3::ZERO).payload);
    m.set_long(ids::LEASH_HOLDER, data.leash_holder);
    m.set_float(ids::RESERVED_038, 1.0);
    m.set_byte(ids::HAS_NPC, 0);
    m.set_short(ids::AIR_SUPPLY_MAX, FULL_AIR);
    m.set_int(ids::MARK_VARIANT, 0);
    m.set_byte(ids::CONTAINER_TYPE, 0);
    m.set_int(ids::CONTAINER_SIZE, 0);
    m.set_int(ids::CONTAINER_STRENGTH_MODIFIER, 0);
    m.set_float(ids::RESERVED_053, sent.width);
    m.set_float(ids::RESERVED_054, sent.height);
    m.set_int(ids::FUSE_TIME, -1);
    m.set_payload(ids::RESERVED_056, vec3(0, 0.0, 0.0, 0.0).payload);
    m.set_byte(ids::SEAT_LOCK_PASSENGER_ROTATION, 0);
    m.set_float(ids::SEAT_LOCK_PASSENGER_ROTATION_DEGREES, 0.0);
    m.set_byte(ids::SEAT_ROTATION_OFFSET, 0);
    m.set_float(ids::SEAT_ROTATION_OFFSET_DEGREES, 0.0);
    m.set_byte(ids::HAS_COMMAND_BLOCK, 0);
    m.set_string(ids::COMMAND_NAME, "");
    m.set_string(ids::LAST_COMMAND_OUTPUT, "");
    m.set_byte(ids::TRACK_COMMAND_OUTPUT, 1);
    m.set_byte(ids::RESERVED_074, 0);
    m.set_int(ids::STRENGTH, 0);
    m.set_int(ids::STRENGTH_MAX, 0);
    m.set_int(ids::DATA_SPELL_CASTING_COLOR, 0);
    m.set_int(ids::DATA_LIFETIME_TICKS, -1);
    m.set_byte(ids::NAMETAG_ALWAYS_SHOW, 1);
    m.set_byte(ids::COLOR_2_INDEX, 0);
    m.set_string(ids::SCORE, "");
    m.set_long(ids::AGENT, -1);
    m.set_int(ids::EATING_COUNTER, 0);
    m.set_long(ids::RESERVED_092, sent.flags2);
    m.set_string(ids::INTERACT_TEXT, "");
    m.set_int(ids::TRADE_TIER, 0);
    m.set_int(ids::MAX_TRADE_TIER, 0);
    m.set_int(ids::TRADE_EXPERIENCE, 0);
    m.set_int(ids::SKIN_ID, 0);
    m.set_int(ids::COMMAND_BLOCK_TICK_DELAY, 3);
    m.set_byte(ids::COMMAND_BLOCK_EXECUTE_ON_FIRST_TICK, 1);
    m.set_float(ids::AMBIENT_SOUND_INTERVAL, 8.0);
    m.set_float(ids::AMBIENT_SOUND_INTERVAL_RANGE, 16.0);
    m.set_string(ids::AMBIENT_SOUND_EVENT_NAME, "ambient");
    m.set_float(ids::FALL_DAMAGE_MULTIPLIER, 1.0);
    m.set_int(ids::LOW_TIER_CURED_TRADE_DISCOUNT, 0);
    m.set_int(ids::HIGH_TIER_CURED_TRADE_DISCOUNT, 0);
    m.set_int(ids::NEARBY_CURED_TRADE_DISCOUNT, 0);
    m.set_int(ids::NEARBY_CURED_DISCOUNT_TIME_STAMP, 0);
    m.set_payload(
        ids::HITBOX,
        DataItemEntryPayload::DataItemCompoundTagPayload(DataItemCompoundTagPayload {
            type_: EnumsDataItemType::Compoundtag,
            value: Nbt(bytes::Bytes::from_static(b"\n\0\0")),
        }),
    );
    m.set_byte(ids::IS_BUOYANT, 0);
    m.set_float(ids::FREEZING_EFFECT_STRENGTH, 0.0);
    m.set_float(ids::MOVEMENT_SOUND_DISTANCE_OFFSET, 1.0);
    m.set_int(ids::HEARTBEAT_INTERVAL_TICKS, 20);
    m.set_int(ids::HEARTBEAT_SOUND_EVENT, 614);
    m.set_payload(ids::PLAYER_LAST_DEATH_POS, pos(0, sent.death_pos).payload);
    m.set_int(ids::PLAYER_LAST_DEATH_DIMENSION, sent.death_dimension);
    m.set_byte(ids::PLAYER_HAS_DIED, i8::from(sent.has_died));
    m.set_payload(
        ids::COLLISION_BOX,
        vec3(0, WIDTH, STANDING_HEIGHT, 0.0).payload,
    );
    m.set_long(ids::VISIBLE_MOB_EFFECTS, 0);
    m.set_string(ids::FILTERED_NAME, "");
    m.set_payload(ids::ENTER_BED_POSITION, vec3(0, 0.0, 0.0, 0.0).payload);
    m.set_float(ids::SEAT_THIRD_PERSON_CAMERA_RADIUS, 0.0);
    m.set_float(ids::SEAT_CAMERA_RELAX_DISTANCE_SMOOTHING, 0.0);
    m.set_int(ids::AIM_ASSIST_PRIORITY_PRESET_ID, -1);
    m.set_int(ids::AIM_ASSIST_PRIORITY_CATEGORY_ID, -1);
    m.set_int(ids::AIM_ASSIST_PRIORITY_ACTOR_ID, -1);
    m.set_long(ids::RESERVED_139, 0);
    m.set_float(ids::NAMEPLATE_RENDER_DISTANCE_MAX, 64.0);
    m.into_data().data
}

/// A joining player's own `SetActorData` rows: [`full_actor_data`]
/// without the keys BDS leaves out of it.
pub fn join_actor_data(name: &str, data: &PlayerActorData) -> Vec<DataItemEntry> {
    full_actor_data(name, data)
        .into_iter()
        .filter(|row| !NOT_IN_JOIN.contains(&row.id))
        .collect()
}

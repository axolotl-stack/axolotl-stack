//! Sleeping in beds, as BDS 1.26.51 does where measured (the capture of a
//! night use: the head's `occupied_bit`, one sleeping-players event, every
//! sleeper up again 100 ticks later); the rest is provisional.
//!
//! - **Falling asleep** ([`start`], after every bed check of
//!   [`super::beds::use_bed`] passed): the head gets `occupied_bit`; the
//!   player gets [`Sleeping`] (head, sleep counter 0, the position it had),
//!   lies at the head's (x + 0.5, y + 0.90625, z + 0.5) turned by the bed's
//!   `direction` (0, 1, 2, 3 → yaw 0, 90, −180, −90; pitch 0) with its fall
//!   reset, and the players around get `SetActorData`: the second flag word
//!   (92) with `SLEEPING` (76), player flags (26) bit 1, the bed position (28, the
//!   head), the enter-bed position (133, where it stood) and a 0.2 × 0.2
//!   box; the player gets `MovePlayer` (mode 1). Then every player gets the
//!   sleeping-players event.
//! - **Sleeping-players event**: `LevelEventGeneric`
//!   [`LEVEL_EVENT_SLEEPING_PLAYERS`] with integer tags `ableToSleep` (the
//!   `playersSleepingPercentage` rule is at most 100), `overworldPlayerCount`
//!   (the sleepers needed: `max(1, ⌈players × percentage / 100⌉)` over
//!   living players outside spectator) and `sleepingPlayerCount`. Sent when
//!   a player falls asleep and when one wakes early, not when everyone
//!   wakes at the end of the night.
//! - **Each tick** ([`tick`]): every sleeper's counter grows by one up to
//!   [`SLEEP_TICKS`]; a sleeper whose bed head is gone wakes early. When
//!   enough players sleep and as many have slept [`SLEEP_TICKS`] ticks,
//!   everyone wakes and, with `doDaylightCycle` on, the clock jumps to the
//!   next morning (`((t + 24000) / 24000) × 24000`, broadcast at once); with
//!   it off the time stays.
//! - **Waking** (each sleeper): `Animate` WakeUp to the player itself, the
//!   head's `occupied_bit` cleared, the player standing at a free spot
//!   beside the bed ([`wake_spot`]) 0.1 above its feet height (above the
//!   head when there is none), the 0.6 × 1.8 box and the flags without
//!   sleeping for the players around, `MovePlayer` (mode 1) to the player.
//! - **Leaving the bed** ([`leave_bed`], `PlayerAction` StopSleeping) wakes
//!   the player early and sends the sleeping-players event.
//!
//! Not modelled: the `chat.type.sleeping` message when a player starts
//! sleeping with others online, healing to full on waking in peaceful, the
//! breath check of a waterlogged bed, the bed's block actor data sent while
//! sleeping, and the player's velocity on falling asleep (the client
//! drives it).

use std::collections::BTreeMap;

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::types::{
    ActorRuntimeId, BlockPos, DataItemBytePayload, DataItemEntry, DataItemEntryPayload,
    DataItemFloatPayload, DataItemInt64Payload, DataItemPosPayload, DataItemVec3Payload,
    EnumsAnimatePacketPayloadAction, EnumsDataItemType,
    EnumsPlayerPositionModeComponentPositionMode, PropertySyncData,
    SynchedActorDataCopyableDataList, Vec2, Vec3,
};
use jolyne::valentine::{
    AnimatePacket, McpePacket, MovePlayerPacket, SetActorDataPacket, actor_data_i_ds as ids,
};
use unastar_packs::nbt::Tag;

use super::beds::{DIRECTION, HEAD, OCCUPIED};
use super::{air, bit, centre, game_rule, generic_event, int, is_lava, name_at, with};
use crate::entity::components::{
    GameMode, Health, PlayerSession, Position, Rotation, RuntimeEntityId,
};
use crate::entity::player_life::environment::PlayerFall;
use crate::entity::systems::BlockSource;
use crate::registry::block::StateValue;
use crate::server::game::blocks::set_block;
use crate::server::game::item_use::{nearby_players, send_to};
use crate::server::game::placement::placed_boxes;
use crate::server::game::types::BlockRegistryResource;
use crate::world::rules::{GameRules, WorldTime};

/// `LevelEventGeneric` id of the sleeping-players event.
pub const LEVEL_EVENT_SLEEPING_PLAYERS: i32 = 9801;
/// Ticks of sleep that end the night.
pub const SLEEP_TICKS: u32 = 100;
/// Height above the head's cell a sleeper lies at.
const SLEEP_HEIGHT: f64 = 0.90625;
/// A sleeper's box (width, height).
const SLEEPING_BOX: (f32, f32) = (0.2, 0.2);
/// A standing player's box (width, height).
const STANDING_BOX: (f32, f32) = (0.6, 1.8);
/// Player flags (26) bit of a sleeper.
const PLAYER_FLAG_SLEEPING: i8 = 1 << 1;
/// The second flag word (flags 64 and up) of a sleeper: `SLEEPING`.
const SLEEPING_FLAG: i64 = 1 << (jolyne::valentine::actor_flags::SLEEPING - 64);

/// The spots a sleeper may wake at, around the head (bed-local: `z`
/// towards the foot), in order. The other hand mirrors `x`.
const WAKE_OFFSETS: [IVec3; 12] = [
    IVec3::new(1, 0, 0),
    IVec3::new(1, 0, 1),
    IVec3::new(1, 0, 2),
    IVec3::new(0, 0, 2),
    IVec3::new(-1, 0, 2),
    IVec3::new(-1, 0, 1),
    IVec3::new(-1, 0, 0),
    IVec3::new(-1, 0, -1),
    IVec3::new(0, 0, -1),
    IVec3::new(1, 0, -1),
    IVec3::new(0, 1, 0),
    IVec3::new(0, 1, 1),
];

/// A player asleep in the bed whose head is at `head`.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Sleeping {
    pub head: IVec3,
    /// Ticks slept, up to [`SLEEP_TICKS`].
    pub ticks: u32,
    /// Where the player was when it lay down.
    pub entered: DVec3,
}

/// The player asleep in the bed with its head at `head`, if any.
pub fn sleeper_in(world: &mut World, head: IVec3) -> Option<Entity> {
    let mut sleepers = world.query::<(Entity, &Sleeping)>();
    sleepers
        .iter(world)
        .find(|(_, sleeping)| sleeping.head == head)
        .map(|(entity, _)| entity)
}

/// The bed-local `offset` turned to the bed's `direction` (legacy 0 south,
/// 1 west, 2 north, 3 east).
fn rotate(offset: IVec3, direction: i32) -> IVec3 {
    let (x, z) = (offset.x, offset.z);
    let (x, z) = match direction.rem_euclid(4) {
        0 => (-x, -z),
        1 => (z, -x),
        2 => (x, z),
        _ => (-z, x),
    };
    IVec3::new(x, offset.y, z)
}

/// The bed-local `x` of the world offset `delta` for a bed lying along
/// `direction` (the inverse of [`rotate`]).
fn local_x(delta: DVec3, direction: i32) -> f64 {
    match direction.rem_euclid(4) {
        0 => -delta.x,
        1 => -delta.z,
        2 => delta.x,
        _ => delta.z,
    }
}

/// Where a sleeper of the bed with its head at `head`, lying along
/// `direction`, gets up: the first of [`WAKE_OFFSETS`] (mirrored when
/// `entered`, the position it lay down from, is on the bed's other hand)
/// where the block below has a collision box, the spot is a bed or has
/// none, and the block above is air (two blocks of air above a bed); a
/// spot without lava in it or below wins over an earlier one with lava.
pub fn wake_spot(
    world: &World,
    head: IVec3,
    direction: i32,
    entered: Option<DVec3>,
) -> Option<IVec3> {
    let mirrored = entered.is_some_and(|at| local_x(at - centre(head), direction) < 0.0);
    let registry = &world.resource::<BlockRegistryResource>().0;
    let air = air(registry);
    let solid = |pos: IVec3| {
        world
            .block_at(pos)
            .is_some_and(|state| !placed_boxes(registry, pos, state).is_empty())
    };
    let is_air = |pos: IVec3| world.block_at(pos) == Some(air);
    let lava = |pos: IVec3| {
        world
            .block_at(pos)
            .is_some_and(|state| is_lava(world, state))
    };
    let mut first = None;
    for offset in WAKE_OFFSETS {
        let offset = if mirrored {
            IVec3::new(-offset.x, offset.y, offset.z)
        } else {
            offset
        };
        let spot = head + rotate(offset, direction);
        let bed = name_at(world, spot) == Some("minecraft:bed");
        let stands = solid(spot - IVec3::Y)
            && (bed || !solid(spot))
            && is_air(spot + IVec3::Y)
            && (!bed || is_air(spot + IVec3::Y * 2));
        if !stands {
            continue;
        }
        if !lava(spot) && !lava(spot - IVec3::Y) {
            return Some(spot);
        }
        first.get_or_insert(spot);
    }
    first
}

/// `player` falls asleep in the bed whose head (`head_state`) is at
/// `head` (see the module docs).
pub fn start(world: &mut World, player: Entity, head: IVec3, head_state: u32) {
    let registry = world.resource::<BlockRegistryResource>().0.clone();
    let Some(entry) = registry.get_by_runtime_id(head_state) else {
        return;
    };
    let direction = int(entry, head_state, DIRECTION).unwrap_or(0);
    set_block(
        world,
        head,
        with(entry, head_state, OCCUPIED, StateValue::Byte(1)),
        false,
    );
    let entered = world
        .get::<Position>(player)
        .map_or(centre(head), |position| position.0);
    let yaw = [0.0, 90.0, -180.0, -90.0][direction.rem_euclid(4) as usize];
    let mut sleeper = world.entity_mut(player);
    if let Some(mut position) = sleeper.get_mut::<Position>() {
        position.0 = head.as_dvec3() + DVec3::new(0.5, SLEEP_HEIGHT, 0.5);
    }
    if let Some(mut rotation) = sleeper.get_mut::<Rotation>() {
        *rotation = Rotation::new(yaw, 0.0);
    }
    if let Some(mut fall) = sleeper.get_mut::<PlayerFall>() {
        *fall = PlayerFall::default();
    }
    sleeper.insert(Sleeping {
        head,
        ticks: 0,
        entered,
    });
    send_body(world, player, Some((head, entered)));
    move_player(world, player);
    send_sleeping_players(world);
}

/// `player` leaves its bed before the night is over (`PlayerAction`
/// StopSleeping): it wakes and every player gets the sleeping-players
/// event.
pub fn leave_bed(world: &mut World, player: Entity) {
    if wake(world, player) {
        send_sleeping_players(world);
    }
}

/// Wake `player` (see the module docs). Returns whether it was asleep.
fn wake(world: &mut World, player: Entity) -> bool {
    let Ok(mut sleeper) = world.get_entity_mut(player) else {
        return false;
    };
    let Some(sleeping) = sleeper.take::<Sleeping>() else {
        return false;
    };
    if let Some(runtime_id) = world.get::<RuntimeEntityId>(player) {
        let packet = McpePacket::from(AnimatePacket {
            action: EnumsAnimatePacketPayloadAction::Wakeup,
            target_actor_runtime_id: ActorRuntimeId {
                actor_runtime_id: runtime_id.0 as u64,
            },
            data: 0.0,
            swing_source: None,
        });
        send_to(world, &[player], &packet);
    }
    let head = sleeping.head;
    let registry = world.resource::<BlockRegistryResource>().0.clone();
    let bed = world.block_at(head).and_then(|state| {
        let entry = registry.get_by_runtime_id(state)?;
        (entry.string_id == "minecraft:bed" && bit(entry, state, HEAD)).then_some((entry, state))
    });
    let spot = bed.and_then(|(entry, state)| {
        if bit(entry, state, OCCUPIED) {
            set_block(
                world,
                head,
                with(entry, state, OCCUPIED, StateValue::Byte(0)),
                false,
            );
        }
        let direction = int(entry, state, DIRECTION).unwrap_or(0);
        wake_spot(world, head, direction, Some(sleeping.entered))
    });
    let eye = match spot {
        Some(spot) => {
            spot.as_dvec3() + DVec3::new(0.5, crate::entity::PLAYER_EYE_HEIGHT + 0.1, 0.5)
        }
        None => head.as_dvec3() + DVec3::new(0.5, 1.0 + crate::entity::PLAYER_EYE_HEIGHT, 0.5),
    };
    if let Some(mut position) = world.get_mut::<Position>(player) {
        position.0 = eye;
    }
    send_body(world, player, None);
    move_player(world, player);
    true
}

/// Tell the players around `player` its sleeping state: lying in the bed
/// `(head, entered)` or standing.
fn send_body(world: &World, player: Entity, bed: Option<(IVec3, DVec3)>) {
    let Some(runtime_id) = world.get::<RuntimeEntityId>(player).map(|r| r.0) else {
        return;
    };
    let float = |id, value| DataItemEntry {
        id,
        payload: DataItemEntryPayload::DataItemFloatPayload(DataItemFloatPayload {
            type_: EnumsDataItemType::Float,
            value,
        }),
    };
    let (width, height) = if bed.is_some() {
        SLEEPING_BOX
    } else {
        STANDING_BOX
    };
    let mut data = vec![
        DataItemEntry {
            id: ids::RESERVED_092,
            payload: DataItemEntryPayload::DataItemInt64Payload(DataItemInt64Payload {
                type_: EnumsDataItemType::Int64,
                value: if bed.is_some() { SLEEPING_FLAG } else { 0 },
            }),
        },
        DataItemEntry {
            id: ids::PLAYER_FLAGS,
            payload: DataItemEntryPayload::DataItemBytePayload(DataItemBytePayload {
                type_: EnumsDataItemType::Byte,
                value: if bed.is_some() {
                    PLAYER_FLAG_SLEEPING
                } else {
                    0
                },
            }),
        },
    ];
    if let Some((head, entered)) = bed {
        data.push(DataItemEntry {
            id: ids::BED_POSITION,
            payload: DataItemEntryPayload::DataItemPosPayload(DataItemPosPayload {
                type_: EnumsDataItemType::Pos,
                value: BlockPos {
                    x: head.x,
                    y: head.y,
                    z: head.z,
                },
            }),
        });
        data.push(DataItemEntry {
            id: ids::ENTER_BED_POSITION,
            payload: DataItemEntryPayload::DataItemVec3Payload(DataItemVec3Payload {
                type_: EnumsDataItemType::Vec3,
                value: Vec3 {
                    x: entered.x as f32,
                    y: entered.y as f32,
                    z: entered.z as f32,
                },
            }),
        });
    }
    data.push(float(ids::RESERVED_053, width));
    data.push(float(ids::RESERVED_054, height));
    let packet = McpePacket::from(SetActorDataPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id as u64,
        },
        actor_data: SynchedActorDataCopyableDataList { data },
        synched_properties: PropertySyncData::default(),
        tick: crate::entity::components::input_tick_of(world, player),
    });
    send_to(world, &nearby_players(world, player), &packet);
}

/// Tell `player`'s client where the server put it (`MovePlayer`, reset).
fn move_player(world: &World, player: Entity) {
    let (Some(runtime_id), Some(position)) = (
        world.get::<RuntimeEntityId>(player),
        world.get::<Position>(player),
    ) else {
        return;
    };
    let rotation = world.get::<Rotation>(player).copied().unwrap_or_default();
    let packet = McpePacket::from(MovePlayerPacket {
        player_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id.0 as u64,
        },
        position: Vec3 {
            x: position.0.x as f32,
            y: position.0.y as f32,
            z: position.0.z as f32,
        },
        rotation: Vec2 {
            x: rotation.pitch,
            y: rotation.yaw,
        },
        y_head_rotation: rotation.head_yaw,
        position_mode: EnumsPlayerPositionModeComponentPositionMode::Respawn,
        on_ground: false,
        riding_runtime_id: ActorRuntimeId {
            actor_runtime_id: 0,
        },
        teleport_data: None,
        tick: crate::entity::components::input_tick_of(world, player),
    });
    send_to(world, &[player], &packet);
}

/// The `playersSleepingPercentage` rule, the sleepers needed and the
/// sleepers (see the module docs).
fn sleep_counts(world: &mut World) -> (i32, u32, u32) {
    let percentage = match game_rule(world, "playersSleepingPercentage") {
        Some(unastar_data::game_rules::GameRuleValue::Int(percentage)) => percentage,
        _ => 100,
    };
    let mut players = world.query::<(
        &PlayerSession,
        Option<&GameMode>,
        Option<&Health>,
        Option<&Sleeping>,
    )>();
    let (mut living, mut sleeping) = (0u32, 0u32);
    for (_, mode, health, asleep) in players.iter(world) {
        if mode == Some(&GameMode::Spectator) || health.is_some_and(Health::is_dead) {
            continue;
        }
        living += 1;
        sleeping += u32::from(asleep.is_some());
    }
    let needed = (u64::from(living) * u64::from(percentage.max(0).unsigned_abs())).div_ceil(100);
    let required = u32::try_from(needed).unwrap_or(u32::MAX).max(1);
    (percentage, required, sleeping)
}

/// Send every player the sleeping-players event.
fn send_sleeping_players(world: &mut World) {
    let (percentage, required, sleeping) = sleep_counts(world);
    let int = |value: i64| Tag::Int(i32::try_from(value).unwrap_or(i32::MAX));
    let fields = BTreeMap::from([
        (
            "ableToSleep".to_owned(),
            Tag::Int(i32::from(percentage <= 100)),
        ),
        ("overworldPlayerCount".to_owned(), int(i64::from(required))),
        ("sleepingPlayerCount".to_owned(), int(i64::from(sleeping))),
    ]);
    let packet = generic_event(LEVEL_EVENT_SLEEPING_PLAYERS, &fields);
    let mut sessions = world.query::<&PlayerSession>();
    for session in sessions.iter(world) {
        let _ = session.send(packet.clone());
    }
}

/// Once per game tick (see the module docs).
pub fn tick(world: &mut World) {
    let mut sleepers = world.query::<(Entity, &mut Sleeping)>();
    let mut asleep = Vec::new();
    for (player, mut sleeping) in sleepers.iter_mut(world) {
        sleeping.ticks = (sleeping.ticks + 1).min(SLEEP_TICKS);
        asleep.push((player, sleeping.head, sleeping.ticks));
    }
    if asleep.is_empty() {
        return;
    }
    let mut woke_early = false;
    for (player, head, _) in &asleep {
        let bed = world.block_at(*head).is_some_and(|state| {
            world
                .registry()
                .get_by_runtime_id(state)
                .is_some_and(|entry| entry.string_id == "minecraft:bed" && bit(entry, state, HEAD))
        });
        if !bed {
            woke_early |= wake(world, *player);
        }
    }
    if woke_early {
        send_sleeping_players(world);
    }
    let (_, required, sleeping) = sleep_counts(world);
    let rested = asleep
        .iter()
        .filter(|(player, _, ticks)| {
            *ticks >= SLEEP_TICKS && world.get::<Sleeping>(*player).is_some()
        })
        .count();
    if sleeping < required || u32::try_from(rested).unwrap_or(u32::MAX) < required {
        return;
    }
    for (player, _, _) in asleep {
        wake(world, player);
    }
    if world
        .get_resource::<GameRules>()
        .is_some_and(|rules| rules.do_daylight_cycle)
    {
        let now = world
            .get_resource::<WorldTime>()
            .map_or(0, |time| time.ticks);
        let morning = (now + WorldTime::DAY_TICKS) / WorldTime::DAY_TICKS * WorldTime::DAY_TICKS;
        world.init_resource::<crate::world::clock::ClockSync>();
        crate::world::clock::set_time(world, morning.min(WorldTime::MAX));
    }
}

#[cfg(test)]
mod tests {
    use jolyne::valentine::McpePacketData;

    use super::super::beds::use_bed;
    use super::super::tests::{FLOOR, Harness, state};
    use super::*;
    use crate::server::game::placement::Horizontal;

    const FOOT: IVec3 = IVec3::new(0, FLOOR + 1, 0);
    const HEAD_AT: IVec3 = IVec3::new(0, FLOOR + 1, 1);

    fn bed_state(head: bool, occupied: bool) -> u32 {
        state(
            "minecraft:bed",
            &[
                (DIRECTION, StateValue::Int(Horizontal::South.legacy())),
                (HEAD, StateValue::Byte(u8::from(head))),
                (OCCUPIED, StateValue::Byte(u8::from(occupied))),
            ],
        )
    }

    /// A night harness with a bed lying south from [`FOOT`] and a player
    /// with a runtime id (so its own packets are sent).
    fn night() -> Harness {
        let mut h = Harness::new();
        h.world.resource_mut::<WorldTime>().ticks = 18000;
        h.world.entity_mut(h.player).insert(RuntimeEntityId(1));
        h.put(FOOT, bed_state(false, false));
        h.put(HEAD_AT, bed_state(true, false));
        h
    }

    fn use_foot(h: &mut Harness) -> bool {
        let state = h.state(FOOT);
        use_bed(&mut h.world, h.player, FOOT, state)
    }

    fn sleeping_players(packets: &[McpePacketData]) -> Vec<Vec<u8>> {
        packets
            .iter()
            .filter_map(|packet| match packet {
                McpePacketData::LevelEventGenericPacket(event)
                    if event.event_id == LEVEL_EVENT_SLEEPING_PLAYERS =>
                {
                    Some(event.__ctd__.0.to_vec())
                }
                _ => None,
            })
            .collect()
    }

    /// The encoded sleeping-players compound with these counts.
    fn counts(able: i32, needed: i32, asleep: i32) -> Vec<u8> {
        let fields = BTreeMap::from([
            ("ableToSleep".to_owned(), Tag::Int(able)),
            ("overworldPlayerCount".to_owned(), Tag::Int(needed)),
            ("sleepingPlayerCount".to_owned(), Tag::Int(asleep)),
        ]);
        match generic_event(LEVEL_EVENT_SLEEPING_PLAYERS, &fields).data {
            McpePacketData::LevelEventGenericPacket(event) => event.__ctd__.0.to_vec(),
            _ => unreachable!(),
        }
    }

    fn animations(packets: &[McpePacketData]) -> usize {
        packets
            .iter()
            .filter(|packet| {
                matches!(packet, McpePacketData::AnimatePacket(animate)
                    if animate.action == EnumsAnimatePacketPayloadAction::Wakeup)
            })
            .count()
    }

    #[test]
    fn a_night_use_puts_the_player_to_sleep() {
        let mut h = night();
        h.packets();
        assert!(use_foot(&mut h));
        assert_eq!(h.state(HEAD_AT), bed_state(true, true));
        let sleeping = *h.world.get::<Sleeping>(h.player).expect("asleep");
        assert_eq!((sleeping.head, sleeping.ticks), (HEAD_AT, 0));
        assert_eq!(sleeping.entered, DVec3::new(0.5, 65.62, 0.5));
        assert_eq!(
            h.world.get::<Position>(h.player).unwrap().0,
            DVec3::new(0.5, 64.90625, 1.5)
        );
        let packets = h.packets();
        let data = packets
            .iter()
            .find_map(|packet| match packet {
                McpePacketData::SetActorDataPacket(data) => Some(data.actor_data.data.clone()),
                _ => None,
            })
            .expect("SetActorData");
        let flags = data
            .iter()
            .find_map(|item| match (&item.id, &item.payload) {
                (&ids::RESERVED_092, DataItemEntryPayload::DataItemInt64Payload(flags)) => {
                    Some(flags.value)
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(flags, 1 << (jolyne::valentine::actor_flags::SLEEPING - 64));
        assert!(data.iter().any(|item| item.id == ids::BED_POSITION
            && matches!(&item.payload, DataItemEntryPayload::DataItemPosPayload(pos)
                if (pos.value.x, pos.value.y, pos.value.z) == (HEAD_AT.x, HEAD_AT.y, HEAD_AT.z))));
        assert!(data.iter().any(|item| item.id == ids::PLAYER_FLAGS
            && matches!(&item.payload, DataItemEntryPayload::DataItemBytePayload(byte)
                if byte.value == PLAYER_FLAG_SLEEPING)));
        assert!(packets.iter().any(|packet| matches!(packet,
            McpePacketData::MovePlayerPacket(moved) if moved.position.y == 64.90625)));
        assert_eq!(sleeping_players(&packets), [counts(1, 1, 1)]);
    }

    #[test]
    fn a_hundred_ticks_wake_everyone_on_the_next_morning() {
        let mut h = night();
        assert!(use_foot(&mut h));
        h.packets();
        h.tick(u64::from(SLEEP_TICKS) - 1);
        assert!(h.world.get::<Sleeping>(h.player).is_some());
        assert_eq!(h.state(HEAD_AT), bed_state(true, true));
        h.tick(1);
        assert!(h.world.get::<Sleeping>(h.player).is_none());
        assert_eq!(h.state(HEAD_AT), bed_state(true, false));
        assert_eq!(h.world.resource::<WorldTime>().ticks, 24000);
        let packets = h.packets();
        assert_eq!(animations(&packets), 1);
        assert!(
            sleeping_players(&packets).is_empty(),
            "no event when everyone wakes"
        );
        // Beside the bed: the first spot is west of the head (the bed lies
        // south), 0.1 above the feet.
        assert_eq!(
            h.world.get::<Position>(h.player).unwrap().0,
            DVec3::new(-0.5, 64.0 + crate::entity::PLAYER_EYE_HEIGHT + 0.1, 1.5)
        );
    }

    #[test]
    fn without_the_daylight_cycle_the_time_stays() {
        let mut h = night();
        h.world.resource_mut::<GameRules>().do_daylight_cycle = false;
        assert!(use_foot(&mut h));
        h.tick(u64::from(SLEEP_TICKS));
        assert_eq!(h.state(HEAD_AT), bed_state(true, false));
        assert_eq!(h.world.resource::<WorldTime>().ticks, 18000);
    }

    #[test]
    fn leaving_the_bed_wakes_early_with_the_event() {
        let mut h = night();
        assert!(use_foot(&mut h));
        h.tick(10);
        h.packets();
        leave_bed(&mut h.world, h.player);
        assert!(h.world.get::<Sleeping>(h.player).is_none());
        assert_eq!(h.state(HEAD_AT), bed_state(true, false));
        let packets = h.packets();
        assert_eq!(animations(&packets), 1);
        assert_eq!(sleeping_players(&packets), [counts(1, 1, 0)]);
        h.tick(u64::from(SLEEP_TICKS));
        assert_eq!(h.world.resource::<WorldTime>().ticks, 18000);
        // Not asleep: leaving again does nothing.
        leave_bed(&mut h.world, h.player);
        assert!(sleeping_players(&h.packets()).is_empty());
    }

    #[test]
    fn a_stop_sleeping_action_reaches_the_bed() {
        use crate::server::game::packet_queues::{BlockAction, BlockPacketQueue};
        let mut h = night();
        assert!(use_foot(&mut h));
        h.world
            .get_resource_or_init::<BlockPacketQueue>()
            .0
            .push((h.player, BlockAction::StopSleeping));
        crate::server::game::blocks::apply_block_actions(&mut h.world);
        assert!(h.world.get::<Sleeping>(h.player).is_none());
        assert_eq!(h.state(HEAD_AT), bed_state(true, false));
    }

    #[test]
    fn a_sleeper_whose_bed_is_gone_wakes() {
        let mut h = night();
        assert!(use_foot(&mut h));
        h.set(HEAD_AT, state("minecraft:air", &[]));
        h.tick(1);
        assert!(h.world.get::<Sleeping>(h.player).is_none());
    }

    #[test]
    fn a_day_use_does_not_sleep() {
        let mut h = night();
        h.world.resource_mut::<WorldTime>().ticks = 6000;
        assert!(use_foot(&mut h));
        assert!(h.world.get::<Sleeping>(h.player).is_none());
        assert_eq!(h.state(HEAD_AT), bed_state(true, false));
    }

    #[test]
    fn a_thunderstorm_counts_as_night() {
        let mut h = night();
        h.world.resource_mut::<WorldTime>().ticks = 6000;
        h.world.insert_resource(crate::world::weather::Weather {
            state: crate::world::weather::WeatherState::Thunder,
            duration: None,
        });
        assert!(use_foot(&mut h));
        assert!(h.world.get::<Sleeping>(h.player).is_some());
    }

    #[test]
    fn the_wake_spot_turns_with_the_bed_and_the_side_entered_from() {
        let h = night();
        // The bed lies south (direction 0): the right hand is west.
        assert_eq!(
            wake_spot(&h.world, HEAD_AT, 0, None),
            Some(HEAD_AT - IVec3::X)
        );
        // Entering from the east takes the other hand.
        assert_eq!(
            wake_spot(&h.world, HEAD_AT, 0, Some(DVec3::new(3.0, 65.0, 1.5))),
            Some(HEAD_AT + IVec3::X)
        );
        // A bed lying north (direction 2): the right hand is east.
        assert_eq!(
            wake_spot(&h.world, HEAD_AT, 2, None),
            Some(HEAD_AT + IVec3::X)
        );
    }

    #[test]
    fn a_bed_without_room_beside_it_is_obstructed() {
        let mut h = night();
        let stone = state("minecraft:stone", &[]);
        for offset in WAKE_OFFSETS {
            let spot = HEAD_AT + rotate(offset, 0);
            if spot != FOOT && spot != HEAD_AT {
                h.put(spot, stone);
            }
        }
        // Above the head and the foot.
        h.put(HEAD_AT + IVec3::Y * 2, stone);
        h.put(FOOT + IVec3::Y * 2, stone);
        assert_eq!(wake_spot(&h.world, HEAD_AT, 0, None), None);
        assert!(use_foot(&mut h));
        assert!(h.world.get::<Sleeping>(h.player).is_none());
    }
}

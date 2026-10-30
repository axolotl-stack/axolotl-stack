//! Blocks power acts on, each on circuit ticks ([`super::circuit`]).
//!
//! - **Doors** (wooden and iron), **trapdoors** and **fence gates** open
//!   when they become powered and close when they lose power (a door when
//!   either half gets power; only the lower half's `open_bit` changes),
//!   with their open/close sounds; a fence gate keeps its direction, as
//!   BDS 1.26.51 does. Only a change of power acts, so a player may still
//!   open or close a wooden one by hand while the power stays the same. A
//!   trapdoor or fence gate placed where power reaches opens at the next
//!   circuit tick; a door placed there stays closed until the power
//!   changes.
//! - **Redstone lamps** light in the circuit tick their power appears and
//!   go out 4 game ticks after the circuit tick that found them unpowered,
//!   unless powered again by then, as BDS 1.26.51 does.
//! - **Note blocks** play their note when they become powered (placing one
//!   in power plays nothing), as BDS 1.26.51 does: a `BlockEvent` at the
//!   block with the instrument as its type and the pitch as its value, and
//!   the LevelSoundEvent `note` at the block's corner (its integer
//!   position) with data `instrument × 256 + pitch`; the instrument comes
//!   from the block below (0 on dirt, 1 on stone as measured, the rest
//!   provisional: Minecraft Wiki) and the pitch from the block actor's
//!   `note` (0 without one). Under a block they stay silent (provisional:
//!   Minecraft Wiki).

use std::collections::{HashMap, HashSet};

use bevy_ecs::prelude::*;
use glam::IVec3;

use super::super::{bit, centre};
use super::power;
use crate::entity::systems::BlockSource;
use crate::registry::block::BlockEntry;

/// Game ticks a lamp stays lit after it lost power.
pub const LAMP_OFF_DELAY: u64 = 4;

/// Circuit tick each lamp that lost power goes out at.
#[derive(Resource, Debug, Default)]
pub struct LampsGoingOut(HashMap<IVec3, u64>);

/// Positions of doors (by lower half), trapdoors, fence gates and note
/// blocks that were powered at their last update.
#[derive(Resource, Debug, Default)]
pub struct PoweredBlocks(HashSet<IVec3>);

/// Record whether the block keyed `key` is powered; `true` when that is a
/// change.
fn power_changed(world: &mut World, key: IVec3, powered: bool) -> bool {
    let mut blocks = world.get_resource_or_init::<PoweredBlocks>();
    if powered {
        blocks.0.insert(key)
    } else {
        blocks.0.remove(&key)
    }
}

fn powered(world: &World, pos: IVec3) -> bool {
    power::received(world, pos, true) > 0
}

/// The lower half of the door half `state` at `pos`.
fn lower_half(world: &World, pos: IVec3, state: u32) -> IVec3 {
    let upper = world
        .registry()
        .get_by_runtime_id(state)
        .is_some_and(|entry| bit(entry, state, super::super::doors::UPPER_BIT));
    if upper { pos - IVec3::Y } else { pos }
}

fn door_powered(world: &World, lower: IVec3) -> bool {
    powered(world, lower) || powered(world, lower + IVec3::Y)
}

/// The door half `state` at `pos` was itself set (placed, toggled): it
/// records its power without acting, so a door placed in power stays
/// closed.
pub fn record_door(world: &mut World, pos: IVec3, state: u32) {
    let lower = lower_half(world, pos, state);
    let on = door_powered(world, lower);
    power_changed(world, lower, on);
}

/// A circuit evaluation of the door half `state` at `pos`.
pub fn door_evaluate(world: &mut World, pos: IVec3, state: u32) {
    let lower = lower_half(world, pos, state);
    let on = door_powered(world, lower);
    if power_changed(world, lower, on)
        && let Some(lower_state) = world.block_at(lower)
    {
        super::super::doors::set_open(world, lower, lower_state, on);
    }
}

/// A circuit evaluation of the trapdoor `state` at `pos`.
pub fn trapdoor_evaluate(world: &mut World, pos: IVec3, state: u32) {
    let on = powered(world, pos);
    if power_changed(world, pos, on) {
        super::super::toggles::power_trapdoor(world, pos, state, on);
    }
}

/// A circuit evaluation of the fence gate `state` at `pos`.
pub fn fence_gate_evaluate(world: &mut World, pos: IVec3, state: u32) {
    let on = powered(world, pos);
    if power_changed(world, pos, on) {
        super::super::toggles::power_fence_gate(world, pos, state, on);
    }
}

/// A circuit evaluation of the lamp at `pos`: powered, it lights (and
/// forgets going out); unpowered and lit, it goes out [`LAMP_OFF_DELAY`]
/// ticks after the evaluation that found it unpowered.
pub fn lamp_evaluate(world: &mut World, pos: IVec3, lit: bool) {
    let now = super::circuit::now(world);
    if powered(world, pos) {
        world.get_resource_or_init::<LampsGoingOut>().0.remove(&pos);
        if !lit {
            set_lamp(world, pos, "minecraft:lit_redstone_lamp");
        }
        return;
    }
    if !lit {
        return;
    }
    let going_out = world
        .get_resource::<LampsGoingOut>()
        .and_then(|lamps| lamps.0.get(&pos).copied());
    match going_out {
        Some(due) if due <= now => {
            world.resource_mut::<LampsGoingOut>().0.remove(&pos);
            set_lamp(world, pos, "minecraft:redstone_lamp");
        }
        Some(_) => {}
        None => {
            let due = now + LAMP_OFF_DELAY;
            world
                .get_resource_or_init::<LampsGoingOut>()
                .0
                .insert(pos, due);
            super::circuit::request_at(world, pos, due);
        }
    }
}

fn set_lamp(world: &mut World, pos: IVec3, name: &str) {
    if let Some(state) = world.registry().default_state_id_by_name(name) {
        crate::server::game::blocks::set_block(world, pos, state, false);
    }
}

/// The note block at `pos` was itself set: it records its power without
/// playing.
pub fn record_note_block(world: &mut World, pos: IVec3) {
    let on = powered(world, pos);
    power_changed(world, pos, on);
}

/// A circuit evaluation of the note block at `pos`: it plays when it
/// became powered.
pub fn note_block_evaluate(world: &mut World, pos: IVec3) {
    let on = powered(world, pos);
    if power_changed(world, pos, on) && on {
        play_note(world, pos);
    }
}

/// Play the note of the note block at `pos` (nothing under a block): the
/// `BlockEvent` and the `note` sound at the block's corner.
pub fn play_note(world: &World, pos: IVec3) {
    if super::super::name_at(world, pos + IVec3::Y) != Some("minecraft:air") {
        return;
    }
    let instrument = power::at(world, pos - IVec3::Y).map_or(0, |(entry, _)| instrument(entry));
    let pitch = super::diode::actor_int(world, pos, "note").map_or(0, |note| note.clamp(0, 24));
    let packet = jolyne::valentine::McpePacket::from(jolyne::valentine::BlockEventPacket {
        block_position: jolyne::valentine::BlockPos {
            x: pos.x,
            y: pos.y,
            z: pos.z,
        },
        event_type: instrument,
        event_value: pitch,
    });
    crate::entity::mob::net::sound::send_to_viewers(world, centre(pos), &packet);
    crate::entity::mob::net::sound::play_level_sound(
        world,
        None,
        pos.as_dvec3(),
        "note",
        (instrument << 8) | pitch,
    );
}

/// The instrument a note block plays on top of `below`: 0 harp, 1 bass
/// drum, 2 snare, 3 hat, 4 bass, 5 bell, 6 flute, 7 chime, 8 guitar,
/// 9 xylophone, 10 iron xylophone, 11 cow bell, 12 didgeridoo, 13 bit,
/// 14 banjo, 15 pling (provisional: Minecraft Wiki).
pub fn instrument(below: &BlockEntry) -> i32 {
    let name = below.string_id.trim_start_matches("minecraft:");
    match name {
        "gold_block" => return 5,
        "clay" => return 6,
        "packed_ice" => return 7,
        "bone_block" => return 9,
        "iron_block" => return 10,
        "soul_sand" => return 11,
        "pumpkin" => return 12,
        "emerald_block" => return 13,
        "hay_block" => return 14,
        "glowstone" => return 15,
        "sea_lantern" | "beacon" => return 3,
        _ => {}
    }
    if name.ends_with("_wool") {
        8
    } else if below.data.material == GLASS_MATERIAL {
        3
    } else if below.has_tag("sand") || below.has_tag("gravel") || name.ends_with("_concrete_powder")
    {
        2
    } else if below.data.material == WOOD_MATERIAL || below.has_tag("wood") || below.has_tag("log")
    {
        4
    } else if below.has_tag("stone") || below.has_tag("minecraft:is_pickaxe_item_destructible") {
        1
    } else {
        0
    }
}

/// Block material ids of wood and glass.
const WOOD_MATERIAL: u8 = 2;
const GLASS_MATERIAL: u8 = 11;

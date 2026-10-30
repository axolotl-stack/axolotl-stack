//! Repeaters and comparators: one input at the back, output at the front,
//! side inputs.
//!
//! `minecraft:cardinal_direction` names the side the input comes from (a
//! repeater or comparator placed looking east gets `west`, as BDS 1.26.51
//! does); the output goes the other way. Timing runs on circuit ticks
//! ([`super::circuit`]).
//!
//! - **Repeaters** (`unpowered_repeater`/`powered_repeater`,
//!   `repeater_delay` 0..=3) switch 0, 2, 4 or 6 game ticks after the
//!   circuit tick that sees their input change (a 1-tick repeater is no
//!   slower than dust; with the wait for that circuit tick, 2/4/6/8 ticks
//!   after a lever) and give full power (strong and weak) into the block in
//!   front; use cycles the delay, as BDS 1.26.51 does. A pulse shorter
//!   than the delay still comes out as one delay long (provisional:
//!   Minecraft Wiki). A powered repeater or comparator pointing into either
//!   side locks the repeater in its current state (only repeaters and
//!   comparators lock); a lock taken or released is seen one circuit tick
//!   later, as BDS 1.26.51 does.
//! - **Comparators** (`unpowered_comparator`/`powered_comparator`,
//!   `output_subtract_bit`, `output_lit_bit`) read the back (the fullness
//!   of a container behind it, or of one behind an unpowered conductor
//!   behind it; otherwise the power arriving there) and the stronger side
//!   (dust, repeaters and comparators only). In compare mode the output is
//!   the back when no side is stronger, else 0; in subtract mode the back
//!   minus the side. The block turns `powered_comparator` with
//!   `output_lit_bit` (or back) and takes its new level in the circuit
//!   tick that sees the change; dust after it follows one circuit tick
//!   later ([`super::circuit`]), as BDS 1.26.51 does. Use toggles the mode
//!   and plays LevelSoundEvent `power.off` into
//!   subtract and `power.on` back to compare, carrying the comparator's new
//!   state, as BDS 1.26.51 does.
//!
//! A comparator's output level is kept per position; one without a kept
//! level outputs what its block actor's `OutputSignal` says (0 without an
//! actor) until its next update.

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use glam::IVec3;

use super::super::{bit, int, name_at, with};
use super::{Kind, kind, power};
use crate::block_entity::{BlockContainer, ContainerKind};
use crate::entity::systems::BlockSource;
use crate::registry::block::{BlockEntry, StateValue};
use crate::server::game::placement::Horizontal;
use crate::server::game::types::{BlockRegistryResource, ItemRegistryResource};

pub const DIRECTION: &str = "minecraft:cardinal_direction";
pub const DELAY: &str = "repeater_delay";
pub const SUBTRACT: &str = "output_subtract_bit";
pub const LIT: &str = "output_lit_bit";

/// Game ticks per redstone tick.
pub const REDSTONE_TICK: u64 = 2;
/// LevelSoundEvent a comparator plays when it switches to subtract mode.
pub const SOUND_SUBTRACT: &str = "power.off";
/// LevelSoundEvent a comparator plays when it switches to compare mode.
pub const SOUND_COMPARE: &str = "power.on";

const REPEATER_PROPERTIES: [&str; 2] = [DIRECTION, DELAY];
const COMPARATOR_PROPERTIES: [&str; 2] = [DIRECTION, SUBTRACT];

/// Comparator output levels by position.
#[derive(Resource, Debug, Default)]
pub struct ComparatorOutputs(HashMap<IVec3, u8>);

/// Circuit tick of each repeater's pending switch.
#[derive(Resource, Debug, Default)]
pub struct RepeaterSwitches(HashMap<IVec3, u64>);

/// The offset towards the side the input comes from.
pub fn input_side(entry: &BlockEntry, state: u32) -> Option<IVec3> {
    match entry.state_value(state, DIRECTION)? {
        StateValue::Str(value) => Horizontal::from_cardinal(value).map(Horizontal::offset),
        _ => None,
    }
}

/// The offset towards the block the output goes into.
pub fn output(entry: &BlockEntry, state: u32) -> Option<IVec3> {
    input_side(entry, state).map(|side| -side)
}

/// The two side offsets of a diode whose input is at `back`.
fn sides(back: IVec3) -> [IVec3; 2] {
    [
        IVec3::new(back.z, 0, -back.x),
        IVec3::new(-back.z, 0, back.x),
    ]
}

/// Power arriving at the back of the diode at `pos`.
fn rear_power(world: &World, pos: IVec3, back: IVec3) -> u8 {
    power::signal(world, pos + back, -back, true)
}

/// Whether a powered repeater or comparator points into a side of the
/// repeater at `pos`.
fn locked(world: &World, pos: IVec3, back: IVec3) -> bool {
    sides(back).into_iter().any(|side| {
        let n = pos + side;
        power::at(world, n).is_some_and(|(entry, _)| {
            matches!(
                kind(entry),
                Some(Kind::Repeater { .. } | Kind::Comparator { .. })
            )
        }) && power::emitted(world, n, -side, false, true) > 0
    })
}

/// Game ticks a repeater state waits after the circuit tick that saw its
/// input change: 0, 2, 4 or 6.
fn repeater_wait(entry: &BlockEntry, state: u32) -> u64 {
    int(entry, state, DELAY).unwrap_or(0).clamp(0, 3) as u64 * REDSTONE_TICK
}

/// `state` of one block moved to the block `to`, keeping `properties`.
fn swap(world: &World, state: u32, to: &str, properties: &[&str]) -> Option<u32> {
    let registry = world.registry();
    let from = registry.get_by_runtime_id(state)?;
    let target = registry.get_by_name(to)?;
    let mut next = target.default_state_id;
    for property in properties {
        if let Some(value) = from.state_value(state, property) {
            next = with(target, next, property, value);
        }
    }
    Some(next)
}

/// A circuit evaluation of the repeater `state` at `pos`: a pending switch
/// that is due runs; otherwise an input that differs from the output
/// starts the delay (a 1-tick repeater switches at once).
pub fn repeater_evaluate(world: &mut World, pos: IVec3, state: u32, powered: bool) {
    let now = super::circuit::now(world);
    let pending = world
        .get_resource::<RepeaterSwitches>()
        .and_then(|switches| switches.0.get(&pos).copied());
    match pending {
        Some(due) if due > now => {}
        Some(_) => {
            world.resource_mut::<RepeaterSwitches>().0.remove(&pos);
            repeater_switch(world, pos, state, powered);
        }
        None => {
            let Some((entry, _)) = power::at(world, pos) else {
                return;
            };
            let Some(back) = input_side(entry, state) else {
                return;
            };
            let wait = repeater_wait(entry, state);
            if locked(world, pos, back) || (rear_power(world, pos, back) > 0) == powered {
                return;
            }
            if wait == 0 {
                repeater_switch(world, pos, state, powered);
            } else {
                pend(world, pos, now + wait);
            }
        }
    }
}

fn pend(world: &mut World, pos: IVec3, due: u64) {
    world
        .get_resource_or_init::<RepeaterSwitches>()
        .0
        .insert(pos, due);
    super::circuit::request_at(world, pos, due);
}

/// The repeater at `pos` switches: off when its input is gone, on
/// otherwise; a pulse shorter than the delay still lasts one delay (at
/// least one circuit tick).
fn repeater_switch(world: &mut World, pos: IVec3, state: u32, powered: bool) {
    let Some((entry, _)) = power::at(world, pos) else {
        return;
    };
    let Some(back) = input_side(entry, state) else {
        return;
    };
    let wait = repeater_wait(entry, state).max(REDSTONE_TICK);
    if locked(world, pos, back) {
        return;
    }
    let should = rear_power(world, pos, back) > 0;
    let to = if powered && !should {
        "minecraft:unpowered_repeater"
    } else if !powered {
        "minecraft:powered_repeater"
    } else {
        return;
    };
    let Some(next) = swap(world, state, to, &REPEATER_PROPERTIES) else {
        return;
    };
    super::circuit::switched(world, pos);
    crate::server::game::blocks::set_block(world, pos, next, false);
    if !powered && !should {
        let now = super::circuit::now(world);
        pend(world, pos, now + wait);
    }
}

/// Use: the next delay (1 → 2 → 3 → 4 → 1 redstone ticks).
pub fn cycle_delay(world: &mut World, pos: IVec3, state: u32) -> bool {
    let Some((entry, _)) = power::at(world, pos) else {
        return false;
    };
    let delay = (int(entry, state, DELAY).unwrap_or(0) + 1) % 4;
    let next = with(entry, state, DELAY, StateValue::Int(delay));
    crate::server::game::blocks::set_block(world, pos, next, false);
    true
}

/// The output level of the comparator at `pos` (see the module docs).
pub fn stored_output(world: &World, pos: IVec3) -> u8 {
    if let Some(output) = world
        .get_resource::<ComparatorOutputs>()
        .and_then(|outputs| outputs.0.get(&pos))
    {
        return *output;
    }
    actor_int(world, pos, "OutputSignal").map_or(0, |level| level.clamp(0, 15) as u8)
}

/// An integer field of the block actor at `pos`, read from its chunk.
pub fn actor_int(world: &World, pos: IVec3, key: &str) -> Option<i32> {
    use zuri_nbt::NBTTag;
    let compound = actor_compound(world, pos)?;
    match compound.0.get(key)? {
        NBTTag::Int(value) => Some(value.0),
        NBTTag::Short(value) => Some(i32::from(value.0)),
        NBTTag::Byte(value) => Some(i32::from(value.0)),
        _ => None,
    }
}

/// The saved block actor at `pos`.
fn actor_compound(world: &World, pos: IVec3) -> Option<zuri_nbt::tag::Compound> {
    let (cx, cz) = crate::world::ecs::world_to_chunk_coords(pos.x, pos.z);
    let chunk = world
        .get_resource::<crate::world::ecs::ChunkManager>()?
        .get_by_coords(cx, cz)?;
    let data = world.get::<crate::world::ecs::ChunkData>(chunk)?;
    let actor = data
        .inner
        .block_entities()
        .iter()
        .find(|actor| actor.position() == pos.to_array())?;
    actor.bedrock_compound()?.ok()
}

/// How full the container at `pos` is, as a comparator reads it: 0 when
/// empty, else `1 + ⌊fill × 14⌋` with `fill` the mean of each slot's
/// count over its stack limit (both halves of a double chest). `None`
/// when the block is not a container with contents.
pub fn container_level(world: &World, pos: IVec3) -> Option<u8> {
    let kind = ContainerKind::from_block(name_at(world, pos)?)?;
    if kind == ContainerKind::EnderChest {
        return None;
    }
    let first = decoded(world, kind, pos);
    let partner = first
        .as_ref()
        .and_then(|c| c.pair)
        .map(|pair| IVec3::new(pair.x, pos.y, pair.z))
        .filter(|other| name_at(world, *other) == name_at(world, pos))
        .and_then(|other| decoded(world, kind, other));
    let mut slots = 0usize;
    let mut fill = 0.0f32;
    for container in first.iter().chain(partner.iter()) {
        slots += container.items.size();
        for slot in 0..container.items.size() {
            if let Some(stack) = container.items.item(slot).filter(|s| !s.is_empty()) {
                fill += f32::from(stack.count) / f32::from(stack.max_stack_size().max(1));
            }
        }
    }
    Some(fullness(fill, slots))
}

/// `1 + ⌊fill / slots × 14⌋` when anything is stored, else 0.
pub fn fullness(fill: f32, slots: usize) -> u8 {
    if fill <= 0.0 || slots == 0 {
        return 0;
    }
    (1.0 + (fill / slots as f32) * 14.0).floor().min(15.0) as u8
}

fn decoded(world: &World, kind: ContainerKind, pos: IVec3) -> Option<BlockContainer> {
    let compound = actor_compound(world, pos)?;
    let items = &world.get_resource::<ItemRegistryResource>()?.0;
    let blocks = &world.get_resource::<BlockRegistryResource>()?.0;
    Some(BlockContainer::decode(kind, pos, compound, items, blocks))
}

/// The output the comparator `state` at `pos` computes from its inputs.
pub fn comparator_target(world: &World, pos: IVec3, entry: &BlockEntry, state: u32) -> u8 {
    let Some(back) = input_side(entry, state) else {
        return 0;
    };
    let rear_pos = pos + back;
    let mut rear = rear_power(world, pos, back);
    if let Some(level) = container_level(world, rear_pos) {
        rear = level;
    } else if rear == 0
        && power::is_conductor(world, rear_pos)
        && let Some(level) = container_level(world, rear_pos + back)
    {
        rear = level;
    }
    let side = sides(back)
        .into_iter()
        .map(|side| side_power(world, pos + side, -side))
        .max()
        .unwrap_or(0);
    if bit(entry, state, SUBTRACT) {
        rear.saturating_sub(side)
    } else if side > rear {
        0
    } else {
        rear
    }
}

/// Side input from the block at `n` into the comparator at `n + dir`.
fn side_power(world: &World, n: IVec3, dir: IVec3) -> u8 {
    let Some((entry, state)) = power::at(world, n) else {
        return 0;
    };
    match kind(entry) {
        Some(Kind::Dust) => power::level(entry, state),
        Some(Kind::Repeater { .. } | Kind::Comparator { .. }) => {
            power::emitted(world, n, dir, false, true)
        }
        _ => 0,
    }
}

/// A circuit evaluation of the comparator `state` at `pos`: it takes its
/// new output level and shows whether it is above 0.
pub fn comparator_evaluate(world: &mut World, pos: IVec3, state: u32, powered: bool) {
    let Some((entry, _)) = power::at(world, pos) else {
        return;
    };
    let target = comparator_target(world, pos, entry, state);
    let lit = bit(entry, state, LIT);
    let level = stored_output(world, pos);
    let on = target > 0;
    let next = if on == powered && on == lit {
        state
    } else {
        let to = if on {
            "minecraft:powered_comparator"
        } else {
            "minecraft:unpowered_comparator"
        };
        swap(world, state, to, &COMPARATOR_PROPERTIES)
            .and_then(|next| {
                let target_entry = world.registry().get_by_runtime_id(next)?;
                Some(with(
                    target_entry,
                    next,
                    LIT,
                    StateValue::Byte(u8::from(on)),
                ))
            })
            .unwrap_or(state)
    };
    if target == level && next == state {
        return;
    }
    world
        .get_resource_or_init::<ComparatorOutputs>()
        .0
        .insert(pos, target);
    super::circuit::switched(world, pos);
    if next != state {
        crate::server::game::blocks::set_block(world, pos, next, false);
    } else {
        // Same block, new level: the neighbours re-read it.
        super::super::block_changed(world, pos);
    }
}

/// Use: toggle compare/subtract with a click.
pub fn toggle_mode(world: &mut World, pos: IVec3, state: u32) -> bool {
    let Some((entry, _)) = power::at(world, pos) else {
        return false;
    };
    let subtract = !bit(entry, state, SUBTRACT);
    let next = with(entry, state, SUBTRACT, StateValue::Byte(u8::from(subtract)));
    crate::server::game::blocks::set_block(world, pos, next, false);
    super::super::sound(
        world,
        pos,
        if subtract {
            SOUND_SUBTRACT
        } else {
            SOUND_COMPARE
        },
        next,
    );
    true
}

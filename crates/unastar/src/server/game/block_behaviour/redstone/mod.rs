//! Redstone components and power, as BDS 1.26.51 does where measured;
//! rules marked provisional come from public Bedrock behaviour.
//!
//! - [`power`]: what each block sends into each neighbour (strong and
//!   weak) and what a block receives; [`power_at`] is what a consumer at a
//!   position gets.
//! - Sources: levers (`open_bit`) and buttons (`button_pressed_bit`) give
//!   15 weakly to every neighbour and strongly to the block they are
//!   attached to; pressure plates ([`plates`]); redstone blocks give 15
//!   weakly to every neighbour; redstone torches ([`torch`]).
//! - [`dust`], [`diode`] (repeaters and comparators) and [`consumers`]
//!   (doors, trapdoors, fence gates, lamps, note blocks).
//! - [`observer`]s pulse when the block they watch changes; [`piston`]s
//!   push and pull blocks.
//!
//! # Updates
//!
//! A change reaches the six neighbours of the changed block through the
//! change path; a conductor next to a change passes it on to the
//! components and consumers around it ([`relay`]), so a lever reaches a
//! door beside the block the lever is on, and a broken source is noticed
//! two blocks away. Observers react only to the change path itself, never
//! to a relay: a conductor in front of one whose power changes has not
//! changed.
//!
//! # Timing
//!
//! Components change only on circuit ticks, every second game tick
//! ([`circuit`]): a reaction to a change made outside an evaluation waits
//! in the circuit queue for the next circuit tick, which
//! [`super::tick_blocks`] runs after the scheduled ticks. Button releases,
//! pressure plate checks, observers and pistons keep their own scheduled
//! ticks.

pub mod circuit;
pub mod consumers;
pub mod diode;
pub mod dust;
pub mod observer;
pub mod piston;
pub mod plates;
pub mod power;

#[cfg(test)]
mod bds_parity;
#[cfg(test)]
mod tests;
pub mod torch;

use bevy_ecs::prelude::*;
use glam::IVec3;

pub use plates::press_plates;

use super::{Family, NEIGHBOURS, Place, SupportKind, int, provides_support, with};
use crate::entity::systems::BlockSource;
use crate::registry::block::{BlockEntry, StateValue};
use crate::server::game::placement::{Horizontal, face_offset};

/// Power level state of dust and pressure plates.
pub const SIGNAL: &str = "redstone_signal";
/// Lever state.
pub const OPEN: &str = "open_bit";
/// Button state.
pub const PRESSED: &str = "button_pressed_bit";

/// Which redstone part a block is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Dust,
    Repeater {
        powered: bool,
    },
    Comparator {
        powered: bool,
    },
    Plate(Plate),
    Lamp {
        lit: bool,
    },
    NoteBlock,
    RedstoneBlock,
    Torch {
        lit: bool,
    },
    Lever,
    Button,
    Observer,
    Piston {
        sticky: bool,
    },
    /// A piston's arm.
    PistonHead {
        sticky: bool,
    },
    /// A block on its way to where a piston moves it.
    MovingBlock,
}

/// Pressure plate kinds (see [`plates`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plate {
    Wooden,
    Stone,
    Light,
    Heavy,
}

/// The redstone part `entry` is, if any.
pub fn kind(entry: &BlockEntry) -> Option<Kind> {
    let id = entry.string_id.strip_prefix("minecraft:")?;
    Some(match id {
        "redstone_wire" => Kind::Dust,
        "unpowered_repeater" => Kind::Repeater { powered: false },
        "powered_repeater" => Kind::Repeater { powered: true },
        "unpowered_comparator" => Kind::Comparator { powered: false },
        "powered_comparator" => Kind::Comparator { powered: true },
        "redstone_lamp" => Kind::Lamp { lit: false },
        "lit_redstone_lamp" => Kind::Lamp { lit: true },
        "noteblock" => Kind::NoteBlock,
        "redstone_block" => Kind::RedstoneBlock,
        "redstone_torch" => Kind::Torch { lit: true },
        "unlit_redstone_torch" => Kind::Torch { lit: false },
        "observer" => Kind::Observer,
        "piston" => Kind::Piston { sticky: false },
        "sticky_piston" => Kind::Piston { sticky: true },
        "piston_arm_collision" => Kind::PistonHead { sticky: false },
        "sticky_piston_arm_collision" => Kind::PistonHead { sticky: true },
        "moving_block" => Kind::MovingBlock,
        "light_weighted_pressure_plate" => Kind::Plate(Plate::Light),
        "heavy_weighted_pressure_plate" => Kind::Plate(Plate::Heavy),
        _ if id.ends_with("_pressure_plate") => Kind::Plate(if id.contains("stone") {
            Plate::Stone
        } else {
            Plate::Wooden
        }),
        _ if entry
            .state_value(entry.default_state_id, super::attached::LEVER_DIRECTION)
            .is_some() =>
        {
            Kind::Lever
        }
        _ if entry.state_value(entry.default_state_id, PRESSED).is_some() => Kind::Button,
        _ => return None,
    })
}

/// Whether `entry` is a redstone component with its own family
/// ([`Family::Redstone`]).
pub fn is_component(entry: &BlockEntry) -> bool {
    let id = entry.string_id.as_str();
    id.ends_with("_pressure_plate")
        || matches!(
            id,
            "minecraft:redstone_wire"
                | "minecraft:unpowered_repeater"
                | "minecraft:powered_repeater"
                | "minecraft:unpowered_comparator"
                | "minecraft:powered_comparator"
                | "minecraft:redstone_lamp"
                | "minecraft:lit_redstone_lamp"
                | "minecraft:noteblock"
                | "minecraft:redstone_block"
                | "minecraft:observer"
                | "minecraft:piston"
                | "minecraft:sticky_piston"
                | "minecraft:piston_arm_collision"
                | "minecraft:sticky_piston_arm_collision"
                | "minecraft:moving_block"
        )
}

/// Whether blocks of `family` react to power (doors, trapdoors, fence
/// gates, redstone torches).
pub fn is_consumer(family: Family) -> bool {
    matches!(
        family,
        Family::Door { .. } | Family::Trapdoor { .. } | Family::FenceGate | Family::Torch
    )
}

/// The offset from a lever, button or torch to the block it hangs on.
pub fn attached_offset(entry: &BlockEntry, state: u32) -> Option<IVec3> {
    use super::attached::{FACING, LEVER_DIRECTION, TORCH_FACING};
    if let Some(StateValue::Str(value)) = entry.state_value(state, TORCH_FACING) {
        return Some(Horizontal::from_cardinal(value).map_or(IVec3::NEG_Y, Horizontal::offset));
    }
    let face = match entry.state_value(state, LEVER_DIRECTION) {
        Some(StateValue::Str(value)) => match value {
            "down_east_west" | "down_north_south" => 0,
            "up_north_south" | "up_east_west" => 1,
            "north" => 2,
            "south" => 3,
            "west" => 4,
            "east" => 5,
            _ => return None,
        },
        _ => int(entry, state, FACING)?,
    };
    face_offset(u8::try_from(face).ok()?).map(|offset| -offset)
}

/// Power the block at `pos` receives from its neighbours.
pub fn power_at(world: &World, pos: IVec3) -> u8 {
    power::received(world, pos, true)
}

/// Repeaters and comparators react to use.
pub fn is_usable(entry: &BlockEntry) -> bool {
    matches!(
        kind(entry),
        Some(Kind::Repeater { .. } | Kind::Comparator { .. })
    )
}

/// Dust needs a full top face below, or glowstone or a redstone lamp
/// below, as BDS 1.26.51 does; repeaters and comparators a full top face;
/// pressure plates the middle of one (provisional: Minecraft Wiki).
pub fn can_survive(blocks: &impl BlockSource, pos: IVec3, _state: u32, entry: &BlockEntry) -> bool {
    let below = pos - IVec3::Y;
    let full_top = || provides_support(blocks, below, IVec3::Y, SupportKind::Full);
    match kind(entry) {
        Some(Kind::Dust) => {
            full_top()
                || matches!(
                    super::name_at(blocks, below),
                    Some(
                        "minecraft:glowstone"
                            | "minecraft:redstone_lamp"
                            | "minecraft:lit_redstone_lamp"
                    )
                )
        }
        Some(Kind::Repeater { .. } | Kind::Comparator { .. }) => full_top(),
        Some(Kind::Plate(_)) => provides_support(blocks, below, IVec3::Y, SupportKind::Centre),
        _ => true,
    }
}

/// Components are placed unpowered (repeaters with the shortest delay,
/// comparators in compare mode) and take their power on the update that
/// follows; observers and pistons face the placing player's look.
pub fn placement(place: &Place<'_, impl BlockSource>) -> Option<u32> {
    let block = place.block;
    let state = place.state;
    Some(match kind(block) {
        Some(Kind::Dust | Kind::Plate(_)) => with(block, state, SIGNAL, StateValue::Int(0)),
        Some(Kind::Repeater { .. }) => with(block, state, diode::DELAY, StateValue::Int(0)),
        Some(Kind::Comparator { .. }) => {
            let state = with(block, state, diode::SUBTRACT, StateValue::Byte(0));
            with(block, state, diode::LIT, StateValue::Byte(0))
        }
        Some(Kind::Observer) => {
            observer::placement(block, state, place.input.yaw, place.input.pitch)
        }
        Some(Kind::Piston { .. }) => {
            piston::placement(block, state, place.input.yaw, place.input.pitch)
        }
        _ => state,
    })
}

/// A neighbour of `pos` changed at `from` (`from == pos`: the block itself
/// was just set). Lamps and note blocks also pass the change on, as
/// conductors ([`relay`]).
pub fn neighbour_changed(world: &mut World, pos: IVec3, from: IVec3) {
    let Some(state) = world.block_at(pos) else {
        return;
    };
    let part = world.registry().get_by_runtime_id(state).and_then(kind);
    match part {
        Some(Kind::Observer) => return observer::changed(world, pos, from),
        Some(Kind::Piston { .. }) => return piston::changed(world, pos, from),
        Some(Kind::PistonHead { .. }) => return piston::arm_changed(world, pos),
        Some(Kind::MovingBlock) => return piston::moving_changed(world, pos),
        _ => {}
    }
    react(world, pos, state, from);
    let conducting = part.is_some_and(|part| matches!(part, Kind::Lamp { .. } | Kind::NoteBlock));
    if conducting && from != pos {
        relay(world, pos, state, from);
    }
}

/// The block `state` at `pos` re-reads its power after a change at `from`:
/// inside a circuit evaluation at once, except that dust, torches,
/// repeaters and comparators see a torch, repeater or comparator that
/// switched in this evaluation only at the next one; outside an evaluation
/// at the next circuit tick ([`circuit`]). A door or note block that was
/// itself just set only records its power.
fn react(world: &mut World, pos: IVec3, state: u32, from: IVec3) {
    let Some(part) = world.registry().get_by_runtime_id(state).map(kind) else {
        return;
    };
    match part {
        Some(Kind::Piston { .. }) => return piston::update(world, pos),
        Some(
            Kind::Plate(_)
            | Kind::RedstoneBlock
            | Kind::Lever
            | Kind::Button
            | Kind::Observer
            | Kind::PistonHead { .. }
            | Kind::MovingBlock,
        ) => return,
        Some(Kind::Torch { .. }) if torch::burnt_out(world, pos) => return,
        Some(Kind::NoteBlock) if from == pos => return consumers::record_note_block(world, pos),
        None => match super::family(world, state) {
            Family::Door { .. } if from == pos => {
                return consumers::record_door(world, pos, state);
            }
            Family::Door { .. } | Family::Trapdoor { .. } | Family::FenceGate => {}
            _ => return,
        },
        _ => {}
    }
    let reads_outputs = matches!(
        part,
        Some(Kind::Dust | Kind::Torch { .. } | Kind::Repeater { .. } | Kind::Comparator { .. })
    );
    let later =
        !circuit::evaluating(world) || (reads_outputs && circuit::near_switched(world, pos));
    if later {
        circuit::request(world, pos);
    } else {
        evaluate_state(world, pos, state);
    }
}

/// A circuit evaluation of the block at `pos`.
fn evaluate(world: &mut World, pos: IVec3) {
    if let Some(state) = world.block_at(pos) {
        evaluate_state(world, pos, state);
    }
}

fn evaluate_state(world: &mut World, pos: IVec3, state: u32) {
    let Some(part) = world.registry().get_by_runtime_id(state).map(kind) else {
        return;
    };
    match part {
        Some(Kind::Dust) => dust::update(world, pos),
        Some(Kind::Repeater { powered }) => diode::repeater_evaluate(world, pos, state, powered),
        Some(Kind::Comparator { powered }) => {
            diode::comparator_evaluate(world, pos, state, powered);
        }
        Some(Kind::Torch { lit }) => torch::evaluate(world, pos, state, lit),
        Some(Kind::Lamp { lit }) => consumers::lamp_evaluate(world, pos, lit),
        Some(Kind::NoteBlock) => consumers::note_block_evaluate(world, pos),
        Some(_) => {}
        None => match super::family(world, state) {
            Family::Door { .. } => consumers::door_evaluate(world, pos, state),
            Family::Trapdoor { .. } => consumers::trapdoor_evaluate(world, pos, state),
            Family::FenceGate => consumers::fence_gate_evaluate(world, pos, state),
            _ => {}
        },
    }
}

/// A neighbour of the non-redstone block `state` at `pos` changed at
/// `from`: a conductor passes the change on to the redstone components
/// and consumers around it (power it holds may have changed).
pub fn relay(world: &mut World, pos: IVec3, state: u32, from: IVec3) {
    let conductor = world
        .registry()
        .get_by_runtime_id(state)
        .is_some_and(power::conducts);
    if !conductor {
        return;
    }
    for offset in NEIGHBOURS {
        let n = pos + offset;
        if n == from {
            continue;
        }
        let Some(neighbour) = world.block_at(n) else {
            continue;
        };
        let family = super::family(world, neighbour);
        if family == Family::Redstone || is_consumer(family) {
            react(world, n, neighbour, pos);
        }
    }
}

/// A due scheduled tick of a redstone component (pressure plate checks,
/// observers, pistons; the rest runs on circuit ticks).
pub fn scheduled_tick(world: &mut World, pos: IVec3, state: u32) {
    let Some(part) = world.registry().get_by_runtime_id(state).map(kind) else {
        return;
    };
    match part {
        Some(Kind::Plate(_)) => plates::check(world, pos),
        Some(Kind::Observer) => observer::tick(world, pos, state),
        Some(Kind::Piston { .. }) => piston::tick(world, pos),
        _ => {}
    }
}

/// Use: a repeater cycles its delay, a comparator toggles its mode.
pub fn use_block(world: &mut World, _player: Entity, pos: IVec3, state: u32) -> bool {
    let Some(part) = world.registry().get_by_runtime_id(state).map(kind) else {
        return false;
    };
    match part {
        Some(Kind::Repeater { .. }) => diode::cycle_delay(world, pos, state),
        Some(Kind::Comparator { .. }) => diode::toggle_mode(world, pos, state),
        _ => false,
    }
}

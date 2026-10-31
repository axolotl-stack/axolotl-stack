//! Power: what each block sends into each neighbour, strong and weak, and
//! what a block receives.
//!
//! A source sends **weak** power into its neighbours and **strong** power
//! into the block it is attached to (or points at). A conductor (an opaque
//! full block that is not itself a redstone component: stone, planks,
//! wool, lamps, note blocks; not glass, slabs, redstone blocks, observers
//! or pistons) holds the
//! strongest power sent into it strongly and passes it to every
//! neighbour. Dust counts as a source for everything except other dust: a
//! block powered by dust powers lamps, repeaters and doors next to it, but
//! not dust (provisional: Minecraft Wiki).

use bevy_ecs::prelude::*;
use glam::IVec3;

use super::super::{HORIZONTAL, NEIGHBOURS, SupportKind, int, provides_support};
use super::{Kind, attached_offset, kind};
use crate::entity::systems::BlockSource;
use crate::registry::block::BlockEntry;

/// Full power.
pub const MAX: u8 = 15;

/// The block at `pos` and its state.
pub fn at(world: &World, pos: IVec3) -> Option<(&BlockEntry, u32)> {
    let state = world.block_at(pos)?;
    Some((world.registry().get_by_runtime_id(state)?, state))
}

/// Whether `entry` conducts power (see the module docs).
pub fn conducts(entry: &BlockEntry) -> bool {
    if entry.is_transparent {
        return false;
    }
    match kind(entry) {
        None | Some(Kind::Lamp { .. } | Kind::NoteBlock) => true,
        Some(_) => false,
    }
}

pub fn is_conductor(world: &World, pos: IVec3) -> bool {
    at(world, pos).is_some_and(|(entry, _)| conducts(entry))
}

pub fn is_dust(world: &World, pos: IVec3) -> bool {
    at(world, pos).is_some_and(|(entry, _)| kind(entry) == Some(Kind::Dust))
}

/// `redstone_signal` of the dust at `pos` (0 for anything else).
pub fn dust_signal(world: &World, pos: IVec3) -> u8 {
    match at(world, pos) {
        Some((entry, state)) if kind(entry) == Some(Kind::Dust) => level(entry, state),
        _ => 0,
    }
}

/// `redstone_signal` of a state, clamped to 0..=15.
pub fn level(entry: &BlockEntry, state: u32) -> u8 {
    int(entry, state, super::SIGNAL).map_or(0, |v| v.clamp(0, 15) as u8)
}

/// Power a utility block sends into its neighbour at `pos + dir`: a
/// lectern during its page-turn pulse, a jukebox while its disc plays, an
/// active sculk sensor.
fn utility_power(world: &World, pos: IVec3, dir: IVec3, entry: &BlockEntry, state: u32) -> u8 {
    use super::super::utility::{Utility, jukebox, lectern, sculk};
    match Utility::of(entry) {
        Some(Utility::Lectern) => lectern::power(entry, state),
        Some(Utility::Jukebox) => jukebox::power(world, pos),
        Some(Utility::Sculk) => sculk::power(world, pos, dir, entry, state),
        _ => 0,
    }
}

/// Power the block at `src` sends into its neighbour at `src + dir`:
/// strong power only with `strong`; dust counts only with `dust`.
pub fn emitted(world: &World, src: IVec3, dir: IVec3, strong: bool, dust: bool) -> u8 {
    let Some((entry, state)) = at(world, src) else {
        return 0;
    };
    let Some(kind) = kind(entry) else {
        // Lecterns pulse on a page turn, jukebox discs play and sculk
        // sensors activate: weak power into their neighbours.
        return if strong {
            0
        } else {
            utility_power(world, src, dir, entry, state)
        };
    };
    let on = |power: u8, strong_dir: IVec3| {
        if !strong || dir == strong_dir {
            power
        } else {
            0
        }
    };
    match kind {
        Kind::Lever | Kind::Button => {
            let property = if kind == Kind::Lever {
                super::OPEN
            } else {
                super::PRESSED
            };
            match attached_offset(entry, state) {
                Some(attached) if super::super::bit(entry, state, property) => on(MAX, attached),
                _ => 0,
            }
        }
        Kind::Plate(plate) => {
            let stored = level(entry, state);
            let power = match plate {
                super::Plate::Wooden | super::Plate::Stone if stored > 0 => MAX,
                _ => stored,
            };
            on(power, IVec3::NEG_Y)
        }
        Kind::RedstoneBlock if !strong => MAX,
        Kind::Torch { lit: true } => {
            let attached = attached_offset(entry, state).unwrap_or(IVec3::NEG_Y);
            if strong {
                if dir == IVec3::Y { MAX } else { 0 }
            } else if dir == attached {
                0
            } else {
                MAX
            }
        }
        Kind::Repeater { powered: true } if super::diode::output(entry, state) == Some(dir) => MAX,
        Kind::Comparator { .. } if super::diode::output(entry, state) == Some(dir) => {
            super::diode::stored_output(world, src)
        }
        Kind::Observer
            if super::observer::powered(entry, state)
                && super::observer::output(entry, state) == Some(dir) =>
        {
            MAX
        }
        Kind::Dust if dust => {
            let power = level(entry, state);
            if power == 0 || dir == IVec3::Y {
                0
            } else if dir == IVec3::NEG_Y || dust_points(world, src, dir) {
                power
            } else {
                0
            }
        }
        _ => 0,
    }
}

/// The strongest strong power sent into the block at `pos`.
pub fn strong_into(world: &World, pos: IVec3, dust: bool) -> u8 {
    let mut best = 0;
    for offset in NEIGHBOURS {
        best = best.max(emitted(world, pos + offset, -offset, true, dust));
        if best == MAX {
            break;
        }
    }
    best
}

/// Whether the torch, repeater or comparator at `src` sends its output
/// (strong power with `strong`) into its neighbour at `src + dir`, on or
/// off.
pub fn output_side(world: &World, src: IVec3, dir: IVec3, strong: bool) -> bool {
    let Some((entry, state)) = at(world, src) else {
        return false;
    };
    match kind(entry) {
        Some(Kind::Torch { .. }) => {
            if strong {
                dir == IVec3::Y
            } else {
                dir != attached_offset(entry, state).unwrap_or(IVec3::NEG_Y)
            }
        }
        Some(Kind::Repeater { .. } | Kind::Comparator { .. }) => {
            super::diode::output(entry, state) == Some(dir)
        }
        _ => false,
    }
}

/// Power the block at `n` gives its neighbour at `n + dir`: a conductor
/// passes the strong power it holds, anything else what it emits.
pub fn signal(world: &World, n: IVec3, dir: IVec3, dust: bool) -> u8 {
    if is_conductor(world, n) {
        strong_into(world, n, dust)
    } else {
        emitted(world, n, dir, false, dust)
    }
}

/// Power the block at `pos` receives from its six neighbours.
pub fn received(world: &World, pos: IVec3, dust: bool) -> u8 {
    let mut best = 0;
    for offset in NEIGHBOURS {
        best = best.max(signal(world, pos + offset, -offset, dust));
        if best == MAX {
            break;
        }
    }
    best
}

/// Whether dust at `pos` connects towards the horizontal `side`: to dust
/// beside it, one block down past a non-conductor or one block up onto a
/// block it can stand on (unless a conductor sits on this dust), to
/// repeaters along their axis and to every other power source.
pub fn dust_connects(world: &World, pos: IVec3, side: IVec3) -> bool {
    let n = pos + side;
    let Some((entry, state)) = at(world, n) else {
        return false;
    };
    match kind(entry) {
        Some(Kind::Dust) => return true,
        Some(Kind::Repeater { .. }) => {
            return super::diode::input_side(entry, state)
                .is_some_and(|input| input == side || input == -side);
        }
        Some(Kind::Observer) => return super::observer::output(entry, state) == Some(-side),
        Some(
            Kind::Lamp { .. }
            | Kind::NoteBlock
            | Kind::Piston { .. }
            | Kind::PistonHead { .. }
            | Kind::MovingBlock,
        )
        | None => {}
        Some(_) => return true,
    }
    if !conducts(entry) && is_dust(world, n - IVec3::Y) {
        return true;
    }
    !is_conductor(world, pos + IVec3::Y)
        && provides_support(world, n, IVec3::Y, SupportKind::Full)
        && is_dust(world, n + IVec3::Y)
}

/// Whether dust at `pos` points into its neighbour on the horizontal
/// `side`: dust with no connection points every way; otherwise it points
/// along its connections, and a single connection on one axis runs on
/// through the opposite side.
pub fn dust_points(world: &World, pos: IVec3, side: IVec3) -> bool {
    let connected = HORIZONTAL.map(|s| dust_connects(world, pos, s));
    if !connected.iter().any(|c| *c) {
        return true;
    }
    let index = |s: IVec3| HORIZONTAL.iter().position(|h| *h == s);
    let (Some(this), Some(back)) = (index(side), index(-side)) else {
        return false;
    };
    connected[this]
        || (connected[back]
            && HORIZONTAL
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != this && *i != back)
                .all(|(i, _)| !connected[i]))
}

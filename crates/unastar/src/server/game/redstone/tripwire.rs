//! Tripwire hooks and the string between them, as BDS 1.26.51 links and
//! powers them (measured with `tools/bds-sweep/plates.py`,
//! `data/measured/redstone/plates_tripwire*.json`; replayed in
//! `redstone/tests.rs`).
//!
//! - A line is a hook, string straight ahead of it and a hook facing back.
//!   A line is brought up to date when a hook or string of it is placed and
//!   when its string's power changes: complete, both hooks and all its
//!   string take `attached_bit` and the hooks power while any of its string
//!   is powered; broken, the hooks (a hook finds the far hook across a gap)
//!   and the string between them lose `attached_bit` and the hooks their
//!   power. Setting string to air brings nothing up to date: a cut line
//!   stays attached until its string is next pressed or released.
//! - String is pressed like a pressure plate (`super::plates`, kind
//!   [`super::plates::PlateKind::Tripwire`]), with or without hooks: only
//!   the string an actor touches stores `powered_bit`; the hooks change in
//!   the tick the string does.

use glam::IVec3;

use crate::entity::systems::BlockSource;
use crate::registry::block::{BlockEntry, StateValue};

pub(crate) const TRIPWIRE: &str = "minecraft:trip_wire";
pub(crate) const HOOK: &str = "minecraft:tripwire_hook";

/// The most string a hook looks along for the hook at the other end. Not
/// measured: the sweep's cells hold lines of at most 18 strings.
pub(crate) const MAX_STRING: i32 = 40;

/// The horizontal directions string runs in.
const SIDES: [IVec3; 4] = [IVec3::NEG_Z, IVec3::Z, IVec3::NEG_X, IVec3::X];

/// The direction a hook of `direction` (0 south, 1 west, 2 north, 3 east)
/// faces: the way its string runs.
fn facing(direction: i32) -> Option<IVec3> {
    Some(match direction {
        0 => IVec3::Z,
        1 => IVec3::NEG_X,
        2 => IVec3::NEG_Z,
        3 => IVec3::X,
        _ => return None,
    })
}

fn entry_at(blocks: &impl BlockSource, pos: IVec3) -> Option<(&BlockEntry, u32)> {
    let state = blocks.block_at(pos)?;
    Some((blocks.registry().get_by_runtime_id(state)?, state))
}

fn byte(entry: &BlockEntry, state: u32, name: &str) -> bool {
    entry.state_value(state, name) == Some(StateValue::Byte(1))
}

fn with_byte(entry: &BlockEntry, state: u32, name: &str, on: bool) -> u32 {
    entry
        .with_state_value(state, name, StateValue::Byte(u8::from(on)))
        .unwrap_or(state)
}

/// The facing of the hook at `pos`, if a hook is there.
pub(crate) fn hook_facing(blocks: &impl BlockSource, pos: IVec3) -> Option<IVec3> {
    let (entry, state) = entry_at(blocks, pos)?;
    if entry.string_id != HOOK {
        return None;
    }
    match entry.state_value(state, "direction")? {
        StateValue::Int(direction) => facing(direction),
        _ => None,
    }
}

fn is_string(blocks: &impl BlockSource, pos: IVec3) -> bool {
    entry_at(blocks, pos).is_some_and(|(entry, _)| entry.string_id == TRIPWIRE)
}

/// Whether string at `pos` links towards `side`: string there, or a hook
/// there facing back at it (BDS 1.26.51: the string beside a hook links to
/// it).
pub(crate) fn links_to(blocks: &impl BlockSource, pos: IVec3, side: IVec3) -> bool {
    is_string(blocks, pos + side) || hook_facing(blocks, pos + side) == Some(-side)
}

/// What lies ahead of the hook at `hook` facing `dir`: the positions up to
/// the next hook within [`MAX_STRING`] + 1 (scanning across anything but
/// hooks, BDS 1.26.51: a hook finds the far hook of a cut line), and that
/// hook if it faces back.
fn line(blocks: &impl BlockSource, hook: IVec3, dir: IVec3) -> (Vec<IVec3>, Option<IVec3>) {
    let mut between = Vec::new();
    for step in 1..=MAX_STRING + 1 {
        let at = hook + dir * step;
        // The scan ends at the first hook: the far hook only if it faces back.
        if let Some(facing) = hook_facing(blocks, at) {
            return (between, (facing == -dir).then_some(at));
        }
        between.push(at);
    }
    (between, None)
}

/// The hooks whose line `pos` (a hook or string) is part of: each with its
/// facing. From string, along each side over string only.
fn hooks_of(blocks: &impl BlockSource, pos: IVec3) -> Vec<(IVec3, IVec3)> {
    if let Some(dir) = hook_facing(blocks, pos) {
        return vec![(pos, dir)];
    }
    let mut hooks = Vec::new();
    for side in SIDES {
        for step in 1..=MAX_STRING + 1 {
            let at = pos + side * step;
            if is_string(blocks, at) {
                continue;
            }
            if hook_facing(blocks, at) == Some(-side) {
                hooks.push((at, -side));
            }
            break;
        }
    }
    hooks
}

/// The writes that bring the lines of `changed` (hooks and string just
/// placed, or string whose power changed) up to date. A line with string
/// all the way to the far hook is attached (both hooks and the string),
/// and its hooks are powered while any of its string is. Otherwise the hook,
/// the far hook it found and the string between are detached and the hooks
/// unpowered (BDS 1.26.51: stepping on a line cut by setting its middle
/// string to air detaches it, `plates_tripwire_more.json`).
pub(crate) fn update_lines(blocks: &impl BlockSource, changed: &[IVec3]) -> Vec<(IVec3, u32)> {
    let mut writes: Vec<(IVec3, u32)> = Vec::new();
    let mut set = |at: IVec3, attached: bool, powered: Option<bool>| {
        if writes.iter().any(|(done, _)| *done == at) {
            return;
        }
        let Some((entry, state)) = entry_at(blocks, at) else {
            return;
        };
        let mut next = with_byte(entry, state, "attached_bit", attached);
        if let Some(powered) = powered {
            next = with_byte(entry, next, "powered_bit", powered);
        }
        if next != state {
            writes.push((at, next));
        }
    };
    for pos in changed {
        // Only hooks and string: string set to air updates nothing.
        if !is_string(blocks, *pos) && hook_facing(blocks, *pos).is_none() {
            continue;
        }
        for (hook, dir) in hooks_of(blocks, *pos) {
            let (between, end) = line(blocks, hook, dir);
            let strings: Vec<IVec3> = between
                .iter()
                .copied()
                .filter(|at| is_string(blocks, *at))
                .collect();
            let complete = end.is_some() && strings.len() == between.len();
            let powered = complete
                && strings.iter().any(|at| {
                    entry_at(blocks, *at)
                        .is_some_and(|(entry, state)| byte(entry, state, "powered_bit"))
                });
            for at in [Some(hook), end].into_iter().flatten() {
                set(at, complete, Some(powered));
            }
            if end.is_some() {
                for at in strings {
                    set(at, complete, None);
                }
            }
        }
    }
    writes
}

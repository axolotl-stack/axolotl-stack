//! Tripwire hooks and the string between them, as BDS 1.26.51 links and
//! powers them (measured with `tools/bds-sweep/plates.py`,
//! `data/measured/redstone/plates_tripwire*.json`; replayed in
//! `redstone/tests.rs`).
//!
//! - A line is a hook, string straight ahead of it and a hook facing back.
//!   Placing a hook or string that completes a line attaches the line: both
//!   hooks and all its string take `attached_bit`. Removing string does not
//!   detach anything (BDS keeps the hooks and the rest of the string
//!   attached after the string between them is set to air).
//! - String is pressed like a pressure plate (`super::plates`, kind
//!   [`super::plates::PlateKind::Tripwire`]): only the string an actor
//!   touches stores `powered_bit`.
//! - A hook stores `powered_bit` (and powers like a switch) while any string
//!   straight ahead of it, up to the next hook, is powered; it changes in the
//!   tick the string does.

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

/// The string straight ahead of the hook at `hook` facing `dir`, and the hook
/// facing back that ends it, if there is one within [`MAX_STRING`].
fn line(blocks: &impl BlockSource, hook: IVec3, dir: IVec3) -> (Vec<IVec3>, Option<IVec3>) {
    let mut string = Vec::new();
    for step in 1..=MAX_STRING + 1 {
        let at = hook + dir * step;
        if is_string(blocks, at) {
            string.push(at);
            continue;
        }
        let end = (hook_facing(blocks, at) == Some(-dir)).then_some(at);
        return (string, end);
    }
    (string, None)
}

/// The hooks whose line `pos` (a hook or string) is part of: each with its
/// facing.
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

/// The writes that attach a line `pos` (a hook or string just placed)
/// completes: both hooks and the string between them take `attached_bit`.
pub(crate) fn attach(blocks: &impl BlockSource, pos: IVec3) -> Vec<(IVec3, u32)> {
    let mut writes = Vec::new();
    for (hook, dir) in hooks_of(blocks, pos) {
        let (string, Some(end)) = line(blocks, hook, dir) else {
            continue;
        };
        for at in [hook, end].into_iter().chain(string) {
            let Some((entry, state)) = entry_at(blocks, at) else {
                continue;
            };
            let next = with_byte(entry, state, "attached_bit", true);
            if next != state && !writes.iter().any(|(done, _)| *done == at) {
                writes.push((at, next));
            }
        }
    }
    writes
}

/// The hooks to change after the string at `changed` changed its power:
/// each hook of those lines powered while any string ahead of it is.
pub(crate) fn hook_power(blocks: &impl BlockSource, changed: &[IVec3]) -> Vec<(IVec3, u32)> {
    let mut writes: Vec<(IVec3, u32)> = Vec::new();
    for pos in changed {
        for (hook, dir) in hooks_of(blocks, *pos) {
            if writes.iter().any(|(done, _)| *done == hook) {
                continue;
            }
            let (string, _) = line(blocks, hook, dir);
            let powered = string.iter().any(|at| {
                entry_at(blocks, *at)
                    .is_some_and(|(entry, state)| byte(entry, state, "powered_bit"))
            });
            let Some((entry, state)) = entry_at(blocks, hook) else {
                continue;
            };
            let next = with_byte(entry, state, "powered_bit", powered);
            if next != state {
                writes.push((hook, next));
            }
        }
    }
    writes
}

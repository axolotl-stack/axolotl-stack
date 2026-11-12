//! Neighbour-dependent block states, measured on BDS 1.26.51.
//!
//! - Stairs with the native tag `minecraft:cornerable_stairs` take
//!   `minecraft:corner` from the stairs in front of and behind them
//!   (`measured/block_updates/stairs.json`, every centre with one or two
//!   neighbouring stairs, both placement orders, mixed materials). With
//!   `weirdo_direction` 0..3 = east, west, south, north as the facing and
//!   `upside_down_bit` as the half: stairs of the same half in front, facing
//!   across, give an outer corner (`outer_left` when they face the facing's
//!   counter-clockwise turn) unless the stairs on the side they face from
//!   share this stair's facing and half; stairs behind give an inner corner
//!   by the same test on the side they face; otherwise `none`.
//! - Fences, panes, bars and walls: [`connected_state`].
//! - A fence gate is `in_wall_bit` when a wall stands on either side along
//!   its panel, open or closed (`measured/block_updates/gate_in_wall.json`);
//!   fences, panes and other blocks do not count.

use glam::IVec3;

use crate::entity::systems::BlockSource;
use crate::registry::block::{BlockEntry, StateValue};
use crate::registry::collision::{Connectable, Side, connectable, connected_state};

/// Native tag of the stairs that take corners.
pub(crate) const CORNERABLE_STAIRS: &str = "minecraft:cornerable_stairs";

/// `state` of `entry` at `pos` with its neighbour-dependent properties
/// re-derived from the neighbours.
pub(crate) fn shaped(blocks: &impl BlockSource, pos: IVec3, entry: &BlockEntry, state: u32) -> u32 {
    let registry = blocks.registry();
    let mut state = connected_state(registry, pos, state, |at| blocks.block_at(at));
    if entry.has_tag(CORNERABLE_STAIRS) {
        state = stair_corner(blocks, pos, entry, state);
    }
    if entry.state_value(state, "in_wall_bit").is_some() {
        state = gate_in_wall(blocks, pos, entry, state);
    }
    if entry.string_id == TRIPWIRE {
        state = tripwire_links(blocks, pos, entry, state);
    }
    state
}

const TRIPWIRE: &str = "minecraft:trip_wire";

/// A tripwire links (`minecraft:connection_<side>`) to the tripwire beside
/// it on each side, and they link back (BDS 1.26.51: string placed beside
/// one tripwire and between two, `testdata/bds-placements-leftovers.json`).
/// Links to tripwire hooks are not measured.
pub(crate) fn tripwire_links(
    blocks: &impl BlockSource,
    pos: IVec3,
    entry: &BlockEntry,
    state: u32,
) -> u32 {
    let registry = blocks.registry();
    let mut state = state;
    for (side, offset) in [
        ("north", IVec3::NEG_Z),
        ("south", IVec3::Z),
        ("west", IVec3::NEG_X),
        ("east", IVec3::X),
    ] {
        let linked = blocks
            .block_at(pos + offset)
            .and_then(|other| registry.get_by_runtime_id(other))
            .is_some_and(|other| other.string_id == TRIPWIRE);
        state = entry
            .with_state_value(
                state,
                &format!("minecraft:connection_{side}"),
                StateValue::Byte(u8::from(linked)),
            )
            .unwrap_or(state);
    }
    state
}

/// A tripwire just placed starts unpowered, unattached and armed, and
/// `suspended` (BDS 1.26.51: a tripwire set with any of those states in a
/// stone shell reads back so, `block_updates/support.json`; string on stone,
/// glass, a slab, grass and in the air too, though the place sound carries
/// the state before this, `testdata/bds-placements-leftovers.json`); its links
/// follow from its neighbours ([`shaped`]).
pub(crate) fn placed_tripwire(blocks: &impl BlockSource, pos: IVec3) -> Option<u32> {
    let state = blocks.block_at(pos)?;
    let entry = blocks.registry().get_by_runtime_id(state)?;
    if entry.string_id != TRIPWIRE {
        return None;
    }
    let mut next = state;
    for (property, value) in [
        ("attached_bit", 0),
        ("disarmed_bit", 0),
        ("powered_bit", 0),
        ("suspended_bit", 1),
    ] {
        next = entry
            .with_state_value(next, property, StateValue::Byte(value))
            .unwrap_or(next);
    }
    (next != state).then_some(next)
}

/// Facing (`weirdo_direction`: 0 east, 1 west, 2 south, 3 north) and upside
/// down half of a stair state.
pub(crate) fn stair_facing(entry: &BlockEntry, state: u32) -> Option<(Side, bool)> {
    let Some(StateValue::Int(direction)) = entry.state_value(state, "weirdo_direction") else {
        return None;
    };
    let facing = match direction {
        0 => Side::East,
        1 => Side::West,
        2 => Side::South,
        3 => Side::North,
        _ => return None,
    };
    let upside_down = entry.state_value(state, "upside_down_bit") == Some(StateValue::Byte(1));
    Some((facing, upside_down))
}

/// The side a quarter turn counter-clockwise (seen from above).
fn counter_clockwise(side: Side) -> Side {
    match side {
        Side::North => Side::West,
        Side::West => Side::South,
        Side::South => Side::East,
        Side::East => Side::North,
    }
}

fn stair_corner(blocks: &impl BlockSource, pos: IVec3, entry: &BlockEntry, state: u32) -> u32 {
    let Some((facing, upside_down)) = stair_facing(entry, state) else {
        return state;
    };
    let registry = blocks.registry();
    let stairs_at = |at: IVec3| {
        let state = blocks.block_at(at)?;
        let entry = registry.get_by_runtime_id(state)?;
        entry
            .has_tag(CORNERABLE_STAIRS)
            .then(|| stair_facing(entry, state))
            .flatten()
    };
    // A corner needs the side it turns toward free of stairs continuing
    // this stair's run.
    let can_turn = |side: Side| {
        stairs_at(pos + side.offset()).is_none_or(|(f, u)| f != facing || u != upside_down)
    };
    let across = |other: Side| other.along_x() != facing.along_x();
    let corner = match stairs_at(pos + facing.offset()) {
        Some((front, half))
            if half == upside_down && across(front) && can_turn(front.opposite()) =>
        {
            if front == counter_clockwise(facing) {
                "outer_left"
            } else {
                "outer_right"
            }
        }
        _ => match stairs_at(pos + facing.opposite().offset()) {
            Some((back, half)) if half == upside_down && across(back) && can_turn(back) => {
                if back == counter_clockwise(facing) {
                    "inner_left"
                } else {
                    "inner_right"
                }
            }
            _ => "none",
        },
    };
    entry
        .with_state_value(state, "minecraft:corner", StateValue::Str(corner))
        .unwrap_or(state)
}

fn gate_in_wall(blocks: &impl BlockSource, pos: IVec3, entry: &BlockEntry, state: u32) -> u32 {
    let spans_x = matches!(
        entry.state_value(state, "minecraft:cardinal_direction"),
        Some(StateValue::Str("north" | "south"))
    );
    let sides = if spans_x {
        [Side::East, Side::West]
    } else {
        [Side::North, Side::South]
    };
    let registry = blocks.registry();
    let in_wall = sides.into_iter().any(|side| {
        blocks
            .block_at(pos + side.offset())
            .and_then(|state| registry.get_by_runtime_id(state))
            .is_some_and(|neighbour| connectable(neighbour) == Some(Connectable::Wall))
    });
    entry
        .with_state_value(state, "in_wall_bit", StateValue::Byte(u8::from(in_wall)))
        .unwrap_or(state)
}

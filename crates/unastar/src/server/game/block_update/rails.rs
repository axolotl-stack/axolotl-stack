//! Rail shapes from neighbouring rails, measured on BDS 1.26.51
//! (`crates/unastar-data/data/measured/block_updates/rails.json`: a rail,
//! powered, detector or activator rail with no, a flat or a raised rail on
//! each side, placed before or after them).
//!
//! A placed rail connects to the neighbouring rails (on its level, one block
//! up or one block down) that are not already joined at both ends: one
//! neighbour gives a straight rail toward it, two at a corner a curve (never
//! for the powered kinds), and a straight rail rises toward a rail one block
//! up (with rails on both axes, a powered, detector or activator rail lies
//! east-west). Each neighbour it connects to then turns toward it. Placing the rail
//! itself fixes its shape: BDS does not re-shape a rail when a neighbour
//! changes later, except through that neighbour's own placement.
//! `rail_direction`: 0 north-south, 1 east-west, 2-5 rising east, west,
//! north, south, 6 south-east, 7 south-west, 8 north-west, 9 north-east.

use glam::IVec3;

use super::UpdateTarget;
use crate::entity::systems::BlockSource;
use crate::registry::block::{BlockEntry, StateValue};

const NORTH: IVec3 = IVec3::NEG_Z;
const SOUTH: IVec3 = IVec3::Z;
const WEST: IVec3 = IVec3::NEG_X;
const EAST: IVec3 = IVec3::X;

const NORTH_SOUTH: i32 = 0;
const EAST_WEST: i32 = 1;
const ASCENDING_EAST: i32 = 2;
const ASCENDING_WEST: i32 = 3;
const ASCENDING_NORTH: i32 = 4;
const ASCENDING_SOUTH: i32 = 5;
const SOUTH_EAST: i32 = 6;
const SOUTH_WEST: i32 = 7;
const NORTH_WEST: i32 = 8;
const NORTH_EAST: i32 = 9;

/// Whether `entry` is a rail, and whether it only lies straight (powered,
/// detector and activator rails).
pub(crate) fn rail_kind(entry: &BlockEntry) -> Option<bool> {
    match entry.string_id.as_str() {
        "minecraft:rail" => Some(false),
        "minecraft:golden_rail" | "minecraft:detector_rail" | "minecraft:activator_rail" => {
            Some(true)
        }
        _ => None,
    }
}

/// The rising side of a sloped rail's `rail_direction`.
pub(crate) fn slope(direction: i32) -> Option<IVec3> {
    match direction {
        ASCENDING_EAST => Some(EAST),
        ASCENDING_WEST => Some(WEST),
        ASCENDING_NORTH => Some(NORTH),
        ASCENDING_SOUTH => Some(SOUTH),
        _ => None,
    }
}

/// A rail and the positions its shape joins.
struct Rail {
    pos: IVec3,
    state: u32,
    straight: bool,
    connections: Vec<IVec3>,
}

fn direction(entry: &BlockEntry, state: u32) -> i32 {
    match entry.state_value(state, "rail_direction") {
        Some(StateValue::Int(value)) => value,
        _ => NORTH_SOUTH,
    }
}

fn connections(pos: IVec3, shape: i32) -> Vec<IVec3> {
    match shape {
        NORTH_SOUTH => vec![pos + NORTH, pos + SOUTH],
        EAST_WEST => vec![pos + WEST, pos + EAST],
        ASCENDING_EAST => vec![pos + WEST, pos + EAST + IVec3::Y],
        ASCENDING_WEST => vec![pos + WEST + IVec3::Y, pos + EAST],
        ASCENDING_NORTH => vec![pos + NORTH + IVec3::Y, pos + SOUTH],
        ASCENDING_SOUTH => vec![pos + NORTH, pos + SOUTH + IVec3::Y],
        SOUTH_EAST => vec![pos + EAST, pos + SOUTH],
        SOUTH_WEST => vec![pos + WEST, pos + SOUTH],
        NORTH_WEST => vec![pos + WEST, pos + NORTH],
        NORTH_EAST => vec![pos + EAST, pos + NORTH],
        _ => Vec::new(),
    }
}

fn is_rail(blocks: &impl BlockSource, pos: IVec3) -> bool {
    blocks
        .block_at(pos)
        .and_then(|state| blocks.registry().get_by_runtime_id(state))
        .is_some_and(|entry| rail_kind(entry).is_some())
}

/// The rail exactly at `pos`.
fn rail_at(blocks: &impl BlockSource, pos: IVec3) -> Option<Rail> {
    let state = blocks.block_at(pos)?;
    let entry = blocks.registry().get_by_runtime_id(state)?;
    let straight = rail_kind(entry)?;
    Some(Rail {
        pos,
        state,
        straight,
        connections: connections(pos, direction(entry, state)),
    })
}

/// The rail at `pos`, or one block above or below it.
fn rail_near(blocks: &impl BlockSource, pos: IVec3) -> Option<Rail> {
    rail_at(blocks, pos)
        .or_else(|| rail_at(blocks, pos + IVec3::Y))
        .or_else(|| rail_at(blocks, pos - IVec3::Y))
}

impl Rail {
    /// Whether a connection of this rail lands on `pos`'s column.
    fn has_connection(&self, pos: IVec3) -> bool {
        self.connections
            .iter()
            .any(|c| c.x == pos.x && c.z == pos.z)
    }

    /// Drops the connections whose rail does not connect back.
    fn remove_soft_connections(&mut self, blocks: &impl BlockSource) {
        let pos = self.pos;
        self.connections = std::mem::take(&mut self.connections)
            .into_iter()
            .filter_map(|c| {
                let other = rail_near(blocks, c)?;
                other.has_connection(pos).then_some(other.pos)
            })
            .collect();
    }

    fn can_connect_to(&self, other: IVec3) -> bool {
        self.has_connection(other) || self.connections.len() != 2
    }

    /// The shape this rail takes when its neighbour at `other` joins it.
    fn connect_to(&mut self, blocks: &impl BlockSource, other: IVec3) -> i32 {
        self.connections.push(other);
        let pos = self.pos;
        let [n, s, w, e] = [NORTH, SOUTH, WEST, EAST].map(|d| self.has_connection(pos + d));
        let mut shape = None;
        if n || s {
            shape = Some(NORTH_SOUTH);
        }
        if w || e {
            shape = Some(EAST_WEST);
        }
        if !self.straight {
            if s && e && !n && !w {
                shape = Some(SOUTH_EAST);
            }
            if s && w && !n && !e {
                shape = Some(SOUTH_WEST);
            }
            if n && w && !s && !e {
                shape = Some(NORTH_WEST);
            }
            if n && e && !s && !w {
                shape = Some(NORTH_EAST);
            }
        }
        let shape = rise(blocks, pos, shape);
        let entry = blocks.registry().get_by_runtime_id(self.state);
        let shape =
            shape.unwrap_or_else(|| entry.map_or(NORTH_SOUTH, |e| direction(e, self.state)));
        self.connections = connections(pos, shape);
        shape
    }
}

/// A straight shape rises toward a rail one block up.
fn rise(blocks: &impl BlockSource, pos: IVec3, shape: Option<i32>) -> Option<i32> {
    let up = |d: IVec3| is_rail(blocks, pos + d + IVec3::Y);
    match shape {
        Some(NORTH_SOUTH) => Some(if up(SOUTH) {
            ASCENDING_SOUTH
        } else if up(NORTH) {
            ASCENDING_NORTH
        } else {
            NORTH_SOUTH
        }),
        Some(EAST_WEST) => Some(if up(WEST) {
            ASCENDING_WEST
        } else if up(EAST) {
            ASCENDING_EAST
        } else {
            EAST_WEST
        }),
        other => other,
    }
}

fn with_direction(blocks: &impl BlockSource, state: u32, shape: i32) -> u32 {
    blocks
        .registry()
        .get_by_runtime_id(state)
        .and_then(|entry| entry.with_state_value(state, "rail_direction", StateValue::Int(shape)))
        .unwrap_or(state)
}

/// A rail was placed at `pos`: give it its shape from the neighbouring rails
/// and turn the neighbours it joins toward it. Returns the positions whose
/// state changed.
pub(crate) fn placed<T: UpdateTarget>(target: &mut T, pos: IVec3) -> Vec<IVec3> {
    let Some(rail) = rail_at(target, pos) else {
        return Vec::new();
    };
    let neighbour = |blocks: &T, side: IVec3| {
        rail_near(blocks, pos + side).is_some_and(|mut other| {
            other.remove_soft_connections(blocks);
            other.can_connect_to(pos)
        })
    };
    let [n, s, w, e] = [NORTH, SOUTH, WEST, EAST].map(|side| neighbour(target, side));
    let (ns, ew) = (n || s, w || e);
    let mut shape = None;
    if ns && !ew {
        shape = Some(NORTH_SOUTH);
    }
    if ew && !ns {
        shape = Some(EAST_WEST);
    }
    let (se, sw, ne, nw) = (s && e, s && w, n && e, n && w);
    if !rail.straight {
        if se && !n && !w {
            shape = Some(SOUTH_EAST);
        }
        if sw && !n && !e {
            shape = Some(SOUTH_WEST);
        }
        if nw && !s && !e {
            shape = Some(NORTH_WEST);
        }
        if ne && !s && !w {
            shape = Some(NORTH_EAST);
        }
    }
    let starting = target
        .registry()
        .get_by_runtime_id(rail.state)
        .map_or(NORTH_SOUTH, |entry| direction(entry, rail.state));
    if shape.is_none() {
        if ns && ew {
            // BDS 1.26.51 lies a powered, detector or activator rail with
            // rails on both axes east-west; a rail keeps its shape (and then
            // curves).
            shape = Some(if rail.straight { EAST_WEST } else { starting });
        } else if ns {
            shape = Some(NORTH_SOUTH);
        } else if ew {
            shape = Some(EAST_WEST);
        }
        if !rail.straight {
            // Unpowered: the last matching corner wins.
            for (corner, curve) in [
                (nw, NORTH_WEST),
                (ne, NORTH_EAST),
                (sw, SOUTH_WEST),
                (se, SOUTH_EAST),
            ] {
                if corner {
                    shape = Some(curve);
                }
            }
        }
    }
    let shape = rise(target, pos, shape).unwrap_or(starting);
    let mut changed = Vec::new();
    let new_state = with_direction(target, rail.state, shape);
    if new_state != rail.state {
        target.write(pos, new_state);
        changed.push(pos);
    }
    for connection in connections(pos, shape) {
        let Some(mut other) = rail_near(target, connection) else {
            continue;
        };
        other.remove_soft_connections(target);
        if !other.can_connect_to(pos) {
            continue;
        }
        let other_shape = other.connect_to(target, pos);
        let other_state = with_direction(target, other.state, other_shape);
        if other_state != other.state {
            target.write(other.pos, other_state);
            changed.push(other.pos);
        }
    }
    changed
}

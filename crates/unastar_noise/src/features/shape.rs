//! Canonical Java neighbour-shape transitions shared by templates and trees.
//!
//! Edge faces retain native discrete-voxel traversal order. State transitions
//! do not execute scheduled ticks: native water/leaf tick requests are a
//! separate stage observation, not immediate block-state mutations.

mod attachments;
mod connections;

use super::blocks::{BlockPos, Direction, JavaState};
use super::eval::FeatureWorld;
use super::java_blocks::{states, tags};
use super::survival::can_survive;
use crate::java_blocks::{SHAPE_CENTER_SUPPORT_FLAGS, SHAPE_SUPPORT_FLAGS, SHAPE_WALL_ABOVE_FLAGS};

#[inline]
fn sturdy(state: JavaState, direction: Direction) -> bool {
    SHAPE_SUPPORT_FLAGS[state.0 as usize] & (1 << direction as u8) != 0
}

#[inline]
fn connection_exception(state: JavaState) -> bool {
    SHAPE_SUPPORT_FLAGS[state.0 as usize] & 64 != 0
}

#[inline]
fn wall_above_flags(state: JavaState) -> u8 {
    SHAPE_WALL_ABOVE_FLAGS[state.0 as usize]
}

#[inline]
pub(crate) fn supports_center(state: JavaState, direction: Direction) -> bool {
    !(direction == Direction::Down && state.is_in(tags::UNSTABLE_BOTTOM_CENTER))
        && SHAPE_CENTER_SUPPORT_FLAGS[state.0 as usize] & (1 << direction as u8) != 0
}

/// Native `BitSetDiscreteVoxelShape`, addressed in absolute block coordinates.
/// Its packed occupancy is allocated only for the actual successful-write box.
pub struct VoxelShape {
    min: BlockPos,
    size: [i32; 3],
    bits: Vec<bool>,
}

impl VoxelShape {
    pub fn new(min: BlockPos, size: [i32; 3]) -> Self {
        Self {
            min,
            size,
            bits: vec![false; (size[0] * size[1] * size[2]) as usize],
        }
    }

    #[inline]
    fn index(&self, x: i32, y: i32, z: i32) -> usize {
        ((x * self.size[1] + y) * self.size[2] + z) as usize
    }

    #[inline]
    pub fn fill(&mut self, pos: BlockPos) {
        let index = self.index(pos.x - self.min.x, pos.y - self.min.y, pos.z - self.min.z);
        self.bits[index] = true;
    }

    #[inline]
    pub fn contains(&self, pos: BlockPos) -> bool {
        self.is_full(pos.x - self.min.x, pos.y - self.min.y, pos.z - self.min.z)
    }

    #[inline]
    fn is_full(&self, x: i32, y: i32, z: i32) -> bool {
        x >= 0
            && y >= 0
            && z >= 0
            && x < self.size[0]
            && y < self.size[1]
            && z < self.size[2]
            && self.bits[self.index(x, y, z)]
    }
}

/// `Block.updateFromNeighbourShapes`: west, east, north, south, down, up.
pub fn update_from_neighbour_shapes<W: FeatureWorld + ?Sized>(
    world: &mut W,
    pos: BlockPos,
    mut state: JavaState,
) -> JavaState {
    for direction in [
        Direction::West,
        Direction::East,
        Direction::North,
        Direction::South,
        Direction::Down,
        Direction::Up,
    ] {
        let neighbour = world.get_state(pos.rel(direction));
        state = update_shape(world, state, pos, direction, neighbour);
    }
    state
}

/// `StructureTemplate.updateShapeAtEdge`: Z runs (X/Y), Y runs (Z/X),
/// then X runs (Y/Z), with the negative face preceding the positive face.
pub fn update_shape_at_edge<W: FeatureWorld + ?Sized>(world: &mut W, shape: &VoxelShape) {
    let face = |world: &mut W, direction: Direction, x: i32, y: i32, z: i32| {
        let pos = shape.min.offset(x, y, z);
        let npos = pos.rel(direction);
        let state = world.get_state(pos);
        let neighbour = world.get_state(npos);
        let changed = update_shape(world, state, pos, direction, neighbour);
        if changed != state {
            world.set_state(pos, changed);
        }
        let changed_neighbour = update_shape(world, neighbour, npos, direction.opposite(), changed);
        if changed_neighbour != neighbour {
            world.set_state(npos, changed_neighbour);
        }
    };
    type Map = fn(i32, i32, i32) -> (i32, i32, i32);
    let [xs, ys, zs] = shape.size;
    let passes: [(i32, i32, i32, Map, Direction, Direction); 3] = [
        (
            xs,
            ys,
            zs,
            |a, b, c| (a, b, c),
            Direction::North,
            Direction::South,
        ),
        (
            zs,
            xs,
            ys,
            |a, b, c| (b, c, a),
            Direction::Down,
            Direction::Up,
        ),
        (
            ys,
            zs,
            xs,
            |a, b, c| (c, a, b),
            Direction::West,
            Direction::East,
        ),
    ];
    for (outer, middle, run, map, negative, positive) in passes {
        for a in 0..outer {
            for b in 0..middle {
                let mut last_full = false;
                for c in 0..=run {
                    let full = c != run && {
                        let (x, y, z) = map(a, b, c);
                        shape.is_full(x, y, z)
                    };
                    if !last_full && full {
                        let (x, y, z) = map(a, b, c);
                        face(world, negative, x, y, z);
                    }
                    if last_full && !full {
                        let (x, y, z) = map(a, b, c - 1);
                        face(world, positive, x, y, z);
                    }
                    last_full = full;
                }
            }
        }
    }
}

/// Native state-only `BlockState.updateShape`, without executing queued ticks.
pub fn update_shape<W: FeatureWorld + ?Sized>(
    world: &mut W,
    state: JavaState,
    pos: BlockPos,
    direction: Direction,
    neighbour: JavaState,
) -> JavaState {
    if let Some(changed) = connections::update_shape(world, state, pos, direction, neighbour) {
        return changed;
    }
    if let Some(changed) = attachments::update_shape(world, state, pos, direction, neighbour) {
        return changed;
    }
    let block = state.block();
    match block.def().kind {
        "minecraft:double_plant"
        | "minecraft:tall_flower"
        | "minecraft:tall_seagrass"
        | "minecraft:small_dripleaf" => {
            let lower = state.value("half") == Some("lower");
            let toward_other_half = matches!(direction, Direction::Up | Direction::Down)
                && lower == (direction == Direction::Up);
            let other_half_ok =
                neighbour.is(block) && neighbour.value("half") != state.value("half");
            if (toward_other_half && !other_half_ok) || !can_survive(world, state, pos) {
                states::AIR
            } else {
                state
            }
        }
        "minecraft:flower"
        | "minecraft:tall_grass"
        | "minecraft:bush"
        | "minecraft:sapling"
        | "minecraft:sweet_berry_bush"
        | "minecraft:flower_bed"
        | "minecraft:firefly_bush"
        | "minecraft:eyeblossom"
        | "minecraft:wither_rose"
        | "minecraft:dry_vegetation"
        | "minecraft:short_dry_grass"
        | "minecraft:tall_dry_grass"
        | "minecraft:leaf_litter"
        | "minecraft:azalea"
        | "minecraft:fungus"
        | "minecraft:roots"
        | "minecraft:nether_sprouts"
        | "minecraft:mushroom"
        | "minecraft:waterlily"
        | "minecraft:seagrass"
        | "minecraft:snow_layer"
        | "minecraft:carpet"
        | "minecraft:wool_carpet" => {
            if can_survive(world, state, pos) {
                state
            } else {
                states::AIR
            }
        }
        _ => {
            if direction == Direction::Up && state.has("snowy") {
                state.with_bool("snowy", neighbour.is_in(tags::SNOW))
            } else {
                state
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::{JavaBlock, test_world::TestWorld};
    use serde_json::Value;
    use std::collections::BTreeMap;

    fn state(text: &str) -> JavaState {
        let (name, properties) = text.split_once('[').map_or((text, ""), |(name, rest)| {
            (name, rest.strip_suffix(']').unwrap())
        });
        let mut state = JavaBlock::by_name(name).unwrap().default_state();
        if !properties.is_empty() {
            for property in properties.split(',') {
                let (name, value) = property.split_once('=').unwrap();
                state = state.with(name, value);
            }
        }
        state
    }

    fn position(value: &Value) -> BlockPos {
        BlockPos::new(
            value[0].as_i64().unwrap() as i32,
            value[1].as_i64().unwrap() as i32,
            value[2].as_i64().unwrap() as i32,
        )
    }

    #[test]
    fn pinned_native_shape_consumers_preserve_full_states() {
        let capture: Value = serde_json::from_str(include_str!(
            "../../tests/data/native_shape_consumers_1_21_11.json"
        ))
        .unwrap();
        assert_eq!(capture["denied_network_attempts"], serde_json::json!([]));
        assert_eq!(capture["server_constructed"], false);
        let cases = capture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 28_956);
        let mut differing = BTreeMap::new();
        let mut examples = Vec::new();
        for case in cases {
            let mut world = TestWorld::default();
            let pos = position(&case["position"]);
            let input = state(case["input"].as_str().unwrap());
            world.blocks.insert(pos, input);
            for row in case["world"].as_array().unwrap() {
                world
                    .blocks
                    .insert(position(&row["pos"]), state(row["state"].as_str().unwrap()));
            }
            let direction = match case["direction"].as_str().unwrap() {
                "DOWN" => Direction::Down,
                "UP" => Direction::Up,
                "NORTH" => Direction::North,
                "SOUTH" => Direction::South,
                "WEST" => Direction::West,
                "EAST" => Direction::East,
                other => panic!("unknown native direction {other}"),
            };
            let neighbour = world.get_state(pos.rel(direction));
            let actual = update_shape(&mut world, input, pos, direction, neighbour);
            let expected = state(case["expected"].as_str().unwrap());
            if actual != expected {
                *differing.entry(input.block().def().kind).or_insert(0usize) += 1;
                if examples.len() < 16 {
                    examples.push((case.clone(), format!("{actual:?}")));
                }
            }
        }
        assert!(
            differing.is_empty(),
            "native shape differences {differing:?}; examples {examples:#?}"
        );
    }
}

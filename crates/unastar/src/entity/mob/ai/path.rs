//! Walk-path requests, target ground resolution and random positions.
//!
//! Paths come from [`crate::entity::navigation::find_walk_path`]; this module
//! turns a mob's `minecraft:navigation.walk` policy into the per-state
//! permission table it needs and resolves goal targets (random stroll and
//! panic positions, a player's or parent's column) onto walkable ground.
//!
//! Door cells follow BDS's door node classification ([`door_passage`]): a
//! mob without `can_pass_doors` treats every door as a wall; a door-breaking
//! mob (`minecraft:annotation.break_door`) crosses closed wooden doors, and
//! closed iron ones with `can_open_iron_doors`; `can_open_doors` crosses
//! closed doors. Everything else meets a door's collision shape as it is:
//! a closed door is a wall, an open one leaves its doorway free.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use rand::Rng;
use rand::rngs::StdRng;

use crate::entity::components::PhysicsBody;
use crate::entity::mob::components::WalkNavigationConfig;
use crate::entity::mob::constants::{
    DOOR_HINGE_STATE, DOOR_OPEN_STATE, GRASS_WALK_TARGET_SCORE, IRON_DOOR_BLOCK,
    PATH_DAMAGING_BLOCKS, PATH_MAX_DROP, PATH_MAX_EXPANSIONS, RANDOM_POS_ATTEMPTS,
};
use crate::entity::navigation::{
    NavigationScratch, WalkBlockRole, WalkNavigation, WalkPathOutcome, WalkPathRequest,
    find_walk_path,
};
use crate::entity::systems::physics::{BlockSource, collect_block_boxes, collides_with_blocks};
use crate::registry::BlockRegistry;
use crate::registry::block::{LiquidKind, StateValue};
use crate::registry::collision::Aabb;

/// Retained nodes per path search: a memory bound, not a behaviour value.
/// [`PATH_MAX_EXPANSIONS`] normally ends a search long before it; terrain
/// with many stacked surfaces per column may exhaust it first.
const PATH_NODE_BUDGET: usize = 2048;

const WATER: u8 = 1;
const DAMAGING: u8 = 2;
const GRASS: u8 = 4;
/// Any door half (wooden, copper or iron).
const DOOR: u8 = 8;
const IRON_DOOR: u8 = 16;
/// A door half whose own state is open.
const OPEN: u8 = 32;

/// Navigation facts per block runtime state, built once from the registry.
#[derive(Resource, Debug, Default)]
pub struct PathBlockTable {
    flags: Vec<u8>,
}

impl PathBlockTable {
    pub fn new(registry: &BlockRegistry) -> Self {
        let mut flags = Vec::new();
        for entry in registry.iter() {
            let base = if PATH_DAMAGING_BLOCKS.contains(&entry.string_id.as_str()) {
                DAMAGING
            } else if entry.string_id == "minecraft:grass_block" {
                GRASS
            } else if entry
                .state_value(entry.min_state_id, DOOR_HINGE_STATE)
                .is_some()
            {
                if entry.string_id == IRON_DOOR_BLOCK {
                    DOOR | IRON_DOOR
                } else {
                    DOOR
                }
            } else {
                0
            };
            for id in entry.min_state_id..=entry.max_state_id {
                let index = id as usize;
                if flags.len() <= index {
                    flags.resize(index + 1, 0);
                }
                let liquid = match registry.liquid_state(id).map(|state| state.kind) {
                    Some(LiquidKind::Water) => WATER,
                    Some(LiquidKind::Lava) => DAMAGING,
                    None => 0,
                };
                let open = if base & DOOR != 0
                    && entry.state_value(id, DOOR_OPEN_STATE) == Some(StateValue::Byte(1))
                {
                    OPEN
                } else {
                    0
                };
                flags[index] = base | liquid | open;
            }
        }
        Self { flags }
    }

    fn get(&self, runtime_id: u32) -> u8 {
        self.flags.get(runtime_id as usize).copied().unwrap_or(0)
    }

    fn is_water(&self, runtime_id: u32) -> bool {
        self.get(runtime_id) & WATER != 0
    }

    fn is_grass(&self, runtime_id: u32) -> bool {
        self.get(runtime_id) & GRASS != 0
    }

    /// Whether `runtime_id` is lava or a block that hurts a body touching it.
    fn is_damaging(&self, runtime_id: u32) -> bool {
        self.get(runtime_id) & DAMAGING != 0
    }

    /// Whether `runtime_id` is a door half (wooden, copper or iron).
    pub fn is_door(&self, runtime_id: u32) -> bool {
        self.get(runtime_id) & DOOR != 0
    }

    /// Whether `runtime_id` is an iron door half.
    pub fn is_iron_door(&self, runtime_id: u32) -> bool {
        self.get(runtime_id) & IRON_DOOR != 0
    }
}

/// The walk policy of one search: which states the body may occupy or stand
/// on.
#[derive(Debug, Clone, Copy)]
pub struct WalkPolicy {
    pub avoid_water: bool,
    pub avoid_damage_blocks: bool,
    pub can_jump: bool,
    /// Lift of one jump (the apex of the jump impulse under living gravity).
    pub jump_height: f64,
    pub can_pass_doors: bool,
    pub can_open_doors: bool,
    pub can_open_iron_doors: bool,
    /// The mob breaks doors (`minecraft:annotation.break_door`).
    pub door_breaker: bool,
    /// `navigation.climb`: the mob climbs walls (see `super::climb`).
    pub climbs_walls: bool,
    /// The mob's follow range: how far around its goal a path search
    /// reaches.
    pub follow_range: f64,
}

impl WalkPolicy {
    pub fn new(
        walk: Option<&WalkNavigationConfig>,
        door_breaker: bool,
        jump_power: f32,
        gravity: f64,
        follow_range: f32,
    ) -> Self {
        let walk = walk.copied();
        Self {
            avoid_water: walk.is_some_and(|w| w.avoid_water),
            avoid_damage_blocks: walk.is_some_and(|w| w.avoid_damage_blocks),
            can_jump: walk.is_some_and(|w| w.can_jump),
            jump_height: jump_apex(jump_power, gravity),
            can_pass_doors: walk.is_some_and(|w| w.can_pass_doors),
            can_open_doors: walk.is_some_and(|w| w.can_open_doors),
            can_open_iron_doors: walk.is_some_and(|w| w.can_open_iron_doors),
            door_breaker,
            climbs_walls: walk.is_some_and(|w| w.climbs_walls),
            follow_range: f64::from(follow_range),
        }
    }
}

/// How a path search treats one block state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Passage {
    /// Not a door, or a door met as its collision shape (closed: a wall;
    /// open: its doorway is free).
    Shape,
    /// A door the body crosses as if it were empty (it opens or breaks it).
    Through,
    /// A door cell the body never enters.
    Wall,
}

/// BDS's door node classification for a block state with `flags`. A
/// door-breaking mob (and a door-opening one, which unastar has none of yet)
/// classifies doors itself: none without `can_pass_doors`; open doors by
/// shape; closed iron doors with `can_open_iron_doors`, closed wooden ones
/// with `can_open_doors` or as a breaker. Other mobs cross closed doors with
/// `can_open_doors`, meet them as walls without `can_pass_doors`, and
/// otherwise as their shape.
fn door_passage(policy: &WalkPolicy, flags: u8) -> Passage {
    if flags & DOOR == 0 {
        return Passage::Shape;
    }
    if !policy.door_breaker {
        return if policy.can_open_doors {
            Passage::Through
        } else if policy.can_pass_doors {
            Passage::Shape
        } else {
            Passage::Wall
        };
    }
    if !policy.can_pass_doors {
        return Passage::Wall;
    }
    if flags & OPEN != 0 {
        return Passage::Shape;
    }
    let crosses = if flags & IRON_DOOR != 0 {
        policy.can_open_iron_doors
    } else {
        policy.can_open_doors || policy.door_breaker
    };
    if crosses {
        Passage::Through
    } else {
        Passage::Wall
    }
}

/// Height gained by a ground jump with initial velocity `power` under the
/// living tick (`move; v = (v - g) * 0.98`).
fn jump_apex(power: f32, gravity: f64) -> f64 {
    use crate::entity::mob::constants::LIVING_VERTICAL_DRAG;
    let mut velocity = f64::from(power);
    let mut height = 0.0;
    while velocity > 0.0 && gravity > 0.0 {
        height += velocity;
        velocity = (velocity - gravity) * LIVING_VERTICAL_DRAG;
    }
    height
}

/// Search a walk path from `start` to `goal` into `out`. Water is never
/// forbidden for a body that is already in it, so a wading mob can leave.
/// With `partial`, a goal the search cannot reach yields the route to the
/// node closest to it instead (none when that is the start).
#[allow(clippy::too_many_arguments)]
pub fn find_path(
    blocks: &impl BlockSource,
    table: &PathBlockTable,
    body: &PhysicsBody,
    policy: WalkPolicy,
    in_water: bool,
    start: DVec3,
    goal: DVec3,
    partial: bool,
    scratch: &mut NavigationScratch,
    out: &mut WalkNavigation,
) -> bool {
    let request = WalkPathRequest {
        start,
        goal,
        max_range: policy.follow_range,
        max_expansions: PATH_MAX_EXPANSIONS,
        node_budget: PATH_NODE_BUDGET,
        step_height: body.step_height,
        jump_height: if policy.can_jump {
            policy.jump_height
        } else {
            0.0
        },
        max_drop: PATH_MAX_DROP,
        partial,
    };
    let forbid = (if policy.avoid_water && !in_water {
        WATER
    } else {
        0
    }) | (if policy.avoid_damage_blocks {
        DAMAGING
    } else {
        0
    });
    let outcome = find_walk_path(
        blocks,
        body,
        request,
        scratch,
        out,
        |id, role: WalkBlockRole| {
            let flags = table.get(id);
            let passage = door_passage(&policy, flags);
            match role {
                WalkBlockRole::Occupied => flags & forbid == 0 && passage != Passage::Wall,
                WalkBlockRole::Support => flags & forbid == 0,
                WalkBlockRole::Collides => passage != Passage::Through,
            }
        },
    );
    matches!(outcome, WalkPathOutcome::Found | WalkPathOutcome::Partial)
}

/// Whether a wall climber at `start` can head straight for the block
/// `target` (its minimum corner): a grid walk on the start's block level
/// over every column the line crosses, checking a `size_xz` × `size_y` ×
/// `size_xz` box of cells around each (the start's box is
/// [`DIRECT_SWEEP_START_MARGIN`] wider), counting only cells whose centre
/// is not behind the start. A climber passes through any cell that is not
/// lava or a damaging block (it climbs over the rest) and needs no floor,
/// but never crosses a damaging floor, or a water floor unless it is in
/// water. A target closer than [`DIRECT_SWEEP_MIN_DIST_SQ`] horizontally
/// cannot be headed for.
#[allow(clippy::too_many_arguments)]
pub fn climber_walks_directly(
    blocks: &impl BlockSource,
    table: &PathBlockTable,
    start: DVec3,
    target: DVec3,
    size_xz: i32,
    size_y: i32,
    in_water: bool,
) -> bool {
    use crate::entity::mob::constants::{DIRECT_SWEEP_MIN_DIST_SQ, DIRECT_SWEEP_START_MARGIN};
    let (dx, dz) = (target.x - start.x, target.z - start.z);
    let length_sq = dx * dx + dz * dz;
    if length_sq < DIRECT_SWEEP_MIN_DIST_SQ {
        return false;
    }
    let length = length_sq.sqrt();
    let direction = (dx / length, dz / length);
    let level = start.y.floor() as i32;
    let passable = |cx: i32, cz: i32, size: i32| {
        let (x0, z0) = (cx - size / 2, cz - size / 2);
        for x in x0..x0 + size {
            for z in z0..z0 + size {
                let ahead = (f64::from(x) + 0.5 - start.x) * direction.0
                    + (f64::from(z) + 0.5 - start.z) * direction.1;
                if ahead < 0.0 {
                    continue;
                }
                for y in level..level + size_y {
                    match blocks.block_at(IVec3::new(x, y, z)) {
                        Some(id) if !table.is_damaging(id) => {}
                        _ => return false,
                    }
                }
                match blocks.block_at(IVec3::new(x, level - 1, z)) {
                    Some(id) if table.is_damaging(id) => return false,
                    Some(id) if table.is_water(id) && !in_water => return false,
                    Some(_) => {}
                    None => return false,
                }
            }
        }
        true
    };
    let (mut x, mut z) = (start.x.floor() as i32, start.z.floor() as i32);
    if !passable(x, z, size_xz + DIRECT_SWEEP_START_MARGIN) {
        return false;
    }
    let step = (1.0 / direction.0.abs(), 1.0 / direction.1.abs());
    let sign = (
        if direction.0 < 0.0 { -1 } else { 1 },
        if direction.1 < 0.0 { -1 } else { 1 },
    );
    // Distance along the line to the next column boundary on each axis.
    let mut next_x =
        (f64::from(x) - start.x + f64::from(i32::from(direction.0 >= 0.0))) / direction.0;
    let mut next_z =
        (f64::from(z) - start.z + f64::from(i32::from(direction.1 >= 0.0))) / direction.1;
    let (end_x, end_z) = (target.x.floor() as i32, target.z.floor() as i32);
    while (end_x - x) * sign.0 > 0 || (end_z - z) * sign.1 > 0 {
        if next_x < next_z {
            next_x += step.0;
            x += sign.0;
        } else {
            next_z += step.1;
            z += sign.1;
        }
        if !passable(x, z, size_xz) {
            return false;
        }
    }
    true
}

fn solid(blocks: &impl BlockSource, pos: IVec3) -> bool {
    let min = pos.as_dvec3();
    collides_with_blocks(blocks, Aabb::new(min, min + DVec3::ONE))
}

/// Feet height on the column of block `column` (x/z) starting at block
/// `start_y`: move up out of solid blocks, then down onto the first block
/// with collision, within `[min_y, max_y]`. Returns the centre of the column
/// at the top of that block's collision shape.
pub fn settle(
    blocks: &impl BlockSource,
    column: IVec3,
    start_y: i32,
    min_y: i32,
    max_y: i32,
    boxes: &mut Vec<Aabb>,
) -> Option<DVec3> {
    let mut pos = IVec3::new(column.x, start_y.clamp(min_y, max_y), column.z);
    while solid(blocks, pos) {
        if pos.y >= max_y {
            return None;
        }
        pos.y += 1;
    }
    settle_down(blocks, pos, min_y, boxes)
}

/// [`settle`] from the free cell `pos`: down onto the first block with
/// collision, not below `min_y`.
fn settle_down(
    blocks: &impl BlockSource,
    mut pos: IVec3,
    min_y: i32,
    boxes: &mut Vec<Aabb>,
) -> Option<DVec3> {
    while !solid(blocks, pos - IVec3::Y) {
        if pos.y <= min_y {
            return None;
        }
        pos.y -= 1;
    }
    let centre = DVec3::new(
        f64::from(pos.x) + 0.5,
        f64::from(pos.y),
        f64::from(pos.z) + 0.5,
    );
    boxes.clear();
    collect_block_boxes(
        blocks,
        Aabb::new(
            centre - DVec3::new(1.0e-4, 1.0, 1.0e-4),
            centre + DVec3::new(1.0e-4, -1.0e-4, 1.0e-4),
        ),
        boxes,
    );
    let top = boxes
        .iter()
        .map(|b| b.max.y)
        .fold(f64::NEG_INFINITY, f64::max);
    top.is_finite().then(|| DVec3::new(centre.x, top, centre.z))
}

/// The walkable column below `point` (a player's or mob's feet): its block
/// column, settled within a few blocks. Feet standing in the free part of a
/// cell that has collision (a doorway beside the door's panel, a slab) stand
/// in that cell, as the native path search ends on the target's own block:
/// on the highest shape under the cell's centre up to the feet, else on the
/// ground below the cell.
pub fn ground_below(
    blocks: &impl BlockSource,
    point: DVec3,
    boxes: &mut Vec<Aabb>,
) -> Option<DVec3> {
    let probe = point + DVec3::Y * 1.0e-3;
    let block = probe.floor().as_ivec3();
    let min_y = block.y - PATH_MAX_DROP as i32 - 1;
    let feet_free = !collides_with_blocks(
        blocks,
        Aabb::new(
            probe - DVec3::new(1.0e-4, 0.0, 1.0e-4),
            probe + DVec3::splat(1.0e-4),
        ),
    );
    if !feet_free || !solid(blocks, block) {
        return settle(blocks, block, block.y, min_y, block.y + 1, boxes);
    }
    let centre = DVec3::new(
        f64::from(block.x) + 0.5,
        f64::from(block.y),
        f64::from(block.z) + 0.5,
    );
    boxes.clear();
    collect_block_boxes(
        blocks,
        Aabb::new(
            centre - DVec3::new(1.0e-4, 0.0, 1.0e-4),
            DVec3::new(centre.x + 1.0e-4, probe.y, centre.z + 1.0e-4),
        ),
        boxes,
    );
    let top = boxes
        .iter()
        .map(|b| b.max.y)
        .fold(f64::NEG_INFINITY, f64::max);
    if top.is_finite() {
        return Some(DVec3::new(centre.x, top, centre.z));
    }
    settle_down(blocks, block, min_y, boxes)
}

/// Up to [`RANDOM_POS_ATTEMPTS`] random walkable targets within `±xz`
/// horizontally and `±y` vertically of `feet`, best walk-target score first
/// (ties keep generation order). With `away`, offsets pointing toward it are
/// rejected. Candidates in water are rejected when the policy avoids water.
#[allow(clippy::too_many_arguments)]
pub fn random_targets(
    blocks: &impl BlockSource,
    table: &PathBlockTable,
    rng: &mut StdRng,
    feet: DVec3,
    xz: i32,
    y: i32,
    away: Option<DVec3>,
    avoid_water: bool,
    boxes: &mut Vec<Aabb>,
    out: &mut Vec<(f32, DVec3)>,
) {
    out.clear();
    let origin = feet.floor().as_ivec3();
    let away = away.map(|from| DVec3::new(feet.x - from.x, 0.0, feet.z - from.z));
    for _ in 0..RANDOM_POS_ATTEMPTS {
        let dx = rng.gen_range(-xz..=xz);
        let dy = rng.gen_range(-y..=y);
        let dz = rng.gen_range(-xz..=xz);
        if let Some(direction) = away
            && f64::from(dx) * direction.x + f64::from(dz) * direction.z < 0.0
        {
            continue;
        }
        let column = origin + IVec3::new(dx, 0, dz);
        let Some(target) = settle(
            blocks,
            column,
            origin.y + dy,
            origin.y - y,
            origin.y + y,
            boxes,
        ) else {
            continue;
        };
        let feet_block = target.floor().as_ivec3();
        if avoid_water
            && blocks
                .block_at(feet_block)
                .is_some_and(|id| table.is_water(id))
        {
            continue;
        }
        let below = (target - DVec3::Y * 1.0e-3).floor().as_ivec3();
        let score = if blocks.block_at(below).is_some_and(|id| table.is_grass(id)) {
            GRASS_WALK_TARGET_SCORE
        } else {
            0.0
        };
        out.push((score, target));
    }
    // Stable: equal scores keep generation order.
    out.sort_by(|a, b| b.0.total_cmp(&a.0));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::systems::physics::tests::TestBlocks;

    fn policy(
        door_breaker: bool,
        can_pass_doors: bool,
        can_open_doors: bool,
        can_open_iron_doors: bool,
    ) -> WalkPolicy {
        WalkPolicy {
            avoid_water: false,
            avoid_damage_blocks: false,
            can_jump: true,
            jump_height: jump_apex(0.42, 0.08),
            can_pass_doors,
            can_open_doors,
            can_open_iron_doors,
            door_breaker,
            climbs_walls: false,
            follow_range: 16.0,
        }
    }

    /// A door half of `name` whose closed panel stands across the x axis.
    fn door_state(name: &str, upper: bool, open: bool) -> u32 {
        let entry = BlockRegistry::vanilla().get_by_name(name).unwrap();
        let set = |state, property, value| entry.with_state_value(state, property, value).unwrap();
        let mut boxes = Vec::new();
        let facing = ["north", "south", "east", "west"]
            .into_iter()
            .map(|direction| {
                set(
                    entry.default_state_id,
                    "minecraft:cardinal_direction",
                    StateValue::Str(direction),
                )
            })
            .find(|&state| {
                boxes.clear();
                crate::registry::collision::block_collision_boxes(
                    set(state, "open_bit", StateValue::Byte(0)),
                    &mut boxes,
                );
                boxes[0].max.x - boxes[0].min.x < 0.5
            })
            .unwrap();
        let state = set(facing, "upper_block_bit", StateValue::Byte(u8::from(upper)));
        set(state, "open_bit", StateValue::Byte(u8::from(open)))
    }

    /// A stone floor at y = 0 split by a two-high stone wall at x = 1 whose
    /// only gap at z = 0 holds a door of `name`.
    fn walled(name: &str, open: bool) -> TestBlocks {
        let mut blocks = TestBlocks::floor("minecraft:stone", 0);
        for z in -4..=4 {
            for y in 1..=2 {
                blocks.set(IVec3::new(1, y, z), "minecraft:stone");
            }
        }
        blocks.set_state(IVec3::new(1, 1, 0), door_state(name, false, open));
        blocks.set_state(IVec3::new(1, 2, 0), door_state(name, true, open));
        blocks
    }

    fn crosses(blocks: &TestBlocks, policy: WalkPolicy) -> bool {
        let table = PathBlockTable::new(BlockRegistry::vanilla());
        let mut out = WalkNavigation::default();
        find_path(
            blocks,
            &table,
            &PhysicsBody::living(0.6, 1.9),
            policy,
            false,
            DVec3::new(-1.5, 1.0, 0.5),
            DVec3::new(3.5, 1.0, 0.5),
            false,
            &mut NavigationScratch::default(),
            &mut out,
        )
    }

    #[test]
    fn table_marks_wooden_and_iron_door_halves() {
        let table = PathBlockTable::new(BlockRegistry::vanilla());
        for upper in [false, true] {
            let oak = door_state("minecraft:wooden_door", upper, false);
            let iron = door_state("minecraft:iron_door", upper, false);
            assert!(table.is_door(oak) && !table.is_iron_door(oak));
            assert!(table.is_door(iron) && table.is_iron_door(iron));
        }
        let trapdoor = BlockRegistry::vanilla()
            .default_state_id_by_name("minecraft:trapdoor")
            .unwrap();
        assert!(!table.is_door(trapdoor));
    }

    #[test]
    fn breakers_path_through_closed_wooden_doors_only_with_can_pass_doors() {
        let oak = walled("minecraft:wooden_door", false);
        assert!(crosses(&oak, policy(true, true, false, false)));
        assert!(!crosses(&oak, policy(true, false, false, false)));
    }

    #[test]
    fn closed_doors_are_walls_for_mobs_that_neither_break_nor_open_them() {
        let oak = walled("minecraft:wooden_door", false);
        assert!(!crosses(&oak, policy(false, true, false, false)));
        assert!(!crosses(&oak, policy(false, false, false, false)));
    }

    #[test]
    fn openers_cross_closed_doors() {
        let oak = walled("minecraft:wooden_door", false);
        assert!(crosses(&oak, policy(false, true, true, false)));
        let iron = walled("minecraft:iron_door", false);
        assert!(crosses(&iron, policy(false, true, true, false)));
    }

    #[test]
    fn breakers_cross_closed_iron_doors_only_with_can_open_iron_doors() {
        let iron = walled("minecraft:iron_door", false);
        assert!(!crosses(&iron, policy(true, true, false, false)));
        assert!(!crosses(&iron, policy(true, true, true, false)));
        assert!(crosses(&iron, policy(true, true, false, true)));
    }

    #[test]
    fn open_doors_need_can_pass_doors() {
        let open = walled("minecraft:wooden_door", true);
        assert!(crosses(&open, policy(false, true, false, false)));
        assert!(crosses(&open, policy(true, true, false, false)));
        assert!(!crosses(&open, policy(false, false, false, false)));
    }

    #[test]
    fn door_passage_follows_the_native_classification() {
        let wooden = DOOR;
        let iron = DOOR | IRON_DOOR;
        let breaker = policy(true, true, false, false);
        assert_eq!(door_passage(&breaker, 0), Passage::Shape);
        assert_eq!(door_passage(&breaker, wooden), Passage::Through);
        assert_eq!(door_passage(&breaker, wooden | OPEN), Passage::Shape);
        assert_eq!(door_passage(&breaker, iron), Passage::Wall);
        let walker = policy(false, true, false, false);
        assert_eq!(door_passage(&walker, wooden), Passage::Shape);
        assert_eq!(door_passage(&walker, iron | OPEN), Passage::Shape);
        let blind = policy(false, false, false, false);
        assert_eq!(door_passage(&blind, wooden | OPEN), Passage::Wall);
    }

    #[test]
    fn climbers_head_straight_over_walls_but_not_through_lava_or_onto_damaging_floors() {
        let table = PathBlockTable::new(BlockRegistry::vanilla());
        let walled = || {
            let mut blocks = TestBlocks::floor("minecraft:stone", 0);
            for z in -4..=4 {
                for y in 1..=3 {
                    blocks.set(IVec3::new(2, y, z), "minecraft:stone");
                }
            }
            blocks
        };
        let start = DVec3::new(-1.5, 1.0, 0.5);
        let sweep = |blocks: &TestBlocks, target: DVec3| {
            climber_walks_directly(blocks, &table, start, target, 2, 2, false)
        };
        let target = DVec3::new(4.0, 4.0, 0.0);
        assert!(sweep(&walled(), target), "a climber crosses the wall");
        assert!(
            !sweep(&walled(), DVec3::new(-1.5, 4.0, 0.5)),
            "no horizontal distance"
        );
        let mut lava = walled();
        lava.set(IVec3::new(3, 1, 0), "minecraft:lava");
        assert!(!sweep(&lava, target));
        let mut magma = walled();
        magma.set(IVec3::new(3, 0, 0), "minecraft:magma");
        assert!(!sweep(&magma, target));
        let mut behind = walled();
        behind.set(IVec3::new(-4, 1, 0), "minecraft:lava");
        assert!(
            sweep(&behind, target),
            "cells behind the start do not count"
        );
    }
}

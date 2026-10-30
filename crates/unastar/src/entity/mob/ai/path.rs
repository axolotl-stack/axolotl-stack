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
    PATH_DAMAGING_BLOCKS, PATH_MAX_DROP, PATH_MAX_RANGE, RANDOM_POS_ATTEMPTS,
};
use crate::entity::navigation::{
    NavigationScratch, WalkBlockRole, WalkNavigation, WalkPathOutcome, WalkPathRequest,
    find_walk_path,
};
use crate::entity::systems::physics::{BlockSource, collect_block_boxes, collides_with_blocks};
use crate::registry::BlockRegistry;
use crate::registry::block::{LiquidKind, StateValue};
use crate::registry::collision::Aabb;

/// Retained nodes per path search. A bound on search work, not a behaviour
/// value: a breadth-first search over the whole [`PATH_MAX_RANGE`] square
/// around the mob stays within it on open ground.
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
}

impl WalkPolicy {
    pub fn new(
        walk: Option<&WalkNavigationConfig>,
        door_breaker: bool,
        jump_power: f32,
        gravity: f64,
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
#[allow(clippy::too_many_arguments)]
pub fn find_path(
    blocks: &impl BlockSource,
    table: &PathBlockTable,
    body: &PhysicsBody,
    policy: WalkPolicy,
    in_water: bool,
    start: DVec3,
    goal: DVec3,
    scratch: &mut NavigationScratch,
    out: &mut WalkNavigation,
) -> bool {
    let request = WalkPathRequest {
        start,
        goal,
        max_range: PATH_MAX_RANGE,
        node_budget: PATH_NODE_BUDGET,
        step_height: body.step_height,
        jump_height: if policy.can_jump {
            policy.jump_height
        } else {
            0.0
        },
        max_drop: PATH_MAX_DROP,
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
    outcome == WalkPathOutcome::Found
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
/// column, settled within a few blocks.
pub fn ground_below(
    blocks: &impl BlockSource,
    point: DVec3,
    boxes: &mut Vec<Aabb>,
) -> Option<DVec3> {
    let block = (point + DVec3::Y * 1.0e-3).floor().as_ivec3();
    settle(
        blocks,
        block,
        block.y,
        block.y - PATH_MAX_DROP as i32 - 1,
        block.y + 1,
        boxes,
    )
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
}

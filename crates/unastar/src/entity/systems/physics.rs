//! Block-aware entity physics.
//!
//! Every entity with `Position`, `Velocity`, `OnGround` and a [`PhysicsBody`]
//! is moved once per tick against the collision boxes of the blocks around it
//! (see [`crate::registry::collision`]). The move clips the Y component
//! first, then X, then Z against every box in the swept volume (the order
//! dragonfly's `entity.MovementComputer` uses), tries a step-up when walking
//! into a ledge, sets `OnGround` when a downward move was stopped, and zeroes
//! the velocity of every blocked axis.
//!
//! Positions are feet positions (bottom-centre of the box). Chunks that are
//! not loaded count as solid so entities never fall through unloaded terrain.

use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use glam::{DVec3, IVec3};

use crate::entity::components::*;
use crate::registry::BlockRegistry;
use crate::registry::block::{LiquidKind, LiquidState};
use crate::registry::collision::{Aabb, block_collision_boxes, connected_state};
use crate::server::game::types::BlockRegistryResource;
use crate::world::chunk::{MIN_Y, SUBCHUNK_COUNT};
use crate::world::ecs::{ChunkData, ChunkManager, world_to_chunk_coords, world_to_local_coords};

/// Read access to block states in loaded chunks.
pub trait BlockSource {
    /// Runtime ID of the block at `pos`. `None` if the chunk is not loaded.
    /// Positions above or below the world height are air.
    fn block_at(&self, pos: IVec3) -> Option<u32>;

    /// Registry used to resolve runtime IDs.
    fn registry(&self) -> &BlockRegistry;
}

fn in_world_height(y: i32) -> bool {
    (MIN_Y..MIN_Y + i32::from(SUBCHUNK_COUNT) * 16).contains(&y)
}

fn chunk_block(chunk: &ChunkData, pos: IVec3) -> u32 {
    let (x, y, z) = world_to_local_coords(pos.x, pos.y, pos.z);
    chunk.inner.get_block(x, y, z)
}

/// [`BlockSource`] for systems: the chunk manager plus chunk data components.
#[derive(SystemParam)]
pub struct WorldBlocks<'w, 's> {
    manager: Res<'w, ChunkManager>,
    chunks: Query<'w, 's, &'static ChunkData>,
    registry: Res<'w, BlockRegistryResource>,
}

impl BlockSource for WorldBlocks<'_, '_> {
    fn block_at(&self, pos: IVec3) -> Option<u32> {
        let (cx, cz) = world_to_chunk_coords(pos.x, pos.z);
        let chunk = self.chunks.get(self.manager.get_by_coords(cx, cz)?).ok()?;
        if !in_world_height(pos.y) {
            return Some(*crate::world::chunk::blocks::AIR);
        }
        Some(chunk_block(chunk, pos))
    }

    fn registry(&self) -> &BlockRegistry {
        &self.registry.0
    }
}

impl BlockSource for World {
    fn block_at(&self, pos: IVec3) -> Option<u32> {
        let (cx, cz) = world_to_chunk_coords(pos.x, pos.z);
        let entity = self.get_resource::<ChunkManager>()?.get_by_coords(cx, cz)?;
        let chunk = self.get::<ChunkData>(entity)?;
        if !in_world_height(pos.y) {
            return Some(*crate::world::chunk::blocks::AIR);
        }
        Some(chunk_block(chunk, pos))
    }

    fn registry(&self) -> &BlockRegistry {
        &self.resource::<BlockRegistryResource>().0
    }
}

/// Append the world-space collision boxes of every block overlapping `area`.
/// Unloaded chunks contribute full cubes. Fences, walls, panes and bars use
/// the connections their neighbours give them
/// ([`crate::registry::collision::connected_state`]).
pub fn collect_block_boxes(blocks: &impl BlockSource, area: Aabb, out: &mut Vec<Aabb>) {
    let min = area.min.floor().as_ivec3();
    let max = area.max.floor().as_ivec3();
    let registry = blocks.registry();
    let mut local = Vec::new();
    // One extra layer below: fences and walls reach 0.5 into the block above.
    for y in (min.y - 1)..=max.y {
        for x in min.x..=max.x {
            for z in min.z..=max.z {
                let pos = IVec3::new(x, y, z);
                let origin = pos.as_dvec3();
                local.clear();
                match blocks.block_at(pos) {
                    None => local.push(Aabb::new(DVec3::ZERO, DVec3::ONE)),
                    Some(runtime_id) => {
                        if registry.get_by_runtime_id(runtime_id).is_some() {
                            let state =
                                connected_state(registry, pos, runtime_id, |p| blocks.block_at(p));
                            block_collision_boxes(state, &mut local);
                        }
                    }
                }
                out.extend(
                    local
                        .iter()
                        .map(|b| b.offset(origin))
                        .filter(|b| b.intersects(&area)),
                );
            }
        }
    }
}

/// Whether any block collision box intersects `area`.
pub fn collides_with_blocks(blocks: &impl BlockSource, area: Aabb) -> bool {
    let mut boxes = Vec::new();
    collect_block_boxes(blocks, area, &mut boxes);
    !boxes.is_empty()
}

/// Outcome of one collision-resolved move.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoveResult {
    /// Box after the move.
    pub aabb: Aabb,
    /// Distance actually travelled.
    pub moved: DVec3,
    /// A downward move was stopped by a block.
    pub on_ground: bool,
}

fn clip_sequence(boxes: &[Aabb], mut aabb: Aabb, delta: DVec3) -> (Aabb, DVec3) {
    let mut dy = delta.y;
    for b in boxes {
        dy = b.clip_y(&aabb, dy);
    }
    aabb = aabb.offset(DVec3::new(0.0, dy, 0.0));
    let mut dx = delta.x;
    for b in boxes {
        dx = b.clip_x(&aabb, dx);
    }
    aabb = aabb.offset(DVec3::new(dx, 0.0, 0.0));
    let mut dz = delta.z;
    for b in boxes {
        dz = b.clip_z(&aabb, dz);
    }
    aabb = aabb.offset(DVec3::new(0.0, 0.0, dz));
    (aabb, DVec3::new(dx, dy, dz))
}

/// Move `aabb` by `delta`, stopping at block collision boxes, with a step-up
/// of at most `step_height` when the entity was on the ground and walked into
/// an obstacle (kept only if it travels further horizontally).
pub fn move_aabb(
    blocks: &impl BlockSource,
    aabb: Aabb,
    delta: DVec3,
    step_height: f64,
    was_on_ground: bool,
) -> MoveResult {
    let mut boxes = Vec::new();
    let swept = aabb
        .expand_towards(delta)
        .expand_towards(DVec3::new(0.0, step_height, 0.0));
    collect_block_boxes(blocks, swept, &mut boxes);

    let (mut moved_box, mut moved) = clip_sequence(&boxes, aabb, delta);
    let stopped_falling = delta.y < 0.0 && moved.y != delta.y;
    let blocked_horizontally = moved.x != delta.x || moved.z != delta.z;

    if step_height > 0.0 && (was_on_ground || stopped_falling) && blocked_horizontally {
        let (up_box, up) = clip_sequence(&boxes, aabb, DVec3::new(delta.x, step_height, delta.z));
        // Settle back down onto whatever the stepped box now stands on.
        let mut down = -up.y;
        for b in &boxes {
            down = b.clip_y(&up_box, down);
        }
        let stepped_box = up_box.offset(DVec3::new(0.0, down, 0.0));
        let stepped = DVec3::new(up.x, up.y + down, up.z);
        if stepped.x * stepped.x + stepped.z * stepped.z > moved.x * moved.x + moved.z * moved.z {
            moved_box = stepped_box;
            moved = stepped;
        }
    }

    MoveResult {
        aabb: moved_box,
        moved,
        on_ground: delta.y < 0.0 && moved.y != delta.y,
    }
}

/// Friction of the sampled support block (`floor(y)-1`). A missing sample
/// uses BDS's default 0.6 in the grounded drag path, not airborne drag.
fn supporting_friction(blocks: &impl BlockSource, feet: DVec3) -> Option<f64> {
    let pos = IVec3::new(
        feet.x.floor() as i32,
        feet.y.floor() as i32 - 1,
        feet.z.floor() as i32,
    );
    let runtime_id = blocks.block_at(pos)?;
    if runtime_id == *crate::world::chunk::blocks::AIR {
        return None;
    }
    blocks
        .registry()
        .get_by_runtime_id(runtime_id)
        .map(|entry| f64::from(entry.data.friction))
}

fn liquid_at(blocks: &impl BlockSource, pos: IVec3) -> Option<LiquidState> {
    blocks.registry().liquid_state(blocks.block_at(pos)?)
}

fn blocks_liquid_flow(blocks: &impl BlockSource, pos: IVec3) -> bool {
    match blocks.block_at(pos) {
        None => true,
        Some(state) => {
            blocks
                .registry()
                .get_by_runtime_id(state)
                .expect("registered block state")
                .data
                .blocks_motion
        }
    }
}

/// Flow is applied before item flotation. The native controls distinguish an
/// open falling liquid from one beside a wall, and cardinal/diagonal currents.
fn liquid_flow(blocks: &impl BlockSource, pos: IVec3, state: LiquidState) -> DVec3 {
    let effective_depth = |depth: u8| if depth >= 8 { 0 } else { i32::from(depth) };
    let depth = effective_depth(state.depth);
    let mut flow = DVec3::ZERO;
    let mut down_wall = false;
    for side in [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z] {
        let neighbour = pos + side;
        let difference = match liquid_at(blocks, neighbour) {
            Some(other) if other.kind == state.kind => Some(effective_depth(other.depth) - depth),
            _ if !blocks_liquid_flow(blocks, neighbour) => liquid_at(blocks, neighbour - IVec3::Y)
                .filter(|other| other.kind == state.kind)
                .map(|other| effective_depth(other.depth) - (depth - 8)),
            _ => None,
        };
        if let Some(difference) = difference {
            flow += side.as_dvec3() * f64::from(difference);
        }
        if state.depth >= 8 && !down_wall {
            down_wall = blocks_liquid_flow(blocks, neighbour)
                || blocks_liquid_flow(blocks, neighbour + IVec3::Y);
        }
    }
    if down_wall {
        flow = flow.normalize_or_zero() - DVec3::Y * 6.0;
    }
    flow.normalize_or_zero()
}

fn item_current(
    blocks: &impl BlockSource,
    aabb: Aabb,
    foot_pos: IVec3,
    foot_state: Option<LiquidState>,
) -> (DVec3, bool) {
    let min = aabb.min.floor().as_ivec3();
    let max = aabb.max.ceil().as_ivec3();
    let mut flows = [DVec3::ZERO; 2];
    let mut counts = [0usize; 2];
    for y in min.y..max.y {
        for x in min.x..max.x {
            for z in min.z..max.z {
                let pos = IVec3::new(x, y, z);
                let state = if pos == foot_pos {
                    foot_state
                } else {
                    liquid_at(blocks, pos)
                };
                if let Some(state) = state {
                    let index = match state.kind {
                        LiquidKind::Water => 0,
                        LiquidKind::Lava => 1,
                    };
                    flows[index] += liquid_flow(blocks, pos, state);
                    counts[index] += 1;
                }
            }
        }
    }
    let mut current = DVec3::ZERO;
    for (index, acceleration) in [0.014, 0.0035].into_iter().enumerate() {
        if counts[index] != 0 {
            let direction = if counts[index] == 1 {
                flows[index]
            } else {
                flows[index].normalize_or_zero()
            };
            current += direction * acceleration;
        }
    }
    (current, counts[0] != 0)
}

fn item_flotation(
    blocks: &impl BlockSource,
    body: &PhysicsBody,
    position: DVec3,
    foot_pos: IVec3,
    foot_state: Option<LiquidState>,
) -> Option<LiquidKind> {
    let wire_pos = (position + DVec3::Y * body.network_y_offset)
        .floor()
        .as_ivec3();
    let wire_state = if wire_pos == foot_pos {
        foot_state
    } else {
        liquid_at(blocks, wire_pos)
    };
    let below_pos = wire_pos - IVec3::Y;
    let below_state = if below_pos == foot_pos {
        foot_state
    } else {
        liquid_at(blocks, below_pos)
    };
    // A falling layer at the wire voxel or below disables flotation even
    // in a normal source layer above. Open/walled controls distinguish this
    // from downward-current suppression; the above-layer capture is at y=1.1.
    if wire_state.is_some_and(|state| state.depth >= 8)
        || below_state.is_some_and(|state| state.depth >= 8)
    {
        return None;
    }
    // BDS samples water above the wire position by the collision height;
    // lava samples the wire position itself. Stacked shallow-fluid controls
    // distinguish this from either a feet-block or contact-flag check.
    let water_point = position + DVec3::Y * (body.network_y_offset + body.height);
    let water_pos = water_point.floor().as_ivec3();
    let water_state = if water_pos == foot_pos {
        foot_state
    } else if water_pos == wire_pos {
        wire_state
    } else {
        liquid_at(blocks, water_pos)
    };
    if let Some(state) = water_state
        && state.kind == LiquidKind::Water
        && water_point.y < f64::from(water_pos.y) + 1.0 - f64::from(state.depth) / 9.0
    {
        return Some(LiquidKind::Water);
    }
    wire_state
        .filter(|state| state.kind == LiquidKind::Lava && state.depth < 8)
        .map(|_| LiquidKind::Lava)
}

/// Advance one entity by one tick at its current (pre-lifecycle-update) age.
/// Returns the new feet position.
pub fn step_body(
    blocks: &impl BlockSource,
    body: &PhysicsBody,
    position: DVec3,
    velocity: &mut DVec3,
    on_ground: &mut bool,
    age: u64,
) -> DVec3 {
    let aabb = Aabb::from_feet(position, body.width, body.height);
    let flotation = if body.model == PhysicsModel::Item {
        let foot_pos = position.floor().as_ivec3();
        let foot_state = liquid_at(blocks, foot_pos);
        let (current, in_water) = item_current(blocks, aabb, foot_pos, foot_state);
        *velocity += current;
        let flotation = item_flotation(blocks, body, position, foot_pos, foot_state);
        if let Some(kind) = flotation {
            velocity.y = (velocity.y + 0.0005) * 0.99 + 0.0006;
            let viscosity = match kind {
                LiquidKind::Water => 0.99,
                LiquidKind::Lava => 0.95,
            };
            velocity.x *= viscosity;
            velocity.z *= viscosity;
        }
        // Native wet-ground captures apply current/flotation before this gate.
        // Water contact bypasses it even when the water is too shallow to float.
        // Skipped lava ticks retain those forces, but do not move or apply drag.
        if *on_ground && velocity.length_squared() <= 0.0001 && !in_water && !age.is_multiple_of(4)
        {
            return position;
        }
        flotation
    } else {
        None
    };
    if body.gravity_stage == GravityStage::BeforeMove && flotation.is_none() {
        velocity.y -= body.gravity;
    }

    let result = move_aabb(blocks, aabb, *velocity, body.step_height, *on_ground);
    if result.moved.x != velocity.x {
        velocity.x = 0.0;
    }
    if result.moved.y != velocity.y {
        velocity.y = 0.0;
    }
    if result.moved.z != velocity.z {
        velocity.z = 0.0;
    }
    *on_ground = result.on_ground;
    let feet = result.aabb.feet();
    if body.model == PhysicsModel::FallingBlock && *on_ground {
        *velocity = DVec3::ZERO;
        return feet;
    }

    let mut horizontal = body.horizontal_drag;
    if *on_ground {
        match body.ground_friction {
            GroundFriction::None => {}
            GroundFriction::Block => {
                horizontal *= supporting_friction(blocks, feet).unwrap_or(0.6);
            }
        }
    }
    match body.gravity_stage {
        GravityStage::BeforeMove => velocity.y *= body.vertical_drag,
        GravityStage::AfterMove => velocity.y = (velocity.y - body.gravity) * body.vertical_drag,
        GravityStage::AfterDrag => velocity.y = velocity.y * body.vertical_drag - body.gravity,
    }
    velocity.x *= horizontal;
    velocity.z *= horizontal;
    feet
}

/// System: move every physics body one tick against block collision.
///
/// Bodies whose own chunk is not loaded are frozen until it loads.
pub fn tick_physics(
    blocks: WorldBlocks,
    mut query: Query<(
        &mut Position,
        &mut Velocity,
        &mut OnGround,
        &PhysicsBody,
        &Age,
    )>,
) {
    for (mut position, mut velocity, mut on_ground, body, age) in query.iter_mut() {
        if blocks.block_at(position.0.floor().as_ivec3()).is_none() {
            continue;
        }
        let mut vel = velocity.0;
        let mut grounded = on_ground.0;
        let feet = step_body(&blocks, body, position.0, &mut vel, &mut grounded, age.0);
        if position.0 != feet {
            position.0 = feet;
        }
        if velocity.0 != vel {
            velocity.0 = vel;
        }
        if on_ground.0 != grounded {
            on_ground.0 = grounded;
        }
    }
}

#[cfg(test)]
mod bds_tests;

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::HashMap;

    /// In-memory block source for deterministic physics tests.
    #[derive(Default)]
    pub(crate) struct TestBlocks {
        pub blocks: HashMap<IVec3, u32>,
        pub unloaded_below: Option<i32>,
    }

    impl TestBlocks {
        pub fn set(&mut self, pos: IVec3, name: &str) {
            let id = BlockRegistry::vanilla()
                .default_state_id_by_name(name)
                .expect(name);
            self.blocks.insert(pos, id);
        }

        pub fn set_state(&mut self, pos: IVec3, runtime_id: u32) {
            self.blocks.insert(pos, runtime_id);
        }

        pub fn floor(name: &str, y: i32) -> Self {
            let mut blocks = Self::default();
            for x in -4..=4 {
                for z in -4..=4 {
                    blocks.set(IVec3::new(x, y, z), name);
                }
            }
            blocks
        }
    }

    impl BlockSource for TestBlocks {
        fn block_at(&self, pos: IVec3) -> Option<u32> {
            if self.unloaded_below.is_some_and(|y| pos.y < y) {
                return None;
            }
            Some(
                self.blocks
                    .get(&pos)
                    .copied()
                    .unwrap_or(*crate::world::chunk::blocks::AIR),
            )
        }

        fn registry(&self) -> &BlockRegistry {
            BlockRegistry::vanilla()
        }
    }

    fn run(
        blocks: &TestBlocks,
        body: PhysicsBody,
        mut pos: DVec3,
        mut vel: DVec3,
        ticks: usize,
    ) -> (DVec3, DVec3, bool) {
        let mut on_ground = false;
        for age in 0..ticks {
            pos = step_body(blocks, &body, pos, &mut vel, &mut on_ground, age as u64);
        }
        (pos, vel, on_ground)
    }

    #[test]
    fn item_falls_with_bds_gravity_and_drag() {
        let blocks = TestBlocks::default();
        let (pos, vel, on_ground) = run(
            &blocks,
            PhysicsBody::ITEM,
            DVec3::new(0.5, 100.0, 0.5),
            DVec3::ZERO,
            1,
        );
        assert!(!on_ground);
        assert!((vel.y - (-0.04 * 0.98)).abs() < 1e-12);
        assert!((pos.y - 99.96).abs() < 1e-12);
    }

    #[test]
    fn item_lands_on_a_full_block_and_rests_exactly_on_top() {
        let blocks = TestBlocks::floor("minecraft:stone", 64);
        let (pos, vel, on_ground) = run(
            &blocks,
            PhysicsBody::ITEM,
            DVec3::new(0.5, 67.0, 0.5),
            DVec3::ZERO,
            60,
        );
        assert!(on_ground);
        assert!((pos.y - 65.0).abs() < 1e-9);
        assert_eq!(
            vel.y, 0.0,
            "gravity is re-applied each tick and clipped by the floor"
        );
    }

    #[test]
    fn item_lands_on_a_bottom_slab_at_half_height() {
        let mut blocks = TestBlocks::default();
        let registry = BlockRegistry::vanilla();
        let slab = registry.get_by_name("minecraft:oak_slab").unwrap();
        let bottom = slab
            .with_state_value(
                slab.default_state_id,
                "minecraft:vertical_half",
                crate::registry::block::StateValue::Str("bottom"),
            )
            .unwrap();
        blocks.set_state(IVec3::new(0, 64, 0), bottom);
        let (pos, _, on_ground) = run(
            &blocks,
            PhysicsBody::ITEM,
            DVec3::new(0.5, 66.0, 0.5),
            DVec3::ZERO,
            60,
        );
        assert!(on_ground);
        assert!((pos.y - 64.5).abs() < 1e-9);
    }

    #[test]
    fn grass_and_air_do_not_stop_items() {
        let mut blocks = TestBlocks::floor("minecraft:stone", 60);
        blocks.set(IVec3::new(0, 61, 0), "minecraft:short_grass");
        let (pos, _, on_ground) = run(
            &blocks,
            PhysicsBody::ITEM,
            DVec3::new(0.5, 63.0, 0.5),
            DVec3::ZERO,
            60,
        );
        assert!(on_ground);
        assert!((pos.y - 61.0).abs() < 1e-9);
    }

    #[test]
    fn horizontal_motion_stops_at_walls() {
        let mut blocks = TestBlocks::floor("minecraft:stone", 64);
        for y in 65..=67 {
            blocks.set(IVec3::new(2, y, 0), "minecraft:stone");
        }
        let (pos, vel, _) = run(
            &blocks,
            PhysicsBody::ITEM,
            DVec3::new(0.5, 65.0, 0.5),
            DVec3::new(1.0, 0.0, 0.0),
            3,
        );
        assert!((pos.x - (2.0 - 0.125)).abs() < 1e-9);
        assert_eq!(vel.x, 0.0);
    }

    #[test]
    fn ground_friction_uses_the_supporting_block() {
        let stone = TestBlocks::floor("minecraft:stone", 64);
        let ice = TestBlocks::floor("minecraft:ice", 64);
        let start = DVec3::new(0.5, 65.0, 0.5);
        let mut ground = true;
        let mut on_stone = DVec3::new(0.1, 0.0, 0.0);
        step_body(
            &stone,
            &PhysicsBody::ITEM,
            start,
            &mut on_stone,
            &mut ground,
            0,
        );
        let mut on_ice = DVec3::new(0.1, 0.0, 0.0);
        let mut ground = true;
        step_body(&ice, &PhysicsBody::ITEM, start, &mut on_ice, &mut ground, 0);
        assert!((on_stone.x - 0.1 * 0.98 * f64::from(0.6f32)).abs() < 1e-12);
        assert!((on_ice.x - 0.1 * 0.98 * f64::from(0.98f32)).abs() < 1e-12);
    }

    #[test]
    fn grounded_item_uses_default_friction_when_sample_below_is_air() {
        let mut blocks = TestBlocks::default();
        let slab = BlockRegistry::vanilla()
            .get_by_name("minecraft:oak_slab")
            .unwrap();
        let bottom = slab
            .with_state_value(
                slab.default_state_id,
                "minecraft:vertical_half",
                crate::registry::block::StateValue::Str("bottom"),
            )
            .unwrap();
        blocks.set_state(IVec3::new(0, 64, 0), bottom);
        let mut velocity = DVec3::new(0.1, 0.0, 0.0);
        let mut ground = true;
        let feet = step_body(
            &blocks,
            &PhysicsBody::ITEM,
            DVec3::new(0.5, 64.5, 0.5),
            &mut velocity,
            &mut ground,
            0,
        );
        assert!(ground);
        assert_eq!(feet.y, 64.5);
        // BDS's friction sample is floor(feet.y)-1 (air here), but a grounded
        // item still uses default block friction rather than airborne drag.
        assert!(
            (velocity.x - 0.1 * 0.98 * 0.6).abs() < 1e-12,
            "{velocity:?}"
        );
    }

    #[test]
    fn living_bodies_step_up_half_slabs_but_not_full_blocks() {
        let mut blocks = TestBlocks::floor("minecraft:stone", 64);
        let registry = BlockRegistry::vanilla();
        let slab = registry.get_by_name("minecraft:oak_slab").unwrap();
        let bottom = slab
            .with_state_value(
                slab.default_state_id,
                "minecraft:vertical_half",
                crate::registry::block::StateValue::Str("bottom"),
            )
            .unwrap();
        blocks.set_state(IVec3::new(1, 65, 0), bottom);
        blocks.set(IVec3::new(1, 65, 2), "minecraft:stone");
        let body = PhysicsBody::living(0.6, 1.8);
        let mut grounded = true;
        let mut vel = DVec3::new(0.5, 0.0, 0.0);
        let pos = step_body(
            &blocks,
            &body,
            DVec3::new(0.5, 65.0, 0.5),
            &mut vel,
            &mut grounded,
            0,
        );
        assert!((pos.y - 65.5).abs() < 1e-9, "stepped onto the slab: {pos}");
        assert!((pos.x - 1.0).abs() < 1e-9);

        let mut grounded = true;
        let mut vel = DVec3::new(0.5, 0.0, 0.0);
        let pos = step_body(
            &blocks,
            &body,
            DVec3::new(0.5, 65.0, 2.5),
            &mut vel,
            &mut grounded,
            0,
        );
        assert!(
            (pos.y - 65.0).abs() < 1e-9,
            "a full block is too high: {pos}"
        );
        assert!((pos.x - (1.0 - 0.3)).abs() < 1e-9);
    }

    #[test]
    fn unloaded_chunks_are_solid() {
        let blocks = TestBlocks {
            unloaded_below: Some(64),
            ..TestBlocks::default()
        };
        let (pos, _, on_ground) = run(
            &blocks,
            PhysicsBody::ITEM,
            DVec3::new(0.5, 66.0, 0.5),
            DVec3::ZERO,
            60,
        );
        assert!(on_ground);
        assert!((pos.y - 64.0).abs() < 1e-9);
    }

    #[test]
    fn fence_arms_towards_neighbours_collide() {
        let mut blocks = TestBlocks::default();
        blocks.set(IVec3::new(0, 64, 0), "minecraft:oak_fence");
        blocks.set(IVec3::new(1, 64, 0), "minecraft:stone");
        // Between the post and the stone, where only the east arm can be.
        let probe = Aabb::new(DVec3::new(0.7, 64.0, 0.45), DVec3::new(0.9, 65.4, 0.55));
        let mut boxes = Vec::new();
        collect_block_boxes(&blocks, probe, &mut boxes);
        assert_eq!(
            boxes,
            vec![Aabb::new(
                DVec3::new(0.375, 64.0, 0.375),
                DVec3::new(1.0, 65.5, 0.625)
            )]
        );
        // Without the stone the stored post state stays a bare post.
        blocks.blocks.remove(&IVec3::new(1, 64, 0));
        assert!(!collides_with_blocks(&blocks, probe));
    }

    #[test]
    fn tick_physics_moves_bodies_in_loaded_chunks_only() {
        let mut world = World::new();
        world.insert_resource(ChunkManager::new(
            crate::world::WorldConfig::void_spawn_platform(1),
        ));
        world.insert_resource(BlockRegistryResource(std::sync::Arc::new(
            BlockRegistry::vanilla().clone(),
        )));
        let mut chunk = crate::world::chunk::Chunk::new(0, 0);
        chunk.set_block(1, 64, 1, *crate::world::chunk::blocks::STONE);
        let chunk = world.spawn(ChunkData::new(chunk)).id();
        world
            .resource_mut::<ChunkManager>()
            .insert(crate::world::ecs::ChunkPosition::new(0, 0), chunk);
        let loaded = world
            .spawn((
                Position(DVec3::new(1.5, 65.5, 1.5)),
                Velocity::default(),
                OnGround(false),
                PhysicsBody::ITEM,
            ))
            .id();
        let unloaded = world
            .spawn((
                Position(DVec3::new(40.5, 65.5, 40.5)),
                Velocity::default(),
                OnGround(false),
                PhysicsBody::ITEM,
            ))
            .id();
        let mut schedule = Schedule::default();
        schedule.add_systems(tick_physics);
        schedule.run(&mut world);
        assert!(
            world.get::<Position>(loaded).unwrap().0.y < 65.5,
            "the first run already moves loaded bodies"
        );
        for _ in 0..40 {
            schedule.run(&mut world);
        }
        assert!((world.get::<Position>(loaded).unwrap().0.y - 65.0).abs() < 1e-9);
        assert!(world.get::<OnGround>(loaded).unwrap().0);
        assert_eq!(world.get::<Position>(unloaded).unwrap().0.y, 65.5);
    }
}

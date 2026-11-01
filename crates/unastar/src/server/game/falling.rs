//! Gravity-affected blocks (sand, gravel, concrete powder, ...).
//!
//! Two ticks after a block changes, a block with the native `falling` flag
//! whose block below is free (air, fire, liquids, replaceable plants) turns
//! into a `minecraft:falling_block` entity. The entity falls under block
//! physics (gravity 0.04, 0.98 drag) and, on landing, re-places its block if
//! the landing cell is replaceable or drops the block as an item otherwise.
//! An entity still falling after 600 ticks also drops as an item. This is
//! the vanilla behaviour dragonfly implements in `block.gravityAffected` and
//! `entity.FallingBlockBehaviour`.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};

use super::blocks::try_set_block;
use super::placement::is_replaceable;
use super::types::BlockRegistryResource;
use crate::ecs::resources::TickCounter;
use crate::entity::components::{
    ActorViewers, Age, FallingBlock, OnGround, PhysicsBody, Position, Rotation, RuntimeId, Velocity,
};
use crate::entity::item_drop::next_actor_id;
use crate::entity::systems::BlockSource;

/// Ticks between a block change and the gravity check.
pub const GRAVITY_CHECK_DELAY: u64 = 2;

/// Falling blocks still in the air after this many ticks drop as items.
pub const MAX_FALL_TICKS: u64 = 600;

/// Positions to check for unsupported falling blocks, with the tick they
/// are due.
#[derive(Resource, Debug, Default)]
pub struct GravityChecks(pub Vec<(IVec3, u64)>);

/// Schedule gravity checks for `pos` and the block above it.
pub(super) fn schedule_gravity_checks(world: &mut World, pos: IVec3) {
    let due = world
        .get_resource::<TickCounter>()
        .map_or(0, |tick| tick.current)
        + GRAVITY_CHECK_DELAY;
    let mut checks = world.get_resource_or_init::<GravityChecks>();
    for target in [pos, pos + IVec3::Y] {
        if !checks.0.iter().any(|(p, _)| *p == target) {
            checks.0.push((target, due));
        }
    }
}

/// Whether a falling block can fall into `pos` (the cell is replaceable).
fn is_free(world: &World, pos: IVec3) -> bool {
    let Some(state) = world.block_at(pos) else {
        return false;
    };
    world
        .get_resource::<BlockRegistryResource>()
        .and_then(|registry| {
            registry
                .0
                .get_by_runtime_id(state)
                .map(|e| is_replaceable(e, state))
        })
        .unwrap_or(false)
}

/// Exclusive system: start falling blocks whose gravity check is due.
pub fn process_gravity_checks(world: &mut World) {
    let now = world
        .get_resource::<TickCounter>()
        .map_or(0, |tick| tick.current);
    let due: Vec<IVec3> = {
        let Some(mut checks) = world.get_resource_mut::<GravityChecks>() else {
            return;
        };
        let (due, pending): (Vec<_>, Vec<_>) = checks.0.drain(..).partition(|(_, at)| *at <= now);
        checks.0 = pending;
        due.into_iter().map(|(pos, _)| pos).collect()
    };
    for pos in due {
        let Some(state) = world.block_at(pos) else {
            continue;
        };
        let falls = world
            .get_resource::<BlockRegistryResource>()
            .and_then(|registry| registry.0.get_by_runtime_id(state).map(|e| e.data.falling))
            .unwrap_or(false);
        if !falls || !is_free(world, pos - IVec3::Y) {
            continue;
        }
        if try_set_block(world, pos, *crate::world::chunk::blocks::AIR, false).is_err() {
            // A reservation can arrive after this gravity check was queued.
            // Keep the material in its source cell and try again when due.
            schedule_gravity_checks(world, pos);
            continue;
        }
        let runtime_id = next_actor_id(world);
        world.spawn((
            FallingBlock { state },
            Position(DVec3::new(
                f64::from(pos.x) + 0.5,
                f64::from(pos.y),
                f64::from(pos.z) + 0.5,
            )),
            Velocity::default(),
            Rotation::default(),
            OnGround(false),
            PhysicsBody::FALLING_BLOCK,
            RuntimeId(runtime_id),
            Age(0),
            ActorViewers::default(),
        ));
    }
}

/// Exclusive system: land falling blocks that stopped on the ground.
pub fn land_falling_blocks(world: &mut World) {
    let landed: Vec<(Entity, DVec3, u32, bool)> = {
        let mut query = world.query::<(Entity, &Position, &OnGround, &FallingBlock, &Age)>();
        query
            .iter(world)
            .filter(|(_, _, on_ground, _, age)| on_ground.0 || age.0 > MAX_FALL_TICKS)
            .map(|(entity, position, on_ground, block, _)| {
                (entity, position.0, block.state, on_ground.0)
            })
            .collect()
    };
    for (entity, position, state, on_ground) in landed {
        let cell = position.floor().as_ivec3();
        if super::containers::mutation_locked(world, cell) || world.block_at(cell).is_none() {
            continue;
        }
        if on_ground && is_free(world, cell) {
            if try_set_block(world, cell, state, true).is_ok() {
                world.despawn(entity);
            }
            continue;
        }
        let drops = super::block_drops::block_drops(world, state, None);
        world.despawn(entity);
        for stack in drops {
            super::block_drops::pop_resource(world, cell, stack);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::blocks::set_block;
    use super::*;
    use crate::entity::components::DroppedItem;
    use crate::entity::systems::tick_physics;
    use crate::registry::{BlockRegistry, ItemRegistry};
    use crate::server::game::types::ItemRegistryResource;
    use crate::world::chunk::blocks;
    use crate::world::ecs::{BlockBroadcastEvent, ChunkData, ChunkManager, ChunkPosition};
    use std::sync::Arc;

    fn world() -> (World, Entity) {
        let mut world = World::new();
        world.insert_resource(ChunkManager::new(
            crate::world::WorldConfig::void_spawn_platform(1),
        ));
        world.insert_resource(BlockRegistryResource(Arc::new(
            BlockRegistry::vanilla().clone(),
        )));
        let mut items = ItemRegistry::new();
        items.load_vanilla();
        world.insert_resource(ItemRegistryResource(Arc::new(items)));
        world.insert_resource(TickCounter::default());
        world.insert_resource(bevy_ecs::message::Messages::<BlockBroadcastEvent>::default());
        let mut chunk = crate::world::chunk::Chunk::new(0, 0);
        for x in 0..16 {
            for z in 0..16 {
                chunk.set_block(x, 60, z, *blocks::STONE);
            }
        }
        let chunk = world.spawn(ChunkData::new(chunk)).id();
        world
            .resource_mut::<ChunkManager>()
            .insert(ChunkPosition::new(0, 0), chunk);
        (world, chunk)
    }

    fn run(world: &mut World, ticks: usize) {
        let mut schedule = Schedule::default();
        schedule.add_systems(
            (
                process_gravity_checks,
                tick_physics,
                |mut ages: Query<&mut Age>| ages.iter_mut().for_each(|mut a| a.tick()),
                land_falling_blocks,
            )
                .chain(),
        );
        for _ in 0..ticks {
            schedule.run(world);
            world.resource_mut::<TickCounter>().current += 1;
        }
    }

    fn block(world: &World, chunk: Entity, y: i16) -> u32 {
        world
            .get::<ChunkData>(chunk)
            .unwrap()
            .inner
            .get_block(8, y, 8)
    }

    #[test]
    fn unsupported_sand_falls_and_lands_on_the_floor() {
        let (mut world, chunk) = world();
        set_block(&mut world, IVec3::new(8, 70, 8), *blocks::SAND, true);
        run(&mut world, 1);
        assert_eq!(block(&world, chunk, 70), *blocks::SAND, "waits two ticks");
        run(&mut world, 2);
        assert_eq!(block(&world, chunk, 70), *blocks::AIR);
        assert_eq!(
            world.query::<&FallingBlock>().iter(&world).count(),
            1,
            "the block became an entity"
        );
        run(&mut world, 60);
        assert_eq!(world.query::<&FallingBlock>().iter(&world).count(), 0);
        assert_eq!(
            block(&world, chunk, 61),
            *blocks::SAND,
            "landed on the stone floor"
        );
    }

    #[test]
    fn supported_sand_stays_and_breaking_its_support_drops_it() {
        let (mut world, chunk) = world();
        set_block(&mut world, IVec3::new(8, 61, 8), *blocks::STONE, false);
        set_block(&mut world, IVec3::new(8, 62, 8), *blocks::GRAVEL, false);
        run(&mut world, 5);
        assert_eq!(block(&world, chunk, 62), *blocks::GRAVEL);
        set_block(&mut world, IVec3::new(8, 61, 8), *blocks::AIR, false);
        run(&mut world, 40);
        assert_eq!(block(&world, chunk, 62), *blocks::AIR);
        assert_eq!(block(&world, chunk, 61), *blocks::GRAVEL);
    }

    #[test]
    fn blocks_landing_on_a_torch_drop_as_items() {
        let (mut world, chunk) = world();
        let torch = BlockRegistry::vanilla()
            .default_state_id_by_name("minecraft:torch")
            .unwrap();
        set_block(&mut world, IVec3::new(8, 61, 8), torch, false);
        set_block(&mut world, IVec3::new(8, 66, 8), *blocks::SAND, false);
        run(&mut world, 60);
        assert_eq!(block(&world, chunk, 61), torch, "the torch stays");
        assert_eq!(world.query::<&FallingBlock>().iter(&world).count(), 0);
        let items: Vec<String> = world
            .query_filtered::<&crate::entity::components::ItemEntityStack, With<DroppedItem>>()
            .iter(&world)
            .map(|s| s.0.item_id().to_owned())
            .collect();
        assert_eq!(items, vec!["minecraft:sand".to_string()]);
    }

    /// Reserve a real cell through the same ownership gate as native writers.
    fn reserve(world: &mut World, position: IVec3) {
        world.init_resource::<super::super::ContainerMutationLocks>();
        assert!(
            world
                .resource_mut::<super::super::ContainerMutationLocks>()
                .acquire("example:edit", [position])
        );
    }

    /// Release only the test operation's exact cell ownership.
    fn release(world: &mut World, position: IVec3) {
        assert!(
            world
                .resource_mut::<super::super::ContainerMutationLocks>()
                .release("example:edit", [position])
        );
    }

    /// Count the actual material held by physical dropped-item actors.
    fn dropped_sand(world: &mut World) -> usize {
        world
            .query_filtered::<&crate::entity::components::ItemEntityStack, With<DroppedItem>>()
            .iter(world)
            .filter(|item| item.0.item_id() == "minecraft:sand")
            .map(|item| usize::from(item.0.count))
            .sum()
    }

    #[test]
    fn source_reserved_after_gravity_was_scheduled_retries_without_duplicating_material() {
        let (mut world, chunk) = world();
        let source = IVec3::new(8, 70, 8);
        set_block(&mut world, source, *blocks::SAND, false);
        reserve(&mut world, source);
        run(&mut world, 9);
        assert_eq!(block(&world, chunk, 70), *blocks::SAND);
        assert_eq!(world.query::<&FallingBlock>().iter(&world).count(), 0);
        assert_eq!(dropped_sand(&mut world), 0);
        assert_eq!(
            world
                .resource::<GravityChecks>()
                .0
                .iter()
                .filter(|(position, _)| *position == source)
                .count(),
            1,
            "one future check keeps the still-owned source eligible"
        );
        release(&mut world, source);
        run(&mut world, 3);
        assert_eq!(block(&world, chunk, 70), *blocks::AIR);
        assert_eq!(world.query::<&FallingBlock>().iter(&world).count(), 1);
        run(&mut world, 60);
        assert_eq!(block(&world, chunk, 61), *blocks::SAND);
        assert_eq!(world.query::<&FallingBlock>().iter(&world).count(), 0);
        assert_eq!(dropped_sand(&mut world), 0);
    }

    #[test]
    fn reserved_landing_keeps_the_actor_even_after_its_drop_timeout() {
        let (mut world, chunk) = world();
        let destination = IVec3::new(8, 61, 8);
        set_block(&mut world, IVec3::new(8, 70, 8), *blocks::SAND, false);
        run(&mut world, 3);
        reserve(&mut world, destination);
        run(&mut world, MAX_FALL_TICKS as usize + 2);
        assert_eq!(block(&world, chunk, 61), *blocks::AIR);
        assert_eq!(world.query::<&FallingBlock>().iter(&world).count(), 1);
        assert_eq!(dropped_sand(&mut world), 0);
        release(&mut world, destination);
        run(&mut world, 2);
        assert_eq!(block(&world, chunk, 61), *blocks::SAND);
        assert_eq!(world.query::<&FallingBlock>().iter(&world).count(), 0);
        assert_eq!(dropped_sand(&mut world), 0);
    }

    #[test]
    fn late_reservation_of_a_nonreplaceable_landing_defers_the_single_item_drop() {
        let (mut world, chunk) = world();
        let destination = IVec3::new(8, 61, 8);
        let torch = BlockRegistry::vanilla()
            .default_state_id_by_name("minecraft:torch")
            .unwrap();
        set_block(&mut world, destination, torch, false);
        set_block(&mut world, IVec3::new(8, 70, 8), *blocks::SAND, false);
        run(&mut world, 3);
        reserve(&mut world, destination);
        run(&mut world, 60);
        assert_eq!(block(&world, chunk, 61), torch);
        assert_eq!(world.query::<&FallingBlock>().iter(&world).count(), 1);
        assert_eq!(dropped_sand(&mut world), 0);
        release(&mut world, destination);
        run(&mut world, 2);
        assert_eq!(world.query::<&FallingBlock>().iter(&world).count(), 0);
        assert_eq!(block(&world, chunk, 61), torch);
        assert_eq!(dropped_sand(&mut world), 1);
        run(&mut world, 10);
        assert_eq!(dropped_sand(&mut world), 1);
    }

    #[test]
    fn an_unloaded_landing_retains_the_actor_until_the_real_cell_returns() {
        let (mut world, chunk) = world();
        let actor = world
            .spawn((
                FallingBlock {
                    state: *blocks::SAND,
                },
                Position(DVec3::new(8.5, 61.0, 8.5)),
                OnGround(true),
                Age(0),
            ))
            .id();
        world.resource_mut::<ChunkManager>().remove_by_coords(0, 0);
        land_falling_blocks(&mut world);
        assert!(world.get::<FallingBlock>(actor).is_some());
        assert_eq!(dropped_sand(&mut world), 0);
        assert_eq!(block(&world, chunk, 61), *blocks::AIR);
        world
            .resource_mut::<ChunkManager>()
            .insert(ChunkPosition::new(0, 0), chunk);
        land_falling_blocks(&mut world);
        assert!(world.get_entity(actor).is_err());
        assert_eq!(block(&world, chunk, 61), *blocks::SAND);
        assert_eq!(dropped_sand(&mut world), 0);
    }

    #[test]
    fn rejected_native_writes_leave_cells_broadcasts_and_gravity_checks_unchanged() {
        use super::super::blocks::BlockWriteError;
        let (mut world, chunk) = world();
        let cell = IVec3::new(8, 61, 8);
        assert_eq!(
            try_set_block(&mut world, cell, *blocks::SAND, false),
            Ok(*blocks::AIR)
        );
        world.resource_mut::<GravityChecks>().0.clear();
        world
            .resource_mut::<Messages<BlockBroadcastEvent>>()
            .clear();
        reserve(&mut world, cell);
        assert_eq!(
            try_set_block(&mut world, cell, *blocks::AIR, true),
            Err(BlockWriteError::Reserved)
        );
        release(&mut world, cell);
        assert_eq!(
            try_set_block(
                &mut world,
                IVec3::new(8, 61 + 65_536, 8),
                *blocks::AIR,
                true,
            ),
            Err(BlockWriteError::OutsideHeight)
        );
        assert_eq!(
            try_set_block(&mut world, IVec3::new(24, 61, 8), *blocks::AIR, true),
            Err(BlockWriteError::Unavailable)
        );
        assert_eq!(block(&world, chunk, 61), *blocks::SAND);
        assert!(world.resource::<GravityChecks>().0.is_empty());
        assert!(world.resource::<Messages<BlockBroadcastEvent>>().is_empty());
    }
}

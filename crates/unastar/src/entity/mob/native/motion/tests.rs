use super::*;
use crate::entity::components::{GameMode, Player};
use crate::entity::mob::{SpawnCause, spawn_mob};
use crate::server::game::{GameServer, ServerConfig};
use crate::world::ChunkManager;
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkPosition};
use crate::world::rules::{Difficulty, GameRules};
use rand::SeedableRng;

/// A loaded stone floor with an unmodified native Blaze and a living target.
fn harness() -> (GameServer, Entity, Entity, Entity) {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..Default::default()
    });
    let world = server.ecs.world_mut();
    world.insert_resource(EntityRandom::seeded(19));
    world.insert_resource(Difficulty::Normal);
    world.resource_mut::<GameRules>().do_mob_spawning = false;
    let mut floor = Chunk::new(0, 0);
    for x in 0..16 {
        for z in 0..16 {
            floor.set_block(x, 63, z, *blocks::STONE);
        }
    }
    let chunk = world
        .spawn((ChunkData::new(floor), ChunkPosition::new(0, 0)))
        .id();
    world
        .resource_mut::<ChunkManager>()
        .insert(ChunkPosition::new(0, 0), chunk);
    let blaze = spawn_mob(
        world,
        "minecraft:blaze",
        DVec3::new(8.5, 68.0, 8.5),
        0.0,
        SpawnCause::Spawned,
    )
    .unwrap();
    let target = spawn_mob(
        world,
        "minecraft:cow",
        DVec3::new(8.5, 80.0, 4.5),
        0.0,
        SpawnCause::Spawned,
    )
    .unwrap();
    world.entity_mut(blaze).insert((
        HoverState {
            offset: 0.0,
            remaining: 100,
        },
        AttackTarget(target),
    ));
    (server, blaze, target, chunk)
}

#[test]
fn hover_height_resamples_every_hundred_active_ticks() {
    let mut rng = rand::rngs::StdRng::seed_from_u64(11);
    let mut state = HoverState::default();
    state.advance(&mut rng);
    let first = state.offset;
    assert_eq!(state.remaining, 100);
    for remaining in (1..100).rev() {
        state.advance(&mut rng);
        assert_eq!(state.offset, first);
        assert_eq!(state.remaining, remaining);
    }
    state.advance(&mut rng);
    assert_eq!(state.remaining, 100);
    assert_ne!(state.offset, first);
    assert!(state.is_valid());
}

#[test]
fn hover_velocity_is_signed_with_dead_zone_and_only_damps_idle_descent() {
    let state = HoverState::default();
    for delta in [-3.0, 3.0] {
        assert_eq!(
            state.vertical(0.7, false, Some(delta)),
            f64::from(0.1_f32).copysign(delta)
        );
    }
    for delta in [-0.04, 0.0, 0.04] {
        assert_eq!(state.vertical(-0.7, false, Some(delta)), 0.0);
    }
    assert_eq!(state.vertical(-0.5, false, None), -0.5 * f64::from(0.6_f32));
    assert_eq!(state.vertical(-0.5, true, None), -0.5);
    assert_eq!(state.vertical(0.5, false, None), 0.5);
}

#[test]
fn real_class_motion_uses_current_target_and_preserves_horizontal_velocity() {
    let (mut server, blaze, target, _) = harness();
    let world = server.ecs.world_mut();
    world.get_mut::<Velocity>(blaze).unwrap().0 = DVec3::new(0.3, -0.5, -0.2);
    tick_native_motion(world);
    assert_eq!(
        world.get::<Velocity>(blaze).unwrap().0,
        DVec3::new(0.3, f64::from(0.1_f32), -0.2)
    );
    world.get_mut::<Position>(target).unwrap().0.y = 64.0;
    tick_native_motion(world);
    assert_eq!(
        world.get::<Velocity>(blaze).unwrap().0.y,
        -f64::from(0.1_f32)
    );
    world
        .entity_mut(target)
        .insert((Player, GameMode::Creative));
    world.get_mut::<Velocity>(blaze).unwrap().0.y = -0.5;
    tick_native_motion(world);
    assert_eq!(
        world.get::<Velocity>(blaze).unwrap().0.y,
        -0.5 * f64::from(0.6_f32)
    );
    assert_eq!(world.get::<HoverState>(blaze).unwrap().remaining, 97);
}

#[test]
fn missing_life_geometry_and_unloaded_chunks_pause_native_motion() {
    let (mut server, blaze, _, _) = harness();
    let world = server.ecs.world_mut();
    let phase = *world.get::<HoverState>(blaze).unwrap();
    world.get_mut::<Velocity>(blaze).unwrap().0.y = -0.5;
    let position = world.entity_mut(blaze).take::<Position>().unwrap();
    tick_native_motion(world);
    assert_eq!(*world.get::<HoverState>(blaze).unwrap(), phase);
    world
        .entity_mut(blaze)
        .insert(Position(DVec3::new(800.0, 68.0, 8.5)));
    tick_native_motion(world);
    assert_eq!(*world.get::<HoverState>(blaze).unwrap(), phase);
    world.entity_mut(blaze).insert(position);
    let health = world.entity_mut(blaze).take::<Health>().unwrap();
    tick_native_motion(world);
    assert_eq!(*world.get::<HoverState>(blaze).unwrap(), phase);
    world.entity_mut(blaze).insert(health);
    world.entity_mut(blaze).insert(Dead);
    tick_native_motion(world);
    assert_eq!(*world.get::<HoverState>(blaze).unwrap(), phase);
    assert_eq!(world.get::<Velocity>(blaze).unwrap().0.y, -0.5);
}

#[test]
fn real_server_hover_hits_ceilings_and_idle_fall_still_uses_living_physics() {
    let (mut server, blaze, _, _) = harness();
    let world = server.ecs.world_mut();
    // Isolate the class-owned motion from authored navigation and target selection.
    world
        .entity_mut(blaze)
        .remove::<crate::entity::mob::ai::MobBrain>();
    crate::server::game::blocks::set_block(
        world,
        glam::IVec3::new(8, 70, 8),
        *blocks::STONE,
        false,
    );
    for _ in 0..15 {
        server.tick();
    }
    let world = server.ecs.world_mut();
    let top =
        world.get::<Position>(blaze).unwrap().0.y + world.get::<PhysicsBody>(blaze).unwrap().height;
    assert!(top <= 70.000001, "ceiling bounds: {top}");
    assert!(world.get::<Position>(blaze).unwrap().0.y > 68.0);
    crate::server::game::blocks::set_block(world, glam::IVec3::new(8, 70, 8), *blocks::AIR, false);
    world.entity_mut(blaze).remove::<AttackTarget>();
    world.get_mut::<Position>(blaze).unwrap().0.y = 72.0;
    world.get_mut::<Velocity>(blaze).unwrap().0.y = -0.5;
    world.get_mut::<OnGround>(blaze).unwrap().0 = false;
    server.tick();
    let world = server.ecs.world();
    let expected = 72.0 - 0.5 * f64::from(0.6_f32);
    assert!((world.get::<Position>(blaze).unwrap().0.y - expected).abs() < 1e-7);
    assert!(world.get::<Velocity>(blaze).unwrap().0.y < 0.0);
}

#[test]
fn native_motion_is_not_activated_for_other_actor_classes() {
    let (mut server, _, cow, _) = harness();
    let world = server.ecs.world_mut();
    world.entity_mut(cow).insert(HoverState::default());
    world.get_mut::<Velocity>(cow).unwrap().0.y = -0.5;
    tick_native_motion(world);
    assert_eq!(
        *world.get::<HoverState>(cow).unwrap(),
        HoverState::default()
    );
    assert_eq!(world.get::<Velocity>(cow).unwrap().0.y, -0.5);
}

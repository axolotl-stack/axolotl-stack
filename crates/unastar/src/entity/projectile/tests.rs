//! Real definition, collision, combat and packet regressions for fire projectiles.

use super::*;
use glam::{DVec3, IVec3};
use unastar_data::actor_identifiers::ActorType;

use crate::entity::EntityRandom;
use crate::entity::components::{GameMode, Health, PlayerSession, Position, Rotation, Velocity};
use crate::entity::mob::combat::player_attack;
use crate::entity::mob::{MobKind, MobRegistry, SpawnCause, spawn_mob};
use crate::entity::systems::BlockSource;
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};
use crate::world::ChunkManager;
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkPosition};
use crate::world::rules::{Difficulty, GameRules};

/// Loaded stone floor and a complete admitted survival player.
fn server() -> (
    GameServer,
    Entity,
    tokio::sync::mpsc::Receiver<jolyne::valentine::McpePacket>,
) {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..Default::default()
    });
    let world = server.ecs.world_mut();
    world.insert_resource(EntityRandom::seeded(17));
    world.insert_resource(Difficulty::Normal);
    world.resource_mut::<GameRules>().do_mob_spawning = false;
    let mut column = Chunk::new(0, 0);
    for x in 0..16 {
        for z in 0..16 {
            column.set_block(x, 63, z, *blocks::STONE);
        }
    }
    let chunk = world
        .spawn((ChunkData::new(column), ChunkPosition::new(0, 0)))
        .id();
    world
        .resource_mut::<ChunkManager>()
        .insert(ChunkPosition::new(0, 0), chunk);
    let (tx, mut rx) = tokio::sync::mpsc::channel(65536);
    let player = server
        .spawn_player(PlayerSpawnData {
            session_id: 1,
            runtime_id: 1,
            display_name: "Shooter".into(),
            xuid: None,
            uuid: None,
            position: DVec3::new(2.5, 65.62, 2.5),
            outbound_tx: tx,
            saved_data: None,
            chunk_radius: 0,
        })
        .unwrap();
    server
        .ecs
        .world_mut()
        .entity_mut(player)
        .insert(GameMode::Survival);
    let chunk = server
        .ecs
        .world()
        .resource::<ChunkManager>()
        .get_by_coords(0, 0)
        .unwrap();
    let mut viewers = crate::world::ecs::ChunkViewers::default();
    viewers.insert(player);
    server.ecs.world_mut().entity_mut(chunk).insert(viewers);
    while rx.try_recv().is_ok() {}
    (server, player, rx)
}

/// Spawn a small fireball without replacing or trimming its authored definition.
fn fireball(world: &mut World, at: DVec3, velocity: DVec3) -> Entity {
    let registry = world.resource::<MobRegistry>();
    assert!(
        registry.get("minecraft:small_fireball").is_some(),
        "{:?}",
        registry.unsupported_reason("minecraft:small_fireball")
    );
    let actor = spawn_mob(
        world,
        "minecraft:small_fireball",
        at,
        0.0,
        SpawnCause::Spawned,
    )
    .unwrap();
    world.entity_mut(actor).insert(Velocity(velocity));
    actor
}

#[test]
fn fire_projectile_has_an_explicit_empty_attribute_profile_and_real_middle_anchor() {
    let (mut server, player, mut rx) = server();
    let world = server.ecs.world_mut();
    let projectile = launch_aimed(
        world,
        player,
        ActorType::SmallFireball,
        DVec3::new(2.5, 65.4, 12.5),
        LaunchSpec {
            throw_sound: false,
            ..Default::default()
        },
    )
    .unwrap();
    // Player feet 64 + half body .9 + authored offset .5.
    assert!((world.get::<Position>(projectile).unwrap().0.y - 65.4).abs() < 1e-6);
    let kind = world.get::<MobKind>(projectile).unwrap();
    assert!(kind.0.attributes.is_empty());
    let attributes = world
        .get::<crate::entity::definition::attributes::ActorAttributes>(projectile)
        .unwrap();
    assert!(attributes.snapshot().into_entries().is_empty());
    let velocity = world.get::<Velocity>(projectile).unwrap().0;
    assert!(velocity.is_finite() && velocity.z > 0.8);
    assert_ne!(
        velocity.length(),
        1.3,
        "authored spread perturbs each axis before power"
    );
    assert!(
        world
            .get::<crate::entity::components::PhysicsBody>(projectile)
            .is_none(),
        "collision geometry must not add living physics"
    );
    assert!(
        world
            .resource::<MobRegistry>()
            .get("minecraft:not_a_projectile")
            .is_none()
    );
    let runtime = world
        .get::<crate::entity::components::RuntimeId>(projectile)
        .unwrap()
        .0 as u64;
    server.tick();
    let packet = std::iter::from_fn(|| rx.try_recv().ok())
        .find_map(|packet| match packet.data {
            jolyne::valentine::McpePacketData::AddActorPacket(packet)
                if packet.target_runtime_id.actor_runtime_id == runtime =>
            {
                Some(packet)
            }
            _ => None,
        })
        .expect("the admitted viewer receives the real projectile actor");
    assert_eq!(&*packet.actor_type, "minecraft:small_fireball");
    assert!(packet.attributes_list.is_empty());
    use valentine::bedrock::codec::BedrockCodec;
    let mut bytes = Vec::new();
    packet.encode(&mut bytes).unwrap();
    let mut input = bytes.as_slice();
    let decoded = jolyne::valentine::AddActorPacket::decode(&mut input, ()).unwrap();
    assert!(input.is_empty());
    assert!(decoded.attributes_list.is_empty());
}

#[test]
fn fire_projectile_direct_hit_damages_and_ignites_but_immunity_prevents_ignition() {
    let (mut server, _, _rx) = server();
    let world = server.ecs.world_mut();
    let cow = spawn_mob(
        world,
        "minecraft:cow",
        DVec3::new(8.5, 64.0, 8.5),
        0.0,
        SpawnCause::Spawned,
    )
    .unwrap();
    let projectile = fireball(world, DVec3::new(7.5, 64.5, 8.5), DVec3::X);
    let config = world.get::<ProjectileConfig>(projectile).unwrap().0.clone();
    let before = world.get::<Health>(cow).unwrap().current;
    assert!(hit::on_hit(
        world,
        projectile,
        None,
        &config,
        DVec3::new(8.2, 64.5, 8.5),
        flight::Target::Actor(cow)
    ));
    assert_eq!(world.get::<Health>(cow).unwrap().current, before - 5.0);
    assert!(
        world
            .get::<crate::entity::mob::combat::fire::Burning>(cow)
            .is_some()
    );
    crate::entity::mob::combat::fire::extinguish(world, cow);
    world
        .get_mut::<crate::entity::definition::flags::ActorFlags>(cow)
        .unwrap()
        .update(jolyne::valentine::actor_flags::FIRE_IMMUNE, true)
        .unwrap();
    world
        .entity_mut(cow)
        .remove::<crate::entity::mob::combat::HurtTimer>();
    hit::on_hit(
        world,
        projectile,
        None,
        &config,
        DVec3::new(8.2, 64.5, 8.5),
        flight::Target::Actor(cow),
    );
    assert!(
        world
            .get::<crate::entity::mob::combat::fire::Burning>(cow)
            .is_none()
    );
}

#[test]
fn actual_collision_ignites_the_hit_face_and_mob_griefing_suppresses_only_world_fire() {
    for griefing in [false, true] {
        let (mut server, _, _rx) = server();
        let world = server.ecs.world_mut();
        world.resource_mut::<GameRules>().mob_griefing = griefing;
        let cow = spawn_mob(
            world,
            "minecraft:cow",
            DVec3::new(12.5, 64.0, 12.5),
            0.0,
            SpawnCause::Spawned,
        )
        .unwrap();
        // A downward segment strikes the top of the actual stone floor.
        let projectile = fireball(world, DVec3::new(8.5, 64.8, 8.5), -DVec3::Y * 1.3);
        world.get_mut::<ProjectileFlight>(projectile).unwrap().owner = Some(cow);
        server.tick();
        let world = server.ecs.world();
        assert!(world.get_entity(projectile).is_err());
        let id = world.block_at(IVec3::new(8, 64, 8)).unwrap();
        let registry = &world
            .resource::<crate::server::game::types::BlockRegistryResource>()
            .0;
        assert_eq!(
            registry.get_by_runtime_id(id).unwrap().string_id,
            if griefing {
                "minecraft:fire"
            } else {
                "minecraft:air"
            }
        );
        assert_eq!(world.block_at(IVec3::new(8, 63, 8)), Some(*blocks::STONE));
    }
}

#[test]
fn player_owned_fire_can_ignite_with_mob_griefing_disabled_and_never_overwrites() {
    let (mut server, player, _rx) = server();
    let world = server.ecs.world_mut();
    world.resource_mut::<GameRules>().mob_griefing = false;
    let settings = config::BlockFire {
        affected_by_griefing: true,
        entity_fire_ticks: 100,
    };
    fire::block(world, Some(player), IVec3::new(5, 64, 5), settings);
    assert_ne!(world.block_at(IVec3::new(5, 64, 5)), Some(*blocks::AIR));
    fire::block(world, Some(player), IVec3::new(5, 63, 5), settings);
    assert_eq!(world.block_at(IVec3::new(5, 63, 5)), Some(*blocks::STONE));
    fire::block(world, Some(player), IVec3::new(32, 64, 32), settings);
    assert_eq!(world.block_at(IVec3::new(32, 64, 32)), None);
}

#[test]
fn actual_player_attack_redirects_without_health_or_a_new_actor_and_changes_owner() {
    let (mut server, player, _rx) = server();
    let world = server.ecs.world_mut();
    let projectile = fireball(world, DVec3::new(2.5, 65.4, 4.0), -DVec3::Z * 1.3);
    world.get_mut::<Rotation>(player).unwrap().yaw = 90.0;
    let runtime = world
        .get::<crate::entity::components::RuntimeId>(projectile)
        .unwrap()
        .0;
    player_attack(world, player, projectile, &Default::default());
    assert_eq!(
        world.get::<ProjectileFlight>(projectile).unwrap().owner,
        Some(player)
    );
    let velocity = world.get::<Velocity>(projectile).unwrap().0;
    assert!((velocity + DVec3::X * 1.3).length() < 1e-6);
    assert!(world.get::<Health>(projectile).is_none());
    assert_eq!(
        world
            .get::<crate::entity::components::RuntimeId>(projectile)
            .unwrap()
            .0,
        runtime
    );
    assert!(
        world
            .get::<crate::entity::components::PhysicsBody>(projectile)
            .is_none()
    );
}

#[test]
fn projectile_impact_reflects_a_reflectable_actor_with_the_incoming_direction() {
    let (mut server, player, _rx) = server();
    let world = server.ecs.world_mut();
    let target = fireball(world, DVec3::new(5.5, 65.0, 5.5), DVec3::Z * 1.3);
    let incoming = launch_with(
        world,
        player,
        ActorType::Snowball,
        None,
        LaunchSpec {
            throw_sound: false,
            ..Default::default()
        },
    )
    .unwrap();
    world.entity_mut(incoming).insert(Velocity(-DVec3::X * 1.5));
    assert!(reflect::from_projectile(world, target, incoming));
    assert_eq!(
        world.get::<ProjectileFlight>(target).unwrap().owner,
        Some(player)
    );
    assert!((world.get::<Velocity>(target).unwrap().0 + DVec3::X * 1.3).length() < 1e-6);
}

#[test]
fn malformed_launch_is_rejected_without_a_projectile_or_inventory_change() {
    let (mut server, player, _rx) = server();
    let before = server.ecs.world().entities().len();
    for target in [DVec3::splat(f64::NAN), DVec3::splat(f64::INFINITY)] {
        assert!(matches!(
            launch_aimed(
                server.ecs.world_mut(),
                player,
                ActorType::SmallFireball,
                target,
                Default::default()
            ),
            Err(LaunchError::InvalidAim)
        ));
    }
    assert_eq!(server.ecs.world().entities().len(), before);
    assert!(server.ecs.world().get::<PlayerSession>(player).is_some());
}

#[test]
fn actual_projectile_sweep_reflects_without_damaging_or_replacing_the_target() {
    let (mut server, player, _rx) = server();
    let world = server.ecs.world_mut();
    let target = fireball(world, DVec3::new(8.5, 65.0, 8.5), DVec3::Z * 0.01);
    let incoming = spawn_mob(
        world,
        "minecraft:snowball",
        DVec3::new(6.5, 65.15, 8.5),
        0.0,
        SpawnCause::Spawned,
    )
    .unwrap();
    world.entity_mut(incoming).insert(Velocity(DVec3::X * 1.5));
    world.get_mut::<ProjectileFlight>(incoming).unwrap().owner = Some(player);
    let runtime = world
        .get::<crate::entity::components::RuntimeId>(target)
        .unwrap()
        .0;
    server.tick();
    server.tick();
    let world = server.ecs.world();
    assert!(
        world.get_entity(incoming).is_err(),
        "the incoming projectile completes its actual hit"
    );
    assert_eq!(
        world.get::<ProjectileFlight>(target).unwrap().owner,
        Some(player)
    );
    assert!(world.get::<Velocity>(target).unwrap().0.x > 0.0);
    assert_eq!(
        world
            .get::<crate::entity::components::RuntimeId>(target)
            .unwrap()
            .0,
        runtime
    );
    assert!(world.get::<Health>(target).is_none());
}

#[test]
fn rejected_damage_does_not_ignite_an_invulnerable_target() {
    let (mut server, player, _rx) = server();
    let world = server.ecs.world_mut();
    let projectile = fireball(world, DVec3::new(4.5, 65.0, 4.5), DVec3::Z);
    let rules = world.get::<ProjectileConfig>(projectile).unwrap().0.clone();
    world.entity_mut(player).insert(GameMode::Creative);
    let before = world.get::<Health>(player).unwrap().current;
    hit::on_hit(
        world,
        projectile,
        None,
        &rules,
        DVec3::new(2.5, 65.0, 2.5),
        flight::Target::Actor(player),
    );
    assert_eq!(world.get::<Health>(player).unwrap().current, before);
    assert!(
        world
            .get::<crate::entity::player_life::environment::Burning>(player)
            .is_none()
    );
    let cow = spawn_mob(
        world,
        "minecraft:cow",
        DVec3::new(8.5, 64.0, 8.5),
        0.0,
        SpawnCause::Spawned,
    )
    .unwrap();
    world
        .entity_mut(cow)
        .insert(crate::entity::mob::combat::HurtTimer {
            until_tick: 100,
            last_damage: 100.0,
        });
    hit::on_hit(
        world,
        projectile,
        None,
        &rules,
        DVec3::new(8.5, 64.5, 8.5),
        flight::Target::Actor(cow),
    );
    assert!(
        world
            .get::<crate::entity::mob::combat::fire::Burning>(cow)
            .is_none()
    );
}

#[test]
fn swept_wall_and_ceiling_impacts_choose_the_air_cell_outside_the_hit_face() {
    for (block, from, velocity, fire_at) in [
        (
            IVec3::new(8, 65, 8),
            DVec3::new(7.5, 65.5, 8.5),
            DVec3::X * 1.3,
            IVec3::new(7, 65, 8),
        ),
        (
            IVec3::new(8, 65, 8),
            DVec3::new(9.5, 65.5, 8.5),
            -DVec3::X * 1.3,
            IVec3::new(9, 65, 8),
        ),
        (
            IVec3::new(8, 66, 8),
            DVec3::new(8.5, 65.5, 8.5),
            DVec3::Y * 1.3,
            IVec3::new(8, 65, 8),
        ),
    ] {
        let (mut server, player, _rx) = server();
        let world = server.ecs.world_mut();
        crate::server::game::blocks::set_block(world, block, *blocks::STONE, false);
        let projectile = fireball(world, from, velocity);
        world.get_mut::<ProjectileFlight>(projectile).unwrap().owner = Some(player);
        server.tick();
        let world = server.ecs.world();
        assert!(world.get_entity(projectile).is_err());
        assert_eq!(world.block_at(block), Some(*blocks::STONE));
        let fire = world.block_at(fire_at).unwrap();
        assert_eq!(
            world
                .resource::<crate::server::game::types::BlockRegistryResource>()
                .0
                .get_by_runtime_id(fire)
                .unwrap()
                .string_id,
            "minecraft:fire"
        );
    }
}

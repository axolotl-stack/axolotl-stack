//! Active-tick schedules and real native actor integration.

use super::*;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::entity::EntityRandom;
use crate::entity::components::{GameMode, Projectile};
use crate::entity::goal_selector::GoalControls;
use crate::entity::mob::{MobKind, MobRegistry, SpawnCause, spawn_mob};
use crate::entity::projectile::ProjectileFlight;
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};
use crate::world::ChunkManager;
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkPosition};
use crate::world::rules::{Difficulty, GameRules};

/// A charged three-shot attack whose durations are authored in active ticks.
fn goal() -> RangedAttackGoal {
    RangedAttackGoal {
        priority: 3,
        controls: GoalControls::MOVE | GoalControls::LOOK,
        attack_interval: (60, 100),
        min_range: 0.0,
        max_range: 48.0,
        burst_interval: 6,
        burst_shots: 3,
        charge_ticks: 80,
        fov: 90.0,
        speed_multiplier: 1.0,
        sight_ticks: 20,
        movement_mode: body::RangedAttackGoalDefinitionInRangeMovementMode::HoldPosition,
        x_max_rotation: 30.0,
        y_max_rotation: 30.0,
        set_persistent: false,
    }
}

#[test]
fn charged_bursts_have_stable_active_tick_phases_and_reload_between_bursts() {
    let mut state = RangedState::default();
    let shots: Vec<_> = (1..=450)
        .filter(|_| state.advance(&goal(), true, true, 12.0))
        .collect();
    assert_eq!(shots, [80, 86, 92, 253, 259, 265, 426, 432, 438]);
}

#[test]
fn unseen_charge_decays_and_inactive_time_cannot_advance_the_state() {
    let mut state = RangedState::default();
    for _ in 0..40 {
        assert!(!state.advance(&goal(), true, true, 12.0));
    }
    assert!(state.charged());
    for _ in 0..20 {
        assert!(!state.advance(&goal(), false, false, 12.0));
    }
    assert_eq!(state.charge, 20);
    for _ in 0..59 {
        assert!(!state.advance(&goal(), true, true, 12.0));
    }
    assert!(state.advance(&goal(), true, true, 12.0));
    assert!(!state.charged());
}

#[test]
fn ordinary_reload_scales_with_distance_and_visibility_is_independent_of_range() {
    let mut config = goal();
    config.charge_ticks = 0;
    for (distance, expected) in [(0.0, 60), (48.0, 100)] {
        let mut state = RangedState::default();
        for _ in 1..expected {
            assert!(!state.advance(&config, true, true, distance));
        }
        assert!(state.advance(&config, true, true, distance));
    }
    let mut state = RangedState::default();
    for _ in 0..25 {
        assert!(!state.advance(&config, false, true, 60.0));
    }
    assert_eq!(
        state.seen, 25,
        "a visible out-of-range target can still be approached"
    );
}

/// Full vanilla registry, loaded stone floor and an admitted survival target.
/// Additional target health permits observing multiple complete native bursts.
fn harness() -> (
    GameServer,
    Entity,
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
    world.resource_mut::<GameRules>().mob_griefing = false;
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
    let (tx, rx) = tokio::sync::mpsc::channel(65536);
    let player = server
        .spawn_player(PlayerSpawnData {
            session_id: 1,
            runtime_id: 1,
            display_name: "Observer".into(),
            xuid: None,
            uuid: Some(uuid::Uuid::from_u128(1).to_string()),
            position: DVec3::new(8.5, 65.62, 2.5),
            outbound_tx: tx,
            saved_data: None,
            chunk_radius: 0,
        })
        .unwrap();
    server
        .ecs
        .world_mut()
        .entity_mut(player)
        .insert((GameMode::Survival, Health::with_current(200.0, 200.0)));
    let world = server.ecs.world_mut();
    assert!(
        world
            .resource::<MobRegistry>()
            .get("minecraft:blaze")
            .is_some(),
        "{:?}",
        world
            .resource::<MobRegistry>()
            .unsupported_reason("minecraft:blaze")
    );
    let blaze = spawn_mob(
        world,
        "minecraft:blaze",
        DVec3::new(8.5, 64.0, 10.5),
        180.0,
        SpawnCause::Spawned,
    )
    .unwrap();
    assert_eq!(
        &*world.get::<MobKind>(blaze).unwrap().0.source,
        &**unastar_data::actor_definitions::get("minecraft:blaze").unwrap(),
        "the complete authored definition is retained"
    );
    (server, player, blaze, rx)
}

/// Reach the actual sensor-driven ranged mode rather than inserting goal stubs.
fn ready(server: &mut GameServer, blaze: Entity, player: Entity) {
    for _ in 0..200 {
        server.tick();
        if server.ecs.world().get::<RangedAttackGoal>(blaze).is_some()
            && server
                .ecs
                .world()
                .get::<AttackTarget>(blaze)
                .is_some_and(|target| target.0 == player)
        {
            return;
        }
    }
    panic!("the original actor did not enter ranged mode");
}

/// Capture exactly the work that the real goal defers beyond its resource scope.
fn request(world: &World, blaze: Entity, player: Entity) -> ShotRequest {
    ShotRequest {
        target: player,
        goal: world.get::<RangedAttackGoal>(blaze).unwrap().clone(),
        shooter: world.get::<ShooterConfig>(blaze).unwrap().clone(),
        groups: world
            .get::<DefinitionState>(blaze)
            .unwrap()
            .group_history()
            .map(|(name, on)| (name.to_owned(), on))
            .collect(),
    }
}

#[test]
fn complete_blaze_definition_fires_owned_bursts_that_damage_and_ignite_a_real_player() {
    let (mut server, player, blaze, _rx) = harness();
    let mut shots = BTreeMap::new();
    let mut charged = false;
    let mut burned = false;
    for tick in 0..380 {
        server.tick();
        let world = server.ecs.world_mut();
        for (actor, flight, kind) in world
            .query_filtered::<(Entity, &ProjectileFlight, &MobKind), With<Projectile>>()
            .iter(world)
        {
            if kind.0.identifier.as_ref() == "minecraft:small_fireball" {
                assert_eq!(flight.owner, Some(blaze));
                shots.entry(actor).or_insert(tick);
            }
        }
        charged |= world
            .get::<crate::entity::definition::flags::ActorFlags>(blaze)
            .unwrap()
            .get(jolyne::valentine::actor_flags::CHARGED)
            .unwrap();
        burned |= world
            .get::<crate::entity::player_life::environment::Burning>(player)
            .is_some();
    }
    assert!(
        charged,
        "charge is replicated through the actor's real flags"
    );
    assert!(
        shots.len() >= 6,
        "observed {} actual projectiles",
        shots.len()
    );
    assert!(server.ecs.world().get::<Health>(player).unwrap().current < 200.0);
    assert!(burned, "accepted fire impacts use player ignition");
    let mut times: Vec<_> = shots.values().copied().collect();
    times.sort();
    assert_eq!((times[1] - times[0], times[2] - times[1]), (6, 6));
    assert_eq!(times[3] - times[2], 161);
}

#[test]
fn original_target_sensor_keeps_hysteresis_and_changes_real_composed_attack_modes() {
    let (mut server, player, blaze, _rx) = harness();
    ready(&mut server, blaze, player);
    for (distance, ranged) in [(2.5, true), (1.5, false), (2.5, false), (3.5, true)] {
        let feet = server
            .ecs
            .world()
            .get::<crate::entity::components::Position>(blaze)
            .unwrap()
            .0;
        server
            .ecs
            .world_mut()
            .get_mut::<crate::entity::components::Position>(player)
            .unwrap()
            .0 = feet + DVec3::new(0.0, crate::entity::PLAYER_EYE_HEIGHT, -distance);
        server.tick();
        server.tick();
        let world = server.ecs.world();
        assert_eq!(
            world.get::<RangedAttackGoal>(blaze).is_some(),
            ranged,
            "distance {distance}"
        );
        assert_eq!(
            world
                .get::<crate::entity::mob::components::MeleeBoxAttackGoal>(blaze)
                .is_some(),
            !ranged
        );
    }
}

#[test]
fn deferred_shots_reject_new_creative_mode_and_current_anger_filter_exclusions() {
    let (mut server, player, blaze, _rx) = harness();
    ready(&mut server, blaze, player);
    let world = server.ecs.world_mut();
    let pending = request(world, blaze, player);
    let count = world.entities().len();
    world.entity_mut(player).insert(GameMode::Creative);
    shoot(world, blaze, pending);
    assert_eq!(world.entities().len(), count);
    world.entity_mut(player).insert(GameMode::Survival);
    let pending = request(world, blaze, player);
    let filter = unastar_rules::filter::Filter::parse(
        &serde_json::json!({"test":"is_family","subject":"other","value":"monster"}),
        unastar_rules::filter::FilterRegistry::vanilla(),
        "test",
        &mut Default::default(),
    )
    .unwrap();
    world
        .entity_mut(blaze)
        .insert(crate::entity::mob::components::reactive::Angry {
            config: Arc::new(crate::entity::mob::components::reactive::AngerConfig {
                duration_ticks: 100,
                delta_ticks: 0,
                calm_event: None,
                filter: Some(filter),
                sound: None,
                sound_interval: (0, 0),
            }),
            until_tick: 1000,
            next_sound_tick: 0,
            calmed: false,
        });
    shoot(world, blaze, pending);
    assert_eq!(world.entities().len(), count);
}

#[test]
fn deferred_shots_reject_pending_mode_removal_and_new_solid_obstructions() {
    let (mut server, player, blaze, _rx) = harness();
    ready(&mut server, blaze, player);
    let world = server.ecs.world_mut();
    let pending = request(world, blaze, player);
    let count = world.entities().len();
    let event = world
        .get::<MobKind>(blaze)
        .unwrap()
        .0
        .events
        .event("switch_to_melee")
        .unwrap();
    crate::entity::mob::events::fire_event(world, blaze, &event, Some(player)).unwrap();
    assert!(
        world.get::<RangedAttackGoal>(blaze).is_some(),
        "the ECS removal is still pending"
    );
    shoot(world, blaze, pending);
    assert_eq!(world.entities().len(), count);
    let (mut server, player, blaze, _rx) = harness();
    ready(&mut server, blaze, player);
    let world = server.ecs.world_mut();
    let pending = request(world, blaze, player);
    let count = world.entities().len();
    for y in 64..=66 {
        crate::server::game::blocks::set_block(
            world,
            glam::IVec3::new(8, y, 6),
            *blocks::STONE,
            false,
        );
    }
    shoot(world, blaze, pending);
    assert_eq!(world.entities().len(), count);
}

#[test]
fn startup_rejects_required_nonprojectiles_but_keeps_optional_spawn_targets() {
    use crate::entity::definition::properties::ActorPropertyRegistry;
    use crate::registry::entity::ActorDefinitionRegistryBuilder;
    use crate::server::game::types::ItemRegistryResource;
    let (server, _, _, _rx) = harness();
    let mut blaze = (**unastar_data::actor_definitions::get("minecraft:blaze").unwrap()).clone();
    blaze.component_groups.get_mut("ranged_mode").unwrap().insert("minecraft:shooter".into(),serde_json::json!({"projectiles":[{"def":"minecraft:cow","filters":{"test":"is_family","subject":"other","value":"monster"}}],"def":"minecraft:small_fireball"}));
    let mut fireball =
        (**unastar_data::actor_definitions::get("minecraft:small_fireball").unwrap()).clone();
    fireball.components.get_mut("minecraft:projectile").unwrap()["on_hit"]["spawn_chance"] = serde_json::json!({"spawn_definition":"minecraft:blaze","first_spawn_chance":1.0,"first_spawn_count":1});
    let mut definitions = ActorDefinitionRegistryBuilder::default();
    definitions.load_vanilla();
    definitions.register(Arc::new(blaze));
    definitions.register(Arc::new(fireball));
    let definitions = definitions.freeze();
    let properties = ActorPropertyRegistry::compile(&definitions).unwrap();
    let registry = MobRegistry::compile(
        &definitions,
        &properties,
        &server.ecs.world().resource::<ItemRegistryResource>().0,
    )
    .unwrap();
    assert!(registry.get("minecraft:blaze").is_none());
    assert!(
        registry
            .unsupported_reason("minecraft:blaze")
            .unwrap()
            .contains("no base projectile")
    );
    assert!(
        registry.get("minecraft:small_fireball").is_some(),
        "an unavailable optional impact spawn must not disable the projectile"
    );
}

#[test]
fn required_projectile_references_close_over_later_validation_failures() {
    use crate::entity::definition::properties::ActorPropertyRegistry;
    use crate::registry::entity::ActorDefinitionRegistryBuilder;
    use crate::server::game::types::ItemRegistryResource;
    let (server, _, _, _rx) = harness();
    let mut fireball =
        (**unastar_data::actor_definitions::get("minecraft:small_fireball").unwrap()).clone();
    fireball.components.insert(
        "minecraft:shooter".into(),
        serde_json::json!({"def":"minecraft:cow"}),
    );
    let mut definitions = ActorDefinitionRegistryBuilder::default();
    definitions.load_vanilla();
    definitions.register(Arc::new(fireball));
    let definitions = definitions.freeze();
    let properties = ActorPropertyRegistry::compile(&definitions).unwrap();
    let registry = MobRegistry::compile(
        &definitions,
        &properties,
        &server.ecs.world().resource::<ItemRegistryResource>().0,
    )
    .unwrap();
    assert!(registry.get("minecraft:small_fireball").is_none());
    assert!(
        registry
            .unsupported_reason("minecraft:small_fireball")
            .unwrap()
            .contains("no base projectile")
    );
    assert!(registry.get("minecraft:blaze").is_none());
    assert!(
        registry
            .unsupported_reason("minecraft:blaze")
            .unwrap()
            .contains("not a spawnable")
    );
}

#[test]
fn vision_loss_inside_inner_boundary_dispatches_even_without_must_see() {
    let (mut server, player, blaze, _rx) = harness();
    ready(&mut server, blaze, player);
    let world = server.ecs.world_mut();
    let mut config = world.get::<TargetNearbySensor>(blaze).unwrap().clone();
    config.inside_range = 16.0;
    config.outside_range = 20.0;
    config.must_see = false;
    config.vision_lost = config.inside.take();
    world.entity_mut(blaze).insert((
        config,
        TargetNearbyState {
            target: Some(player),
            inside: Some(true),
            visible: true,
        },
    ));
    for y in 64..=67 {
        crate::server::game::blocks::set_block(
            world,
            glam::IVec3::new(8, y, 6),
            *blocks::STONE,
            false,
        );
    }
    sensor(world, blaze);
    assert!(
        !world
            .get::<DefinitionState>(blaze)
            .unwrap()
            .has_actor_component(ActorComponent::BehaviorRangedAttack)
    );
    assert!(!world.get::<TargetNearbyState>(blaze).unwrap().visible);
    sensor(world, blaze);
    assert!(!world.get::<TargetNearbyState>(blaze).unwrap().visible);
}

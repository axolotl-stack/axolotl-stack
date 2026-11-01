//! Authored step heights and riding facts through real definition factories,
//! tick ordering and loaded block collision geometry.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::actor_flags;
use serde_json::{Value, json};
use unastar_packs::actor::{Loaded, load_value};

use crate::entity::components::{PhysicsBody, Position};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::properties::ActorPropertyRegistry;
use crate::entity::mob::components::common;
use crate::entity::mob::components::movement::VariableMaxAutoStep;
use crate::entity::mob::events::MobFacts;
use crate::entity::mob::{MobRegistry, SpawnCause, spawn_mob};
use crate::entity::systems::physics::move_aabb;
use crate::registry::BlockRegistry;
use crate::registry::collision::Aabb;
use crate::registry::entity::ActorDefinitionRegistryBuilder;
use crate::server::game::types::ItemRegistryResource;
use crate::server::game::{GameServer, ServerConfig};
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkManager, ChunkPosition, ChunkViewers};

const FEET: DVec3 = DVec3::new(4.5, 64.0, 4.5);

/// Compile a small zombie representation with independently varied movement
/// input through the normal component factories.
fn server(config: Value) -> GameServer {
    let document = json!({
        "format_version":"1.26.50",
        "minecraft:entity": {
            "description":{"identifier":"minecraft:zombie","is_summonable":true},
            "components": {
                "minecraft:collision_box":{"width":0.6,"height":1.8},
                "minecraft:health":{"value":20}, "minecraft:physics":{},
                "minecraft:variable_max_auto_step":config
            }
        }
    });
    let Loaded::Definition { definition, .. } = load_value(
        "automatic step control",
        &document,
        unastar_data::actor_definitions::parser(),
    )
    .unwrap() else {
        panic!("control skipped")
    };
    let mut definitions = ActorDefinitionRegistryBuilder::default();
    definitions.register(Arc::from(definition));
    let definitions = definitions.freeze();
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let world = server.ecs.world_mut();
    let properties = ActorPropertyRegistry::compile(&definitions).unwrap();
    let mobs = MobRegistry::compile(
        &definitions,
        &properties,
        &world.resource::<ItemRegistryResource>().0,
    )
    .unwrap();
    world.insert_resource(mobs);
    world.insert_resource(properties);
    let mut column = Chunk::new(0, 0);
    for x in 0..16 {
        for z in 0..16 {
            column.set_block(x, 63, z, *blocks::STONE);
        }
    }
    column.set_block(5, 64, 4, *blocks::STONE);
    let chunk = world
        .spawn((ChunkData::new(column), ChunkViewers::default()))
        .id();
    world
        .resource_mut::<ChunkManager>()
        .insert(ChunkPosition::new(0, 0), chunk);
    server
}

/// Read the effective height used by actual physics and navigation.
fn height(server: &GameServer, actor: Entity) -> f64 {
    server
        .ecs
        .world()
        .get::<PhysicsBody>(actor)
        .unwrap()
        .step_height
}

#[test]
fn defaults_are_typed_and_invalid_heights_fail_the_archetype() {
    let defaults = unastar_data::actor_components::decode::<
        unastar_data::actor_components::body::VariableMaxAutoStep,
    >(&json!({}))
    .unwrap();
    assert_eq!(defaults.base_value, 0.5625);
    assert_eq!(defaults.controlled_value, 0.5625);
    assert_eq!(defaults.jump_prevented_value, 0.5625);
    assert!(
        unastar_data::actor_components::decode::<
            unastar_data::actor_components::body::VariableMaxAutoStep,
        >(&json!({"typo":1}))
        .is_err()
    );
    let mut server = server(json!({"base_value":-1}));
    assert!(
        spawn_mob(
            server.ecs.world_mut(),
            "minecraft:zombie",
            FEET,
            0.0,
            SpawnCause::Spawned
        )
        .is_err()
    );
}

#[test]
fn actual_collision_steps_one_block_only_with_the_authored_base_height() {
    let mut server = server(json!({"base_value":1.0625,"controlled_value":0.5625}));
    let actor = spawn_mob(
        server.ecs.world_mut(),
        "minecraft:zombie",
        FEET,
        0.0,
        SpawnCause::Spawned,
    )
    .unwrap();
    server.tick();
    assert_eq!(height(&server, actor), 1.0625);
    let aabb = Aabb::from_feet(FEET, 0.6, 1.8);
    let tall = move_aabb(
        server.ecs.world(),
        aabb,
        DVec3::X,
        height(&server, actor),
        true,
    );
    assert_eq!(tall.moved, DVec3::new(1.0, 1.0, 0.0));
    server
        .ecs
        .world_mut()
        .get_mut::<ActorFlags>(actor)
        .unwrap()
        .update(actor_flags::WASD_CONTROLLED, true)
        .unwrap();
    server.tick();
    assert_eq!(height(&server, actor), 0.5625);
    let short = move_aabb(
        server.ecs.world(),
        aabb,
        DVec3::X,
        height(&server, actor),
        true,
    );
    assert!(short.moved.x < 0.21 && short.moved.y == 0.0);
}

#[test]
fn current_ground_and_component_removal_refresh_without_health_changes() {
    let mut server = server(json!({"base_value":1.0625,"jump_prevented_value":0.25}));
    let actor = spawn_mob(
        server.ecs.world_mut(),
        "minecraft:zombie",
        FEET,
        0.0,
        SpawnCause::Spawned,
    )
    .unwrap();
    server.tick();
    assert_eq!(height(&server, actor), 1.0625);
    let world = server.ecs.world_mut();
    let chunk = world
        .resource::<ChunkManager>()
        .get_by_coords(0, 0)
        .unwrap();
    let honey = BlockRegistry::vanilla()
        .get_by_name("minecraft:honey_block")
        .unwrap()
        .default_state_id;
    world
        .get_mut::<ChunkData>(chunk)
        .unwrap()
        .inner
        .set_block(4, 63, 4, honey);
    world.get_mut::<Position>(actor).unwrap().0 = FEET - DVec3::Y * 0.0625;
    server.tick();
    assert_eq!(height(&server, actor), 0.25);
    server.ecs.world_mut().get_mut::<Position>(actor).unwrap().0 = FEET + DVec3::Z;
    server.tick();
    assert_eq!(height(&server, actor), 1.0625);
    server
        .ecs
        .world_mut()
        .entity_mut(actor)
        .remove::<VariableMaxAutoStep>();
    server.tick();
    assert_eq!(height(&server, actor), 0.5625);
}

#[test]
fn riding_filter_reads_both_subjects_actual_flag_and_clears_after_dismount() {
    let mut server = server(json!({}));
    let world = server.ecs.world_mut();
    let actor = spawn_mob(world, "minecraft:zombie", FEET, 0.0, SpawnCause::Spawned).unwrap();
    let other = world.spawn(ActorFlags::default()).id();
    let own = common::filter(&json!({"test":"is_riding","subject":"self","value":true})).unwrap();
    let them = common::filter(&json!({"test":"is_riding","subject":"other","value":true})).unwrap();
    let check = |world: &World| {
        let facts = MobFacts {
            world,
            entity: actor,
            other: Some(other),
        };
        (own.evaluate(&facts), them.evaluate(&facts))
    };
    assert_eq!(check(world), (false, false));
    world
        .get_mut::<ActorFlags>(actor)
        .unwrap()
        .update(actor_flags::RIDING, true)
        .unwrap();
    assert_eq!(check(world), (true, false));
    world
        .get_mut::<ActorFlags>(other)
        .unwrap()
        .update(actor_flags::RIDING, true)
        .unwrap();
    assert_eq!(check(world), (true, true));
    world
        .get_mut::<ActorFlags>(actor)
        .unwrap()
        .update(actor_flags::RIDING, false)
        .unwrap();
    assert_eq!(check(world), (false, true));
}

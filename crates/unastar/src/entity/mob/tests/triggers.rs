//! Authored combat events exercised through real damage and AI commits.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::DVec3;
use serde_json::{Value, json};
use unastar_data::actor_components::body::ActorDamageCause;
use unastar_packs::actor::{Loaded, load_value};

use crate::entity::EntityRandom;
use crate::entity::components::{GameMode, Health, Player, Position, Rotation, RuntimeEntityId};
use crate::entity::definition::properties::{
    ActorProperties, ActorPropertyRegistry, NamedPropertyValue, PropertyValue,
};
use crate::entity::mob::combat::{Hit, HurtTimer, hurt_mob};
use crate::entity::mob::components::KnockbackPower;
use crate::entity::mob::components::hostile::FollowRangeConfig;
use crate::entity::mob::state::AttackTarget;
use crate::entity::mob::{MobRegistry, SpawnCause, spawn_mob};
use crate::registry::entity::ActorDefinitionRegistryBuilder;
use crate::server::game::types::ItemRegistryResource;
use crate::server::game::{GameServer, ServerConfig};
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkManager, ChunkPosition, ChunkViewers};
use crate::world::rules::Difficulty;

const FEET: DVec3 = DVec3::new(4.5, 64.0, 4.5);

struct Harness {
    server: GameServer,
    player: Entity,
    mob: Entity,
}

/// Compile a small authored definition using the normal typed factories and
/// native zombie representation. This is a component control, not a replacement
/// for a vanilla mob compatibility test.
fn harness(components: Value) -> Harness {
    let document = json!({
        "format_version": "1.26.50",
        "minecraft:entity": {
            "description": {
                "identifier": "minecraft:zombie", "is_summonable": true,
                "properties": {
                    "test:hurt": {"type":"bool", "default":false},
                    "test:player_hurt": {"type":"bool", "default":false},
                    "test:targeted": {"type":"bool", "default":false}
                }
            },
            "components": components,
            "events": {
                "test:hurt": {"set_property":{"test:hurt":true}},
                "test:player_hurt": {"set_property":{"test:player_hurt":true}},
                "test:targeted": {"set_property":{"test:targeted":true}}
            }
        }
    });
    let Loaded::Definition { definition, .. } = load_value(
        "combat event control",
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
    assert!(
        mobs.get("minecraft:zombie").is_some(),
        "{:?}",
        mobs.unsupported_reason("minecraft:zombie")
    );
    world.insert_resource(mobs);
    world.insert_resource(properties);
    world.insert_resource(EntityRandom::seeded(10));
    *world.resource_mut::<Difficulty>() = Difficulty::Normal;
    let player = world
        .spawn((
            Player,
            Health::default(),
            GameMode::Creative,
            Position(FEET + DVec3::new(6.0, crate::entity::PLAYER_EYE_HEIGHT, 0.0)),
            Rotation::default(),
            RuntimeEntityId(10),
        ))
        .id();
    let mut column = Chunk::new(0, 0);
    for x in 0..16 {
        for z in 0..16 {
            column.set_block(x, 63, z, *blocks::STONE);
        }
    }
    let mut viewers = ChunkViewers::default();
    viewers.insert(player);
    let chunk = world.spawn((ChunkData::new(column), viewers)).id();
    world
        .resource_mut::<ChunkManager>()
        .insert(ChunkPosition::new(0, 0), chunk);
    let mob = spawn_mob(world, "minecraft:zombie", FEET, 0.0, SpawnCause::Spawned).unwrap();
    Harness {
        server,
        player,
        mob,
    }
}

/// Components common to the stationary component control.
fn components() -> Value {
    json!({
        "minecraft:health":{"value":100,"max":100},
        "minecraft:collision_box":{"width":0.6,"height":1.8},
        "minecraft:movement":{"value":0.0},
        "minecraft:movement.basic":{}, "minecraft:navigation.walk":{},
        "minecraft:physics":{},
        "minecraft:on_hurt":{"event":"test:hurt","target":"self"},
        "minecraft:on_hurt_by_player":{"event":"test:player_hurt","target":"self",
            "filters":{"test":"is_family","subject":"other","value":"player"}},
        "minecraft:on_target_acquired":{"event":"test:targeted","target":"self"}
    })
}

impl Harness {
    /// Read the authored Boolean event observation.
    fn flag(&self, property: &str) -> bool {
        let properties = self
            .server
            .ecs
            .world()
            .get::<ActorProperties>(self.mob)
            .unwrap();
        properties
            .value(&properties.layout().slot(property).unwrap())
            .unwrap()
            == PropertyValue::Bool(true)
    }

    /// Reset an observation without changing trigger or AI composition.
    fn reset(&mut self, property: &str) {
        self.server
            .ecs
            .world_mut()
            .get_mut::<ActorProperties>(self.mob)
            .unwrap()
            .set_named(property, NamedPropertyValue::Bool(false))
            .unwrap();
    }

    /// Commit one hit through the normal timer/sensor/health damage path.
    fn hit(&mut self, damage: f32, attacker: Option<Entity>, by_player: bool) -> f32 {
        hurt_mob(
            self.server.ecs.world_mut(),
            self.mob,
            Hit {
                damage,
                cause: ActorDamageCause::EntityAttack,
                attacker,
                by_player,
                knockback_from: None,
                knockback_power: KnockbackPower::DEFAULT,
            },
        )
    }
}

#[test]
fn accepted_hits_dispatch_filtered_triggers_but_timer_rejections_do_not() {
    let mut h = harness(components());
    let player = h.player;
    assert_eq!(h.hit(2.0, Some(player), true), 2.0);
    assert!(h.flag("test:hurt"));
    assert!(h.flag("test:player_hurt"));
    h.reset("test:hurt");
    h.reset("test:player_hurt");
    assert_eq!(h.hit(2.0, Some(player), true), 0.0);
    assert!(!h.flag("test:hurt"));
    assert!(!h.flag("test:player_hurt"));
    let mob = h.mob;
    h.server
        .ecs
        .world_mut()
        .entity_mut(mob)
        .remove::<HurtTimer>();
    assert_eq!(h.hit(1.0, None, false), 1.0);
    assert!(h.flag("test:hurt"));
    assert!(!h.flag("test:player_hurt"));
    h.reset("test:hurt");
    h.server
        .ecs
        .world_mut()
        .entity_mut(mob)
        .remove::<HurtTimer>();
    assert_eq!(h.hit(0.0, Some(player), true), 0.0);
    assert!(h.flag("test:hurt"));
    assert!(h.flag("test:player_hurt"));
}

#[test]
fn a_cancelled_damage_sensor_does_not_emit_hit_lifecycle_events() {
    let mut body = components();
    body["minecraft:damage_sensor"] = json!({"triggers":[{
        "cause":"entity_attack", "deals_damage":"no"
    }]});
    let mut h = harness(body);
    let player = h.player;
    assert_eq!(h.hit(4.0, Some(player), true), 0.0);
    assert!(!h.flag("test:hurt"));
    assert!(!h.flag("test:player_hurt"));
    assert_eq!(
        h.server.ecs.world().get::<Health>(h.mob).unwrap().current,
        100.0
    );
    assert!(h.server.ecs.world().get::<HurtTimer>(h.mob).is_none());
}

#[test]
fn target_events_fire_once_and_authored_follow_range_overrides_legacy_radius() {
    let mut body = components();
    body["minecraft:follow_range"] = json!({"value":8,"max":8});
    body["minecraft:behavior.nearest_attackable_target"] = json!({
        "priority":1,"within_radius":2,"must_see":false,"scan_interval":1,
        "entity_types":[{"filters":{"test":"is_family","subject":"other","value":"player"},"max_dist":32}]
    });
    let mut h = harness(body);
    let (player, mob) = (h.player, h.mob);
    *h.server
        .ecs
        .world_mut()
        .get_mut::<GameMode>(player)
        .unwrap() = GameMode::Survival;
    for _ in 0..5 {
        h.server.tick();
    }
    assert_eq!(
        h.server.ecs.world().get::<AttackTarget>(mob).unwrap().0,
        player
    );
    assert!(h.flag("test:targeted"));
    h.reset("test:targeted");
    for _ in 0..5 {
        h.server.tick();
    }
    assert!(
        !h.flag("test:targeted"),
        "retaining the same target is not a new acquisition"
    );
    h.server
        .ecs
        .world_mut()
        .get_mut::<Position>(player)
        .unwrap()
        .0
        .x = 15.5;
    for _ in 0..5 {
        h.server.tick();
    }
    assert!(h.server.ecs.world().get::<AttackTarget>(mob).is_none());
    assert!(!h.flag("test:targeted"), "clearing is not an acquisition");
    h.server
        .ecs
        .world_mut()
        .get_mut::<Position>(player)
        .unwrap()
        .0
        .x = 10.5;
    for _ in 0..5 {
        h.server.tick();
    }
    assert!(h.flag("test:targeted"));
    assert_eq!(
        h.server
            .ecs
            .world()
            .get::<FollowRangeConfig>(mob)
            .unwrap()
            .value,
        8.0
    );
}

#[test]
fn lifecycle_trigger_filters_and_other_receivers_use_actual_counterpart() {
    let mut body = components();
    body["minecraft:on_hurt"] = json!({"event":"test:hurt","target":"other",
        "filters":{"test":"is_family","subject":"other","value":"player"}});
    let mut h = harness(body);
    assert_eq!(h.hit(1.0, None, false), 1.0);
    assert!(!h.flag("test:hurt"));
    let mob = h.mob;
    h.server
        .ecs
        .world_mut()
        .entity_mut(mob)
        .remove::<HurtTimer>();
    let trigger = crate::entity::mob::components::triggers::ActorTrigger {
        event: Arc::from("test:hurt"),
        target: crate::entity::mob::components::triggers::TriggerTarget::OtherActor,
        filter: None,
    };
    let other = spawn_mob(
        h.server.ecs.world_mut(),
        "minecraft:zombie",
        FEET + DVec3::X * 2.0,
        0.0,
        SpawnCause::Spawned,
    )
    .unwrap();
    crate::entity::mob::life::triggers::fire(h.server.ecs.world_mut(), mob, &trigger, Some(other));
    let properties = h.server.ecs.world().get::<ActorProperties>(other).unwrap();
    assert_eq!(
        properties
            .value(&properties.layout().slot("test:hurt").unwrap())
            .unwrap(),
        PropertyValue::Bool(true)
    );
    assert!(!h.flag("test:hurt"));
}

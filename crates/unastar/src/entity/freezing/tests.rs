use super::*;
use jolyne::valentine::{McpePacketData, actor_flags};
use tokio::sync::mpsc;

use crate::entity::components::{
    GameMode, HeldSlot, Hunger, MainInventory, PlayerInput, PlayerSession, Rotation, Velocity,
};
use crate::entity::definition::attributes::ActorAttributes;
use crate::entity::definition::components::MovementConfig;
use crate::entity::mob::{SpawnCause, spawn_mob};
use crate::server::game::{GameServer, ServerConfig, world_stack};
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkManager, ChunkPosition, ChunkViewers};

const FEET: DVec3 = DVec3::new(4.5, 64.0, 4.5);
const PLAYER_FEET: DVec3 = DVec3::new(10.5, 64.0, 4.5);

struct Harness {
    server: GameServer,
    mob: Entity,
    player: Entity,
    rx: mpsc::Receiver<McpePacket>,
}

/// A loaded stone floor with one real cow and one survival player, each
/// standing inside a separate powder-snow block.
fn harness() -> Harness {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let world = server.ecs.world_mut();
    world.insert_resource(crate::entity::EntityRandom::seeded(10));
    let (tx, rx) = mpsc::channel(1 << 16);
    let player = world
        .spawn((
            Player,
            Position(PLAYER_FEET + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT),
            Rotation::default(),
            GameMode::Survival,
            Health {
                current: 100.0,
                max: 100.0,
            },
            MainInventory::default(),
            ArmourInventory::default(),
            HeldSlot(0),
            Hunger::default(),
            PlayerInput::default(),
            crate::entity::components::Age::default(),
            RuntimeEntityId(10),
            PlayerSession::new(10, "test".into(), None, None, tx),
        ))
        .id();
    let mut column = Chunk::new(0, 0);
    for x in 0..16 {
        for z in 0..16 {
            column.set_block(x, 63, z, *blocks::STONE);
        }
    }
    let powder = world
        .registry()
        .default_state_id_by_name("minecraft:powder_snow")
        .unwrap();
    column.set_block(4, 64, 4, powder);
    column.set_block(10, 64, 4, powder);
    let mut viewers = ChunkViewers::default();
    viewers.insert(player);
    let chunk = world.spawn((ChunkData::new(column), viewers)).id();
    world
        .resource_mut::<ChunkManager>()
        .insert(ChunkPosition::new(0, 0), chunk);
    let mob = spawn_mob(world, "minecraft:cow", FEET, 0.0, SpawnCause::Spawned).unwrap();
    *world.get_mut::<Health>(mob).unwrap() = Health {
        current: 100.0,
        max: 100.0,
    };
    world.get_mut::<MovementConfig>(mob).unwrap().value = 0.25;
    Harness {
        server,
        mob,
        player,
        rx,
    }
}

impl Harness {
    /// Hold both actors stationary while running the real scheduled systems.
    fn step(&mut self) {
        let world = self.server.ecs.world_mut();
        world.get_mut::<Position>(self.mob).unwrap().0 = FEET;
        world.get_mut::<Position>(self.player).unwrap().0 =
            PLAYER_FEET + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT;
        world.get_mut::<Velocity>(self.mob).unwrap().0 = DVec3::ZERO;
        self.server.tick();
    }
    /// Run exactly this many scheduled ticks.
    fn steps(&mut self, n: u64) {
        for _ in 0..n {
            self.step();
        }
    }
    /// Run up to, but not including, the global damage tick.
    fn until(&mut self, now: u64) {
        while self.server.ecs.world().resource::<TickCounter>().current < now {
            self.step();
        }
    }
    /// Read the actor's current cold state.
    fn cold(&self, actor: Entity) -> FreezingState {
        self.server
            .ecs
            .world()
            .get::<FreezingState>(actor)
            .copied()
            .unwrap_or_default()
    }
    /// Read health after the real damage path has committed.
    fn health(&self, actor: Entity) -> f32 {
        self.server
            .ecs
            .world()
            .get::<Health>(actor)
            .unwrap()
            .current
    }
}

#[test]
fn cold_accumulation_damage_cadence_and_vulnerability_use_real_living_damage_paths() {
    let mut h = harness();
    let (mob, player) = (h.mob, h.player);
    h.server
        .ecs
        .world_mut()
        .entity_mut(mob)
        .insert(FreezingVulnerable);
    h.steps(139);
    assert!(!h.cold(mob).is_frozen());
    assert_eq!(h.health(mob), 100.0);
    assert_eq!(h.health(player), 100.0);
    h.steps(2);
    assert!(h.cold(mob).is_frozen());
    assert!(h.cold(player).is_frozen());
    h.until(159);
    assert_eq!(h.health(mob), 100.0);
    h.step();
    assert_eq!(h.health(mob), 95.0);
    assert_eq!(h.health(player), 99.0);
    h.until(199);
    assert_eq!(h.health(mob), 95.0);
    h.step();
    assert_eq!(h.health(mob), 90.0);
    assert_eq!(h.health(player), 98.0);
    assert_eq!(
        h.server
            .ecs
            .world()
            .get::<crate::entity::mob::state::LastHurt>(mob)
            .unwrap()
            .cause,
        ActorDamageCause::Freezing
    );
}

#[test]
fn cold_updates_movement_and_shaking_without_mutating_authored_speed() {
    let mut h = harness();
    let mob = h.mob;
    h.steps(141);
    let world = h.server.ecs.world();
    assert_eq!(world.get::<MovementConfig>(mob).unwrap().value, 0.25);
    let attributes = world.get::<ActorAttributes>(mob).unwrap();
    let movement = attributes.layout().resolve("minecraft:movement").unwrap();
    assert_eq!(attributes.get(&movement).unwrap().current, 0.20);
    assert!(
        world
            .get::<ActorFlags>(mob)
            .unwrap()
            .get(actor_flags::SHAKING)
            .unwrap()
    );
    assert_ne!(
        crate::server::game::item_use::player_flags(world, h.player, false)
            & (1i64 << actor_flags::SHAKING),
        0
    );
    h.server
        .ecs
        .world_mut()
        .entity_mut(mob)
        .insert(FreezingImmune);
    h.steps(71);
    assert_eq!(h.cold(mob).strength(), 0.0);
    let world = h.server.ecs.world();
    assert!(
        !world
            .get::<ActorFlags>(mob)
            .unwrap()
            .get(actor_flags::SHAKING)
            .unwrap()
    );
    assert_eq!(
        world
            .get::<ActorAttributes>(mob)
            .unwrap()
            .get(&movement)
            .unwrap()
            .current,
        0.25
    );
}

#[test]
fn equipped_leather_and_definition_immunity_protect_but_carried_leather_does_not() {
    let mut h = harness();
    let (mob, player) = (h.mob, h.player);
    h.server
        .ecs
        .world_mut()
        .entity_mut(mob)
        .insert(FreezingImmune);
    let leather = world_stack(h.server.ecs.world(), "minecraft:leather_boots", 1).unwrap();
    h.server
        .ecs
        .world_mut()
        .get_mut::<MainInventory>(player)
        .unwrap()
        .0
        .set_item(0, leather.clone())
        .unwrap();
    h.steps(141);
    assert_eq!(h.cold(mob).strength(), 0.0);
    assert!(h.cold(player).is_frozen());
    h.server
        .ecs
        .world_mut()
        .get_mut::<ArmourInventory>(player)
        .unwrap()
        .set_boots(leather)
        .unwrap();
    h.steps(71);
    assert_eq!(h.cold(player).strength(), 0.0);
    assert_eq!(
        h.health(player),
        100.0,
        "new protection suppresses the pending damage tick while thawing"
    );
}

#[test]
fn touching_faces_do_not_freeze_and_leaving_the_block_thaws_twice_as_fast() {
    let mut h = harness();
    let world = h.server.ecs.world();
    let powder = world
        .registry()
        .default_state_id_by_name("minecraft:powder_snow")
        .unwrap();
    assert!(!in_powder_snow(
        world,
        Aabb::from_feet(DVec3::new(3.7, 64.0, 4.5), 0.6, 1.8),
        powder
    ));
    assert!(in_powder_snow(
        world,
        Aabb::from_feet(DVec3::new(3.71, 64.0, 4.5), 0.6, 1.8),
        powder
    ));
    h.steps(50);
    let before = h.cold(h.mob).strength();
    let state = h.cold(h.mob).advance(false, false);
    assert!((before - state.strength() - THAW_INCREMENT).abs() < f32::EPSILON);
    let chunk = h
        .server
        .ecs
        .world()
        .resource::<ChunkManager>()
        .get_by_coords(0, 0)
        .unwrap();
    h.server
        .ecs
        .world_mut()
        .get_mut::<ChunkData>(chunk)
        .unwrap()
        .inner
        .set_block(4, 64, 4, *blocks::AIR);
    h.steps(26);
    assert_eq!(h.cold(h.mob).strength(), 0.0);
    assert_eq!(h.health(h.mob), 100.0);
}

#[test]
fn player_packets_include_cold_strength_movement_and_respawn_clears_them() {
    let mut h = harness();
    let player = h.player;
    h.step();
    let packets: Vec<_> = std::iter::from_fn(|| h.rx.try_recv().ok()).collect();
    let strength = h.cold(player).strength();
    assert!(packets.iter().any(|packet| matches!(&packet.data,McpePacketData::SetActorDataPacket(data)
        if data.actor_data.data.iter().any(|entry| entry.id == actor_data_i_ds::FREEZING_EFFECT_STRENGTH
            && matches!(&entry.payload, jolyne::valentine::types::DataItemEntryPayload::DataItemFloatPayload(value) if value.value == strength)))));
    assert!(packets.iter().any(|packet| matches!(&packet.data,McpePacketData::UpdateAttributesPacket(data)
        if data.attribute_list.iter().any(|attribute| &*attribute.name=="minecraft:movement" && attribute.current_value==h.cold(player).movement(0.1)))));
    h.server.ecs.world_mut().entity_mut(player).insert(Dead);
    crate::entity::player_life::respawn::respawn(h.server.ecs.world_mut(), player);
    assert_eq!(h.cold(player).strength(), 0.0);
    assert_eq!(
        crate::server::game::item_use::player_flags(h.server.ecs.world(), player, false)
            & (1i64 << actor_flags::SHAKING),
        0
    );
}

#[test]
fn cold_damage_obeys_creative_immunity() {
    let mut h = harness();
    let player = h.player;
    *h.server
        .ecs
        .world_mut()
        .get_mut::<GameMode>(player)
        .unwrap() = GameMode::Creative;
    h.steps(201);
    assert_eq!(h.health(player), 100.0);
}

#[test]
fn marker_groups_remove_runtime_conditions_and_thaw_preserves_authored_shaking() {
    use crate::entity::definition::{
        CompiledDefinition, GroupChange, apply_pending_definitions, attach_definition,
        queue_group_change,
    };
    use serde_json::json;
    use std::sync::Arc;
    use unastar_packs::actor::{Loaded, load_value};

    let document = json!({"format_version":"1.26.50","minecraft:entity":{
        "description":{"identifier":"test:cold","is_summonable":true},
        "components":{"minecraft:is_shaking":{}},
        "component_groups":{"cold":{
            "minecraft:freezing_immune":{},"minecraft:freezing_vulnerable":{}
        }}
    }});
    let Loaded::Definition { definition, .. } = load_value(
        "cold condition control",
        &document,
        unastar_data::actor_definitions::parser(),
    )
    .unwrap() else {
        panic!("control skipped")
    };
    let mut kinds = ComponentKindsBuilder::default();
    register_component_kinds(&mut kinds).unwrap();
    crate::entity::definition::flags::register_component_kinds(&mut kinds).unwrap();
    let definition = CompiledDefinition::compile(&definition, kinds.freeze()).unwrap();
    let group = definition.group("cold").unwrap();
    let mut world = World::new();
    let actor = world.spawn_empty().id();
    attach_definition(&mut world, actor, Arc::clone(&definition)).unwrap();
    queue_group_change(&mut world, actor, GroupChange::Add(group.clone())).unwrap();
    apply_pending_definitions(&mut world);
    assert!(world.get::<FreezingImmune>(actor).is_some());
    assert!(world.get::<FreezingVulnerable>(actor).is_some());
    queue_group_change(&mut world, actor, GroupChange::Remove(group)).unwrap();
    apply_pending_definitions(&mut world);
    assert!(world.get::<FreezingImmune>(actor).is_none());
    assert!(world.get::<FreezingVulnerable>(actor).is_none());
    publish(&mut world, actor, FreezingState::default());
    assert!(
        world
            .get::<ActorFlags>(actor)
            .unwrap()
            .get(actor_flags::SHAKING)
            .unwrap()
    );
}

#[test]
fn cold_movement_wire_modifier_matches_the_applied_value_and_clears_on_thaw() {
    let cold = FreezingState { strength: 1.0 };
    let attribute = crate::entity::player_life::attributes::movement_with_cold(cold);
    assert_eq!(attribute.current_value, 0.05);
    assert_eq!(attribute.default_value, 0.1);
    assert_eq!(attribute.modifiers.len(), 1);
    let modifier = &attribute.modifiers[0];
    assert_eq!(modifier.amount, -0.05);
    assert_eq!((modifier.operation, modifier.operand), (0, 2));
    assert!(modifier.is_serializable);
    let warm = crate::entity::player_life::attributes::movement_with_cold(FreezingState::default());
    assert_eq!(warm.current_value, 0.1);
    assert!(warm.modifiers.is_empty());
}

#[test]
fn damage_cadence_uses_actor_age_instead_of_world_uptime() {
    let mut h = harness();
    h.server
        .ecs
        .world_mut()
        .resource_mut::<TickCounter>()
        .current = 37;
    h.steps(159);
    assert_eq!(h.health(h.mob), 100.0);
    assert_eq!(h.health(h.player), 100.0);
    h.step();
    assert_eq!(h.health(h.mob), 99.0);
    assert_eq!(h.health(h.player), 99.0);
    assert_eq!(
        h.server
            .ecs
            .world()
            .get::<crate::entity::components::Age>(h.mob)
            .unwrap()
            .0,
        160
    );
}

#[test]
fn quarantine_withheld_position_and_health_cannot_advance_or_publish_cold() {
    let mut h = harness();
    let player = h.player;
    h.steps(30);
    let before = h.cold(player);
    let (position, health) = h
        .server
        .ecs
        .world_mut()
        .entity_mut(player)
        .take::<(Position, Health)>()
        .unwrap();
    while h.rx.try_recv().is_ok() {}
    for _ in 0..20 {
        h.server.tick();
        refresh(h.server.ecs.world_mut(), player);
    }
    assert_eq!(h.cold(player), before);
    assert!(h.server.ecs.world().get::<Position>(player).is_none());
    assert!(h.server.ecs.world().get::<Health>(player).is_none());
    let packets: Vec<_> = std::iter::from_fn(|| h.rx.try_recv().ok()).collect();
    assert!(!packets.iter().any(|packet| matches!(&packet.data,McpePacketData::SetActorDataPacket(data)
        if data.target_runtime_id.actor_runtime_id==10 && data.actor_data.data.iter().any(|entry|entry.id==actor_data_i_ds::FREEZING_EFFECT_STRENGTH))));
    h.server
        .ecs
        .world_mut()
        .entity_mut(player)
        .insert((position, health));
    h.step();
    assert!(h.cold(player).strength() > before.strength());
}

#[test]
fn persisted_cold_validation_rejects_corruption_and_older_records_default_warm() {
    use crate::storage::PlayerData;
    let uuid = uuid::Uuid::from_u128(10);
    let saved = PlayerData {
        uuid: uuid.to_string(),
        ..Default::default()
    };
    let mut old = serde_json::to_value(&saved).unwrap();
    old.as_object_mut().unwrap().remove("cold_exposure");
    old.as_object_mut().unwrap().remove("age_ticks");
    let old: PlayerData = serde_json::from_value(old).unwrap();
    old.validate(uuid).unwrap();
    assert_eq!(old.cold_exposure, 0.0);
    assert_eq!(old.age_ticks, 0);
    for invalid in [f32::NAN, f32::INFINITY, -0.01, 1.01] {
        let saved = PlayerData {
            cold_exposure: invalid,
            ..saved.clone()
        };
        assert!(saved.validate(uuid).is_err());
        assert!(FreezingState::from_strength(invalid).is_err());
    }
}

#[tokio::test]
async fn real_player_save_reopen_and_join_preserve_cold_and_actor_tick_phase() {
    use crate::entity::components::{Age, PlayerUuid};
    use crate::server::game::types::PlayerProviderResource;
    use crate::server::game::{PlayerSpawnData, SessionEntityMap};
    use crate::storage::{LevelDBPlayerProvider, PlayerProvider};
    use std::sync::Arc;
    let directory = tempfile::tempdir().unwrap();
    let uuid = uuid::Uuid::from_u128(10);
    let (saved_cold, saved_age) = {
        let mut h = harness();
        let player = h.player;
        h.server
            .ecs
            .world_mut()
            .entity_mut(player)
            .insert(PlayerUuid(uuid));
        h.server
            .ecs
            .world_mut()
            .resource_mut::<SessionEntityMap>()
            .insert(10, player);
        let provider = Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap());
        h.server
            .ecs
            .world_mut()
            .insert_resource(PlayerProviderResource(provider));
        h.steps(125);
        let cold = h.cold(player).strength();
        let age = h.server.ecs.world().get::<Age>(player).unwrap().0;
        assert!(h.server.save_player(10).await);
        (cold, age)
    };
    let provider = LevelDBPlayerProvider::open(directory.path()).unwrap();
    let saved = provider.load(uuid).await.unwrap().unwrap();
    assert_eq!(saved.cold_exposure, saved_cold);
    assert_eq!(saved.age_ticks, saved_age);
    let position = DVec3::from_array(saved.position);
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let (tx, mut rx) = mpsc::channel(1 << 16);
    let player = server
        .spawn_player(PlayerSpawnData {
            session_id: 10,
            display_name: "restored".into(),
            xuid: None,
            uuid: Some(uuid.to_string()),
            runtime_id: 10,
            position,
            outbound_tx: tx,
            saved_data: Some(saved),
            chunk_radius: 0,
        })
        .unwrap();
    assert_eq!(
        server
            .ecs
            .world()
            .get::<FreezingState>(player)
            .unwrap()
            .strength(),
        saved_cold
    );
    assert_eq!(server.ecs.world().get::<Age>(player).unwrap().0, saved_age);
    let packets: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert!(packets.iter().any(|packet| matches!(&packet.data,McpePacketData::UpdateAttributesPacket(data)
        if data.attribute_list.iter().any(|attribute| &*attribute.name=="minecraft:movement" && attribute.current_value == FreezingState::from_strength(saved_cold).unwrap().movement(0.1)))));
    assert!(packets.iter().any(|packet| matches!(&packet.data,McpePacketData::SetActorDataPacket(data)
        if data.actor_data.data.iter().any(|entry|entry.id==actor_data_i_ds::FREEZING_EFFECT_STRENGTH))));
}

#[test]
fn invalid_saved_cold_cannot_publish_or_partially_spawn_a_player() {
    use crate::server::game::PlayerSpawnData;
    use crate::storage::PlayerData;
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let uuid = uuid::Uuid::from_u128(10);
    let saved = PlayerData {
        uuid: uuid.to_string(),
        cold_exposure: 1.1,
        ..Default::default()
    };
    let entities = server.ecs.world().entities().len();
    let (tx, mut rx) = mpsc::channel(100);
    assert!(
        server
            .spawn_player(PlayerSpawnData {
                session_id: 10,
                display_name: "invalid".into(),
                xuid: None,
                uuid: Some(uuid.to_string()),
                runtime_id: 10,
                position: PLAYER_FEET,
                outbound_tx: tx,
                saved_data: Some(saved),
                chunk_radius: 0,
            })
            .is_err()
    );
    assert_eq!(server.ecs.world().entities().len(), entities);
    assert!(rx.try_recv().is_err());
}

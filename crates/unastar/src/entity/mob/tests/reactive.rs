//! Gaze and anger controls exercise real composition, AI, block sight,
//! equipment and actor persistence without depending on another mob goal.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::{McpePacket, McpePacketData, actor_data_i_ds as ids, actor_flags};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use unastar_packs::actor::{Loaded, load_value};
use zuri_nbt::{NBTTag, encoding::LittleEndian};

use crate::ecs::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::{
    ArmourInventory, GameMode, Health, HeldSlot, MainInventory, Player, PlayerSession, Position,
    Rotation, RuntimeEntityId,
};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::properties::{
    ActorProperties, ActorPropertyRegistry, NamedPropertyValue, PropertyValue,
};
use crate::entity::definition::{GroupChange, apply_pending_definitions, queue_group_change};
use crate::entity::mob::components::reactive::Angry;
use crate::entity::mob::persistence::{encode_for_tests, restore_mob};
use crate::entity::mob::state::AttackTarget;
use crate::entity::mob::{MobKind, MobRegistry, SpawnCause, spawn_mob};
use crate::registry::entity::ActorDefinitionRegistryBuilder;
use crate::server::game::types::ItemRegistryResource;
use crate::server::game::{GameServer, ServerConfig, world_stack};
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkManager, ChunkPosition, ChunkViewers};
use crate::world::rules::Difficulty;

const FEET: DVec3 = DVec3::new(4.5, 64.0, 4.5);
struct Harness {
    server: GameServer,
    actor: Entity,
    player: Entity,
    rx: mpsc::Receiver<McpePacket>,
}

/// Compile one independently authored component control through native factories.
fn harness(gaze: Value, anger: Value) -> Harness {
    let document = json!({
        "format_version":"1.26.50",
        "minecraft:entity":{
            "description":{"identifier":"minecraft:zombie","is_summonable":true,
                "properties":{
                    "test:seen":{"type":"bool","default":false},
                    "test:lost":{"type":"bool","default":false},
                    "test:targeted":{"type":"bool","default":false},
                    "test:calmed":{"type":"bool","default":false}}},
            "components":{
                "minecraft:health":{"value":100},
                "minecraft:collision_box":{"width":0.6,"height":1.8},
                "minecraft:physics":{},"minecraft:looked_at":gaze,
                "minecraft:on_target_acquired":{"event":"test:targeted","target":"self"}},
            "component_groups":{"test:anger":{"minecraft:angry":anger}},
            "events":{
                "test:targeted":{"set_property":{"test:targeted":true},"add":{"component_groups":["test:anger"]}},
                "test:calmed":{"set_property":{"test:calmed":true},"remove":{"component_groups":["test:anger"]}},
                "test:seen":{"set_property":{"test:seen":true}},
                "test:lost":{"set_property":{"test:lost":true}}}
        }
    });
    let Loaded::Definition { definition, .. } = load_value(
        "reactive control",
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
        ..Default::default()
    });
    let world = server.ecs.world_mut();
    world.insert_resource(EntityRandom::seeded(8));
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
    world.insert_resource(properties);
    world.insert_resource(mobs);
    *world.resource_mut::<Difficulty>() = Difficulty::Normal;
    let (tx, rx) = mpsc::channel(16384);
    let player = world
        .spawn((
            Player,
            Position(FEET + DVec3::new(0.0, crate::entity::PLAYER_EYE_HEIGHT, 6.0)),
            Rotation::default(),
            Health::default(),
            GameMode::Survival,
            ArmourInventory::default(),
            MainInventory::default(),
            HeldSlot(0),
            RuntimeEntityId(1),
            PlayerSession::new(1, "observer".into(), None, None, tx),
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
    let actor = spawn_mob(world, "minecraft:zombie", FEET, 0.0, SpawnCause::Spawned).unwrap();
    let mut harness = Harness {
        server,
        actor,
        player,
        rx,
    };
    harness.aim(true);
    harness
}

/// The timed player-gaze control includes an actual head equipment filter.
fn gaze() -> Value {
    json!({
        "find_players_only":true,"search_radius":16,"min_looked_at_duration":0.25,
        "set_target":"once_and_stop_scanning",
        "filters":{"test":"has_equipment","subject":"other","domain":"head","operator":"not","value":"carved_pumpkin"},
        "looked_at_event":{"event":"test:seen","target":"self"},
        "not_looked_at_event":{"event":"test:lost","target":"self"}
    })
}
/// Expiry fires an authored event which removes the angry group.
fn anger() -> Value {
    json!({"duration":1,"calm_event":{"event":"test:calmed","target":"self"}})
}

impl Harness {
    /// Advance normal server frames.
    fn ticks(&mut self, count: usize) {
        for _ in 0..count {
            self.server.tick();
        }
    }
    /// Read one committed authored observation.
    fn flag(&self, name: &str) -> bool {
        let properties = self
            .server
            .ecs
            .world()
            .get::<ActorProperties>(self.actor)
            .unwrap();
        properties
            .value(&properties.layout().slot(name).unwrap())
            .unwrap()
            == PropertyValue::Bool(true)
    }
    /// Reset an observation while leaving component/brain state intact.
    fn reset(&mut self, name: &str) {
        self.server
            .ecs
            .world_mut()
            .get_mut::<ActorProperties>(self.actor)
            .unwrap()
            .set_named(name, NamedPropertyValue::Bool(false))
            .unwrap();
    }
    /// Aim at the actual actor eye, or turn away without moving either actor.
    fn aim(&mut self, toward: bool) {
        let eye = self
            .server
            .ecs
            .world()
            .get::<Position>(self.player)
            .unwrap()
            .0;
        let head = FEET + DVec3::Y * 1.8 * crate::entity::mob::constants::MOB_EYE_HEIGHT_RATIO;
        let delta = (head - eye).normalize();
        let yaw = (delta.z.atan2(delta.x).to_degrees() - 90.0) as f32;
        let pitch = (-delta.y.asin().to_degrees()) as f32;
        *self
            .server
            .ecs
            .world_mut()
            .get_mut::<Rotation>(self.player)
            .unwrap() = Rotation::new(if toward { yaw } else { yaw + 180.0 }, pitch);
    }
    /// Whether a target has been committed, not just staged by a goal.
    fn target(&self) -> Option<Entity> {
        self.server
            .ecs
            .world()
            .get::<AttackTarget>(self.actor)
            .map(|target| target.0)
    }
    /// Change one real loaded cell without altering gaze state.
    fn block(&mut self, x: u8, y: i16, z: u8, state: u32) {
        let world = self.server.ecs.world_mut();
        let chunk = world
            .resource::<ChunkManager>()
            .get_by_coords(0, 0)
            .unwrap();
        world
            .get_mut::<ChunkData>(chunk)
            .unwrap()
            .inner
            .set_block(x, y, z, state);
    }
    /// Start the same group by its normal definition barrier for timer controls.
    fn start_anger(&mut self) {
        let world = self.server.ecs.world_mut();
        let group = world
            .get::<MobKind>(self.actor)
            .unwrap()
            .0
            .definition
            .group("test:anger")
            .unwrap();
        queue_group_change(world, self.actor, GroupChange::Add(group)).unwrap();
        apply_pending_definitions(world);
    }
}

#[test]
fn continuous_gaze_commits_one_target_and_group_event_after_minimum_duration() {
    let mut h = harness(gaze(), anger());
    h.ticks(5);
    assert_eq!(h.target(), None);
    assert!(!h.flag("test:seen"));
    h.ticks(1);
    assert_eq!(h.target(), Some(h.player));
    assert!(h.flag("test:targeted"));
    h.ticks(1);
    assert!(h.server.ecs.world().get::<Angry>(h.actor).is_some());
    assert!(
        h.server
            .ecs
            .world()
            .get::<ActorFlags>(h.actor)
            .unwrap()
            .get(actor_flags::ANGRY)
            .unwrap()
    );
    h.reset("test:seen");
    h.reset("test:targeted");
    h.ticks(8);
    assert!(!h.flag("test:seen"));
    assert!(!h.flag("test:targeted"));
    assert_eq!(
        h.target(),
        Some(h.player),
        "component rebuild retains the accepted target"
    );
}

#[test]
fn interruption_wall_head_equipment_and_invulnerable_modes_do_not_accumulate_gaze() {
    let mut h = harness(gaze(), anger());
    h.ticks(3);
    h.aim(false);
    h.ticks(1);
    h.aim(true);
    h.ticks(5);
    assert_eq!(h.target(), None);
    h.block(4, 65, 7, *blocks::STONE);
    h.ticks(8);
    assert_eq!(h.target(), None);
    h.block(4, 65, 7, *blocks::AIR);
    let stack = world_stack(h.server.ecs.world(), "minecraft:carved_pumpkin", 1).unwrap();
    h.server
        .ecs
        .world_mut()
        .get_mut::<ArmourInventory>(h.player)
        .unwrap()
        .set_helmet(stack)
        .unwrap();
    h.ticks(8);
    assert_eq!(h.target(), None);
    h.server
        .ecs
        .world_mut()
        .get_mut::<ArmourInventory>(h.player)
        .unwrap()
        .set_helmet(crate::item::ItemStack::empty())
        .unwrap();
    for mode in [GameMode::Creative, GameMode::Spectator] {
        *h.server
            .ecs
            .world_mut()
            .get_mut::<GameMode>(h.player)
            .unwrap() = mode;
        h.ticks(8);
        assert_eq!(h.target(), None);
    }
    *h.server
        .ecs
        .world_mut()
        .get_mut::<GameMode>(h.player)
        .unwrap() = GameMode::Survival;
    h.ticks(6);
    assert_eq!(h.target(), Some(h.player));
}

#[test]
fn anger_expiry_clears_target_replication_and_group_once() {
    let mut h = harness(gaze(), anger());
    h.ticks(7);
    h.aim(false);
    let until = h
        .server
        .ecs
        .world()
        .get::<Angry>(h.actor)
        .unwrap()
        .until_tick;
    while h.server.ecs.world().resource::<TickCounter>().current < until {
        h.ticks(1);
    }
    assert_eq!(h.target(), Some(h.player));
    h.ticks(1);
    assert_eq!(h.target(), None);
    assert!(h.flag("test:calmed"));
    assert!(h.server.ecs.world().get::<Angry>(h.actor).is_none());
    assert!(
        !h.server
            .ecs
            .world()
            .get::<ActorFlags>(h.actor)
            .unwrap()
            .get(actor_flags::ANGRY)
            .unwrap()
    );
    let mut saw_angry = false;
    let mut saw_calm = false;
    while let Ok(packet) = h.rx.try_recv() {
        if let McpePacketData::SetActorDataPacket(packet) = packet.data {
            for row in packet.actor_data.data {
                if row.id == ids::RESERVED_0
                    && let jolyne::valentine::types::DataItemEntryPayload::DataItemInt64Payload(
                        word,
                    ) = row.payload
                {
                    if word.value & (1i64 << actor_flags::ANGRY) != 0 {
                        saw_angry = true;
                    } else if saw_angry {
                        saw_calm = true;
                    }
                }
            }
        }
    }
    assert!(
        saw_angry && saw_calm,
        "viewers receive both state transitions"
    );
    h.reset("test:calmed");
    h.ticks(25);
    assert!(!h.flag("test:calmed"));
}

#[test]
fn detection_without_targeting_has_entry_exit_events_and_keeps_the_viewer_unattacked() {
    let mut config = gaze();
    config["set_target"] = json!("never");
    config["min_looked_at_duration"] = json!(0);
    let mut h = harness(config, anger());
    h.ticks(1);
    assert!(h.flag("test:seen"));
    assert_eq!(h.target(), None);
    h.reset("test:seen");
    h.ticks(5);
    assert!(!h.flag("test:seen"));
    h.aim(false);
    h.ticks(1);
    assert!(h.flag("test:lost"));
    h.aim(true);
    h.ticks(1);
    assert!(h.flag("test:seen"));
    assert_eq!(h.target(), None);
}

#[test]
fn saved_anger_keeps_remaining_duration_and_rejects_corrupt_runtime_fields() {
    let mut config = gaze();
    config["set_target"] = json!("never");
    let mut h = harness(config, anger());
    h.aim(false);
    h.start_anger();
    h.ticks(7);
    let now = h.server.ecs.world().resource::<TickCounter>().current;
    let remaining = h
        .server
        .ecs
        .world()
        .get::<Angry>(h.actor)
        .unwrap()
        .until_tick
        - now;
    let bytes = encode_for_tests(h.server.ecs.world_mut(), h.actor);
    h.server.ecs.world_mut().despawn(h.actor);
    h.ticks(100);
    let now = h.server.ecs.world().resource::<TickCounter>().current;
    let restored = restore_mob(h.server.ecs.world_mut(), &bytes).unwrap();
    h.actor = restored;
    assert_eq!(
        h.server
            .ecs
            .world()
            .get::<Angry>(restored)
            .unwrap()
            .until_tick,
        now + remaining
    );
    let mut input = bytes.as_slice();
    let NBTTag::Compound(mut root) = NBTTag::read(&mut input, LittleEndian).unwrap() else {
        panic!("root")
    };
    root.0
        .insert("UniqueID".into(), NBTTag::Long(999_991i64.into()));
    root.0
        .insert("UnastarAngerTicks".into(), NBTTag::Long((-1i64).into()));
    let mut bad = Vec::new();
    NBTTag::Compound(root)
        .write(&mut bad, LittleEndian)
        .unwrap();
    assert!(restore_mob(h.server.ecs.world_mut(), &bad).is_err());
}

#[test]
fn completed_anger_without_a_calm_event_does_not_restart_on_restore() {
    let mut config = gaze();
    config["set_target"] = json!("never");
    let mut h = harness(config, json!({"duration":0}));
    h.aim(false);
    h.start_anger();
    h.ticks(1);
    assert!(h.server.ecs.world().get::<Angry>(h.actor).unwrap().calmed);
    let bytes = encode_for_tests(h.server.ecs.world_mut(), h.actor);
    h.server.ecs.world_mut().despawn(h.actor);
    let actor = restore_mob(h.server.ecs.world_mut(), &bytes).unwrap();
    assert!(h.server.ecs.world().get::<Angry>(actor).unwrap().calmed);
    assert!(
        !h.server
            .ecs
            .world()
            .get::<ActorFlags>(actor)
            .unwrap()
            .get(actor_flags::ANGRY)
            .unwrap()
    );
}

#[test]
fn distance_scaling_narrows_the_actual_view_cone() {
    for (scaled, detected) in [(true, false), (false, true)] {
        let mut config = gaze();
        config["set_target"] = json!("never");
        config["min_looked_at_duration"] = json!(0);
        config["scale_fov_by_distance"] = json!(scaled);
        let mut h = harness(config, anger());
        h.server
            .ecs
            .world_mut()
            .get_mut::<Rotation>(h.player)
            .unwrap()
            .yaw += 5.0;
        h.ticks(1);
        assert_eq!(h.flag("test:seen"), detected);
    }
}

#[test]
fn angry_sound_uses_the_authored_interval_and_stops_when_the_state_expires() {
    let mut config = gaze();
    config["set_target"] = json!("never");
    let mut h = harness(
        config,
        json!({"duration":0.3,"angry_sound":"angry","sound_interval":[0.1,0.1]}),
    );
    h.aim(false);
    h.start_anger();
    h.ticks(10);
    let mut sounds = 0;
    while let Ok(packet) = h.rx.try_recv() {
        if matches!(packet.data,McpePacketData::LevelSoundEventPacket(sound) if sound.sound_event=="angry")
        {
            sounds += 1;
        }
    }
    assert_eq!(
        sounds, 3,
        "one sound per two ticks before the six-tick deadline"
    );
    h.ticks(10);
    assert!(!std::iter::from_fn(||h.rx.try_recv().ok()).any(|packet|
        matches!(packet.data,McpePacketData::LevelSoundEventPacket(sound) if sound.sound_event=="angry")));
}

#[test]
fn anger_exclusion_clears_a_committed_target_without_reacquisition_events() {
    let mut h = harness(
        gaze(),
        json!({"duration":10,"filters":{
        "test":"is_family","subject":"other","operator":"not","value":"player"}}),
    );
    h.ticks(6);
    assert_eq!(h.target(), Some(h.player));
    h.ticks(1);
    assert_eq!(
        h.target(),
        None,
        "new angry composition excludes this target"
    );
    h.reset("test:targeted");
    h.ticks(8);
    assert_eq!(h.target(), None);
    assert!(!h.flag("test:targeted"));
}

#[test]
fn a_due_but_unprocessed_saved_timer_still_dispatches_its_calm_event() {
    let mut config = gaze();
    config["set_target"] = json!("never");
    let mut h = harness(config, anger());
    h.aim(false);
    h.start_anger();
    let deadline = h
        .server
        .ecs
        .world()
        .get::<Angry>(h.actor)
        .unwrap()
        .until_tick;
    h.server
        .ecs
        .world_mut()
        .resource_mut::<TickCounter>()
        .current = deadline;
    let bytes = encode_for_tests(h.server.ecs.world_mut(), h.actor);
    h.server.ecs.world_mut().despawn(h.actor);
    h.actor = restore_mob(h.server.ecs.world_mut(), &bytes).unwrap();
    assert!(!h.server.ecs.world().get::<Angry>(h.actor).unwrap().calmed);
    h.ticks(1);
    assert!(h.flag("test:calmed"));
    assert!(h.server.ecs.world().get::<Angry>(h.actor).is_none());
}

#[test]
fn detection_cooldown_defers_new_checks_until_its_actual_deadline() {
    let mut config = gaze();
    config["set_target"] = json!("never");
    config["min_looked_at_duration"] = json!(0);
    config["looked_at_cooldown"] = json!([0.2, 0.2]);
    let mut h = harness(config, anger());
    h.ticks(1);
    assert!(h.flag("test:seen"));
    h.aim(false);
    h.ticks(3);
    assert!(!h.flag("test:lost"));
    h.ticks(1);
    assert!(h.flag("test:lost"));
    h.reset("test:seen");
    h.aim(true);
    h.ticks(1);
    assert!(h.flag("test:seen"));
}

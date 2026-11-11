//! Mooshroom interactions against BDS 1.26.51
//! (`testdata/bds-1.26.51-mooshroom.json`: `tools/bds-sweep/sweep.py
//! mooshroom --client-capture`, `--items basic` and `--items geometry`, distilled
//! by `mooshroom_fixture.py`).
//!
//! Each item is replayed on the same mooshroom (adult or baby, red or brown,
//! without its brain: BDS's was held by a hidden Slowness 255) where BDS
//! spawned it, used by a survival player with BDS's eyes and held item. For
//! every interaction BDS let through, the server must send the same
//! particle (id and position) before the player's `interact` swing and
//! before the held slot changes, leave the same item in the hand, play the
//! same sounds at the same place, end with the same mark variant, drop the
//! same items and turn the mooshroom into the same actor at its feet and
//! facing, all within the two ticks after the interaction. BDS sends the
//! sounds, the actor data, the drops and the new actor one or two ticks
//! after the particle (see `interact::authored_interaction`); their ticks
//! are not compared.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::actor_data_i_ds as ids;
use jolyne::valentine::actor_flags;
use jolyne::valentine::types::{
    ActorRuntimeId, EnumsItemUseOnActorInventoryTransactionActionType as ActionType,
    ItemUseOnActorInventoryTransaction,
};
use jolyne::valentine::{McpePacket, McpePacketData};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::entity::EntityRandom;
use crate::entity::components::{
    GameMode, HeldSlot, MainInventory, Player, PlayerSession, Position, Rotation, RuntimeEntityId,
    RuntimeId,
};
use crate::entity::definition::apply_pending_definitions;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::mob::ai::MobBrain;
use crate::entity::mob::components::variant::appearance_id;
use crate::entity::mob::{MobKind, SpawnCause, spawn_mob};
use crate::item::ItemStack;
use crate::server::game::{GameServer, ServerConfig, world_stack};
use crate::world::ChunkManager;
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkPosition, ChunkViewers};
use crate::world::rules::{Difficulty, GameRules};

fn fixture() -> &'static Value {
    static FIXTURE: OnceLock<Value> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        serde_json::from_str(include_str!("testdata/bds-1.26.51-mooshroom.json")).unwrap()
    })
}

fn vec3(value: &Value) -> DVec3 {
    DVec3::new(
        value[0].as_f64().unwrap(),
        value[1].as_f64().unwrap(),
        value[2].as_f64().unwrap(),
    )
}

/// A grass floor at y = -61 over the chunks around the origin, seen by one
/// survival player.
fn server() -> (GameServer, Entity, mpsc::Receiver<McpePacket>) {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let world = server.ecs.world_mut();
    world.insert_resource(EntityRandom::seeded(17));
    *world.resource_mut::<Difficulty>() = Difficulty::Easy;
    world.resource_mut::<GameRules>().do_mob_spawning = false;
    let (tx, rx) = mpsc::channel::<McpePacket>(1 << 14);
    let player = world
        .spawn((
            Player,
            Position(DVec3::new(
                0.5,
                -60.0 + crate::entity::PLAYER_EYE_HEIGHT,
                0.5,
            )),
            Rotation::default(),
            GameMode::Survival,
            MainInventory::default(),
            HeldSlot(0),
            RuntimeEntityId(1),
            PlayerSession::new(1, "sweep".into(), None, None, tx),
        ))
        .id();
    for cx in -1..=1 {
        for cz in -1..=1 {
            let mut column = Chunk::new(cx, cz);
            for x in 0..16 {
                for z in 0..16 {
                    column.set_block(x, -61, z, *blocks::GRASS_BLOCK);
                }
            }
            let mut viewers = ChunkViewers::default();
            viewers.insert(player);
            let chunk = world.spawn((ChunkData::new(column), viewers)).id();
            world
                .resource_mut::<ChunkManager>()
                .insert(ChunkPosition::new(cx, cz), chunk);
        }
    }
    (server, player, rx)
}

/// An adult mooshroom (spawning picks a baby 1 time in 20) at `feet`,
/// without its brain.
fn mooshroom(world: &mut World, feet: DVec3, yaw: f32) -> Entity {
    let mob = loop {
        let mob = spawn_mob(world, "minecraft:mooshroom", feet, yaw, SpawnCause::Spawned).unwrap();
        apply_pending_definitions(world);
        if !flag(world, mob, actor_flags::BABY) {
            break mob;
        }
        world.despawn(mob);
    };
    world.entity_mut(mob).remove::<MobBrain>();
    mob
}

fn flag(world: &World, mob: Entity, index: u32) -> bool {
    world
        .get::<ActorFlags>(mob)
        .is_some_and(|flags| flags.get(index).unwrap())
}

fn fire(world: &mut World, mob: Entity, event: &str) {
    let handle = world
        .get::<MobKind>(mob)
        .and_then(|kind| kind.0.events.event(event))
        .unwrap_or_else(|| panic!("mooshroom event {event}"));
    crate::entity::mob::events::fire_event(world, mob, &handle, None).unwrap();
    apply_pending_definitions(world);
}

fn hold(world: &mut World, player: Entity, held: &Value) {
    let stack = world_stack(
        world,
        held["item"].as_str().unwrap(),
        held["count"].as_u64().unwrap() as u8,
    )
    .expect("vanilla item");
    world
        .get_mut::<MainInventory>(player)
        .unwrap()
        .0
        .set_item(0, stack)
        .unwrap();
}

/// What one interaction did, as BDS or this server shows it.
#[derive(Debug, Default)]
struct Outcome {
    /// Particle ids and positions, in order.
    particles: Vec<(i32, DVec3)>,
    /// The `interact` swing came after every particle and before the held
    /// slot changed.
    swing_in_order: bool,
    held_after: Option<(String, i16, u8)>,
    /// Sound name, position, baby flag.
    sounds: Vec<(String, DVec3, bool)>,
    mark_variant: Option<i32>,
    /// Dropped item → count.
    drops: BTreeMap<String, u32>,
    /// The new actor's identifier, feet and yaw.
    transformed: Option<(String, DVec3, f32)>,
}

fn expected(interaction: &Value) -> Outcome {
    let held_after = &interaction["held_after"];
    let mut drops = BTreeMap::new();
    for drop in interaction["drops"].as_array().unwrap() {
        *drops
            .entry(drop["item"].as_str().unwrap().to_owned())
            .or_default() += drop["count"].as_u64().unwrap() as u32;
    }
    Outcome {
        particles: interaction["particles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| (p["data"].as_i64().unwrap() as i32, vec3(&p["position"])))
            .collect(),
        swing_in_order: interaction["swing"] == true,
        held_after: held_after.is_object().then(|| {
            (
                held_after["item"]
                    .as_str()
                    .unwrap_or("minecraft:air")
                    .to_owned(),
                held_after["aux"].as_i64().unwrap() as i16,
                held_after["count"].as_u64().unwrap() as u8,
            )
        }),
        sounds: interaction["sounds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                (
                    s["sound"].as_str().unwrap().to_owned(),
                    vec3(&s["position"]),
                    s["baby"] == true,
                )
            })
            .collect(),
        mark_variant: interaction["mark_variant"]["value"]
            .as_i64()
            .map(|value| value as i32),
        drops,
        transformed: interaction["transformed"].is_object().then(|| {
            let t = &interaction["transformed"];
            (
                t["actor"].as_str().unwrap().to_owned(),
                vec3(&t["position"]),
                t["yaw"].as_f64().unwrap() as f32,
            )
        }),
    }
}

/// Use the held item on `mob` (the player at `eyes`), run two ticks and
/// report what the player saw and what changed.
fn interact(
    server: &mut GameServer,
    rx: &mut mpsc::Receiver<McpePacket>,
    player: Entity,
    mob: Entity,
    eyes: DVec3,
    mark_before: Option<i32>,
) -> Outcome {
    let world = server.ecs.world_mut();
    world.get_mut::<Position>(player).unwrap().0 = eyes;
    let runtime_id = world.get::<RuntimeId>(mob).unwrap().0;
    let feet = world.get::<Position>(mob).unwrap().0;
    let items_before = item_counts(world);
    while rx.try_recv().is_ok() {}
    let transaction = ItemUseOnActorInventoryTransaction {
        runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id as u64,
        },
        action_type: ActionType::Interact,
        slot: 0,
        ..Default::default()
    };
    super::interact::player_interact(world, player, mob, &transaction);
    let mut packets = Vec::new();
    while let Ok(packet) = rx.try_recv() {
        packets.push(packet.data);
    }
    for _ in 0..2 {
        server.tick();
        while let Ok(packet) = rx.try_recv() {
            packets.push(packet.data);
        }
    }
    let mut outcome = Outcome::default();
    let mut swing = None;
    let mut slot_change = None;
    for (index, packet) in packets.iter().enumerate() {
        match packet {
            McpePacketData::LevelEventPacket(event) if event.event_id == 2009 => {
                let p = &event.position;
                outcome.particles.push((
                    event.data,
                    DVec3::new(f64::from(p.x), f64::from(p.y), f64::from(p.z)),
                ));
            }
            McpePacketData::AnimatePacket(animate)
                if animate.swing_source.as_deref() == Some("interact")
                    && animate.target_actor_runtime_id.actor_runtime_id == 1 =>
            {
                swing.get_or_insert(index);
            }
            McpePacketData::InventorySlotPacket(_) | McpePacketData::InventoryContentPacket(_) => {
                slot_change.get_or_insert(index);
            }
            McpePacketData::LevelSoundEventPacket(sound)
                if sound.actor_unique_id == runtime_id && sound.sound_event != "ambient" =>
            {
                let p = &sound.position;
                outcome.sounds.push((
                    sound.sound_event.to_string(),
                    DVec3::new(f64::from(p.x), f64::from(p.y), f64::from(p.z)),
                    sound.is_baby,
                ));
            }
            _ => {}
        }
    }
    let last_particle = packets.iter().rposition(|packet| {
        matches!(packet, McpePacketData::LevelEventPacket(event) if event.event_id == 2009)
    });
    outcome.swing_in_order = swing.is_some_and(|swing| {
        last_particle.is_none_or(|particle| particle < swing)
            && slot_change.is_none_or(|slot| swing < slot)
    });
    let world = server.ecs.world_mut();
    if slot_change.is_some() {
        let held = world
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(0)
            .cloned()
            .unwrap_or_else(ItemStack::empty);
        outcome.held_after = Some(if held.is_empty() {
            ("minecraft:air".to_owned(), 0, 0)
        } else {
            (held.item_id().to_owned(), held.damage, held.count)
        });
    }
    if world.get_entity(mob).is_ok() {
        let mark = world
            .get::<EntityMetadata>(mob)
            .and_then(|metadata| appearance_id(metadata, ids::MARK_VARIANT));
        outcome.mark_variant = mark.filter(|mark| Some(*mark) != mark_before);
    } else {
        let mut others = world.query::<(&MobKind, &Position, &Rotation)>();
        outcome.transformed = others
            .iter(world)
            .find(|(_, position, _)| position.0.distance(feet) < 0.5)
            .map(|(kind, position, rotation)| {
                (kind.0.identifier.to_string(), position.0, rotation.yaw)
            });
    }
    let items_after = item_counts(world);
    for (item, count) in items_after {
        let before = items_before.get(&item).copied().unwrap_or(0);
        if count > before {
            outcome.drops.insert(item, count - before);
        }
    }
    outcome
}

/// Item entities in the world: item → total count.
fn item_counts(world: &mut World) -> BTreeMap<String, u32> {
    let mut counts = BTreeMap::new();
    let mut items = world.query::<&crate::entity::components::ItemEntityStack>();
    for item in items.iter(world) {
        *counts.entry(item.0.item_id().to_owned()).or_default() += u32::from(item.0.count);
    }
    counts
}

fn close(a: DVec3, b: DVec3) -> bool {
    a.distance(b) < 2e-4
}

#[test]
fn mooshroom_interactions_match_bds() {
    let items = fixture()["items"].as_array().unwrap();
    assert_eq!(items.len(), 6);
    let mut checked = 0;
    for item in items {
        let key = item["key"].as_str().unwrap();
        let (mut server, player, mut rx) = server();
        let at = &item["at"];
        let feet = vec3(at);
        let world = server.ecs.world_mut();
        let mob = mooshroom(world, feet, at[3].as_f64().unwrap() as f32);
        for event in item["events"].as_array().unwrap() {
            fire(world, mob, event.as_str().unwrap());
        }
        server.tick();
        for interaction in item["interactions"].as_array().unwrap() {
            let want = expected(interaction);
            let world = server.ecs.world_mut();
            hold(world, player, &interaction["held"]);
            world.get_mut::<Position>(mob).unwrap().0 = feet;
            let mark_before = world
                .get::<EntityMetadata>(mob)
                .and_then(|metadata| appearance_id(metadata, ids::MARK_VARIANT));
            let got = interact(
                &mut server,
                &mut rx,
                player,
                mob,
                vec3(&interaction["eyes"]),
                mark_before,
            );
            assert_eq!(
                got.particles.len(),
                want.particles.len(),
                "{key}: particles {:?} vs BDS {:?}",
                got.particles,
                want.particles
            );
            for ((id, at), (want_id, want_at)) in got.particles.iter().zip(&want.particles) {
                assert_eq!(id, want_id, "{key}: particle id");
                assert!(
                    close(*at, *want_at),
                    "{key}: particle at {at} vs BDS {want_at}"
                );
            }
            assert!(
                got.swing_in_order,
                "{key}: swing after the particle, before the slot"
            );
            assert_eq!(
                got.held_after, want.held_after,
                "{key}: held item afterwards"
            );
            assert_eq!(
                got.sounds.len(),
                want.sounds.len(),
                "{key}: {:?}",
                got.sounds
            );
            for ((sound, at, baby), (want_sound, want_at, want_baby)) in
                got.sounds.iter().zip(&want.sounds)
            {
                assert_eq!((sound, baby), (want_sound, want_baby), "{key}: sound");
                assert!(
                    close(*at, *want_at),
                    "{key}: {sound} at {at} vs BDS {want_at}"
                );
            }
            assert_eq!(got.mark_variant, want.mark_variant, "{key}: mark variant");
            assert_eq!(got.drops, want.drops, "{key}: drops");
            match (&got.transformed, &want.transformed) {
                (Some((actor, at, yaw)), Some((want_actor, want_at, want_yaw))) => {
                    assert_eq!(actor, want_actor, "{key}: transformed into");
                    assert!(close(*at, *want_at), "{key}: new actor at {at}");
                    assert_eq!(yaw, want_yaw, "{key}: new actor's facing");
                }
                (got, want) => assert_eq!(got.is_some(), want.is_some(), "{key}: transformed"),
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 7, "every BDS interaction replayed");
}

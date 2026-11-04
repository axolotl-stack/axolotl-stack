//! The vanilla creeper and the explosion system through a real
//! `GameServer`: native AddActor equality, fuse timing and its stop, block
//! destruction by blast resistance and `mobGriefing`, damage and knockback
//! against the BDS 1.26.51 capture, flint-and-steel ignition, the charged
//! creeper, `/summon` spawn events, loot and experience.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::actor_data_i_ds as ids;
use jolyne::valentine::actor_flags;
use jolyne::valentine::types::{
    ActorRuntimeId, EnumsActorEvent, EnumsAnimatePacketPayloadAction,
    EnumsItemUseOnActorInventoryTransactionActionType as ActionType,
};
use jolyne::valentine::{
    AddActorPacket, ItemUseOnActorInventoryTransaction, McpePacket, McpePacketData,
};
use tokio::sync::mpsc;
use unastar_data::actor_identifiers::ActorType;
use valentine::bedrock::codec::BedrockCodec;
use valentine::bedrock::version::v1_26_51::types::DataItemEntryPayload;

use crate::entity::EntityRandom;
use crate::entity::components::{
    DroppedItem, Experience, GameMode, Health, HeldSlot, ItemEntityStack, MainInventory, Player,
    PlayerSession, Position, Rotation, RuntimeEntityId, RuntimeId,
};
use crate::entity::definition::apply_pending_definitions;
use crate::entity::definition::attributes::ActorAttributes;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::definition::properties::ActorProperties;
use crate::entity::mob::components::explode::{Explode, Fuse};
use crate::entity::mob::net::network::add_actor_packet;
use crate::entity::mob::state::Dead;
use crate::entity::mob::*;
use crate::entity::xp_orb::XpOrb;
use crate::server::game::{GameServer, ServerConfig, world_stack};
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkManager, ChunkPosition, ChunkViewers};
use crate::world::explosion::{Explosion, explode};
use crate::world::rules::{Difficulty, GameRules};

/// Noon and midnight on the world clock.
const NOON: u64 = 6_000;
const MIDNIGHT: u64 = 18_000;

struct Harness {
    server: GameServer,
    player: Entity,
    rx: mpsc::Receiver<McpePacket>,
}

/// A grass floor at y = 63 over chunks -1..=1 × -1..=1 at `time`, viewed by
/// one player in `mode` standing on it at (8.5, 64, 0.5).
fn harness(seed: u64, difficulty: Difficulty, time: u64, mode: GameMode) -> Harness {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let world = server.ecs.world_mut();
    world.insert_resource(EntityRandom::seeded(seed));
    *world.resource_mut::<Difficulty>() = difficulty;
    crate::world::clock::set_time(world, time);
    let (tx, rx) = mpsc::channel(1 << 16);
    let player = world
        .spawn((
            Player,
            Position(DVec3::new(
                8.5,
                64.0 + crate::entity::PLAYER_EYE_HEIGHT,
                0.5,
            )),
            Rotation::default(),
            mode,
            MainInventory::default(),
            HeldSlot(0),
            Health::default(),
            Experience::default(),
            RuntimeEntityId(1),
            PlayerSession::new(1, "steve".into(), None, None, tx),
        ))
        .id();
    for cx in -1..=1 {
        for cz in -1..=1 {
            let mut column = Chunk::new(cx, cz);
            for x in 0..16 {
                for z in 0..16 {
                    column.set_block(x, 63, z, *blocks::GRASS_BLOCK);
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
    Harness { server, player, rx }
}

fn native(bytes: &[u8]) -> AddActorPacket {
    assert_eq!(bytes[0], 0x0d);
    let mut input = &bytes[1..];
    let packet = AddActorPacket::decode(&mut input, ()).unwrap();
    assert!(input.is_empty());
    packet
}

fn flag(world: &World, mob: Entity, index: u32) -> bool {
    world.get::<ActorFlags>(mob).unwrap().get(index).unwrap()
}

fn fuse_time(world: &World, mob: Entity) -> i32 {
    match world
        .get::<EntityMetadata>(mob)
        .unwrap()
        .get(ids::FUSE_TIME)
    {
        Some(DataItemEntryPayload::DataItemIntPayload(payload)) => payload.value,
        other => panic!("FUSE_TIME is not an int: {other:?}"),
    }
}

impl Harness {
    fn world(&mut self) -> &mut World {
        self.server.ecs.world_mut()
    }

    fn creeper(&mut self, feet: DVec3) -> Entity {
        let world = self.world();
        let mob = spawn_mob(world, "minecraft:creeper", feet, 0.0, SpawnCause::Spawned).unwrap();
        apply_pending_definitions(world);
        mob
    }

    /// Fire `event` on `mob`; its group changes apply at the next tick.
    fn fire(&mut self, mob: Entity, event: &str) {
        let world = self.world();
        let handle = world
            .get::<MobKind>(mob)
            .unwrap()
            .0
            .events
            .event(event)
            .unwrap();
        events::fire_event(world, mob, &handle, None).unwrap();
    }

    fn block(&mut self, pos: IVec3) -> u32 {
        use crate::entity::systems::BlockSource;
        self.world().block_at(pos).unwrap()
    }

    fn set_block(&mut self, pos: IVec3, name: &str) {
        let state = state(name);
        let world = self.world();
        let (cx, cz) = crate::world::ecs::world_to_chunk_coords(pos.x, pos.z);
        let chunk = world
            .resource::<ChunkManager>()
            .get_by_coords(cx, cz)
            .unwrap();
        let (x, y, z) = crate::world::ecs::world_to_local_coords(pos.x, pos.y, pos.z);
        world
            .get_mut::<ChunkData>(chunk)
            .unwrap()
            .inner
            .set_block(x, y, z, state);
    }

    fn place_player(&mut self, feet: DVec3) {
        let player = self.player;
        self.world().get_mut::<Position>(player).unwrap().0 =
            feet + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT;
    }

    fn ticks(&mut self, n: u64) {
        for _ in 0..n {
            self.server.tick();
        }
    }

    fn health(&mut self) -> f32 {
        let player = self.player;
        self.world().get::<Health>(player).unwrap().current
    }

    fn packets(&mut self) -> Vec<McpePacketData> {
        std::iter::from_fn(|| self.rx.try_recv().ok().map(|p| p.data)).collect()
    }

    fn exists(&mut self, mob: Entity) -> bool {
        self.world().get_entity(mob).is_ok()
    }

    /// Light `creeper`'s forced fuse and tick until it explodes; the
    /// packets of the explosion tick, with the block updates and sound the
    /// next tick starts with.
    fn explode_forced(&mut self, creeper: Entity) -> Vec<McpePacketData> {
        self.fire(creeper, "minecraft:start_exploding_forced");
        self.ticks(30);
        assert!(self.exists(creeper), "the fuse burns 30 ticks");
        self.packets();
        self.server.tick();
        assert!(!self.exists(creeper), "the creeper exploded");
        crate::server::game::blocks::send_block_updates(self.world());
        self.packets()
    }

    /// Every dropped item stack and the total experience of every orb,
    /// removing both from the world.
    fn take_drops(&mut self) -> (Vec<(String, u32)>, u32) {
        let world = self.world();
        let drops: Vec<(Entity, String, u32)> = world
            .query_filtered::<(Entity, &ItemEntityStack), With<DroppedItem>>()
            .iter(world)
            .map(|(entity, stack)| {
                (
                    entity,
                    stack.0.item_id().to_owned(),
                    u32::from(stack.0.count),
                )
            })
            .collect();
        let orbs: Vec<(Entity, u32)> = world
            .query::<(Entity, &XpOrb)>()
            .iter(world)
            .map(|(entity, orb)| (entity, orb.value))
            .collect();
        for entity in drops.iter().map(|d| d.0).chain(orbs.iter().map(|o| o.0)) {
            world.despawn(entity);
        }
        (
            drops.into_iter().map(|(_, id, n)| (id, n)).collect(),
            orbs.iter().map(|o| o.1).sum(),
        )
    }
}

fn state(name: &str) -> u32 {
    crate::registry::BlockRegistry::vanilla()
        .default_state_id_by_name(name)
        .unwrap()
}

#[test]
fn vanilla_creeper_compiles_to_a_spawnable_archetype() {
    let mut h = harness(1, Difficulty::Normal, MIDNIGHT, GameMode::Survival);
    let registry = h.world().resource::<MobRegistry>();
    assert!(
        registry.get("minecraft:creeper").is_some(),
        "creeper unsupported: {:?}",
        registry.unsupported_reason("minecraft:creeper")
    );
}

#[test]
fn spawned_creeper_add_actor_matches_native_summon() {
    let mut h = harness(2, Difficulty::Normal, MIDNIGHT, GameMode::Survival);
    let native = native(include_bytes!(
        "../../../../../bds-extractor/captures/1.26.51/operator/summons/creeper.bin"
    ));
    let feet = DVec3::new(
        f64::from(native.position.x),
        f64::from(native.position.y),
        f64::from(native.position.z),
    );
    let creeper = h.creeper(feet);
    let world = h.world();
    assert_eq!(fuse_time(world, creeper), -1, "BDS shows FUSE_TIME -1");
    let ours = add_actor_packet(
        "minecraft:creeper",
        world.get::<RuntimeId>(creeper).unwrap().0,
        world.get::<Position>(creeper).unwrap().0,
        DVec3::new(0.0, f64::from(native.velocity.y), 0.0),
        world.get::<Rotation>(creeper).unwrap(),
        world.get::<EntityMetadata>(creeper).unwrap(),
        world.get::<ActorProperties>(creeper).unwrap(),
        world.get::<ActorAttributes>(creeper).unwrap(),
    );
    assert_eq!(ours.actor_type, native.actor_type);
    assert_eq!(ours.attributes_list, native.attributes_list);
    let missing: Vec<_> = native
        .actor_data
        .data
        .iter()
        .filter(|entry| !ours.actor_data.data.contains(entry))
        .collect();
    let extra: Vec<_> = ours
        .actor_data
        .data
        .iter()
        .filter(|entry| !native.actor_data.data.contains(entry))
        .collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "native only: {missing:?}\nours only: {extra:?}"
    );
    assert_eq!(ours.actor_data, native.actor_data);
    assert_eq!(ours.actor_links, native.actor_links);
    assert_eq!(ours.synched_properties, native.synched_properties);
}

#[test]
fn a_lit_fuse_counts_down_thirty_ticks_then_the_creeper_explodes_without_loot() {
    let mut h = harness(3, Difficulty::Normal, NOON, GameMode::Creative);
    h.place_player(DVec3::new(30.5, 64.0, 0.5));
    let creeper = h.creeper(DVec3::new(0.5, 64.0, 0.5));
    h.fire(creeper, "minecraft:start_exploding_forced");
    for k in 1..=30 {
        h.server.tick();
        let world = h.world();
        assert!(flag(world, creeper, actor_flags::IGNITED), "tick {k}");
        assert_eq!(fuse_time(world, creeper), 30 - k, "tick {k}");
    }
    h.packets();
    h.server.tick();
    assert!(!h.exists(creeper), "the creeper exploded the tick after 0");
    let packets = h.packets();
    assert!(packets.iter().any(|p| matches!(p,
        McpePacketData::LevelEventPacket(event) if event.event_id == 2025 && event.data == 3)));
    h.ticks(2);
    let (drops, experience) = h.take_drops();
    assert!(
        drops.iter().all(|(id, _)| id != "minecraft:gunpowder"),
        "an exploding creeper drops no loot: {drops:?}"
    );
    assert_eq!(experience, 0, "and no experience");
}

#[test]
fn the_fuse_hisses_one_tick_after_it_lights_at_the_head() {
    let mut h = harness(4, Difficulty::Normal, NOON, GameMode::Creative);
    let creeper = h.creeper(DVec3::new(4.5, 64.0, 0.5));
    h.fire(creeper, "minecraft:start_exploding_forced");
    h.server.tick();
    let lit = h.packets();
    assert!(!lit.iter().any(
        |p| matches!(p, McpePacketData::LevelSoundEventPacket(s) if &*s.sound_event == "fuse")
    ));
    h.server.tick();
    let hiss: Vec<_> = h
        .packets()
        .into_iter()
        .filter_map(|p| match p {
            McpePacketData::LevelSoundEventPacket(sound) if &*sound.sound_event == "fuse" => {
                Some(sound)
            }
            _ => None,
        })
        .collect();
    assert_eq!(hiss.len(), 1);
    assert!((hiss[0].position.y - (64.0 + 1.8 * 0.85)).abs() < 1e-4);
    assert_eq!(&*hiss[0].actor_identifier, "minecraft:creeper");
}

#[test]
fn the_fuse_stops_when_the_player_leaves_the_outside_range() {
    let mut h = harness(5, Difficulty::Normal, MIDNIGHT, GameMode::Survival);
    let creeper = h.creeper(DVec3::new(6.5, 64.0, 0.5));
    let lit = (0..20).find(|_| {
        h.server.tick();
        flag(h.world(), creeper, actor_flags::IGNITED)
    });
    assert!(
        lit.is_some(),
        "a creeper next to its target lights its fuse"
    );
    // Beyond the 6-block outside range (box gap) the fuse goes out.
    h.place_player(DVec3::new(22.5, 64.0, 0.5));
    let out = (0..5).find(|_| {
        h.server.tick();
        !flag(h.world(), creeper, actor_flags::IGNITED)
    });
    assert!(out.is_some(), "the fuse went out");
    let world = h.world();
    assert!(world.get::<Explode>(creeper).is_none());
    assert!(world.get::<Fuse>(creeper).is_none());
    assert!(world.get::<Dead>(creeper).is_none());
}

#[test]
fn an_explosion_breaks_weak_blocks_but_not_obsidian() {
    let mut h = harness(6, Difficulty::Normal, NOON, GameMode::Creative);
    h.place_player(DVec3::new(30.5, 64.0, 0.5));
    h.set_block(IVec3::new(-1, 64, 0), "minecraft:dirt");
    h.set_block(IVec3::new(1, 64, 0), "minecraft:obsidian");
    let creeper = h.creeper(DVec3::new(0.5, 64.0, 0.5));
    let packets = h.explode_forced(creeper);
    let air = state("minecraft:air");
    assert_eq!(h.block(IVec3::new(-1, 64, 0)), air, "dirt is destroyed");
    assert_eq!(h.block(IVec3::new(0, 63, 0)), air, "the floor below too");
    assert_eq!(
        h.block(IVec3::new(1, 64, 0)),
        state("minecraft:obsidian"),
        "obsidian resists"
    );
    let updates = packets
        .iter()
        .filter(|p| {
            matches!(p, McpePacketData::UpdateBlockPacket(update)
            if update.block_position.x == -1 && update.block_position.y == 64)
        })
        .count();
    assert_eq!(updates, 1, "viewers get the dirt's update once");
}

#[test]
fn with_mob_griefing_off_an_explosion_breaks_nothing_but_still_hurts() {
    let mut h = harness(7, Difficulty::Normal, NOON, GameMode::Survival);
    h.world().resource_mut::<GameRules>().mob_griefing = false;
    h.set_block(IVec3::new(-1, 64, 0), "minecraft:dirt");
    let creeper = h.creeper(DVec3::new(0.5, 64.0, 0.5));
    h.place_player(DVec3::new(3.5, 64.0, 0.5));
    // Creative while the fuse burns keeps the creeper from walking to it.
    let player = h.player;
    *h.world().get_mut::<GameMode>(player).unwrap() = GameMode::Creative;
    h.fire(creeper, "minecraft:start_exploding_forced");
    h.ticks(30);
    *h.world().get_mut::<GameMode>(player).unwrap() = GameMode::Survival;
    h.server.tick();
    assert!(!h.exists(creeper));
    assert_eq!(h.block(IVec3::new(-1, 64, 0)), state("minecraft:dirt"));
    assert_eq!(h.block(IVec3::new(0, 63, 0)), *blocks::GRASS_BLOCK);
    assert!(h.health() < 20.0, "the blast still hurts");
    assert!(
        !h.packets()
            .iter()
            .any(|p| matches!(p, McpePacketData::UpdateBlockPacket(_)))
    );
}

/// The player's health after `creeper`'s explosion (power 3, at its feet)
/// with the player's feet `offset` from the creeper's, and the packets.
fn blast(offset: DVec3, difficulty: Difficulty) -> (f32, Vec<McpePacketData>) {
    let mut h = harness(8, difficulty, NOON, GameMode::Survival);
    let feet = DVec3::new(8.5, 64.0, 0.5);
    h.place_player(feet + offset);
    let creeper = h.creeper(feet);
    h.packets();
    let world = h.world();
    explode(
        world,
        &Explosion {
            centre: feet,
            power: 3.0,
            source: Some(creeper),
            breaks_blocks: true,
            causes_fire: false,
            allow_underwater: false,
            max_resistance: f32::MAX,
            damage_scaling: 1.0,
            knockback_scaling: 1.0,
            sound: "explode".into(),
        },
    );
    (h.health(), h.packets())
}

#[test]
fn explosion_damage_and_knockback_match_the_bds_capture() {
    // The capture: a player five blocks from the creeper on Easy, no armour.
    let (health, packets) = blast(DVec3::new(-5.0, 0.0, 0.0), Difficulty::Easy);
    assert!(
        (health - 17.036312).abs() < 1e-4,
        "health {health} (BDS 17.036312)"
    );
    let order: Vec<&McpePacketData> = packets
        .iter()
        .filter(|p| {
            matches!(
                p,
                McpePacketData::SetActorMotionPacket(_)
                    | McpePacketData::ActorEventPacket(_)
                    | McpePacketData::LevelEventPacket(_)
                    | McpePacketData::LevelEventGenericPacket(_)
            )
        })
        .collect();
    let [
        McpePacketData::SetActorMotionPacket(knockback),
        McpePacketData::ActorEventPacket(hurt),
        McpePacketData::SetActorMotionPacket(pushed),
        McpePacketData::LevelEventPacket(particles),
        McpePacketData::LevelEventGenericPacket(blocks),
    ] = order.as_slice()
    else {
        panic!("packet order: {order:?}");
    };
    assert!((knockback.motion.x + 0.4).abs() < 1e-5 && (knockback.motion.y - 0.4).abs() < 1e-5);
    assert_eq!(hurt.event_id, EnumsActorEvent::Hurt);
    assert_eq!(hurt.data, 11, "entity_explosion");
    assert!(
        (pushed.motion.x + 0.517_980_2).abs() < 1e-5
            && (pushed.motion.y - 0.438_225_8).abs() < 1e-5
            && pushed.motion.z.abs() < 1e-6,
        "{:?}",
        pushed.motion
    );
    assert_eq!((particles.event_id, particles.data), (2025, 3));
    assert_eq!(blocks.event_id, 2026);
}

#[test]
fn explosion_damage_falls_off_with_distance() {
    // Closer than about 3 blocks the blast kills a player without armour.
    let healths: Vec<f32> = [3.0, 4.0, 5.0, 6.0]
        .into_iter()
        .map(|distance| blast(DVec3::new(-distance, 0.0, 0.0), Difficulty::Normal).0)
        .collect();
    assert!(
        healths.windows(2).all(|pair| pair[0] < pair[1]),
        "{healths:?}"
    );
    let (out_of_reach, _) = blast(DVec3::new(-7.5, 0.0, 0.0), Difficulty::Normal);
    assert_eq!(out_of_reach, 20.0, "beyond 2 × power nothing is hurt");
}

#[test]
fn flint_and_steel_ignites_the_creeper() {
    let mut h = harness(9, Difficulty::Normal, NOON, GameMode::Survival);
    let stack = world_stack(h.world(), "minecraft:flint_and_steel", 1).unwrap();
    let player = h.player;
    h.world()
        .get_mut::<MainInventory>(player)
        .unwrap()
        .0
        .set_item(0, stack)
        .unwrap();
    let creeper = h.creeper(DVec3::new(10.5, 64.0, 0.5));
    // One tick spawns the creeper on the player's client; the player
    // stands beyond its targeting range meanwhile.
    h.place_player(DVec3::new(30.5, 64.0, 0.5));
    h.server.tick();
    h.place_player(DVec3::new(8.5, 64.0, 0.5));
    h.packets();
    let runtime_id = h.world().get::<RuntimeId>(creeper).unwrap().0;
    crate::entity::mob::interaction::interact::player_interact(
        h.world(),
        player,
        creeper,
        &ItemUseOnActorInventoryTransaction {
            runtime_id: ActorRuntimeId {
                actor_runtime_id: runtime_id as u64,
            },
            action_type: ActionType::Interact,
            slot: 0,
            ..Default::default()
        },
    );
    let held = h
        .world()
        .get::<MainInventory>(player)
        .unwrap()
        .0
        .item(0)
        .unwrap()
        .clone();
    assert_eq!(
        crate::item::enchant::durability_damage(&held),
        1,
        "the flint and steel wears by one"
    );
    let packets = h.packets();
    assert!(packets.iter().any(|p| matches!(p,
        McpePacketData::LevelSoundEventPacket(sound)
            if &*sound.sound_event == "ignite" && (sound.position.y - 64.9).abs() < 1e-4)));
    assert!(packets.iter().any(|p| matches!(p,
        McpePacketData::AnimatePacket(animate)
            if animate.action == EnumsAnimatePacketPayloadAction::Swing
                && animate.target_actor_runtime_id.actor_runtime_id == 1
                && animate.swing_source.as_deref() == Some("interact"))));
    h.server.tick();
    assert!(flag(h.world(), creeper, actor_flags::IGNITED));
    assert_eq!(fuse_time(h.world(), creeper), 29);

    // A lit creeper does not take a second ignition.
    crate::entity::mob::interaction::interact::player_interact(
        h.world(),
        player,
        creeper,
        &ItemUseOnActorInventoryTransaction {
            runtime_id: ActorRuntimeId {
                actor_runtime_id: runtime_id as u64,
            },
            action_type: ActionType::Interact,
            slot: 0,
            ..Default::default()
        },
    );
    let held = h
        .world()
        .get::<MainInventory>(player)
        .unwrap()
        .0
        .item(0)
        .unwrap()
        .clone();
    assert_eq!(crate::item::enchant::durability_damage(&held), 1);
}

#[test]
fn a_charged_creeper_shows_powered_and_explodes_with_power_six() {
    let mut h = harness(10, Difficulty::Normal, NOON, GameMode::Creative);
    h.place_player(DVec3::new(30.5, 64.0, 0.5));
    let world = h.world();
    let creeper = spawn_mob_with_event(
        world,
        "minecraft:creeper",
        DVec3::new(0.5, 64.0, 0.5),
        0.0,
        "minecraft:become_charged",
    )
    .unwrap();
    assert!(flag(world, creeper, actor_flags::POWERED));
    let packets = h.explode_forced(creeper);
    assert!(packets.iter().any(|p| matches!(p,
        McpePacketData::LevelEventPacket(event) if event.event_id == 2025 && event.data == 6)));
    assert!(
        spawn_mob_with_event(
            h.world(),
            "minecraft:creeper",
            DVec3::new(0.5, 64.0, 0.5),
            0.0,
            "minecraft:no_such_event",
        )
        .is_err()
    );
}

#[test]
fn creepers_drop_gunpowder_and_experience_and_a_disc_when_a_skeleton_kills_them() {
    let mut h = harness(11, Difficulty::Normal, NOON, GameMode::Survival);
    let mut gunpowder = 0;
    for _ in 0..50 {
        let creeper = h.creeper(DVec3::new(0.5, 64.0, 0.5));
        let player = h.player;
        let world = h.world();
        world.get_mut::<Health>(creeper).unwrap().current = 1.0;
        world.get_mut::<Position>(player).unwrap().0 =
            DVec3::new(-1.5, 64.0 + crate::entity::PLAYER_EYE_HEIGHT, 0.5);
        combat::player_attack(world, player, creeper, &Default::default());
        assert!(world.get::<Dead>(creeper).is_some());
        for (item, count) in h.take_drops().0 {
            assert_eq!(item, "minecraft:gunpowder", "a player kill drops no disc");
            gunpowder += count;
        }
        h.world().despawn(creeper);
    }
    assert!(gunpowder > 0);

    // Experience comes when the death animation ends.
    let creeper = h.creeper(DVec3::new(0.5, 64.0, 0.5));
    let player = h.player;
    let world = h.world();
    world.get_mut::<Health>(creeper).unwrap().current = 1.0;
    combat::player_attack(world, player, creeper, &Default::default());
    for _ in 0..40 {
        if !h.exists(creeper) {
            break;
        }
        h.server.tick();
    }
    assert_eq!(h.take_drops().1, 5);

    // The disc pools need a skeleton (or stray, bogged, parched) killer.
    let creeper = h.creeper(DVec3::new(0.5, 64.0, 0.5));
    let name = h
        .world()
        .get::<crate::entity::mob::components::LootConfig>(creeper)
        .unwrap()
        .table
        .clone();
    let table = unastar_data::loot::get(&name).unwrap();
    let mut rng = <rand::rngs::StdRng as rand::SeedableRng>::seed_from_u64(1);
    for (killer, disc) in [
        (Some(ActorType::Skeleton), true),
        (Some(ActorType::Stray), true),
        (Some(ActorType::Zombie), false),
        (None, false),
    ] {
        let ctx = crate::server::game::LootContext {
            killer,
            ..Default::default()
        };
        let drops = unastar_rules::loot::roll(unastar_data::loot::vanilla(), table, &ctx, &mut rng)
            .unwrap();
        let discs = drops
            .iter()
            .filter(|drop| drop.item_id.starts_with("minecraft:record"))
            .count();
        assert_eq!(discs, usize::from(disc), "{killer:?}: {drops:?}");
    }
}

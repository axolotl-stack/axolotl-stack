//! The vanilla spider and cave spider through a real `GameServer`: native
//! AddActor equality, wall climbing, day/night hostility, the leap, cave
//! spider poison per difficulty, loot and experience.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::actor_flags;
use jolyne::valentine::{AddActorPacket, McpePacket};
use tokio::sync::mpsc;
use unastar_data::actor_components::body::ActorDamageCause;
use valentine::bedrock::codec::BedrockCodec;

use crate::entity::EntityRandom;
use crate::entity::components::{
    DroppedItem, EffectType, Effects, Experience, GameMode, Health, HeldSlot, ItemEntityStack,
    MainInventory, PhysicsBody, Player, PlayerSession, Position, Rotation, RuntimeEntityId,
    RuntimeId, Velocity,
};
use crate::entity::definition::apply_pending_definitions;
use crate::entity::definition::attributes::ActorAttributes;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::definition::properties::ActorProperties;
use crate::entity::mob::components::KnockbackPower;
use crate::entity::mob::net::network::add_actor_packet;
use crate::entity::mob::state::{AttackTarget, Dead};
use crate::entity::mob::*;
use crate::entity::xp_orb::XpOrb;
use crate::server::game::{GameServer, ServerConfig};
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkManager, ChunkPosition, ChunkViewers};
use crate::world::rules::Difficulty;

/// Noon and midnight on the world clock.
const NOON: u64 = 6_000;
const MIDNIGHT: u64 = 18_000;

struct Harness {
    server: GameServer,
    player: Entity,
    _rx: mpsc::Receiver<McpePacket>,
}

/// A grass floor at y = 63 over chunks -1..=1 × -1..=1 at `time`, viewed by
/// one survival player standing on it at (8.5, 64, 0.5).
fn harness(seed: u64, difficulty: Difficulty, time: u64) -> Harness {
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
            GameMode::Survival,
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
    Harness {
        server,
        player,
        _rx: rx,
    }
}

fn native(bytes: &[u8]) -> AddActorPacket {
    assert_eq!(bytes[0], 0x0d);
    let mut input = &bytes[1..];
    let packet = AddActorPacket::decode(&mut input, ()).unwrap();
    assert!(input.is_empty());
    packet
}

impl Harness {
    fn world(&mut self) -> &mut World {
        self.server.ecs.world_mut()
    }

    fn spawn(&mut self, identifier: &str, feet: DVec3) -> Entity {
        let world = self.world();
        let mob = spawn_mob(world, identifier, feet, 0.0, SpawnCause::Spawned).unwrap();
        apply_pending_definitions(world);
        mob
    }

    fn set_block(&mut self, pos: IVec3, name: &str) {
        let state = crate::registry::BlockRegistry::vanilla()
            .default_state_id_by_name(name)
            .unwrap();
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

    /// Fill the box `min..=max` with `name`.
    fn fill(&mut self, min: IVec3, max: IVec3, name: &str) {
        for x in min.x..=max.x {
            for y in min.y..=max.y {
                for z in min.z..=max.z {
                    self.set_block(IVec3::new(x, y, z), name);
                }
            }
        }
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

    fn target(&mut self, mob: Entity) -> Option<Entity> {
        self.world().get::<AttackTarget>(mob).map(|t| t.0)
    }

    /// Tick until the player loses health (at most `ticks`); the tick it did.
    fn first_hit(&mut self, ticks: u64) -> Option<u64> {
        let start = self.health();
        (0..ticks).find(|_| {
            self.server.tick();
            self.health() < start
        })
    }

    /// Kill `mob`: punched by the player with one health left, or by a
    /// damage without an attacker.
    fn kill(&mut self, mob: Entity, by_player: bool) {
        let player = self.player;
        let world = self.world();
        world.get_mut::<Health>(mob).unwrap().current = 1.0;
        if by_player {
            let feet = world.get::<Position>(mob).unwrap().0;
            world.get_mut::<Position>(player).unwrap().0 =
                feet + DVec3::new(-2.0, crate::entity::PLAYER_EYE_HEIGHT, 0.0);
            combat::player_attack(world, player, mob, &Default::default());
        } else {
            combat::hurt_mob(
                world,
                mob,
                combat::Hit {
                    damage: 5.0,
                    cause: ActorDamageCause::Magic,
                    attacker: None,
                    by_player: false,
                    knockback_from: None,
                    knockback_power: KnockbackPower::DEFAULT,
                },
            );
        }
        assert!(world.get::<Dead>(mob).is_some());
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

fn flag(world: &World, mob: Entity, index: u32) -> bool {
    world.get::<ActorFlags>(mob).unwrap().get(index).unwrap()
}

#[test]
fn vanilla_spiders_compile_to_spawnable_archetypes() {
    let mut h = harness(1, Difficulty::Normal, MIDNIGHT);
    let registry = h.world().resource::<MobRegistry>();
    for identifier in ["minecraft:spider", "minecraft:cave_spider"] {
        assert!(
            registry.get(identifier).is_some(),
            "{identifier} unsupported: {:?}",
            registry.unsupported_reason(identifier)
        );
    }
}

fn assert_add_actor_matches_native_summon(identifier: &str, capture: &[u8]) {
    let mut h = harness(2, Difficulty::Normal, MIDNIGHT);
    let native = native(capture);
    let feet = DVec3::new(
        f64::from(native.position.x),
        f64::from(native.position.y),
        f64::from(native.position.z),
    );
    let mob = h.spawn(identifier, feet);
    let world = h.world();
    let ours = add_actor_packet(
        identifier,
        world.get::<RuntimeId>(mob).unwrap().0,
        world.get::<Position>(mob).unwrap().0,
        DVec3::new(0.0, f64::from(native.velocity.y), 0.0),
        world.get::<Rotation>(mob).unwrap(),
        world.get::<EntityMetadata>(mob).unwrap(),
        world.get::<ActorProperties>(mob).unwrap(),
        world.get::<ActorAttributes>(mob).unwrap(),
    );
    assert_eq!(ours.actor_type, native.actor_type, "{identifier}");
    assert_eq!(ours.attributes_list, native.attributes_list, "{identifier}");
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
        "{identifier}: native only: {missing:?}\nours only: {extra:?}"
    );
    assert_eq!(ours.actor_data, native.actor_data, "{identifier}");
    assert_eq!(ours.actor_links, native.actor_links, "{identifier}");
    assert_eq!(
        ours.synched_properties, native.synched_properties,
        "{identifier}"
    );
}

#[test]
fn spawned_spider_add_actor_matches_native_summon() {
    assert_add_actor_matches_native_summon(
        "minecraft:spider",
        include_bytes!("../../../../../bds-extractor/captures/1.26.51/operator/summons/spider.bin"),
    );
}

#[test]
fn spawned_cave_spider_add_actor_matches_native_summon() {
    assert_add_actor_matches_native_summon(
        "minecraft:cave_spider",
        include_bytes!(
            "../../../../../bds-extractor/captures/1.26.51/operator/summons/cave_spider.bin"
        ),
    );
}

#[test]
fn spider_climbs_a_three_block_wall_to_reach_a_player_on_top() {
    let mut h = harness(3, Difficulty::Normal, MIDNIGHT);
    // A platform three blocks high; the player stands near its edge.
    h.fill(
        IVec3::new(4, 64, -8),
        IVec3::new(9, 66, 8),
        "minecraft:stone",
    );
    h.place_player(DVec3::new(5.5, 67.0, 0.5));
    let spider = h.spawn("minecraft:spider", DVec3::new(0.5, 64.0, 0.5));
    let start = h.health();
    let (mut climbed, mut peak) = (false, 64.0_f64);
    for _ in 0..600 {
        h.server.tick();
        let world = h.world();
        climbed |= flag(world, spider, actor_flags::WALLCLIMBING);
        peak = peak.max(world.get::<Position>(spider).unwrap().0.y);
        if h.health() < start {
            break;
        }
    }
    assert!(climbed, "the spider showed the wall-climbing flag");
    assert!(peak >= 67.0, "the spider reached the top: peak {peak}");
    assert!(h.health() < start, "the spider hit the player on the top");
}

#[test]
fn spider_is_neutral_by_day() {
    let mut h = harness(4, Difficulty::Normal, NOON);
    let spider = h.spawn("minecraft:spider", DVec3::new(4.5, 64.0, 0.5));
    h.ticks(200);
    assert_eq!(h.target(spider), None);
    assert_eq!(h.health(), 20.0);
}

#[test]
fn spider_is_hostile_at_night() {
    let mut h = harness(5, Difficulty::Normal, MIDNIGHT);
    let spider = h.spawn("minecraft:spider", DVec3::new(2.5, 64.0, 0.5));
    let player = h.player;
    assert!(h.first_hit(400).is_some(), "the spider attacked at night");
    assert_eq!(h.target(spider), Some(player));
}

#[test]
fn spider_hurt_by_a_player_in_daylight_fights_back() {
    let mut h = harness(6, Difficulty::Normal, NOON);
    let spider = h.spawn("minecraft:spider", DVec3::new(6.5, 64.0, 0.5));
    let player = h.player;
    let world = h.world();
    combat::player_attack(world, player, spider, &Default::default());
    assert!(h.first_hit(400).is_some(), "the hurt spider attacked");
    assert_eq!(h.target(spider), Some(player));
}

#[test]
fn spider_leaps_at_a_target_it_has_no_path_to() {
    let mut h = harness(7, Difficulty::Normal, NOON);
    // A walled 3 × 3 cell: the spider has no path out, so its melee goal
    // cannot start and the leap goal may.
    h.fill(
        IVec3::new(-2, 64, -2),
        IVec3::new(2, 67, 2),
        "minecraft:stone",
    );
    h.fill(
        IVec3::new(-1, 64, -1),
        IVec3::new(1, 67, 1),
        "minecraft:air",
    );
    h.place_player(DVec3::new(3.5, 64.0, 0.5));
    let player = h.player;
    let target_eye = DVec3::new(3.5, 64.0 + crate::entity::PLAYER_EYE_HEIGHT, 0.5);
    // `yd` is authored as an f32.
    let yd = f64::from(0.4_f32);
    let mut leaps = 0;
    for _ in 0..10 {
        let spider = h.spawn("minecraft:spider", DVec3::new(0.5, 64.0, 0.5));
        ai::set_attack_target(h.world(), spider, Some(player));
        let body = *h.world().get::<PhysicsBody>(spider).unwrap();
        for _ in 0..40 {
            let before = *h.world().get::<Position>(spider).unwrap();
            h.server.tick();
            let velocity = h.world().get::<Velocity>(spider).unwrap().0;
            // The tick of the leap: vertical velocity yd, then one airborne
            // physics step.
            if (velocity.y - (yd - body.gravity) * body.vertical_drag).abs() < 1e-9 {
                let toward = target_eye - before.0;
                assert!(
                    velocity.x * toward.x + velocity.z * toward.z > 0.0,
                    "the leap heads for the target: {velocity:?}"
                );
                leaps += 1;
                break;
            }
        }
        h.world().despawn(spider);
    }
    assert!(leaps > 0, "a spider leapt in ten tries");
}

/// A cave spider angry at the player, hitting until the first hit lands;
/// returns the player's poison afterwards.
fn cave_spider_poison(difficulty: Difficulty) -> Option<u32> {
    let mut h = harness(8, difficulty, NOON);
    let spider = h.spawn("minecraft:cave_spider", DVec3::new(6.5, 64.0, 0.5));
    let player = h.player;
    ai::set_attack_target(h.world(), spider, Some(player));
    assert!(
        h.first_hit(400).is_some(),
        "{difficulty:?}: the cave spider hit"
    );
    h.world()
        .get::<Effects>(player)
        .and_then(|effects| effects.get(EffectType::POISON))
        .map(|poison| poison.duration_ticks)
}

#[test]
fn cave_spider_poisons_on_normal_and_hard_but_not_easy() {
    assert_eq!(cave_spider_poison(Difficulty::Easy), None);
    let normal = cave_spider_poison(Difficulty::Normal).expect("poisoned on Normal");
    assert!((130..=140).contains(&normal), "7 s: {normal}");
    let hard = cave_spider_poison(Difficulty::Hard).expect("poisoned on Hard");
    assert!((290..=300).contains(&hard), "15 s: {hard}");
}

#[test]
fn cave_spider_poison_hurts_the_player() {
    let mut h = harness(9, Difficulty::Normal, NOON);
    let spider = h.spawn("minecraft:cave_spider", DVec3::new(6.5, 64.0, 0.5));
    let player = h.player;
    ai::set_attack_target(h.world(), spider, Some(player));
    assert!(h.first_hit(400).is_some());
    h.world().despawn(spider);
    let after_hit = h.health();
    h.ticks(60);
    assert!(
        h.health() < after_hit,
        "poison kept hurting the player: {after_hit} -> {}",
        h.health()
    );
}

#[test]
fn spiders_drop_string_and_eyes_only_for_player_kills_with_experience() {
    for identifier in ["minecraft:spider", "minecraft:cave_spider"] {
        let mut h = harness(10, Difficulty::Normal, NOON);
        for by_player in [true, false] {
            let (mut string, mut eyes) = (0, 0);
            for _ in 0..100 {
                let spider = h.spawn(identifier, DVec3::new(0.5, 64.0, 0.5));
                h.kill(spider, by_player);
                for (item, count) in h.take_drops().0 {
                    match item.as_str() {
                        "minecraft:string" => string += count,
                        "minecraft:spider_eye" => eyes += count,
                        other => panic!("{identifier}: unexpected drop {other}"),
                    }
                }
                h.world().despawn(spider);
            }
            assert!(string > 0, "{identifier}: by_player = {by_player}");
            if by_player {
                assert!(eyes > 0, "{identifier}: eyes from player kills");
            } else {
                assert_eq!(eyes, 0, "{identifier}: eyes need a player kill");
            }

            // Experience comes when the death animation ends.
            let spider = h.spawn(identifier, DVec3::new(0.5, 64.0, 0.5));
            h.kill(spider, by_player);
            for _ in 0..40 {
                if h.world().get_entity(spider).is_err() {
                    break;
                }
                h.server.tick();
            }
            let experience = h.take_drops().1;
            assert_eq!(
                experience,
                if by_player { 5 } else { 0 },
                "{identifier}: by_player = {by_player}"
            );
        }
    }
}

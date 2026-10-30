//! The vanilla skeleton and stray through a real `GameServer`: native
//! AddActor equality, the ranged attack (targeting, reload, the arrow and
//! its damage, accuracy by difficulty), holding position and fleeing
//! wolves, daylight burning and fleeing the sun, the stray's slowness
//! arrows, loot and experience.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::actor_data_i_ds as ids;
use jolyne::valentine::actor_flags;
use jolyne::valentine::{AddActorPacket, McpePacket, McpePacketData};
use tokio::sync::mpsc;
use unastar_data::actor_components::body::ActorDamageCause;
use unastar_data::actor_identifiers::ActorType;
use valentine::bedrock::codec::BedrockCodec;
use valentine::bedrock::version::v1_26_51::types::DataItemEntryPayload;

use crate::entity::EntityRandom;
use crate::entity::components::{
    DroppedItem, EffectType, Effects, Experience, GameMode, Health, HeldSlot, ItemEntityStack,
    MainInventory, Player, PlayerSession, Position, Projectile, Rotation, RuntimeEntityId,
    RuntimeId, Velocity,
};
use crate::entity::definition::apply_pending_definitions;
use crate::entity::definition::attributes::ActorAttributes;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::definition::properties::ActorProperties;
use crate::entity::mob::combat::fire::Burning;
use crate::entity::mob::components::KnockbackPower;
use crate::entity::mob::gear::{GearSlot, MobEquipment};
use crate::entity::mob::net::network::add_actor_packet;
use crate::entity::mob::state::{AttackTarget, Dead};
use crate::entity::mob::*;
use crate::entity::projectile::{MobShot, ProjectileFlight, ProjectileItem, launch_at};
use crate::entity::xp_orb::XpOrb;
use crate::server::game::{GameServer, ServerConfig, world_stack};
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkManager, ChunkPosition, ChunkViewers};
use crate::world::rules::Difficulty;

/// Noon and midnight on the world clock.
const NOON: u64 = 6_000;
const MIDNIGHT: u64 = 18_000;

const SKELETON: &str = "minecraft:skeleton";
const STRAY: &str = "minecraft:stray";

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

fn int_data(world: &World, actor: Entity, id: u32) -> Option<i64> {
    match world.get::<EntityMetadata>(actor).unwrap().get(id)? {
        DataItemEntryPayload::DataItemIntPayload(payload) => Some(i64::from(payload.value)),
        DataItemEntryPayload::DataItemBytePayload(payload) => Some(i64::from(payload.value)),
        other => panic!("data {id} is not an int: {other:?}"),
    }
}

/// The `(tick, projectile)` shots and `(tick, health lost)` hits seen.
type Shots = (Vec<(u64, Entity)>, Vec<(u64, f32)>);

/// Every projectile in the world.
fn projectiles(world: &mut World) -> Vec<Entity> {
    world
        .query_filtered::<Entity, With<Projectile>>()
        .iter(world)
        .collect()
}

impl Harness {
    fn world(&mut self) -> &mut World {
        self.server.ecs.world_mut()
    }

    fn mob(&mut self, identifier: &str, feet: DVec3) -> Entity {
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

    fn ticks(&mut self, n: u64) {
        for _ in 0..n {
            self.server.tick();
        }
    }

    fn health(&mut self) -> f32 {
        let player = self.player;
        self.world().get::<Health>(player).unwrap().current
    }

    fn feet(&mut self, mob: Entity) -> DVec3 {
        self.world().get::<Position>(mob).unwrap().0
    }

    fn packets(&mut self) -> Vec<McpePacketData> {
        std::iter::from_fn(|| self.rx.try_recv().ok().map(|p| p.data)).collect()
    }

    fn exists(&mut self, mob: Entity) -> bool {
        self.world().get_entity(mob).is_ok()
    }

    /// Wear `item` on `mob`'s head (`None`: bare-headed).
    fn helmet(&mut self, mob: Entity, item: Option<&str>) {
        let world = self.world();
        let stack = item.map(|item| world_stack(world, item, 1).unwrap());
        let mut entity = world.entity_mut(mob);
        let mut equipment = entity.entry::<MobEquipment>().or_default();
        let mut equipment = equipment.get_mut();
        match stack {
            Some(stack) => {
                equipment.set(GearSlot::Head, stack, false);
            }
            None => {
                equipment.take(GearSlot::Head);
            }
        }
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

    /// Every dropped item stack `(id, data, count)` and the total
    /// experience of every orb, removing both from the world.
    fn take_drops(&mut self) -> (Vec<(String, i16, u32)>, u32) {
        let world = self.world();
        let drops: Vec<(Entity, String, i16, u32)> = world
            .query_filtered::<(Entity, &ItemEntityStack), With<DroppedItem>>()
            .iter(world)
            .map(|(entity, stack)| {
                (
                    entity,
                    stack.0.item_id().to_owned(),
                    stack.0.damage,
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
            drops
                .into_iter()
                .map(|(_, id, data, n)| (id, data, n))
                .collect(),
            orbs.iter().map(|o| o.1).sum(),
        )
    }

    /// Tick up to `ticks` times; the ticks on which `shooter` launched a
    /// projectile, with the projectile, and the ticks on which the player
    /// lost health, with the amount.
    fn watch_shots(&mut self, shooter: Entity, ticks: u64, wanted: usize) -> Shots {
        let mut known: std::collections::HashSet<Entity> =
            projectiles(self.world()).into_iter().collect();
        let mut shots = Vec::new();
        let mut hits = Vec::new();
        let mut health = self.health();
        for tick in 0..ticks {
            self.server.tick();
            for projectile in projectiles(self.world()) {
                if known.insert(projectile) {
                    let owner = self
                        .world()
                        .get::<ProjectileFlight>(projectile)
                        .and_then(|flight| flight.owner);
                    assert_eq!(owner, Some(shooter), "only the shooter shoots");
                    shots.push((tick, projectile));
                }
            }
            let now = self.health();
            if now < health {
                hits.push((tick, health - now));
            }
            health = now;
            if shots.len() >= wanted && hits.len() >= wanted.min(1) {
                break;
            }
        }
        (shots, hits)
    }
}

fn golden(identifier: &str, bytes: &[u8]) {
    let mut h = harness(2, Difficulty::Normal, MIDNIGHT, GameMode::Survival);
    let native = native(bytes);
    let feet = DVec3::new(
        f64::from(native.position.x),
        f64::from(native.position.y),
        f64::from(native.position.z),
    );
    let mob = h.mob(identifier, feet);
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
fn vanilla_skeleton_and_stray_compile_to_spawnable_archetypes() {
    let mut h = harness(1, Difficulty::Normal, MIDNIGHT, GameMode::Survival);
    let registry = h.world().resource::<MobRegistry>();
    for identifier in [SKELETON, STRAY] {
        assert!(
            registry.get(identifier).is_some(),
            "{identifier} unsupported: {:?}",
            registry.unsupported_reason(identifier)
        );
    }
}

#[test]
fn spawned_skeleton_add_actor_matches_native_summon() {
    golden(
        SKELETON,
        include_bytes!(
            "../../../../../bds-extractor/captures/1.26.51/operator/summons/skeleton.bin"
        ),
    );
}

#[test]
fn spawned_stray_add_actor_matches_native_summon() {
    golden(
        STRAY,
        include_bytes!("../../../../../bds-extractor/captures/1.26.51/operator/summons/stray.bin"),
    );
}

#[test]
fn skeletons_spawn_holding_a_bow() {
    let mut h = harness(3, Difficulty::Easy, MIDNIGHT, GameMode::Creative);
    for identifier in [SKELETON, STRAY] {
        let mob = h.mob(identifier, DVec3::new(0.5, 64.0, 0.5));
        let world = h.world();
        let hand = world
            .get::<MobEquipment>(mob)
            .unwrap()
            .get(GearSlot::Mainhand)
            .item_id()
            .to_owned();
        assert_eq!(hand, "minecraft:bow", "{identifier}");
    }
}

#[test]
fn skeleton_targets_the_player_and_shoots_every_61_ticks_on_normal_and_41_on_hard() {
    for (difficulty, period) in [(Difficulty::Normal, 61), (Difficulty::Hard, 41)] {
        let mut h = harness(4, difficulty, MIDNIGHT, GameMode::Survival);
        let skeleton = h.mob(SKELETON, DVec3::new(0.5, 64.0, 0.5));
        let player = h.player;
        let (shots, hits) = h.watch_shots(skeleton, 400, 3);
        let world = h.world();
        assert_eq!(
            world.get::<AttackTarget>(skeleton),
            Some(&AttackTarget(player))
        );
        assert!(
            flag(world, skeleton, actor_flags::FACING_TARGET_TO_RANGE_ATTACK),
            "the bow is drawn while the goal runs"
        );
        assert_eq!(shots.len(), 3, "{difficulty:?}: {shots:?}");
        assert_eq!(shots[1].0 - shots[0].0, period, "{difficulty:?}: {shots:?}");
        assert_eq!(shots[2].0 - shots[1].0, period, "{difficulty:?}: {shots:?}");
        // The arrow hits the player standing 8 blocks away.
        assert!(!hits.is_empty(), "{difficulty:?}: no hit");
        for (_, dealt) in &hits {
            // ceil(speed × (2 + 0.11 × difficulty + 0.25 × gaussian)),
            // not scaled by the player-side difficulty rules.
            assert!((2.0..=6.0).contains(dealt), "{difficulty:?}: hits {hits:?}");
        }
        let (_, arrow) = shots[0];
        let world = h.world();
        if let Ok(arrow) = world.get_entity(arrow) {
            assert!(
                arrow.get::<ProjectileItem>().is_none(),
                "nothing to pick up"
            );
            assert!(!arrow.get::<ProjectileFlight>().is_some_and(|f| f.critical));
        }
        // One bow sound per shot.
        let bows: Vec<_> = h
            .packets()
            .into_iter()
            .filter_map(|p| match p {
                McpePacketData::LevelSoundEventPacket(sound) if &*sound.sound_event == "bow" => {
                    Some(sound)
                }
                _ => None,
            })
            .collect();
        assert_eq!(bows.len(), shots.len(), "{difficulty:?}");
        assert_eq!(&*bows[0].actor_identifier, SKELETON);
    }
}

#[test]
fn the_bow_is_drawn_only_while_the_skeleton_has_a_target() {
    let mut h = harness(5, Difficulty::Normal, MIDNIGHT, GameMode::Creative);
    let skeleton = h.mob(SKELETON, DVec3::new(0.5, 64.0, 0.5));
    h.ticks(60);
    let world = h.world();
    assert!(world.get::<AttackTarget>(skeleton).is_none());
    assert!(!flag(
        world,
        skeleton,
        actor_flags::FACING_TARGET_TO_RANGE_ATTACK
    ));
    assert!(projectiles(world).is_empty(), "no target, no shot");
}

/// The mean distance of the unit launch directions from their mean, over
/// `n` shots at the player 8 blocks away.
fn spread(difficulty: Difficulty, n: usize) -> f64 {
    let mut h = harness(6, difficulty, MIDNIGHT, GameMode::Creative);
    let skeleton = h.mob(SKELETON, DVec3::new(0.5, 64.0, 0.5));
    let player = h.player;
    let world = h.world();
    let directions: Vec<DVec3> = (0..n)
        .map(|_| {
            let arrow = launch_at(
                world,
                skeleton,
                ActorType::Arrow,
                player,
                MobShot::default(),
            )
            .unwrap();
            let velocity = world.get::<Velocity>(arrow).unwrap().0;
            world.despawn(arrow);
            velocity.normalize()
        })
        .collect();
    let mean = directions.iter().sum::<DVec3>() / n as f64;
    directions.iter().map(|d| d.distance(mean)).sum::<f64>() / n as f64
}

#[test]
fn skeleton_arrows_are_more_accurate_on_harder_difficulties() {
    // Uncertainty 16 − 4 × difficulty: 12, 8 and 4.
    let [easy, normal, hard] =
        [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard].map(|d| spread(d, 400));
    assert!(easy > normal && normal > hard, "{easy} {normal} {hard}");
    let ratio = easy / hard;
    assert!((2.4..=3.6).contains(&ratio), "easy / hard = {ratio}");
    let ratio = normal / hard;
    assert!((1.6..=2.4).contains(&ratio), "normal / hard = {ratio}");
}

/// The yaw (degrees) that faces from `from` toward `to`.
fn yaw_toward(from: DVec3, to: DVec3) -> f64 {
    let d = to - from;
    (-d.x).atan2(d.z).to_degrees()
}

fn angle_between(a: f64, b: f64) -> f64 {
    ((a - b + 540.0).rem_euclid(360.0) - 180.0).abs()
}

#[test]
fn in_range_the_skeleton_holds_its_position_and_faces_the_target() {
    let mut h = harness(7, Difficulty::Normal, MIDNIGHT, GameMode::Survival);
    let skeleton = h.mob(SKELETON, DVec3::new(0.5, 64.0, 3.5));
    // Seen for a second, in range: it stops walking.
    h.ticks(40);
    let held = h.feet(skeleton);
    for tick in 0..60 {
        h.server.tick();
        let feet = h.feet(skeleton);
        let moved = horizontal(feet - held);
        assert!(moved < 0.05, "tick {tick}: moved {moved}");
    }
    let player = h.player;
    let target = h.world().get::<Position>(player).unwrap().0;
    let rotation = *h.world().get::<Rotation>(skeleton).unwrap();
    let want = yaw_toward(held, target);
    assert!(
        angle_between(f64::from(rotation.yaw), want) < 1.0,
        "body yaw {} toward {want}",
        rotation.yaw
    );
    assert!(
        angle_between(f64::from(rotation.head_yaw), want) < 1.0,
        "head yaw {} toward {want}",
        rotation.head_yaw
    );
}

/// The horizontal length of `d`.
fn horizontal(d: DVec3) -> f64 {
    d.x.hypot(d.z)
}

#[test]
fn out_of_range_the_skeleton_walks_toward_the_target() {
    let mut h = harness(8, Difficulty::Normal, MIDNIGHT, GameMode::Survival);
    // Beyond the 15-block attack range, inside the 16-block follow range.
    let skeleton = h.mob(SKELETON, DVec3::new(-7.1, 64.0, 0.5));
    let player = h.player;
    let target = h.world().get::<Position>(player).unwrap().0;
    let start = horizontal(h.feet(skeleton) - target);
    h.ticks(80);
    let end = horizontal(h.feet(skeleton) - target);
    assert!(
        end < start - 0.4 && end <= 15.2,
        "approached from {start} to {end}"
    );
}

#[test]
fn skeletons_flee_wolves() {
    let mut h = harness(9, Difficulty::Normal, MIDNIGHT, GameMode::Creative);
    let skeleton = h.mob(SKELETON, DVec3::new(0.5, 64.0, 0.5));
    // The wolf is not spawnable yet: a cow in the wolf family stands in.
    let wolf = h.mob("minecraft:cow", DVec3::new(3.5, 64.0, 0.5));
    let world = h.world();
    let families = std::sync::Arc::clone(world.resource::<MobRegistry>().families())
        .membership(&["wolf".to_owned()])
        .unwrap();
    world.entity_mut(wolf).insert(families);
    let wolf_feet = h.feet(wolf);
    let start = (h.feet(skeleton) - wolf_feet).length();
    let mut farthest = start;
    for _ in 0..80 {
        h.server.tick();
        // Keep the wolf in place.
        if let Some(mut position) = h.world().get_mut::<Position>(wolf) {
            position.0 = wolf_feet;
        }
        farthest = farthest.max((h.feet(skeleton) - wolf_feet).length());
    }
    assert!(farthest > start + 2.0, "fled from {start} to {farthest}");
}

/// Tick until the daylight check has run a few times and report whether
/// `mob` caught fire.
fn burns(h: &mut Harness, mob: Entity) -> bool {
    h.ticks(6);
    let world = h.world();
    let burning = world.get::<Burning>(mob).is_some();
    assert_eq!(burning, flag(world, mob, actor_flags::ONFIRE));
    burning
}

#[test]
fn skeletons_burn_in_daylight_but_not_with_a_helmet() {
    for identifier in [SKELETON, STRAY] {
        let mut h = harness(10, Difficulty::Easy, NOON, GameMode::Creative);
        let mob = h.mob(identifier, DVec3::new(0.5, 64.0, 0.5));
        h.helmet(mob, None);
        assert!(burns(&mut h, mob), "{identifier} burns bare-headed");

        let mut h = harness(10, Difficulty::Easy, NOON, GameMode::Creative);
        let mob = h.mob(identifier, DVec3::new(0.5, 64.0, 0.5));
        h.helmet(mob, Some("minecraft:iron_helmet"));
        assert!(!burns(&mut h, mob), "{identifier} with a helmet");

        let mut h = harness(10, Difficulty::Easy, MIDNIGHT, GameMode::Creative);
        let mob = h.mob(identifier, DVec3::new(0.5, 64.0, 0.5));
        assert!(!burns(&mut h, mob), "{identifier} at night");
    }
}

#[test]
fn a_burning_skeleton_flees_the_sun_under_cover() {
    let mut h = harness(11, Difficulty::Easy, NOON, GameMode::Creative);
    // A leaf roof over x 2..=7, z -3..=3: covered, yet bright beneath.
    let roofed = |feet: DVec3| (2.0..8.0).contains(&feet.x) && (-3.0..4.0).contains(&feet.z);
    for x in 2..=7 {
        for z in -3..=3 {
            h.set_block(IVec3::new(x, 66, z), "minecraft:oak_leaves");
        }
    }
    let skeleton = h.mob(SKELETON, DVec3::new(-3.5, 64.0, 0.5));
    h.helmet(skeleton, None);
    assert!(burns(&mut h, skeleton), "it catches fire in the open");
    let mut covered = None;
    for tick in 0..200 {
        h.server.tick();
        if roofed(h.feet(skeleton)) {
            covered = Some(tick);
            break;
        }
    }
    assert!(
        covered.is_some(),
        "it walked under the roof: {:?}",
        h.feet(skeleton)
    );

    // Not burning, it stays where it is.
    let mut h = harness(11, Difficulty::Easy, NOON, GameMode::Creative);
    let skeleton = h.mob(SKELETON, DVec3::new(-3.5, 64.0, 0.5));
    h.helmet(skeleton, Some("minecraft:iron_helmet"));
    for _ in 0..200 {
        h.server.tick();
        assert!(!roofed(h.feet(skeleton)), "no reason to seek cover");
    }
}

#[test]
fn stray_arrows_carry_long_slowness_and_slow_the_player_for_600_ticks() {
    let mut h = harness(12, Difficulty::Normal, MIDNIGHT, GameMode::Survival);
    let stray = h.mob(STRAY, DVec3::new(0.5, 64.0, 0.5));
    let player = h.player;
    let (shots, hits) = h.watch_shots(stray, 300, 1);
    assert!(!shots.is_empty() && !hits.is_empty(), "{shots:?} {hits:?}");
    let (_, arrow) = shots[0];
    if h.exists(arrow) {
        // Long slowness is potion 18; the arrow carries it plus one.
        assert_eq!(int_data(h.world(), arrow, ids::CUSTOM_DISPLAY), Some(19));
    }
    let world = h.world();
    let slowness = world
        .get::<Effects>(player)
        .and_then(|effects| effects.get(EffectType::SLOWNESS))
        .cloned()
        .expect("the hit slowed the player");
    assert_eq!(slowness.level, 0);
    // 4800 / 8, less the effect ticks since the hit.
    assert!(
        (598..=600).contains(&slowness.duration_ticks),
        "{}",
        slowness.duration_ticks
    );

    // A skeleton's arrow carries nothing.
    let mut h = harness(12, Difficulty::Normal, MIDNIGHT, GameMode::Survival);
    let skeleton = h.mob(SKELETON, DVec3::new(0.5, 64.0, 0.5));
    let player = h.player;
    let (shots, hits) = h.watch_shots(skeleton, 300, 1);
    assert!(!shots.is_empty() && !hits.is_empty(), "{shots:?} {hits:?}");
    let world = h.world();
    assert!(
        !world
            .get::<Effects>(player)
            .is_some_and(|effects| effects.has(EffectType::SLOWNESS))
    );
}

#[test]
fn skeleton_and_stray_drop_arrows_and_bones_and_the_stray_a_tipped_arrow_only_for_a_player() {
    let mut h = harness(13, Difficulty::Easy, MIDNIGHT, GameMode::Creative);
    for identifier in [SKELETON, STRAY] {
        for by_player in [true, false] {
            let (mut arrows, mut bones, mut tipped) = (0, 0, 0);
            for _ in 0..100 {
                let mob = h.mob(identifier, DVec3::new(0.5, 64.0, 0.5));
                // The bow it holds never drops here.
                h.world()
                    .get_mut::<MobEquipment>(mob)
                    .unwrap()
                    .take(GearSlot::Mainhand);
                h.kill(mob, by_player);
                for (item, data, count) in h.take_drops().0 {
                    match (item.as_str(), data) {
                        ("minecraft:arrow", 0) => arrows += count,
                        // Slowness (potion 17) plus one.
                        ("minecraft:arrow", 18) => tipped += count,
                        ("minecraft:bone", _) => bones += count,
                        other => panic!("{identifier} dropped {other:?}"),
                    }
                }
                h.world().despawn(mob);
            }
            assert!(
                arrows > 0 && bones > 0,
                "{identifier} by_player {by_player}"
            );
            if identifier == STRAY && by_player {
                assert!(tipped > 0, "half of the player kills drop one");
            } else {
                assert_eq!(tipped, 0, "{identifier} by_player {by_player}");
            }
        }
    }
}

#[test]
fn a_player_kill_gives_five_experience_plus_one_to_three_per_held_item() {
    let mut h = harness(14, Difficulty::Easy, MIDNIGHT, GameMode::Creative);
    let mut with_bow = 0;
    for by_player in [true, true, true, true, true, true, false, false] {
        let mob = h.mob(SKELETON, DVec3::new(0.5, 64.0, 0.5));
        h.kill(mob, by_player);
        // The bow leaves the hand only when it dropped.
        let held = crate::entity::mob::gear::equipment_count(h.world(), mob);
        for _ in 0..40 {
            if !h.exists(mob) {
                break;
            }
            h.server.tick();
        }
        assert!(!h.exists(mob), "the death animation ended");
        let experience = h.take_drops().1;
        let range = match (by_player, held) {
            (false, _) => 0..=0,
            (true, 0) => 5..=5,
            (true, _) => 6..=8,
        };
        with_bow += u32::from(by_player && held > 0);
        assert!(
            range.contains(&experience),
            "by_player {by_player}, {held} held: {experience}"
        );
    }
    assert!(with_bow > 0, "most player kills leave the bow held");
}

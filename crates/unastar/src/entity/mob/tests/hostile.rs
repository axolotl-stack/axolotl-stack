//! Hostile mobs beyond the zombie on the data-driven runtime: the husk, the
//! wither skeleton and the endermite. Native AddActor equality (appearance
//! ids, the native skeleton kind and the fire-immune flag included), the
//! appearance-id filters, attack effects and fire immunity, driven through
//! the real spawn, tick and hurt paths.

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::actor_flags;
use jolyne::valentine::{AddActorPacket, McpePacket};
use serde_json::json;
use tokio::sync::mpsc;
use unastar_data::actor_components::body::ActorDamageCause;
use unastar_rules::filter::{Filter, FilterRegistry};
use valentine::bedrock::codec::BedrockCodec;

use crate::entity::EntityRandom;
use crate::entity::components::{
    EffectType, Effects, Experience, GameMode, Health, HeldSlot, MainInventory, Player,
    PlayerSession, Position, Rotation, RuntimeEntityId, RuntimeId,
};
use crate::entity::definition::apply_pending_definitions;
use crate::entity::definition::attributes::ActorAttributes;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::definition::properties::ActorProperties;
use crate::entity::mob::combat::{self, fire};
use crate::entity::mob::components::KnockbackPower;
use crate::entity::mob::events::MobFacts;
use crate::entity::mob::net::network::add_actor_packet;
use crate::entity::mob::*;
use crate::server::game::{GameServer, ServerConfig};
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkManager, ChunkPosition, ChunkViewers};
use crate::world::rules::Difficulty;

const MIDNIGHT: i64 = 18_000;

struct Harness {
    server: GameServer,
    player: Entity,
    rx: mpsc::Receiver<McpePacket>,
}

/// A grass floor at y = 63 over chunks -1..=1 × -1..=1 at midnight, viewed
/// by one survival player standing on it at (8.5, 64, 0.5).
fn harness(seed: u64, difficulty: Difficulty) -> Harness {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let world = server.ecs.world_mut();
    world.insert_resource(EntityRandom::seeded(seed));
    *world.resource_mut::<Difficulty>() = difficulty;
    crate::world::clock::set_time(world, MIDNIGHT);
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
    Harness { server, player, rx }
}

fn flag(world: &World, mob: Entity, index: u32) -> bool {
    world.get::<ActorFlags>(mob).unwrap().get(index).unwrap()
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

    /// A mob of `identifier` at `feet` that is a baby exactly when `baby`.
    fn spawn(&mut self, identifier: &str, feet: DVec3, baby: bool) -> Entity {
        let world = self.world();
        for _ in 0..2000 {
            let mob = spawn_mob(world, identifier, feet, 0.0, SpawnCause::Spawned).unwrap();
            apply_pending_definitions(world);
            if flag(world, mob, actor_flags::BABY) == baby {
                return mob;
            }
            world.despawn(mob);
        }
        panic!("no {identifier} with baby = {baby} in 2000 spawns");
    }

    /// A mob of `identifier` spawned where the native summon stood.
    fn at_native(&mut self, identifier: &str, native: &AddActorPacket) -> Entity {
        let feet = DVec3::new(
            f64::from(native.position.x),
            f64::from(native.position.y),
            f64::from(native.position.z),
        );
        self.spawn(identifier, feet, false)
    }

    fn ticks(&mut self, n: u64) {
        for _ in 0..n {
            self.server.tick();
        }
    }
}

/// Our AddActor for `mob` equals the native summon's in every field the
/// spawn decides (type, attributes, actor data, links, synced properties).
fn assert_add_actor_matches(world: &World, identifier: &str, mob: Entity, native: &AddActorPacket) {
    let ours = add_actor_packet(
        identifier,
        world.get::<RuntimeId>(mob).unwrap().0,
        world.get::<Position>(mob).unwrap().0,
        DVec3::new(0.0, f64::from(native.velocity.y), 0.0),
        world.get::<Rotation>(mob).unwrap(),
        true,
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
        "{identifier}: native only: {missing:?}\nours only: {extra:?}"
    );
    assert_eq!(ours.actor_data, native.actor_data);
    assert_eq!(ours.actor_links, native.actor_links);
    assert_eq!(ours.synched_properties, native.synched_properties);
}

fn filter(json: serde_json::Value) -> Filter {
    let mut diagnostics = unastar_rules::Diagnostics::default();
    let filter = Filter::parse(
        &json,
        FilterRegistry::vanilla(),
        "filters",
        &mut diagnostics,
    )
    .expect("filter parses");
    assert!(diagnostics.items.is_empty(), "{:?}", diagnostics.items);
    filter
}

#[test]
fn vanilla_husk_wither_skeleton_endermite_and_zombie_pigman_compile_to_spawnable_archetypes() {
    let mut h = harness(1, Difficulty::Easy);
    let registry = h.world().resource::<MobRegistry>();
    for identifier in [
        "minecraft:husk",
        "minecraft:wither_skeleton",
        "minecraft:endermite",
        "minecraft:zombie_pigman",
        "minecraft:zoglin",
    ] {
        assert!(
            registry.get(identifier).is_some(),
            "{identifier} unsupported: {:?}",
            registry.unsupported_reason(identifier)
        );
    }
}

#[test]
fn the_armor_stand_is_refused_until_its_native_class_behaviour_exists() {
    let mut h = harness(18, Difficulty::Easy);
    let world = h.world();
    let reason = world
        .resource::<MobRegistry>()
        .unsupported("minecraft:armor_stand")
        .cloned();
    assert!(
        matches!(reason.as_deref(), Some(ArchetypeError::NativeClass(..))),
        "{reason:?}"
    );
}

#[test]
fn non_living_actors_are_refused_instead_of_spawned_as_mobs() {
    let mut h = harness(2, Difficulty::Easy);
    for identifier in [
        "minecraft:cushion",
        "minecraft:lightning_bolt",
        "minecraft:fireworks_rocket",
    ] {
        let world = h.world();
        let reason = world
            .resource::<MobRegistry>()
            .unsupported(identifier)
            .cloned();
        assert!(
            matches!(reason.as_deref(), Some(ArchetypeError::NotLiving(_))),
            "{identifier}: {reason:?}"
        );
        let spawned = spawn_mob(
            world,
            identifier,
            DVec3::new(0.5, 64.0, 0.5),
            0.0,
            SpawnCause::Spawned,
        );
        assert!(matches!(spawned, Err(MobSpawnError::Unsupported { .. })));
    }
}

#[test]
fn spawned_adult_husk_add_actor_matches_native_summon() {
    let mut h = harness(3, Difficulty::Easy);
    let native = native(include_bytes!(
        "../../../../../bds-extractor/captures/1.26.51/operator/summons/husk.bin"
    ));
    let husk = h.at_native("minecraft:husk", &native);
    assert_add_actor_matches(h.world(), "minecraft:husk", husk, &native);
}

#[test]
fn spawned_wither_skeleton_add_actor_matches_native_summon() {
    let mut h = harness(4, Difficulty::Easy);
    let native = native(include_bytes!(
        "../../../../../bds-extractor/captures/1.26.51/operator/summons/wither_skeleton.bin"
    ));
    let skeleton = h.at_native("minecraft:wither_skeleton", &native);
    assert_add_actor_matches(h.world(), "minecraft:wither_skeleton", skeleton, &native);
}

#[test]
fn spawned_endermite_add_actor_matches_native_summon() {
    let mut h = harness(5, Difficulty::Easy);
    let native = native(include_bytes!(
        "../../../../../bds-extractor/captures/1.26.51/operator/summons/endermite.bin"
    ));
    let endermite = h.at_native("minecraft:endermite", &native);
    assert_add_actor_matches(h.world(), "minecraft:endermite", endermite, &native);
}

#[test]
fn spawned_adult_zoglin_add_actor_matches_native_summon() {
    let mut h = harness(17, Difficulty::Easy);
    let native = native(include_bytes!(
        "../../../../../bds-extractor/captures/1.26.51/operator/summons/zoglin.bin"
    ));
    let zoglin = h.at_native("minecraft:zoglin", &native);
    assert_add_actor_matches(h.world(), "minecraft:zoglin", zoglin, &native);
}

#[test]
fn appearance_filters_read_the_ids_the_actor_data_carries() {
    let mut h = harness(6, Difficulty::Easy);
    let husk = h.spawn("minecraft:husk", DVec3::new(0.5, 64.0, 0.5), false);
    let wither = h.spawn(
        "minecraft:wither_skeleton",
        DVec3::new(-3.5, 64.0, 0.5),
        false,
    );
    let zombie = h.spawn("minecraft:zombie", DVec3::new(3.5, 64.0, 0.5), false);
    let world = h.world();
    let passes = |entity, other, json| {
        filter(json).evaluate(&MobFacts {
            world,
            entity,
            other,
        })
    };
    // The husk's `minecraft:variant` value, the wither skeleton's native
    // kind and the zombie's constructor value.
    let husk_variant = json!({"test": "is_variant", "value": 2});
    assert!(passes(husk, None, husk_variant.clone()));
    assert!(!passes(zombie, None, husk_variant));
    assert!(passes(
        wither,
        None,
        json!({"test": "is_variant", "value": 1})
    ));
    assert!(passes(
        zombie,
        None,
        json!({"test": "is_variant", "value": 0})
    ));
    assert!(passes(
        husk,
        None,
        json!({"test": "is_mark_variant", "value": 0})
    ));
    assert!(passes(
        husk,
        None,
        json!({"test": "is_skin_id", "value": 0})
    ));
    assert!(passes(
        zombie,
        Some(husk),
        json!({"subject": "other", "test": "is_variant", "operator": ">", "value": 1})
    ));
    // Without the other actor the test is false, whatever the operator.
    assert!(!passes(
        zombie,
        None,
        json!({"subject": "other", "test": "is_variant", "operator": "!=", "value": 7})
    ));
    // `has_component` also answers for the other actor.
    assert!(passes(
        zombie,
        Some(wither),
        json!({"subject": "other", "test": "has_component", "value": "minecraft:fire_immune"})
    ));
    assert!(!passes(
        wither,
        Some(zombie),
        json!({"subject": "other", "test": "has_component", "value": "minecraft:fire_immune"})
    ));
}

#[test]
fn wither_skeletons_never_catch_fire() {
    let mut h = harness(7, Difficulty::Easy);
    let zombie = h.spawn("minecraft:zombie", DVec3::new(3.5, 64.0, 0.5), false);
    for identifier in [
        "minecraft:wither_skeleton",
        "minecraft:zombie_pigman",
        "minecraft:zoglin",
    ] {
        let immune = h.spawn(identifier, DVec3::new(0.5, 64.0, 0.5), false);
        let world = h.world();
        assert!(
            flag(world, immune, actor_flags::FIRE_IMMUNE),
            "{identifier}"
        );
        fire::ignite(world, immune, 160);
        assert!(!fire::is_burning(world, immune), "{identifier}");
        assert!(!flag(world, immune, actor_flags::ONFIRE), "{identifier}");
        world.despawn(immune);
    }
    let world = h.world();
    assert!(!flag(world, zombie, actor_flags::FIRE_IMMUNE));
    fire::ignite(world, zombie, 160);
    assert!(fire::is_burning(world, zombie));
}

/// A husk or wither skeleton hit gives the player its attack effect with the
/// amplifier and duration BDS sends for that mob and difficulty
/// (`testdata/bds-1.26.51-attack-effects.json`): the authored duration at
/// every observed difficulty.
#[test]
fn husk_and_wither_skeleton_hits_apply_their_attack_effects() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../testdata/bds-1.26.51-attack-effects.json")).unwrap();
    let observations = fixture["observations"].as_array().unwrap();
    assert_eq!(observations.len(), 5);
    for row in observations {
        let identifier = row["mob"].as_str().unwrap();
        let difficulty = match row["difficulty"].as_str().unwrap() {
            "easy" => Difficulty::Easy,
            "normal" => Difficulty::Normal,
            "hard" => Difficulty::Hard,
            other => panic!("difficulty {other}"),
        };
        let effect = EffectType(row["effect_id"].as_u64().unwrap() as u8);
        let duration = row["duration_ticks"].as_u64().unwrap() as u32;
        let mut h = harness(8, difficulty);
        let mob = h.spawn(identifier, DVec3::new(5.5, 64.0, 0.5), false);
        let player = h.player;
        let start = h.world().get::<Health>(player).unwrap().current;
        let mut hit = false;
        for _ in 0..400 {
            h.ticks(1);
            if h.world().get::<Health>(player).unwrap().current < start {
                hit = true;
                break;
            }
        }
        assert!(hit, "{identifier} on {difficulty:?} never hit the player");
        let world = h.world();
        let applied = world
            .get::<Effects>(player)
            .and_then(|effects| effects.get(effect).cloned())
            .unwrap_or_else(|| panic!("{identifier}: no {effect:?}"));
        assert_eq!(
            u64::from(applied.level),
            row["amplifier"].as_u64().unwrap(),
            "{identifier} on {difficulty:?}"
        );
        // Read at the end of the hit tick: at most one effect tick has run.
        assert!(
            (duration - 1..=duration).contains(&applied.duration_ticks),
            "{identifier} on {difficulty:?}: {} ticks, BDS {duration}",
            applied.duration_ticks
        );
        assert!(world.get::<Health>(mob).is_some());
    }
}

/// One hit of `damage` with `cause` and no attacker; the health removed.
fn hurt(world: &mut World, mob: Entity, cause: ActorDamageCause, damage: f32) -> f32 {
    combat::hurt_mob(
        world,
        mob,
        combat::Hit {
            damage,
            cause,
            attacker: None,
            by_player: false,
            knockback_from: None,
            knockback_power: KnockbackPower::DEFAULT,
        },
    )
}

/// Every `/damage` cause against the fire-immune and control mobs of the
/// BDS capture: a cause the native fire-immune mobs ignore while their
/// controls take it is ignored by our wither skeleton and zombie pigman and
/// taken by our zombie; every other cause hurts all three alike.
#[test]
fn fire_immune_mobs_ignore_exactly_the_fire_causes_bds_ignores() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../testdata/bds-1.26.51-fire-immune-causes.json"
    ))
    .unwrap();
    let outcome = |row: &serde_json::Value, mob: &str| row[mob].as_bool().unwrap();
    let mut h = harness(9, Difficulty::Easy);
    let mut ignored = Vec::new();
    let mut fails_everywhere = Vec::new();
    for row in fixture["causes"].as_array().unwrap() {
        let name = row["cause"].as_str().unwrap();
        let cause: ActorDamageCause = serde_json::from_value(json!(name)).unwrap();
        let controls = outcome(row, "minecraft:zombie") && outcome(row, "minecraft:skeleton");
        let immune_hurt = outcome(row, "minecraft:zombie_pigman");
        assert_eq!(
            immune_hurt,
            outcome(row, "minecraft:wither_skeleton"),
            "{name}"
        );
        let wither = h.spawn(
            "minecraft:wither_skeleton",
            DVec3::new(0.5, 64.0, 0.5),
            false,
        );
        let pigman = h.spawn(
            "minecraft:zombie_pigman",
            DVec3::new(-3.5, 64.0, 0.5),
            false,
        );
        let zombie = h.spawn("minecraft:zombie", DVec3::new(3.5, 64.0, 0.5), false);
        let world = h.world();
        let (ours_wither, ours_pigman, ours_control) = (
            hurt(world, wither, cause, 1.0),
            hurt(world, pigman, cause, 1.0),
            hurt(world, zombie, cause, 1.0),
        );
        if controls && !immune_hurt {
            ignored.push(name);
            assert_eq!(ours_wither, 0.0, "{name}: the wither skeleton was hurt");
            assert_eq!(ours_pigman, 0.0, "{name}: the zombie pigman was hurt");
            assert_eq!(ours_control, 1.0, "{name}: the zombie was not hurt");
        } else if controls {
            assert_eq!(
                (ours_wither, ours_pigman, ours_control),
                (1.0, 1.0, 1.0),
                "{name}"
            );
        } else {
            // Fails on every mob, fire-immune or not: not about fire.
            fails_everywhere.push(name);
            assert_eq!(ours_wither, ours_control, "{name}");
            assert_eq!(ours_pigman, ours_control, "{name}");
        }
        world.despawn(wither);
        world.despawn(pigman);
        world.despawn(zombie);
    }
    assert_eq!(ignored, ["lava", "fire", "magma", "fire_tick"]);
    assert_eq!(fails_everywhere, ["suffocation"]);
}

/// A mob's target reaches viewers as the `TARGET` actor data: the target's
/// actor ID while it targets a survival player, 0 once the player turns
/// creative and is dropped (the sequence the BDS 1.26.51 husk capture shows).
#[test]
fn the_target_is_replicated_as_target_actor_data() {
    use jolyne::valentine::actor_data_i_ds as ids;
    use jolyne::valentine::types::DataItemEntryPayload;
    let mut h = harness(10, Difficulty::Normal);
    let husk = h.spawn("minecraft:husk", DVec3::new(4.5, 64.0, 0.5), false);
    let target = |world: &World| match world.get::<EntityMetadata>(husk).unwrap().get(ids::TARGET) {
        Some(DataItemEntryPayload::DataItemInt64Payload(long)) => long.value,
        other => panic!("TARGET {other:?}"),
    };
    assert_eq!(target(h.world()), 0);
    h.ticks(5);
    let player = h.player;
    let player_id = h.world().get::<RuntimeEntityId>(player).unwrap().0;
    assert_eq!(target(h.world()), player_id);
    *h.world().get_mut::<GameMode>(player).unwrap() = GameMode::Creative;
    h.ticks(2);
    assert_eq!(target(h.world()), 0);
}

/// `behavior.hurt_by_target`: a zombie, husk, wither skeleton or zoglin,
/// ignoring the creative player, turns on the cow that hurt it. The zombie
/// and husk list `entity_types` (anything but a breeze); the wither skeleton
/// and the zoglin list none, the published default, which retaliates
/// against any attacker.
#[test]
fn hurt_mobs_target_the_mob_that_hurt_them() {
    use crate::entity::mob::state::AttackTarget;
    // The zoglin attacks cows on sight within 16 blocks: its cow stands 20
    // blocks away, which retaliation without `entity_types` still reaches.
    for (identifier, cow_x) in [
        ("minecraft:wither_skeleton", 3.5),
        ("minecraft:zombie", 3.5),
        ("minecraft:husk", 3.5),
        ("minecraft:zoglin", 20.5),
        ("minecraft:zombie_villager_v2", 3.5),
    ] {
        let mut h = harness(11, Difficulty::Normal);
        let player = h.player;
        *h.world().get_mut::<GameMode>(player).unwrap() = GameMode::Creative;
        let mob = h.spawn(identifier, DVec3::new(0.5, 64.0, 0.5), false);
        h.world()
            .entity_mut(mob)
            .remove::<crate::entity::mob::components::RandomStrollGoal>();
        let cow = h.spawn("minecraft:cow", DVec3::new(cow_x, 64.0, 0.5), false);
        h.world()
            .entity_mut(cow)
            .remove::<crate::entity::mob::components::RandomStrollGoal>();
        h.ticks(5);
        assert!(h.world().get::<AttackTarget>(mob).is_none(), "{identifier}");
        let applied = combat::hurt_mob(
            h.world(),
            mob,
            combat::Hit {
                damage: 1.0,
                cause: ActorDamageCause::EntityAttack,
                attacker: Some(cow),
                by_player: false,
                knockback_from: Some(DVec3::new(cow_x, 64.0, 0.5)),
                knockback_power: KnockbackPower::DEFAULT,
            },
        );
        assert_eq!(applied, 1.0, "{identifier}");
        h.ticks(1);
        assert_eq!(
            h.world().get::<AttackTarget>(mob),
            Some(&AttackTarget(cow)),
            "{identifier}"
        );
        h.ticks(1);
        // Beyond its follow range (16) the zoglin lets the cow go again on
        // the next tick, as the BDS pigman 40 blocks away did.
        let kept = h.world().get::<AttackTarget>(mob) == Some(&AttackTarget(cow));
        assert_eq!(kept, cow_x < 16.0, "{identifier}");
    }
}

#[test]
fn spawned_baby_zombie_pigman_add_actor_matches_native_summon() {
    let mut h = harness(12, Difficulty::Easy);
    let native = native(include_bytes!(
        "../../../../../bds-extractor/captures/1.26.51/operator/summons/zombie_pigman.bin"
    ));
    let feet = DVec3::new(
        f64::from(native.position.x),
        f64::from(native.position.y),
        f64::from(native.position.z),
    );
    // The native summon drew a baby.
    let pigman = h.spawn("minecraft:zombie_pigman", feet, true);
    assert_add_actor_matches(h.world(), "minecraft:zombie_pigman", pigman, &native);
}

/// The calm run of `testdata/bds-1.26.51-pigman-anger.json`: two angry
/// pigmen fight the player, who then turns creative so both drop it; each
/// calms (ANGRY cleared) one anger span after its own last swing, not after
/// the anger started or the target was dropped.
#[test]
fn an_angry_zombie_pigman_calms_one_span_after_its_last_swing() {
    use jolyne::valentine::McpePacketData;
    use jolyne::valentine::types::EnumsActorEvent;
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../testdata/bds-1.26.51-pigman-anger.json")).unwrap();
    let span = fixture["calm"]["duration_seconds"].as_u64().unwrap() * 20;
    let mut h = harness(14, Difficulty::Normal);
    let player = h.player;
    let feet = DVec3::new(8.5, 64.0, 12.5);
    h.world().get_mut::<Position>(player).unwrap().0 =
        feet + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT;
    *h.world().get_mut::<Health>(player).unwrap() = Health {
        current: 10_000.0,
        max: 10_000.0,
    };
    let a = h.spawn(
        "minecraft:zombie_pigman",
        feet + DVec3::new(4.0, 0.0, 0.0),
        false,
    );
    let b = h.spawn(
        "minecraft:zombie_pigman",
        feet + DVec3::new(4.0, 0.0, 6.0),
        false,
    );
    let ids = [a, b].map(|mob| h.world().get::<RuntimeId>(mob).unwrap().0);
    combat::hurt_mob(
        h.world(),
        a,
        combat::Hit {
            damage: 1.0,
            cause: ActorDamageCause::EntityAttack,
            attacker: Some(player),
            by_player: true,
            knockback_from: Some(feet + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT),
            knockback_power: KnockbackPower::DEFAULT,
        },
    );
    let mut last_swing = [None::<u64>; 2];
    let mut calm = [None::<u64>; 2];
    for tick in 0..(90 + span + 40) {
        if tick == 90 {
            // As in the capture: the player turns creative after 4.5 s.
            *h.world().get_mut::<GameMode>(player).unwrap() = GameMode::Creative;
        }
        h.ticks(1);
        let now = h.world().resource::<crate::ecs::TickCounter>().current;
        while let Ok(packet) = h.rx.try_recv() {
            if let McpePacketData::ActorEventPacket(event) = packet.data
                && event.event_id == EnumsActorEvent::StartAttacking
                && let Some(index) = ids
                    .iter()
                    .position(|id| *id as u64 == event.target_runtime_id.actor_runtime_id)
            {
                last_swing[index] = Some(now);
            }
        }
        for (index, mob) in [a, b].into_iter().enumerate() {
            if calm[index].is_none()
                && last_swing[index].is_some()
                && !flag(h.world(), mob, actor_flags::ANGRY)
            {
                calm[index] = Some(now);
            }
        }
    }
    for index in 0..2 {
        let swing = last_swing[index].expect("the pigman swung at the player");
        let calmed = calm[index].expect("the pigman calmed");
        assert!(
            swing < 90 + 5,
            "pigman {index} swung after the player turned creative"
        );
        // BDS: 24.95 s after the swing (packet timestamps, ±1 tick): the
        // span counts down from the swing's own tick.
        assert!(
            (span - 2..=span).contains(&(calmed - swing)),
            "pigman {index} calmed {} ticks after its last swing",
            calmed - swing
        );
    }
}

/// The zombie pigman's spawn randomizer can draw `spawn_baby_jockey`, an
/// event its definition never declares: like BDS
/// (`testdata/bds-1.26.51-pigman-spawn-branches.json`), that pigman spawns
/// with no component group and so no family, at about the branch's weight.
#[test]
fn a_zombie_pigman_drawing_the_undeclared_jockey_branch_spawns_without_groups() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../testdata/bds-1.26.51-pigman-spawn-branches.json"
    ))
    .unwrap();
    let counts = &fixture["counts"];
    let total = counts["total"].as_u64().unwrap();
    let mut h = harness(15, Difficulty::Easy);
    let world = h.world();
    let (mut none, mut babies, mut adults) = (0u64, 0u64, 0u64);
    for _ in 0..total {
        let mob = spawn_mob(
            world,
            "minecraft:zombie_pigman",
            DVec3::new(0.5, 64.0, 0.5),
            0.0,
            SpawnCause::Spawned,
        )
        .unwrap();
        apply_pending_definitions(world);
        let family = |name| crate::entity::mob::events::is_family(world, mob, name);
        if !family("monster") {
            none += 1;
            assert!(!family("zombie_pigman") && !family("baby_zombie_pigman"));
        } else if family("baby_zombie_pigman") {
            babies += 1;
        } else {
            assert!(family("zombie_pigman"));
            adults += 1;
        }
        world.despawn(mob);
    }
    assert_eq!(none + babies + adults, total);
    // Two binomial draws of the same weights: allow four standard
    // deviations of their difference.
    for (ours, key) in [
        (none, "without_family_monster"),
        (babies, "family_baby_zombie_pigman"),
        (adults, "family_zombie_pigman"),
    ] {
        let bds = counts[key].as_u64().unwrap() as f64;
        let p = bds / total as f64;
        let band = 4.0 * (2.0 * total as f64 * p * (1.0 - p)).sqrt();
        assert!(
            (ours as f64 - bds).abs() <= band.max(4.0),
            "{key}: ours {ours}, BDS {bds}"
        );
    }
    assert!(none > 0, "the undeclared branch never drawn");
}

/// `/summon minecraft:zombie_villager` creates the actor BDS emits for it,
/// a `zombie_villager_v2`, whose AddActor equals the native summon's once
/// the spawn draws the profession (`VARIANT` 0) and skin (`SKIN_ID` 0) the
/// native one drew.
#[test]
fn summoned_zombie_villager_is_a_v2_whose_add_actor_matches_native_summon() {
    let mut h = harness(19, Difficulty::Easy);
    let native = native(include_bytes!(
        "../../../../../bds-extractor/captures/1.26.51/operator/summons/zombie_villager.bin"
    ));
    assert_eq!(&*native.actor_type, "minecraft:zombie_villager_v2");
    let feet = DVec3::new(
        f64::from(native.position.x),
        f64::from(native.position.y),
        f64::from(native.position.z),
    );
    let world = h.world();
    let drawn = |world: &World, mob: Entity, id: u32| match world
        .get::<EntityMetadata>(mob)
        .and_then(|metadata| metadata.get(id))
    {
        Some(jolyne::valentine::types::DataItemEntryPayload::DataItemIntPayload(item)) => {
            Some(item.value)
        }
        _ => None,
    };
    let mob = (0..4000)
        .find_map(|_| {
            let mob = spawn_mob(
                world,
                "minecraft:zombie_villager",
                feet,
                0.0,
                SpawnCause::Spawned,
            )
            .unwrap();
            apply_pending_definitions(world);
            assert_eq!(
                &*world.get::<MobKind>(mob).unwrap().0.identifier,
                "minecraft:zombie_villager_v2"
            );
            let native_like = !flag(world, mob, actor_flags::BABY)
                && drawn(world, mob, jolyne::valentine::actor_data_i_ds::VARIANT) == Some(0)
                && drawn(world, mob, jolyne::valentine::actor_data_i_ds::SKIN_ID) == Some(0);
            if native_like {
                Some(mob)
            } else {
                world.despawn(mob);
                None
            }
        })
        .expect("an adult unskilled zombie villager with skin 0");
    assert_add_actor_matches(h.world(), "minecraft:zombie_villager_v2", mob, &native);
}

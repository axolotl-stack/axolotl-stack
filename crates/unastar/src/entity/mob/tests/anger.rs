//! Anger (`minecraft:angry`) replayed against the BDS 1.26.51 anger captures
//! (`testdata/bds-1.26.51-anger-chain.json`): zombie pigmen pinned in
//! barrier cells so their distances hold, a chain that passes the anger on
//! from receiver to receiver, the calm that drops every target, retaliation
//! at any distance, and the warning sound of a mob that takes a target.

use std::collections::BTreeMap;

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::{McpePacket, McpePacketData, actor_flags};
use tokio::sync::mpsc;
use unastar_data::actor_components::body::ActorDamageCause;

use crate::entity::EntityRandom;
use crate::entity::components::{
    Experience, GameMode, Health, HeldSlot, MainInventory, Player, PlayerSession, Position,
    Rotation, RuntimeEntityId, RuntimeId,
};
use crate::entity::definition::apply_pending_definitions;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::mob::components::KnockbackPower;
use crate::entity::mob::state::AttackTarget;
use crate::entity::mob::*;
use crate::registry::BlockRegistry;
use crate::server::game::{GameServer, ServerConfig};
use crate::world::chunk::Chunk;
use crate::world::ecs::{ChunkData, ChunkManager, ChunkPosition, ChunkViewers};
use crate::world::rules::Difficulty;

/// The player's feet; the fixture's offsets are from here.
const PLAYER_FEET: DVec3 = DVec3::new(0.5, 64.0, 0.5);

fn fixture() -> serde_json::Value {
    serde_json::from_str(include_str!("../testdata/bds-1.26.51-anger-chain.json")).unwrap()
}

fn offset(row: &serde_json::Value) -> DVec3 {
    let v: Vec<f64> = serde_json::from_value(row["offset"].clone()).unwrap();
    DVec3::new(v[0], v[1], v[2])
}

struct Arena {
    server: GameServer,
    player: Entity,
    rx: mpsc::Receiver<McpePacket>,
}

/// A grass floor whose top is y = 64 over chunks -4..=4 × -4..=4 at
/// midnight on Normal, viewed by one survival player at [`PLAYER_FEET`]
/// who cannot be killed.
fn arena(seed: u64) -> Arena {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let world = server.ecs.world_mut();
    world.insert_resource(EntityRandom::seeded(seed));
    *world.resource_mut::<Difficulty>() = Difficulty::Normal;
    crate::world::clock::set_time(world, 18_000);
    let (tx, rx) = mpsc::channel(1 << 16);
    let player = world
        .spawn((
            Player,
            Position(PLAYER_FEET + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT),
            Rotation::default(),
            GameMode::Survival,
            MainInventory::default(),
            HeldSlot(0),
            Health {
                current: 10_000.0,
                max: 10_000.0,
            },
            Experience::default(),
            RuntimeEntityId(1),
            PlayerSession::new(1, "steve".into(), None, None, tx),
        ))
        .id();
    let grass = BlockRegistry::vanilla()
        .default_state_id_by_name("minecraft:grass_block")
        .unwrap();
    for cx in -4..=4 {
        for cz in -4..=4 {
            let mut column = Chunk::new(cx, cz);
            for x in 0..16 {
                for z in 0..16 {
                    column.set_block(x, 63, z, grass);
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
    Arena { server, player, rx }
}

impl Arena {
    fn world(&mut self) -> &mut World {
        self.server.ecs.world_mut()
    }

    fn set_block(&mut self, pos: IVec3, name: &str) {
        let state = BlockRegistry::vanilla()
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

    /// An adult zombie pigman (with its component groups) standing in a
    /// two-high barrier cell at `feet`, so it cannot move.
    fn pinned_pigman(&mut self, feet: DVec3) -> Entity {
        let cell = feet.floor().as_ivec3();
        for dx in -1..=1 {
            for dz in -1..=1 {
                for dy in 0..=2 {
                    if (dx, dz) != (0, 0) || dy == 2 {
                        self.set_block(cell + IVec3::new(dx, dy, dz), "minecraft:barrier");
                    }
                }
            }
        }
        let world = self.world();
        for _ in 0..2000 {
            let mob = spawn_mob(
                world,
                "minecraft:zombie_pigman",
                feet,
                0.0,
                SpawnCause::Spawned,
            )
            .unwrap();
            apply_pending_definitions(world);
            let baby = world
                .get::<ActorFlags>(mob)
                .unwrap()
                .get(actor_flags::BABY)
                .unwrap();
            if !baby && crate::entity::mob::events::is_family(world, mob, "zombie_pigman") {
                return mob;
            }
            world.despawn(mob);
        }
        panic!("no adult zombie pigman");
    }

    /// The player hits `mob` once (as `/damage <mob> 1 entity_attack entity
    /// @s` does in the capture).
    fn hit(&mut self, mob: Entity) {
        let player = self.player;
        let eye = PLAYER_FEET + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT;
        let dealt = combat::hurt_mob(
            self.world(),
            mob,
            combat::Hit {
                damage: 1.0,
                cause: ActorDamageCause::EntityAttack,
                attacker: Some(player),
                by_player: true,
                knockback_from: Some(eye),
                knockback_power: KnockbackPower::DEFAULT,
            },
        );
        assert_eq!(dealt, 1.0);
    }
}

/// What a viewer saw of one mob, by tick.
#[derive(Debug, Default, Clone)]
struct Seen {
    target_tick: Option<u64>,
    target_dropped_tick: Option<u64>,
    angry_tick: Option<u64>,
    calm_tick: Option<u64>,
    warning: Option<(u64, DVec3)>,
}

/// Tick `ticks` times, recording for each mob the first tick its `TARGET`
/// actor data named someone, the first tick after that it went back to 0,
/// the ticks `ANGRY` rose and fell, and its first `mob.warning` sound.
fn watch(a: &mut Arena, mobs: &[(String, Entity)], ticks: u64) -> BTreeMap<String, Seen> {
    use jolyne::valentine::actor_data_i_ds as ids;
    use jolyne::valentine::types::DataItemEntryPayload;
    let mut seen: BTreeMap<String, Seen> = BTreeMap::new();
    let ids_by_runtime: BTreeMap<i64, String> = mobs
        .iter()
        .map(|(name, mob)| (a.world().get::<RuntimeId>(*mob).unwrap().0, name.clone()))
        .collect();
    while a.rx.try_recv().is_ok() {}
    for tick in 1..=ticks {
        a.server.tick();
        while let Ok(packet) = a.rx.try_recv() {
            if let McpePacketData::LevelSoundEventPacket(sound) = packet.data
                && &*sound.sound_event == "mob.warning"
                && let Some(name) = ids_by_runtime.get(&sound.actor_unique_id)
            {
                let entry = seen.entry(name.clone()).or_default();
                if entry.warning.is_none() {
                    let p = sound.position;
                    entry.warning = Some((
                        tick,
                        DVec3::new(f64::from(p.x), f64::from(p.y), f64::from(p.z)),
                    ));
                }
            }
        }
        for (name, mob) in mobs {
            let world = a.world();
            let Some(metadata) = world.get::<EntityMetadata>(*mob) else {
                continue;
            };
            let target = match metadata.get(ids::TARGET) {
                Some(DataItemEntryPayload::DataItemInt64Payload(long)) => long.value,
                _ => 0,
            };
            let angry = world
                .get::<ActorFlags>(*mob)
                .unwrap()
                .get(actor_flags::ANGRY)
                .unwrap();
            let entry = seen.entry(name.clone()).or_default();
            if target != 0 && entry.target_tick.is_none() {
                entry.target_tick = Some(tick);
            }
            if target == 0 && entry.target_tick.is_some() && entry.target_dropped_tick.is_none() {
                entry.target_dropped_tick = Some(tick);
            }
            if angry && entry.angry_tick.is_none() {
                entry.angry_tick = Some(tick);
            }
            if !angry && entry.angry_tick.is_some() && entry.calm_tick.is_none() {
                entry.calm_tick = Some(tick);
            }
        }
    }
    seen
}

fn tick_of(row: &serde_json::Value, key: &str) -> Option<u64> {
    row[key].as_u64()
}

/// The chain of `testdata/bds-1.26.51-anger-chain.json`: four pinned
/// pigmen 15 blocks apart, the first one hit. Each takes the attacker as
/// its target one tick after the previous one turned angry (the anger
/// spreads again from every receiver), turns angry and plays `mob.warning`
/// at its head one tick after its own target, and calms 499 ticks after
/// turning angry, dropping its target, the hit pigman too: every tick as in
/// BDS.
#[test]
fn anger_passes_from_pigman_to_pigman_and_every_calm_drops_the_target() {
    let fixture = fixture();
    let mut a = arena(1);
    let mut mobs = Vec::new();
    for row in fixture["chain"].as_array().unwrap() {
        let mob = a.pinned_pigman(PLAYER_FEET + offset(row));
        mobs.push((row["name"].as_str().unwrap().to_owned(), mob));
    }
    a.server.tick();
    a.hit(mobs[0].1);
    let last = fixture["chain"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| tick_of(row, "calm_tick"))
        .max()
        .unwrap();
    let seen = watch(&mut a, &mobs, last + 5);
    for row in fixture["chain"].as_array().unwrap() {
        let name = row["name"].as_str().unwrap();
        let ours = &seen[name];
        assert_eq!(
            ours.target_tick,
            tick_of(row, "target_tick"),
            "{name} target"
        );
        assert_eq!(ours.angry_tick, tick_of(row, "angry_tick"), "{name} angry");
        assert_eq!(ours.calm_tick, tick_of(row, "calm_tick"), "{name} calm");
        assert_eq!(
            ours.target_dropped_tick,
            tick_of(row, "target_dropped_tick"),
            "{name} drop"
        );
        let (tick, position) = ours.warning.expect("a warning sound");
        assert_eq!(Some(tick), tick_of(row, "warning_tick"), "{name} warning");
        let head: Vec<f64> =
            serde_json::from_value(row["warning_position_offset"].clone()).unwrap();
        let want = PLAYER_FEET + DVec3::new(head[0], head[1], head[2]);
        assert!(
            (position - want).length() < 1e-3,
            "{name} warning at {position}, BDS {want}"
        );
    }
    for (name, mob) in &mobs {
        assert!(a.world().get::<AttackTarget>(*mob).is_none(), "{name}");
    }
}

/// The retaliation rows of the fixture: pinned pigmen 12 and 40 blocks away
/// both take the player who hit them as their target on the next tick
/// (`behavior.hurt_by_target` without `entity_types` has no distance
/// limit); the far one loses it again on the tick it turns angry, beyond its
/// follow range, and stays angry without taking a target again.
#[test]
fn hit_pigmen_retaliate_at_any_distance_and_lose_targets_beyond_their_follow_range() {
    let fixture = fixture();
    for row in fixture["retaliation"].as_array().unwrap() {
        let name = row["name"].as_str().unwrap();
        // G (24 blocks) kept its target in BDS: zombie-class mobs spawn with
        // a random follow range bonus that is not modelled (see the parity
        // gaps), so only the rows inside and beyond every bonus are replayed.
        if name == "G" {
            continue;
        }
        let mut a = arena(2);
        let mob = a.pinned_pigman(PLAYER_FEET + offset(row));
        a.server.tick();
        a.hit(mob);
        let mobs = [(name.to_owned(), mob)];
        let seen = watch(&mut a, &mobs, 60);
        let ours = &seen[name];
        assert_eq!(
            ours.target_tick,
            tick_of(row, "target_tick"),
            "{name} target"
        );
        assert_eq!(ours.angry_tick, tick_of(row, "angry_tick"), "{name} angry");
        assert_eq!(
            ours.target_dropped_tick,
            tick_of(row, "target_dropped_tick"),
            "{name} drop"
        );
        assert!(ours.calm_tick.is_none(), "{name} stays angry");
    }
}

// ----- the anger fields no spawnable vanilla mob combines yet -----
//
// The vanilla zombie pigman with its angry group rewritten (and, for some
// tests, extra components), installed as the definition of
// `minecraft:zombie_pigman`. These follow the native per-tick anger order
// (`mob::life::anger`); the piglins, bees and wandering traders that author
// these fields are not spawnable yet.

const ANGRY_GROUP: &str = "minecraft:pig_zombie_angry";

/// Install the vanilla zombie pigman with `angry` as its angry group's
/// `minecraft:angry` and `extra` added to its base components.
fn install_pigman(a: &mut Arena, angry: serde_json::Value, extra: &[(&str, serde_json::Value)]) {
    use crate::entity::definition::properties::ActorPropertyRegistry;
    use crate::registry::entity::ActorDefinitionRegistryBuilder;
    let mut definition =
        (**unastar_data::actor_definitions::get("minecraft:zombie_pigman").unwrap()).clone();
    definition
        .component_groups
        .get_mut(ANGRY_GROUP)
        .unwrap()
        .insert("minecraft:angry".into(), angry);
    for (name, body) in extra {
        definition.components.insert((*name).into(), body.clone());
    }
    let mut builder = ActorDefinitionRegistryBuilder::default();
    builder.load_vanilla();
    builder.register(std::sync::Arc::new(definition));
    let definitions = builder.freeze();
    let properties = ActorPropertyRegistry::compile(&definitions).unwrap();
    let world = a.world();
    let items = std::sync::Arc::clone(
        &world
            .resource::<crate::server::game::types::ItemRegistryResource>()
            .0,
    );
    let mobs = MobRegistry::compile(&definitions, &properties, &items).unwrap();
    assert!(
        mobs.get("minecraft:zombie_pigman").is_some(),
        "{:?}",
        mobs.unsupported_reason("minecraft:zombie_pigman")
    );
    world.insert_resource(definitions);
    world.insert_resource(properties);
    world.insert_resource(mobs);
}

/// The vanilla pigman anger with `fields` merged in.
fn anger(fields: serde_json::Value) -> serde_json::Value {
    let mut base = serde_json::json!({
        "broadcast_anger": true,
        "broadcast_anger_when_dying": false,
        "broadcast_range": 20,
        "calm_event": {"event": "minecraft:on_calm", "target": "self"},
        "duration": 25
    });
    for (key, value) in fields.as_object().unwrap() {
        base[key] = value.clone();
    }
    base
}

fn angry(world: &World, mob: Entity) -> bool {
    world
        .get::<ActorFlags>(mob)
        .unwrap()
        .get(actor_flags::ANGRY)
        .unwrap()
}

fn target(world: &World, mob: Entity) -> Option<Entity> {
    world.get::<AttackTarget>(mob).map(|t| t.0)
}

fn in_group(world: &World, mob: Entity, group: &str) -> bool {
    world
        .get::<crate::entity::definition::DefinitionState>(mob)
        .unwrap()
        .group_history()
        .any(|(name, added)| name == group && added)
}

/// `filters`: an anger whose target fails them cannot hold. The hit pigman
/// takes the player as its target, then on its first angry tick drops it,
/// never shows `ANGRY` and fires its calm event (back in the calm group);
/// a pigman nearby is not provoked.
#[test]
fn an_anger_whose_target_fails_the_filters_ends_at_once() {
    let mut a = arena(3);
    install_pigman(
        &mut a,
        anger(serde_json::json!({
            "filters": {"test": "is_family", "subject": "other", "operator": "!=", "value": "player"}
        })),
        &[],
    );
    let hit = a.pinned_pigman(PLAYER_FEET + DVec3::new(4.0, 0.0, 0.0));
    let kin = a.pinned_pigman(PLAYER_FEET + DVec3::new(4.0, 0.0, 6.0));
    a.server.tick();
    a.hit(hit);
    a.server.tick();
    let player = a.player;
    assert_eq!(target(a.world(), hit), Some(player), "retaliates first");
    for _ in 0..5 {
        a.server.tick();
        let world = a.world();
        assert!(!angry(world, hit) && !angry(world, kin));
    }
    let world = a.world();
    assert_eq!(target(world, hit), None, "the target was dropped");
    assert_eq!(target(world, kin), None, "nothing spread");
    assert!(
        in_group(world, hit, "minecraft:pig_zombie_calm"),
        "calm event"
    );
    assert!(
        world
            .get::<crate::entity::mob::life::anger::Angry>(hit)
            .is_none()
    );
}

/// `broadcast_targets`: the anger spreads to mobs of the listed families
/// (here cows) instead of the angry mob's own kind.
#[test]
fn anger_spreads_to_the_broadcast_target_families_instead_of_kin() {
    let mut a = arena(4);
    install_pigman(
        &mut a,
        anger(serde_json::json!({"broadcast_targets": ["cow"]})),
        &[],
    );
    let hit = a.pinned_pigman(PLAYER_FEET + DVec3::new(4.0, 0.0, 0.0));
    let kin = a.pinned_pigman(PLAYER_FEET + DVec3::new(4.0, 0.0, 6.0));
    let world = a.world();
    let cow = spawn_mob(
        world,
        "minecraft:cow",
        PLAYER_FEET + DVec3::new(4.0, 0.0, -6.0),
        0.0,
        SpawnCause::Spawned,
    )
    .unwrap();
    a.server.tick();
    a.hit(hit);
    for _ in 0..4 {
        a.server.tick();
    }
    let player = a.player;
    let world = a.world();
    assert_eq!(target(world, hit), Some(player));
    assert_eq!(
        target(world, cow),
        Some(player),
        "the cow family received it"
    );
    assert_eq!(target(world, kin), None, "kin are not a broadcast target");
}

/// `broadcast_anger_on_being_attacked` without `broadcast_anger`: an angry
/// pigman spreads its anger on the ticks it is hit (and the next), every
/// time, and not in between.
#[test]
fn anger_spreads_again_each_time_the_angry_mob_is_hit() {
    let mut a = arena(5);
    install_pigman(
        &mut a,
        anger(serde_json::json!({
            "broadcast_anger": false,
            "broadcast_anger_on_being_attacked": true
        })),
        &[],
    );
    let hit = a.pinned_pigman(PLAYER_FEET + DVec3::new(4.0, 0.0, 0.0));
    a.server.tick();
    a.hit(hit);
    for _ in 0..14 {
        a.server.tick();
    }
    let player = a.player;
    assert_eq!(target(a.world(), hit), Some(player));
    assert!(angry(a.world(), hit));
    // Kin arriving after the hit is not provoked while nothing hits it.
    let kin = a.pinned_pigman(PLAYER_FEET + DVec3::new(4.0, 0.0, 6.0));
    for _ in 0..5 {
        a.server.tick();
    }
    assert_eq!(target(a.world(), kin), None, "no spread without a hit");
    // A second hit while angry spreads it (the hurt timer has run out).
    a.hit(hit);
    for _ in 0..2 {
        a.server.tick();
    }
    assert_eq!(target(a.world(), kin), Some(player), "spread on the hit");
}

/// `broadcast_anger_when_dying`: a pigman killed by the hit that spreads
/// its anger (`broadcast_anger_on_being_attacked`) still spreads it while
/// dying only when the field is true.
#[test]
fn a_dying_mob_spreads_its_anger_only_with_broadcast_anger_when_dying() {
    for when_dying in [true, false] {
        let mut a = arena(6);
        install_pigman(
            &mut a,
            anger(serde_json::json!({
                "broadcast_anger": false,
                "broadcast_anger_on_being_attacked": true,
                "broadcast_anger_when_dying": when_dying
            })),
            &[],
        );
        let hit = a.pinned_pigman(PLAYER_FEET + DVec3::new(4.0, 0.0, 0.0));
        a.server.tick();
        a.hit(hit);
        for _ in 0..14 {
            a.server.tick();
        }
        assert!(angry(a.world(), hit));
        let kin = a.pinned_pigman(PLAYER_FEET + DVec3::new(4.0, 0.0, 6.0));
        a.server.tick();
        a.world().get_mut::<Health>(hit).unwrap().current = 1.0;
        a.hit(hit);
        assert!(
            a.world()
                .get::<crate::entity::mob::state::Dead>(hit)
                .is_some()
        );
        for _ in 0..2 {
            a.server.tick();
        }
        let player = a.player;
        let spread = target(a.world(), kin) == Some(player);
        assert_eq!(
            spread, when_dying,
            "broadcast_anger_when_dying {when_dying}"
        );
    }
}

/// `on_friendly_anger`: a mob the anger spreads to fires it with the angry
/// mob as `other` (here an event that makes the receiver a baby).
#[test]
fn a_mob_the_anger_spreads_to_fires_on_friendly_anger() {
    let mut a = arena(7);
    install_pigman(
        &mut a,
        anger(serde_json::json!({})),
        &[(
            "minecraft:on_friendly_anger",
            serde_json::json!({"event": "minecraft:as_baby", "target": "self"}),
        )],
    );
    let hit = a.pinned_pigman(PLAYER_FEET + DVec3::new(4.0, 0.0, 0.0));
    let kin = a.pinned_pigman(PLAYER_FEET + DVec3::new(4.0, 0.0, 6.0));
    a.server.tick();
    a.hit(hit);
    for _ in 0..4 {
        a.server.tick();
    }
    let world = a.world();
    let baby = |mob| {
        world
            .get::<ActorFlags>(mob)
            .unwrap()
            .get(actor_flags::BABY)
            .unwrap()
    };
    assert!(baby(kin), "the receiver's on_friendly_anger fired");
    assert!(!baby(hit), "the angry mob's own anger does not fire it");
}

/// `on_target_escape` fires whenever the target is dropped: when the
/// target goal loses it beyond the follow range (the 40-block retaliation of
/// the fixture) and when the anger runs out (the event here makes the
/// pigman a baby).
#[test]
fn on_target_escape_fires_when_the_target_is_lost_or_the_anger_ends() {
    let escape = (
        "minecraft:on_target_escape",
        serde_json::json!({"event": "minecraft:as_baby", "target": "self"}),
    );
    let baby = |world: &World, mob: Entity| {
        world
            .get::<ActorFlags>(mob)
            .unwrap()
            .get(actor_flags::BABY)
            .unwrap()
    };
    // Beyond the follow range: lost on the tick after it was taken.
    let mut a = arena(8);
    install_pigman(
        &mut a,
        anger(serde_json::json!({})),
        std::slice::from_ref(&escape),
    );
    let far = a.pinned_pigman(PLAYER_FEET + DVec3::new(0.0, 0.0, -40.0));
    a.server.tick();
    a.hit(far);
    a.server.tick();
    assert!(target(a.world(), far).is_some());
    assert!(!baby(a.world(), far));
    for _ in 0..2 {
        a.server.tick();
    }
    assert!(target(a.world(), far).is_none());
    assert!(baby(a.world(), far), "escape fired on the lost target");

    // The calm drops the target the anger kept.
    let mut a = arena(9);
    install_pigman(
        &mut a,
        anger(serde_json::json!({})),
        std::slice::from_ref(&escape),
    );
    let near = a.pinned_pigman(PLAYER_FEET + DVec3::new(4.0, 0.0, 0.0));
    a.server.tick();
    a.hit(near);
    for _ in 0..10 {
        a.server.tick();
    }
    assert!(target(a.world(), near).is_some() && !baby(a.world(), near));
    for _ in 0..500 {
        a.server.tick();
    }
    assert!(target(a.world(), near).is_none());
    assert!(baby(a.world(), near), "escape fired at the calm");
}

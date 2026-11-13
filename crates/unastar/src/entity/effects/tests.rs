//! The effect system against BDS 1.26.51 (`testdata/bds-1.26.51-effects.json`:
//! `tools/bds-sweep/sweep.py effects --client-capture` runs distilled by
//! `effects_fixture.py`).
//!
//! Each mob item is replayed tick by tick: the same mob, without its brain
//! (BDS's stood in a barrier cell, or was held by a hidden Slowness 255),
//! where BDS spawned it, seen by a creative player, gets the same effects and
//! damage on the same ticks through the shared effect API. Every `MobEffect`,
//! visible-effects actor data, effect flag (`INVISIBLE`, `CAN_SHOW_NAME`,
//! `FIRE_IMMUNE`), `UpdateAttributes` entry and `ActorEvent` its viewer gets
//! must be BDS's, on the same tick, and a mob the effects move must stand
//! where BDS's stood. BDS's zombie took 0.984 of every `/damage` (this
//! server gives mobs no armour against it), so a zombie's health is compared
//! as its changes, outside the `/damage` ticks.
//!
//! The player items are replayed on a survival player the same way. BDS
//! steps a player's effects when it handles the player's input, so a
//! player's effect packets are compared within a tick of BDS's (and the
//! duration a `clear` reports within one); their health is not compared
//! (BDS's sweep player carried its health from item to item), only the
//! health boost's maximum, defaults and modifier and the movement attribute
//! speed and slowness modify.

use std::sync::OnceLock;

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::actor_flags;
use jolyne::valentine::{McpePacket, McpePacketData};
use serde_json::Value;
use tokio::sync::mpsc;
use unastar_data::actor_components::body::ActorDamageCause;

use super::*;
use crate::entity::EntityRandom;
use crate::entity::components::{
    Experience, GameMode, HeldSlot, MainInventory, Player, PlayerSession, Position, Rotation,
    RuntimeEntityId,
};
use crate::entity::definition::apply_pending_definitions;
use crate::entity::mob::ai::MobBrain;
use crate::entity::mob::{SpawnCause, spawn_mob};
use crate::server::game::{GameServer, ServerConfig};
use crate::world::ChunkManager;
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkPosition, ChunkViewers};
use crate::world::rules::{Difficulty, GameRules};

fn fixture() -> &'static Value {
    static FIXTURE: OnceLock<Value> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        serde_json::from_str(include_str!("testdata/bds-1.26.51-effects.json")).unwrap()
    })
}

fn item(key: &str) -> &'static Value {
    &fixture()["items"][key]
}

/// The flag bits effects set; BDS's other flag changes (a moving mob's
/// `MOVING`) are not compared.
const EFFECT_FLAGS: [u32; 3] = [
    actor_flags::INVISIBLE,
    actor_flags::CAN_SHOW_NAME,
    actor_flags::FIRE_IMMUNE,
];

fn effect_flags(word: i64) -> u64 {
    EFFECT_FLAGS
        .iter()
        .fold(0, |bits, flag| bits | (word as u64 & (1 << flag)))
}

/// One compared packet, as text (floats to four decimals).
fn attribute_text(
    name: &str,
    current: f64,
    max: f64,
    default: f64,
    default_max: f64,
    modifiers: &[(String, f64, i64, i64, String)],
) -> String {
    let modifiers: Vec<String> = modifiers
        .iter()
        .map(|(name, amount, operation, operand, id)| {
            format!("{name} {amount:.4} op{operation} on{operand} {id}")
        })
        .collect();
    // As `f32`, how both sides carry them (the unbounded maximum included).
    let f = |value: f64| value as f32;
    format!(
        "attribute {name} {:.4}/{} default {:.4}/{} [{}]",
        f(current),
        f(max),
        f(default),
        f(default_max),
        modifiers.join(", ")
    )
}

/// BDS's packets about the replayed actor (`about`), from tick 1 on.
fn expected(item: &Value, about: &str) -> Vec<(u64, String)> {
    let mut flags = None;
    let mut out = Vec::new();
    for packet in item["packets"].as_array().unwrap() {
        let t = packet["t"].as_u64().unwrap();
        if packet["about"] != about {
            continue;
        }
        let kind = packet["kind"].as_str().unwrap();
        match kind {
            "data" => {
                let data = packet["data"].as_object().unwrap();
                if let Some(word) = data.get("0").and_then(Value::as_i64) {
                    let bits = effect_flags(word);
                    if flags.is_some_and(|before| before != bits) && t > 0 {
                        out.push((t, format!("flags {bits:#x}")));
                    }
                    flags = Some(bits);
                }
                if let Some(visible) = data.get("131").and_then(Value::as_i64)
                    && t > 0
                {
                    out.push((t, format!("visible {visible}")));
                }
            }
            "effect" if t > 0 => out.push((
                t,
                format!(
                    "effect {} {} level {} for {} particles {}",
                    packet["event"].as_str().unwrap(),
                    packet["effect"],
                    packet["amplifier"],
                    packet["duration"],
                    packet["particles"]
                ),
            )),
            "attributes" if t > 0 => {
                for row in packet["attributes"].as_array().unwrap() {
                    let modifiers: Vec<(String, f64, i64, i64, String)> = row["modifiers"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|m| {
                            (
                                m["name"].as_str().unwrap().to_owned(),
                                m["amount"].as_f64().unwrap(),
                                m["operation"].as_i64().unwrap(),
                                m["operand"].as_i64().unwrap(),
                                m["id"].as_str().unwrap().to_owned(),
                            )
                        })
                        .collect();
                    out.push((
                        t,
                        attribute_text(
                            row["name"].as_str().unwrap(),
                            row["current"].as_f64().unwrap(),
                            row["max"].as_f64().unwrap(),
                            row["default"].as_f64().unwrap(),
                            row["default_max"].as_f64().unwrap(),
                            &modifiers,
                        ),
                    ));
                }
            }
            "event" if t > 0 => out.push((
                t,
                format!(
                    "event {} {}",
                    packet["event"].as_str().unwrap(),
                    packet["data"]
                ),
            )),
            _ => {}
        }
    }
    out
}

/// What the replay produced.
struct Replay {
    packets: Vec<(u64, String)>,
    /// Feet position and ground contact after each tick (index = sample).
    positions: Vec<(DVec3, bool)>,
}

/// A grass floor at y = -61 (the BDS superflat) over chunks -1..=1 ×
/// -1..=1 at midnight on easy, seen by a player standing at (0.5, -60,
/// 0.5): creative for a mob item, survival for a player item (which it is
/// the subject of).
fn server(survival: bool) -> (GameServer, Entity, mpsc::Receiver<McpePacket>) {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let world = server.ecs.world_mut();
    world.insert_resource(EntityRandom::seeded(11));
    *world.resource_mut::<Difficulty>() = Difficulty::Easy;
    world.resource_mut::<GameRules>().do_mob_spawning = false;
    crate::world::clock::set_time(world, 18000);
    let (tx, rx) = mpsc::channel::<McpePacket>(1 << 16);
    let player = world
        .spawn((
            Player,
            Position(DVec3::new(
                0.5,
                -60.0 + crate::entity::PLAYER_EYE_HEIGHT,
                0.5,
            )),
            Rotation::default(),
            if survival {
                GameMode::Survival
            } else {
                GameMode::Creative
            },
            MainInventory::default(),
            HeldSlot(0),
            Health::default(),
            Hunger::default(),
            Experience::default(),
            Effects::default(),
            RuntimeEntityId(1),
            PlayerSession::new(1, "viewer".into(), None, None, tx),
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

fn set_block(world: &mut World, [x, y, z]: [i32; 3], block: u32) {
    let (cx, cz) = crate::world::ecs::world_to_chunk_coords(x, z);
    let chunk = world
        .resource::<ChunkManager>()
        .get_by_coords(cx, cz)
        .unwrap();
    world.get_mut::<ChunkData>(chunk).unwrap().inner.set_block(
        x.rem_euclid(16) as u8,
        y as i16,
        z.rem_euclid(16) as u8,
        block,
    );
}

/// `fill x y z x y z barrier hollow` (the item's barrier cell).
fn fill(world: &mut World, command: &str) {
    let parts: Vec<&str> = command.split_whitespace().collect();
    let n = |i: usize| parts[i].parse::<i32>().unwrap();
    let (from, to) = ([n(1), n(2), n(3)], [n(4), n(5), n(6)]);
    assert_eq!(&parts[7..], ["barrier", "hollow"], "{command}");
    let barrier = crate::registry::BlockRegistry::vanilla()
        .default_state_id_by_name("minecraft:barrier")
        .unwrap();
    for x in from[0]..=to[0] {
        for y in from[1]..=to[1] {
            for z in from[2]..=to[2] {
                let shell = [x == from[0] || x == to[0], y == from[1] || y == to[1]]
                    .into_iter()
                    .chain([z == from[2] || z == to[2]])
                    .any(|edge| edge);
                set_block(world, [x, y, z], if shell { barrier } else { *blocks::AIR });
            }
        }
    }
}

/// Run one of the item's commands on `subject` through the shared effect
/// API: `effect <target> clear`, `effect <target> <name> <seconds> <level>
/// [hide particles]` (an instant effect lasts the seconds as ticks), and
/// `damage <target> <amount> [cause]` (no attacker).
fn run(world: &mut World, subject: Entity, command: &str) {
    let parts: Vec<&str> = command.split_whitespace().collect();
    match parts[0] {
        "effect" if parts[2] == "clear" => {
            clear(world, subject);
        }
        "effect" => {
            let effect = crate::entity::components::EffectType::from_name(parts[2]).unwrap();
            let seconds: u32 = parts[3].parse().unwrap();
            let state = ActiveEffect {
                level: parts[4].parse().unwrap(),
                duration_ticks: if effect.is_instant() {
                    seconds
                } else {
                    seconds * 20
                },
                ambient: false,
                show_particles: parts.get(5) != Some(&"true"),
                infinite: false,
            };
            give(world, subject, effect, state);
        }
        "damage" => {
            let cause = parts.get(3).map_or(ActorDamageCause::None, |name| {
                serde_json::from_value(Value::String((*name).to_owned())).unwrap()
            });
            let hit = Hit {
                damage: parts[2].parse().unwrap(),
                cause,
                attacker: None,
                by_player: false,
                knockback_from: None,
                knockback_power: KnockbackPower::DEFAULT,
            };
            if world.get::<Player>(subject).is_some() {
                crate::entity::player_life::hurt_player(world, subject, &hit);
            } else {
                crate::entity::mob::combat::hurt_mob(world, subject, hit);
            }
        }
        other => panic!("unexpected command {other}: {command}"),
    }
}

/// Our packets about `runtime_id` (the runtime id the viewer knows it by).
fn record(
    packet: McpePacketData,
    runtime_id: u64,
    t: u64,
    flags: &mut Option<u64>,
    out: &mut Vec<(u64, String)>,
) {
    match packet {
        McpePacketData::AddActorPacket(p) if p.target_runtime_id.actor_runtime_id == runtime_id => {
            use jolyne::valentine::types::DataItemEntryPayload;
            for row in &p.actor_data.data {
                if row.id == jolyne::valentine::actor_data_i_ds::RESERVED_0
                    && let DataItemEntryPayload::DataItemInt64Payload(value) = &row.payload
                {
                    *flags = Some(effect_flags(value.value));
                }
            }
        }
        McpePacketData::MobEffectPacket(p)
            if p.target_runtime_id.actor_runtime_id == runtime_id =>
        {
            out.push((
                t,
                format!(
                    "effect {:?} {} level {} for {} particles {}",
                    p.event_id,
                    p.effect_id,
                    p.effect_amplifier,
                    p.effect_duration_ticks,
                    p.show_particles
                ),
            ));
        }
        McpePacketData::SetActorDataPacket(p)
            if p.target_runtime_id.actor_runtime_id == runtime_id =>
        {
            use jolyne::valentine::types::DataItemEntryPayload;
            for row in &p.actor_data.data {
                let DataItemEntryPayload::DataItemInt64Payload(value) = &row.payload else {
                    continue;
                };
                if row.id == jolyne::valentine::actor_data_i_ds::RESERVED_0 {
                    let bits = effect_flags(value.value);
                    if flags.is_some_and(|before| before != bits) {
                        out.push((t, format!("flags {bits:#x}")));
                    }
                    *flags = Some(bits);
                } else if row.id == jolyne::valentine::actor_data_i_ds::VISIBLE_MOB_EFFECTS {
                    out.push((t, format!("visible {}", value.value)));
                }
            }
        }
        McpePacketData::UpdateAttributesPacket(p)
            if p.target_runtime_id.actor_runtime_id == runtime_id =>
        {
            for row in p.attribute_list {
                let modifiers: Vec<(String, f64, i64, i64, String)> = row
                    .modifiers
                    .iter()
                    .map(|m| {
                        (
                            m.name.to_string(),
                            f64::from(m.amount),
                            i64::from(m.operation),
                            i64::from(m.operand),
                            m.id.to_string(),
                        )
                    })
                    .collect();
                out.push((
                    t,
                    attribute_text(
                        row.name.as_ref(),
                        f64::from(row.current_value),
                        f64::from(row.max_value),
                        f64::from(row.default_value),
                        f64::from(row.default_max_value),
                        &modifiers,
                    ),
                ));
            }
        }
        McpePacketData::ActorEventPacket(p)
            if p.target_runtime_id.actor_runtime_id == runtime_id =>
        {
            out.push((t, format!("event {:?} {}", p.event_id, p.data)));
        }
        _ => {}
    }
}

/// The item's commands by tick.
fn actions(item: &Value) -> Vec<(u64, Vec<String>)> {
    item["spec"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|action| {
            (
                action["t"].as_u64().unwrap(),
                action["commands"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|c| c.as_str().unwrap().to_owned())
                    .collect(),
            )
        })
        .collect()
}

fn replay_player(key: &str) -> Vec<(u64, String)> {
    let item = item(key);
    let (mut server, player, mut rx) = server(true);
    let world = server.ecs.world_mut();
    let (health, hunger) = (Health::default(), Hunger::default());
    world.entity_mut(player).insert((
        crate::entity::components::PlayerInput::default(),
        crate::server::game::attributes::SentAttributes::sent(&health, &hunger, 0.0),
    ));
    let ticks = item["spec"]["ticks"].as_u64().unwrap();
    let actions = actions(item);
    let mut packets = Vec::new();
    let mut flags = None;
    for t in 0..ticks {
        let world = server.ecs.world_mut();
        for (_, commands) in actions.iter().filter(|(at, _)| *at == t) {
            for command in commands {
                run(world, player, command);
            }
        }
        server.tick();
        while let Ok(packet) = rx.try_recv() {
            record(packet.data, 1, t, &mut flags, &mut packets);
        }
    }
    packets
}

/// The compared parts of a player item: `MobEffect`, and the health boost
/// and movement attributes (health without its current value).
fn player_view(packets: &[(u64, String)], ticks: u64) -> Vec<(u64, Vec<String>)> {
    // Health entries count only when the maximum or a modifier changed.
    let mut last_health: Vec<String> = ["health max", "20", "default", "[]"]
        .map(str::to_owned)
        .to_vec();
    packets
        .iter()
        .filter(|(t, _)| *t > 0 && *t < ticks)
        .filter_map(|(t, text)| {
            let words: Vec<String> = text.split_whitespace().map(str::to_owned).collect();
            match words[0].as_str() {
                "effect" => Some((*t, words)),
                "attribute" if words[1] == "minecraft:movement" => Some((*t, words)),
                "attribute" if words[1] == "minecraft:health" => {
                    let max = words[2].split('/').nth(1).unwrap().to_owned();
                    let mut kept = vec!["health max".to_owned(), max];
                    kept.extend(words[3..].iter().filter(|w| !w.contains('/')).cloned());
                    let changed = kept != last_health;
                    last_health = kept.clone();
                    changed.then_some((*t, kept))
                }
                _ => None,
            }
        })
        .collect()
}

/// Every player item but the ones whose effects only change health.
const PLAYER_ITEMS: [&str; 20] = [
    "player|poison",
    "player|poison-2",
    "player|regeneration",
    "player|regeneration-2",
    "player|wither",
    "player|instant-damage",
    "player|instant-health",
    "player|resistance",
    "player|fire-resistance",
    "player|health-boost",
    "player|absorption",
    "player|speed-slowness",
    "player|invisibility",
    "player|hidden",
    "player|several",
    "player|stacking",
    "player|clear",
    "player|expiry",
    "player|hunger",
    "player|saturation",
];

#[test]
fn player_effects_reach_the_player_like_bds() {
    for key in PLAYER_ITEMS {
        let item = item(key);
        let ticks = item["spec"]["ticks"].as_u64().unwrap();
        let bds = player_view(&expected(item, "player"), ticks);
        let ours = player_view(&replay_player(key), ticks);
        assert_eq!(ours.len(), bds.len(), "{key}\n ours {ours:?}\n BDS {bds:?}");
        for ((t, ours), (bds_t, bds)) in ours.iter().zip(&bds) {
            assert!(
                t.abs_diff(*bds_t) <= 1,
                "{key}: {ours:?} at {t}, BDS {bds:?} at {bds_t}"
            );
            // A duration reported by a `clear` may be a tick apart too.
            let close = ours.len() == bds.len()
                && ours.iter().zip(bds).all(|(a, b)| {
                    a == b
                        || (ours[0] == "effect"
                            && a.parse::<i64>()
                                .ok()
                                .zip(b.parse::<i64>().ok())
                                .is_some_and(|(a, b)| a.abs_diff(b) <= 1))
                });
            assert!(close, "{key} at {t}: {ours:?}, BDS {bds:?}");
        }
    }
}

fn replay_mob(key: &str) -> Replay {
    let item = item(key);
    let (mut server, viewer, mut rx) = server(false);
    let world = server.ecs.world_mut();
    for command in item["spec"]["setup"].as_array().into_iter().flatten() {
        fill(world, command.as_str().unwrap());
    }
    let at = &item["spec"]["at"];
    let spawn = DVec3::new(
        at[0].as_f64().unwrap(),
        at[1].as_f64().unwrap(),
        at[2].as_f64().unwrap(),
    );
    let mob = loop {
        let mob = spawn_mob(
            world,
            item["mob"].as_str().unwrap(),
            spawn,
            at[3].as_f64().unwrap() as f32,
            SpawnCause::Spawned,
        )
        .unwrap();
        apply_pending_definitions(world);
        let baby = world
            .get::<ActorFlags>(mob)
            .unwrap()
            .get(actor_flags::BABY)
            .unwrap();
        if !baby {
            break mob;
        }
        world.despawn(mob);
    };
    world.entity_mut(mob).remove::<MobBrain>();
    if item["spec"]["frozen"] == true {
        run(world, mob, "effect @s slowness 100000 255 true");
    }
    let runtime_id = world.get::<RuntimeId>(mob).unwrap().0 as u64;
    let _ = viewer;
    let ticks = item["spec"]["ticks"].as_u64().unwrap();
    let actions = actions(item);
    let mut packets = Vec::new();
    let mut flags = None;
    let mut positions = vec![(spawn, false)];
    for t in 0..=ticks {
        let world = server.ecs.world_mut();
        for (_, commands) in actions.iter().filter(|(at, _)| *at == t) {
            for command in commands {
                run(world, mob, command);
            }
        }
        server.tick();
        let world = server.ecs.world();
        if let (Some(position), Some(ground)) = (
            world.get::<Position>(mob),
            world.get::<crate::entity::components::OnGround>(mob),
        ) {
            positions.push((position.0, ground.0));
        }
        while let Ok(packet) = rx.try_recv() {
            record(packet.data, runtime_id, t, &mut flags, &mut packets);
        }
    }
    packets.retain(|(t, _)| *t > 0);
    Replay { packets, positions }
}

/// A zombie's health as changes, without the ticks of `/damage` (see the
/// module docs).
fn zombie_health(packets: &[(u64, String)], damage_ticks: &[u64]) -> Vec<(u64, String)> {
    let mut last = 20.0;
    let mut out = Vec::new();
    for (t, text) in packets {
        let Some(rest) = text.strip_prefix("attribute minecraft:health ") else {
            out.push((*t, text.clone()));
            continue;
        };
        let current: f64 = rest.split('/').next().unwrap().parse().unwrap();
        if !damage_ticks.contains(t) {
            out.push((*t, format!("health change {:.4}", current - last)));
        }
        last = current;
    }
    out
}

/// Every mob item but the moving ones, whose packets are compared below.
const MOB_ITEMS: [&str; 23] = [
    "cow|poison",
    "cow|poison-2",
    "cow|regeneration",
    "cow|regeneration-2",
    "cow|wither",
    "cow|instant-damage",
    "cow|instant-health",
    "cow|resistance",
    "cow|fire-resistance",
    "cow|health-boost",
    "cow|absorption",
    "cow|speed-slowness",
    "cow|invisibility",
    "cow|hidden",
    "cow|several",
    "cow|stacking",
    "cow|clear",
    "cow|expiry",
    "zombie|poison",
    "zombie|regeneration",
    "zombie|wither",
    "zombie|instant-damage",
    "zombie|instant-health",
];

#[test]
fn mob_effects_reach_viewers_and_act_like_bds() {
    for key in MOB_ITEMS {
        let item = item(key);
        let replay = replay_mob(key);
        let mut bds = expected(item, "mob");
        let mut ours = replay.packets;
        // The order within a tick is not compared (BDS sends the actor data
        // a command changed before the effect step's hurt event).
        bds.sort();
        ours.sort();
        if key.starts_with("zombie|") {
            let damage_ticks: Vec<u64> = actions(item)
                .iter()
                .filter(|(_, commands)| commands.iter().any(|c| c.starts_with("damage")))
                .map(|(t, _)| *t)
                .collect();
            bds = zombie_health(&bds, &damage_ticks);
            ours = zombie_health(&ours, &damage_ticks);
        }
        assert_eq!(ours, bds, "{key}");
    }
}

/// A levitating and a slow-falling cow (frozen, out of the cell) move like
/// BDS's: tick by tick within BDS's `f32` positions.
#[test]
fn levitation_and_slow_falling_move_mobs_like_bds() {
    for key in ["cow|levitation", "cow|slow-falling"] {
        let item = item(key);
        let replay = replay_mob(key);
        let rows = item["mob_positions"].as_array().unwrap();
        for (sample, row) in rows.iter().enumerate() {
            let bds = DVec3::new(
                row[0].as_f64().unwrap(),
                row[1].as_f64().unwrap(),
                row[2].as_f64().unwrap(),
            );
            let Some(&(ours, _)) = replay.positions.get(sample) else {
                panic!(
                    "{key}: the mob is gone after {} samples (last {:?})",
                    replay.positions.len(),
                    replay.positions.last()
                );
            };
            assert!(
                (ours - bds).abs().max_element() < 1.0e-4,
                "{key} sample {sample}: {ours}, BDS {bds}"
            );
        }
        let bds: Vec<(u64, String)> = expected(item, "mob")
            .into_iter()
            .filter(|(_, text)| text.starts_with("effect") || text.starts_with("visible"))
            .collect();
        let ours: Vec<(u64, String)> = replay
            .packets
            .into_iter()
            .filter(|(_, text)| text.starts_with("effect") || text.starts_with("visible"))
            .collect();
        assert_eq!(ours, bds, "{key}");
    }
}

#[test]
fn visible_effects_pack_seven_bits_each_lowest_id_first() {
    let mut effects = Effects::default();
    effects.add(EffectType::SPEED, 0, 100);
    effects.add(EffectType::POISON, 0, 100);
    effects.add(EffectType::WEAKNESS, 0, 100);
    effects.add(EffectType::ABSORPTION, 0, 100);
    assert_eq!(visible_effects(&effects), 4789036);
    effects
        .active
        .get_mut(&EffectType::SPEED)
        .unwrap()
        .show_particles = false;
    assert_eq!(visible_effects(&effects), 594732);
}

#[test]
fn pulse_intervals_halve_per_level() {
    // Regeneration II (level 1) heals every 25 ticks.
    let heals = (1..=100).filter(|r| pulses(50, 1, *r)).count();
    assert_eq!(heals, 4);
    assert!(
        pulses(50, 6, 7),
        "an interval below one tick acts every tick"
    );
}

/// Instant health from item use goes through the shared step: it heals on
/// the player's next effect step, and a level of 30 or more heals (the
/// amount saturates at `4 << 30`) instead of wrapping to nothing.
#[test]
fn instant_health_from_item_use_heals_at_any_level() {
    for level in [30, 31, 255] {
        let (mut server, player, _rx) = server(true);
        let world = server.ecs.world_mut();
        world.get_mut::<Health>(player).unwrap().current = 5.0;
        crate::server::game::effects::add_effect(
            world,
            player,
            EffectType::INSTANT_HEALTH,
            level,
            1,
        );
        assert_eq!(world.get::<Health>(player).unwrap().current, 5.0);
        server.tick();
        let world = server.ecs.world_mut();
        assert_eq!(
            world.get::<Health>(player).unwrap().current,
            20.0,
            "level {level}"
        );
    }
}

/// Saturation from item use is instant like `/effect saturation`: it feeds
/// once, on the next step.
#[test]
fn saturation_from_item_use_feeds_once() {
    let (mut server, player, _rx) = server(true);
    let world = server.ecs.world_mut();
    world.get_mut::<Hunger>(player).unwrap().food_level = 10;
    crate::server::game::effects::add_effect(world, player, EffectType::SATURATION, 1, 6);
    for _ in 0..6 {
        server.tick();
    }
    let world = server.ecs.world_mut();
    assert_eq!(world.get::<Hunger>(player).unwrap().food_level, 12);
}

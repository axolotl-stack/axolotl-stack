//! Throwables against BDS 1.26.51.
//!
//! `testdata/bds-throwables.jsonl` is cut (`tools/bds-sweep/combat_fixture.py
//! --windows 38-165,300-435,565-777,970-1075 --also LevelEvent,InventoryContent`)
//! from a `bds-sweep-client` capture of `tools/bds-sweep/scripts/throwables.txt`
//! played against plain BDS 1.26.51 (`tools/bds-sweep/script.py`,
//! 2026-11-11). Each scenario is set up as the script's commands set it up,
//! the client's packets (inputs included, so the throws look where the
//! client looked) are routed at their input ticks, and BDS's answers are
//! compared with ours kind by kind:
//! - item cooldowns;
//! - sounds and where they play;
//! - the projectiles spawned: actor, position, velocity and potion;
//! - splash and particle level events;
//! - effects given to the thrower, and its hurt events;
//! - what slot 0 holds after each throw;
//! - experience orbs: per bottle, BDS's and ours must each split one total
//!   of 3 to 11 into the native orb sizes (a random roll).
//!
//! `testdata/bds-splash-potions.jsonl` (`--windows 38-1725 --player 9
//! --also LevelEvent,UpdateSubChunkBlocks --no-inputs`) is the capture of
//! `scripts/throwables-2.txt`: poison thrown at the thrower's feet with a
//! second player at five distances, water next to fire, then every potion
//! data value 0-46.

use std::collections::BTreeMap;

use glam::IVec3;
use jolyne::valentine::types::{
    DataItemEntryPayload, EnumsActorEvent, EnumsMobEffectPacketPayloadEvent,
};
use jolyne::valentine::{McpePacket, McpePacketData, actor_data_i_ds as ids};

use crate::entity::components::{GameMode, MainInventory};
use crate::entity::projectile::potion::{splash_colour, splash_factor};
use crate::entity::xp_orb::orb_values;
use crate::item::test_support::stack;
use crate::server::game::capture_replay::{BDS_PLAYER, Record, Replay, SESSION, records};

const BDS: &str = include_str!("testdata/bds-throwables.jsonl");
const SPLASH: &str = include_str!("testdata/bds-splash-potions.jsonl");

struct Scenario {
    name: &'static str,
    inputs: std::ops::Range<u64>,
}

const SCENARIOS: [Scenario; 5] = [
    Scenario {
        name: "snowballs and an egg",
        inputs: 38..165,
    },
    Scenario {
        name: "experience bottles",
        inputs: 300..435,
    },
    Scenario {
        name: "splash potions",
        inputs: 565..777,
    },
    Scenario {
        name: "a snowball in creative",
        inputs: 970..994,
    },
    Scenario {
        name: "a splash potion in creative",
        inputs: 1030..1075,
    },
];

/// The script's commands inside a scenario, at their input ticks.
fn step(h: &mut Replay, input: u64) {
    let player = h.player;
    let potion = |h: &mut Replay, data: i16| {
        h.put(0, stack("minecraft:splash_potion", 1).with_damage(data));
    };
    match input {
        40 => h.put(0, stack("minecraft:snowball", 4)),
        112 | 160 | 432 | 777 | 990 | 1070 => h.clear(),
        120 => h.put(0, stack("minecraft:egg", 4)),
        305 => h.put(0, stack("minecraft:experience_bottle", 8)),
        570 => potion(h, 0),
        612 => potion(h, 25),
        652 => potion(h, 21),
        692 => potion(h, 23),
        734 => potion(h, 14),
        1035 => potion(h, 14),
        610 | 650 | 690 | 730 | 775 => {
            crate::server::game::effects::clear_effects(h.world(), player);
        }
        970 | 1030 => *h.world().get_mut::<GameMode>(player).unwrap() = GameMode::Creative,
        975 => h.put(0, stack("minecraft:snowball", 2)),
        _ => {}
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Event {
    Cooldown(String, i32),
    /// Name and position in hundredths of a block.
    Sound(String, [i32; 3]),
    /// Actor, position and velocity in hundredths, potion.
    Spawn(String, [i32; 3], [i32; 3], Option<i16>),
    /// Event id, data, position in hundredths.
    Level(i32, i32, [i32; 3]),
    /// Effect, amplifier, duration.
    Effect(i32, i32, i32),
    Hurt,
    /// Item id and count in slot 0 after a throw.
    Slot(i32, u8),
    /// One bottle's orbs: they fit a total of 3 to 11.
    Orbs(bool),
}

impl Event {
    fn kind(&self) -> &'static str {
        match self {
            Event::Cooldown(..) => "cooldowns",
            Event::Sound(..) => "sounds",
            Event::Spawn(..) => "projectiles",
            Event::Level(..) => "level events",
            Event::Effect(..) => "effects",
            Event::Hurt => "hurt",
            Event::Slot(..) => "slot",
            Event::Orbs(..) => "orbs",
        }
    }
}

fn hundredths(v: [f32; 3]) -> [i32; 3] {
    v.map(|v| (f64::from(v) * 100.0).round() as i32)
}

/// Whether `orbs` is how a total of 3 to 11 experience splits.
fn fits_a_bottle(orbs: &[u32]) -> bool {
    let total: u32 = orbs.iter().sum();
    let mut sorted = orbs.to_vec();
    sorted.sort_unstable_by(|a, b| b.cmp(a));
    (3..=11).contains(&total) && orb_values(total).collect::<Vec<_>>() == sorted
}

/// Ignored sounds: the world's, and the pickup of the orbs.
fn ambient(sound: &str) -> bool {
    matches!(sound, "pop" | "spawn" | "ambient" | "mob.warning" | "born")
}

/// Turns server packets into events; `orbs` collects one bottle's orbs.
fn event(packet: &McpePacketData, player: u64, orbs: &mut Vec<u32>) -> Option<Event> {
    match packet {
        McpePacketData::PlayerStartItemCooldownPacket(cooldown) => Some(Event::Cooldown(
            cooldown.item_category.to_string(),
            cooldown.duration_ticks,
        )),
        McpePacketData::LevelSoundEventPacket(sound) => {
            let name = sound.sound_event.to_string();
            let p = &sound.position;
            (!ambient(&name)).then(|| Event::Sound(name, hundredths([p.x, p.y, p.z])))
        }
        McpePacketData::AddActorPacket(add) if &*add.actor_type == "minecraft:xp_orb" => {
            let value = add.actor_data.data.iter().find_map(|entry| {
                match (&entry.payload, entry.id == ids::VALUE) {
                    (DataItemEntryPayload::DataItemIntPayload(v), true) => Some(v.value as u32),
                    _ => None,
                }
            })?;
            orbs.push(value);
            None
        }
        // Whether an egg hatches is a roll (the chicken tests compare its
        // rate).
        McpePacketData::AddActorPacket(add) if &*add.actor_type == "minecraft:chicken" => None,
        McpePacketData::AddActorPacket(add) => {
            let potion = add.actor_data.data.iter().find_map(|entry| {
                match (&entry.payload, entry.id == ids::AUX_VALUE_DATA) {
                    (DataItemEntryPayload::DataItemShortPayload(v), true) => Some(v.value),
                    _ => None,
                }
            });
            let (p, v) = (&add.position, &add.velocity);
            Some(Event::Spawn(
                add.actor_type.to_string(),
                hundredths([p.x, p.y, p.z]),
                hundredths([v.x, v.y, v.z]),
                potion,
            ))
        }
        McpePacketData::LevelEventPacket(level) if matches!(level.event_id, 2002 | 2009) => {
            let p = &level.position;
            Some(Event::Level(
                level.event_id,
                level.data,
                hundredths([p.x, p.y, p.z]),
            ))
        }
        McpePacketData::MobEffectPacket(effect)
            if effect.target_runtime_id.actor_runtime_id == player
                && effect.event_id == EnumsMobEffectPacketPayloadEvent::Add =>
        {
            Some(Event::Effect(
                effect.effect_id,
                effect.effect_amplifier,
                effect.effect_duration_ticks,
            ))
        }
        McpePacketData::ActorEventPacket(event)
            if event.target_runtime_id.actor_runtime_id == player
                && event.event_id == EnumsActorEvent::Hurt =>
        {
            Some(Event::Hurt)
        }
        _ => None,
    }
}

fn is_use(packet: &McpePacketData) -> bool {
    matches!(packet, McpePacketData::InventoryTransactionPacket(t)
        if matches!(t.transaction, jolyne::valentine::types::InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(_)))
}

/// Positions within a hundredth of a block (BDS computes in f32).
fn close(a: &[i32; 3], b: &[i32; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() <= 1)
}

fn same(bds: Option<&Vec<Event>>, ours: Option<&Vec<Event>>) -> bool {
    let (bds, ours) = (
        bds.map_or(&[][..], Vec::as_slice),
        ours.map_or(&[][..], Vec::as_slice),
    );
    bds.len() == ours.len()
        && bds.iter().zip(ours).all(|pair| match pair {
            (Event::Sound(a, p), Event::Sound(b, q)) => a == b && close(p, q),
            (Event::Spawn(a, p, v, x), Event::Spawn(b, q, w, y)) => {
                a == b && x == y && close(p, q) && close(v, w)
            }
            (Event::Level(a, d, p), Event::Level(b, e, q)) => a == b && d == e && close(p, q),
            (a, b) => a == b,
        })
}

fn by_kind(events: Vec<Event>) -> BTreeMap<&'static str, Vec<Event>> {
    let mut out: BTreeMap<&'static str, Vec<Event>> = BTreeMap::new();
    for event in events {
        out.entry(event.kind()).or_default().push(event);
    }
    out
}

/// BDS's events in a window: the slot after each throw is the slot 0 of
/// the first inventory BDS resent after it.
fn bds_events(window: &[&Record]) -> Vec<Event> {
    let mut out = Vec::new();
    let mut orbs = Vec::new();
    // The input of the last throw whose resent inventory is still to come.
    let mut after_use = None;
    for record in window {
        if record.from_client {
            if is_use(&record.packet) {
                after_use = Some(record.input);
            }
            continue;
        }
        if let McpePacketData::InventoryContentPacket(content) = &record.packet {
            // BDS resends the inventory within two inputs of a survival
            // throw, not after a creative one.
            let fresh = after_use.is_some_and(|input| record.input <= input + 2);
            if fresh && content.container_id == 0 {
                let slot = &content.slots[0];
                out.push(Event::Slot(i32::from(slot.id), slot.stacksize as u8));
                after_use = None;
            }
            continue;
        }
        let before = orbs.len();
        out.extend(event(&record.packet, BDS_PLAYER, &mut orbs));
        // A bottle's orbs are added together, in one tick.
        if orbs.len() == before && !orbs.is_empty() {
            out.push(Event::Orbs(fits_a_bottle(&orbs)));
            orbs.clear();
        }
    }
    if !orbs.is_empty() {
        out.push(Event::Orbs(fits_a_bottle(&orbs)));
    }
    out
}

#[test]
fn throwables_answer_like_bds() {
    let records = records(BDS);
    let mut h = Replay::new();
    let player = h.player;
    // BDS's world is a superflat: a floor wherever the throws land.
    for x in 7..=24 {
        for z in -2..=2 {
            h.set(IVec3::new(x, -61, z), "minecraft:stone");
        }
    }
    let mut failures = Vec::new();
    for scenario in &SCENARIOS {
        h.clear();
        h.select(0);
        *h.world().get_mut::<GameMode>(player).unwrap() = GameMode::Survival;
        let window: Vec<&Record> = records
            .iter()
            .filter(|record| scenario.inputs.contains(&record.input))
            .collect();
        let bds = by_kind(bds_events(&window));
        let client: Vec<&&Record> = window.iter().filter(|record| record.from_client).collect();
        let mut ours = Vec::new();
        let mut orbs = Vec::new();
        let mut next = 0;
        for input in scenario.inputs.clone() {
            step(&mut h, input);
            let mut used = false;
            while next < client.len() && client[next].input < input {
                used |= is_use(&client[next].packet);
                h.route(McpePacket::from(client[next].packet.clone()));
                next += 1;
            }
            h.tick();
            let creative = h.world().get::<GameMode>(player) == Some(&GameMode::Creative);
            if used && !creative {
                let slot = h
                    .world()
                    .get::<MainInventory>(player)
                    .unwrap()
                    .0
                    .item(0)
                    .cloned()
                    .unwrap_or_default();
                let id = slot
                    .item()
                    .filter(|_| slot.count > 0)
                    .map_or(0, |entry| entry.network_id());
                ours.push(Event::Slot(id, slot.count));
            }
            for packet in h.drain() {
                ours.extend(event(&packet, SESSION, &mut orbs));
            }
            if !orbs.is_empty() {
                ours.push(Event::Orbs(fits_a_bottle(&orbs)));
                orbs.clear();
            }
        }
        let ours = by_kind(ours);
        for kind in bds
            .keys()
            .chain(ours.keys())
            .collect::<std::collections::BTreeSet<_>>()
        {
            if !same(bds.get(kind), ours.get(kind)) {
                failures.push(format!(
                    "{}, {kind}:\n  BDS  {:?}\n  ours {:?}",
                    scenario.name,
                    bds.get(kind),
                    ours.get(kind)
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// BDS's splash colour of every potion data value and the effects it gave
/// the thrower at the splash (distance 0) must be ours.
#[test]
fn every_splash_potion_answers_like_bds() {
    let records = records(SPLASH);
    let mut current: Option<i16> = None;
    let mut colours: BTreeMap<i16, i32> = BTreeMap::new();
    let mut effects: BTreeMap<i16, Vec<(i32, i32, i32)>> = BTreeMap::new();
    for record in records.iter().filter(|r| r.input >= 305) {
        match &record.packet {
            McpePacketData::InventoryTransactionPacket(t) if record.from_client => {
                if let jolyne::valentine::types::InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(u) = &t.transaction {
                    current = Some(u.item.auxvalue as i16);
                }
            }
            McpePacketData::LevelEventPacket(level) if level.event_id == 2002 => {
                colours.insert(current.unwrap(), level.data);
            }
            McpePacketData::MobEffectPacket(effect)
                if effect.target_runtime_id.actor_runtime_id == 9
                    && effect.event_id == EnumsMobEffectPacketPayloadEvent::Add =>
            {
                effects.entry(current.unwrap()).or_default().push((
                    effect.effect_id,
                    effect.effect_amplifier,
                    effect.effect_duration_ticks,
                ));
            }
            _ => {}
        }
    }
    assert_eq!(colours.len(), 47);
    let mut wrong = Vec::new();
    for data in 0..47i16 {
        if colours[&data] != splash_colour(data) {
            wrong.push(format!(
                "{data}: colour BDS {}, ours {}",
                colours[&data],
                splash_colour(data)
            ));
        }
        // Instant effects show no MobEffect on a splash.
        let ours: Vec<(i32, i32, i32)> = super::food::potion_effects(data)
            .iter()
            .filter(|(effect, ..)| {
                !matches!(
                    *effect,
                    crate::entity::components::EffectType::INSTANT_HEALTH
                        | crate::entity::components::EffectType::INSTANT_DAMAGE
                )
            })
            .map(|&(effect, amplifier, duration)| {
                (i32::from(effect.0), i32::from(amplifier), duration as i32)
            })
            .collect();
        let bds = effects.get(&data).cloned().unwrap_or_default();
        if bds != ours {
            wrong.push(format!("{data}: effects BDS {bds:?}, ours {ours:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The second player of the poison throws: 1, 2, 3, 3.9 and 4.5 blocks
/// east of the splash (its box 0.3 nearer), given what BDS gave it.
#[test]
fn splash_potions_fall_off_with_the_distance_to_the_box() {
    let records = records(SPLASH);
    let durations: Vec<i32> = records
        .iter()
        .filter(|r| r.input < 240)
        .filter_map(|r| match &r.packet {
            McpePacketData::MobEffectPacket(effect)
                if effect.target_runtime_id.actor_runtime_id == 1
                    && effect.event_id == EnumsMobEffectPacketPayloadEvent::Add =>
            {
                Some(effect.effect_duration_ticks)
            }
            _ => None,
        })
        .collect();
    let ours: Vec<i32> = [1.0, 2.0, 3.0, 3.9, 4.5]
        .iter()
        .filter_map(|east: &f64| splash_factor(east - 0.3))
        .map(|factor| (factor * 900.0).ceil() as i32)
        .collect();
    assert_eq!(durations, ours);
}

/// Water splashed next to fire: BDS put out the fires 1 block from the
/// splash, not the one 2 blocks away.
#[test]
fn water_splashes_put_out_the_fire_beside_them() {
    let records = records(SPLASH);
    let doused: Vec<[i32; 3]> = records
        .iter()
        .filter(|r| (262..299).contains(&r.input))
        .filter_map(|r| match &r.packet {
            McpePacketData::UpdateSubChunkBlocksPacket(update) => Some(
                update
                    .blocks_changed
                    .blocks_changed_standards
                    .iter()
                    .map(|b| [b.pos.x, b.pos.y, b.pos.z])
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .flatten()
        .collect();
    let mut h = Replay::new();
    for x in 1..=2 {
        h.set(IVec3::new(x, -61, 0), "minecraft:netherrack");
    }
    h.set(IVec3::new(0, -61, 1), "minecraft:netherrack");
    for pos in [[1, -60, 0], [2, -60, 0], [0, -60, 1]] {
        h.set(IVec3::from_array(pos), "minecraft:fire");
    }
    let player = h.player;
    h.put(0, stack("minecraft:splash_potion", 1));
    {
        let mut rotation = h
            .world()
            .get_mut::<crate::entity::components::Rotation>(player)
            .unwrap();
        rotation.pitch = 90.0;
    }
    super::ranged::throw(h.world(), player);
    for _ in 0..10 {
        h.tick();
    }
    let air = |h: &mut Replay, pos: [i32; 3]| {
        use crate::entity::systems::BlockSource;
        let id = h.world().block_at(IVec3::from_array(pos)).unwrap();
        crate::registry::BlockRegistry::vanilla()
            .get_by_runtime_id(id)
            .unwrap()
            .string_id
            == "minecraft:air"
    };
    let mut ours: Vec<[i32; 3]> = [[1, -60, 0], [2, -60, 0], [0, -60, 1]]
        .into_iter()
        .filter(|pos| air(&mut h, *pos))
        .collect();
    let mut doused = doused;
    doused.sort();
    ours.sort();
    assert_eq!(ours, doused);
}

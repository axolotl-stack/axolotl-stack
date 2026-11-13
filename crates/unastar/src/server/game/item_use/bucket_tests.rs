//! Buckets and spawn eggs against BDS 1.26.51.
//!
//! `testdata/bds-buckets-eggs.jsonl` is cut (`tools/bds-sweep/combat_fixture.py
//! --windows 30-880 --also InventoryContent`) from a `bds-sweep-client`
//! capture of `tools/bds-sweep/scripts/buckets-eggs.txt` played against plain
//! BDS 1.26.51 (`tools/bds-sweep/script.py`, 2026-11-11). Each scenario is set
//! up as the script's commands set it up, the client's packets are routed at
//! their input ticks, and BDS's answers are compared with ours kind by kind:
//! - the blocks that change around the pit at (2,-61,0) (a block resent
//!   unchanged is not a change);
//! - sounds and where they play;
//! - the actors spawned (actor and position);
//! - `PlayerSpawnedMob` events, effects removed and `CompletedUsingItem`;
//! - what slot 0 holds after each survival use (BDS resends the inventory).
//!
//! Not compared: water into a slab (it waterlogs on BDS, on a second block
//! layer this server does not have), the fish buckets (their mobs cannot
//! spawn here yet), a spawn egg on a block's side whose neighbour is solid,
//! and a spawn egg on a monster spawner.

use std::collections::{BTreeMap, HashMap};

use glam::IVec3;
use jolyne::valentine::types::{EnumsActorEvent, EnumsMobEffectPacketPayloadEvent};
use jolyne::valentine::{McpePacket, McpePacketData};

use crate::entity::components::{GameMode, MainInventory};
use crate::entity::systems::BlockSource;
use crate::item::test_support::stack;
use crate::registry::BlockRegistry;
use crate::server::game::capture_replay::{BDS_PLAYER, Record, Replay, SESSION, records};

const BDS: &str = include_str!("testdata/bds-buckets-eggs.jsonl");

struct Scenario {
    name: &'static str,
    inputs: std::ops::Range<u64>,
}

const SCENARIOS: [Scenario; 8] = [
    Scenario {
        name: "water",
        inputs: 30..120,
    },
    Scenario {
        name: "lava",
        inputs: 125..182,
    },
    Scenario {
        name: "powder snow",
        inputs: 183..247,
    },
    Scenario {
        name: "milk",
        inputs: 308..362,
    },
    Scenario {
        name: "buckets in creative",
        inputs: 549..607,
    },
    Scenario {
        name: "spawn eggs",
        inputs: 607..733,
    },
    Scenario {
        name: "a spawn egg in creative",
        inputs: 762..798,
    },
    Scenario {
        name: "a spawn egg in the air",
        inputs: 798..819,
    },
];

/// The script's commands, at their input ticks.
fn step(h: &mut Replay, input: u64) {
    let player = h.player;
    let mode = |h: &mut Replay, mode: GameMode| {
        *h.world().get_mut::<GameMode>(player).unwrap() = mode;
    };
    let put = |h: &mut Replay, item: &str, count: u8| h.put(0, stack(item, count));
    match input {
        32 | 120 | 180 | 605 => h.set(IVec3::new(2, -61, 0), "minecraft:air"),
        40 | 555 => put(h, "minecraft:water_bucket", 1),
        65 | 95 | 220 | 580 => put(h, "minecraft:bucket", 1),
        90 => h.set(IVec3::new(2, -61, 0), "minecraft:water"),
        121 | 181 | 246 | 360 | 606 | 762 | 797 => h.clear(),
        130 => put(h, "minecraft:lava_bucket", 1),
        155 => put(h, "minecraft:bucket", 3),
        195 => put(h, "minecraft:powder_snow_bucket", 1),
        245 => h.set(IVec3::new(3, -60, 0), "minecraft:air"),
        310 => crate::server::game::effects::add_effect(
            h.world(),
            player,
            crate::entity::components::EffectType::SPEED,
            0,
            600,
        ),
        315 => put(h, "minecraft:milk_bucket", 1),
        550 | 765 => mode(h, GameMode::Creative),
        607 | 798 => mode(h, GameMode::Survival),
        608 => h.set(IVec3::new(2, -61, 0), "minecraft:stone"),
        615 => put(h, "minecraft:cow_spawn_egg", 1),
        645 => put(h, "minecraft:pig_spawn_egg", 1),
        675 => put(h, "minecraft:chicken_spawn_egg", 1),
        705 => put(h, "minecraft:zombie_spawn_egg", 1),
        770 => put(h, "minecraft:pig_spawn_egg", 1),
        800 => put(h, "minecraft:cow_spawn_egg", 1),
        640 | 670 | 700 | 730 | 795 => kill_mobs(h),
        _ => {}
    }
}

/// `/kill @e[type=!player]`.
fn kill_mobs(h: &mut Replay) {
    let world = h.world();
    let mobs: Vec<_> = world
        .query_filtered::<bevy_ecs::entity::Entity, bevy_ecs::query::With<crate::entity::mob::MobKind>>()
        .iter(world)
        .collect();
    for mob in mobs {
        world.despawn(mob);
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Event {
    Block([i32; 3], u32),
    /// Name and position in hundredths.
    Sound(String, [i32; 3]),
    Spawn(String, [i32; 3]),
    SpawnedMob(i32),
    EffectRemoved(i32),
    Completed(i16, i32),
    Slot(i32, u8),
}

impl Event {
    fn kind(&self) -> &'static str {
        match self {
            Event::Block(..) => "blocks",
            Event::Sound(..) => "sounds",
            Event::Spawn(..) => "actors",
            Event::SpawnedMob(..) => "spawned mob events",
            Event::EffectRemoved(..) => "effects removed",
            Event::Completed(..) => "completed uses",
            Event::Slot(..) => "slot",
        }
    }
}

fn watched(p: [i32; 3]) -> bool {
    (1..=4).contains(&p[0]) && (-63..=-58).contains(&p[1]) && (-1..=1).contains(&p[2])
}

/// The world's blocks around the pit, by their network ids.
fn snapshot(h: &mut Replay) -> HashMap<[i32; 3], u32> {
    let registry = BlockRegistry::vanilla();
    let mut out = HashMap::new();
    for x in 1..=4 {
        for y in -63..=-58 {
            for z in -1..=1 {
                let id = h.world().block_at(IVec3::new(x, y, z)).unwrap();
                out.insert([x, y, z], registry.network_hash(id).unwrap());
            }
        }
    }
    out
}

fn ignored_sound(name: &str) -> bool {
    matches!(
        name,
        "pop" | "spawn" | "ambient" | "flop" | "ambient.in.water" | "mob.warning"
    )
}

fn event(
    packet: &McpePacketData,
    player: u64,
    known: &mut HashMap<[i32; 3], u32>,
) -> Option<Event> {
    match packet {
        McpePacketData::UpdateBlockPacket(update) if update.layer == 0 => {
            let p = &update.block_position;
            let pos = [p.x, p.y, p.z];
            (watched(pos)
                && known.insert(pos, update.block_runtime_id) != Some(update.block_runtime_id))
            .then_some(Event::Block(pos, update.block_runtime_id))
        }
        McpePacketData::LevelSoundEventPacket(sound) if !ignored_sound(&sound.sound_event) => {
            let p = &sound.position;
            Some(Event::Sound(
                sound.sound_event.to_string(),
                [p.x, p.y, p.z].map(|v| (f64::from(v) * 100.0).round() as i32),
            ))
        }
        McpePacketData::AddActorPacket(add) => {
            let p = &add.position;
            Some(Event::Spawn(
                add.actor_type.to_string(),
                [p.x, p.y, p.z].map(|v| (f64::from(v) * 100.0).round() as i32),
            ))
        }
        McpePacketData::ActorEventPacket(event)
            if event.target_runtime_id.actor_runtime_id == player
                && event.event_id == EnumsActorEvent::PlayerSpawnedMob =>
        {
            Some(Event::SpawnedMob(event.data))
        }
        McpePacketData::MobEffectPacket(effect)
            if effect.target_runtime_id.actor_runtime_id == player
                && effect.event_id == EnumsMobEffectPacketPayloadEvent::Remove =>
        {
            Some(Event::EffectRemoved(effect.effect_id))
        }
        McpePacketData::CompletedUsingItemPacket(done) => {
            Some(Event::Completed(done.item_id, done.item_use_method))
        }
        _ => None,
    }
}

fn is_use(packet: &McpePacketData) -> bool {
    matches!(packet, McpePacketData::InventoryTransactionPacket(t)
        if matches!(t.transaction, jolyne::valentine::types::InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(_)))
}

/// Whether the use left something to compare in slot 0: a click on a block
/// with an item (not the empty-handed clicks that teach the client the
/// blocks; BDS resends the inventory at once after a click, survival or
/// creative).
fn uses_item(packet: &McpePacketData) -> bool {
    match packet {
        McpePacketData::InventoryTransactionPacket(t) => match &t.transaction {
            jolyne::valentine::types::InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(u) => {
                u.item.id != 0
                    && u.action_type
                        == jolyne::valentine::types::EnumsItemUseInventoryTransactionActionType::Place
            }
            _ => false,
        },
        _ => false,
    }
}

fn by_kind(events: Vec<Event>) -> BTreeMap<&'static str, Vec<Event>> {
    let mut out: BTreeMap<&'static str, Vec<Event>> = BTreeMap::new();
    for event in events {
        out.entry(event.kind()).or_default().push(event);
    }
    out
}

#[test]
fn buckets_and_spawn_eggs_answer_like_bds() {
    let records = records(BDS);
    let mut h = Replay::with_difficulty(crate::world::rules::Difficulty::Peaceful);
    let player = h.player;
    // BDS's superflat has dirt under the stone floor.
    for x in -4..=6 {
        for z in -4..=4 {
            for y in [-63, -62] {
                h.set(IVec3::new(x, y, z), "minecraft:dirt");
            }
        }
    }
    h.tick();
    h.drain();
    let mut failures = Vec::new();
    for scenario in &SCENARIOS {
        h.select(0);
        let start = snapshot(&mut h);
        let window: Vec<&Record> = records
            .iter()
            .filter(|record| scenario.inputs.contains(&record.input))
            .collect();
        let mut known = start.clone();
        let mut bds = Vec::new();
        let mut after_use = None;
        for record in &window {
            if record.from_client {
                if uses_item(&record.packet) {
                    after_use = Some(record.input);
                }
                continue;
            }
            if let McpePacketData::InventoryContentPacket(content) = &record.packet {
                let fresh = after_use.is_some_and(|input| record.input <= input + 2);
                if fresh && content.container_id == 0 {
                    let slot = &content.slots[0];
                    bds.push(Event::Slot(i32::from(slot.id), slot.stacksize as u8));
                    after_use = None;
                }
                continue;
            }
            bds.extend(event(&record.packet, BDS_PLAYER, &mut known));
        }
        let bds = by_kind(bds);
        let client: Vec<&&Record> = window.iter().filter(|record| record.from_client).collect();
        let mut known = start;
        let mut ours = Vec::new();
        let mut next = 0;
        for input in scenario.inputs.clone() {
            step(&mut h, input);
            let mut used = false;
            while next < client.len() && client[next].input < input {
                used |= is_use(&client[next].packet) && uses_item(&client[next].packet);
                h.route(McpePacket::from(client[next].packet.clone()));
                next += 1;
            }
            let before = h
                .world()
                .get::<MainInventory>(player)
                .unwrap()
                .0
                .item(0)
                .cloned()
                .unwrap_or_default();
            h.tick();
            let packets = h.drain();
            let answered = packets
                .iter()
                .any(|packet| event(packet, SESSION, &mut known.clone()).is_some());
            if used {
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
                // BDS resends the inventory only after a click that did
                // something.
                if answered || slot != before {
                    ours.push(Event::Slot(id, slot.count));
                }
            }
            for packet in packets {
                ours.extend(event(&packet, SESSION, &mut known));
            }
        }
        let ours = by_kind(ours);
        for kind in bds
            .keys()
            .chain(ours.keys())
            .collect::<std::collections::BTreeSet<_>>()
        {
            if bds.get(kind) != ours.get(kind) {
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

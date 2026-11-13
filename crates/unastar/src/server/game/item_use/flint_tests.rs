//! Flint and steel against BDS 1.26.51.
//!
//! `testdata/bds-flint-and-steel.jsonl` is cut (`tools/bds-sweep/combat_fixture.py
//! --windows 295-325,335-365,375-405,415-445`) from a `bds-sweep-client`
//! capture of `tools/bds-sweep/scripts/containers-flint.txt` played against
//! plain BDS 1.26.51 (2026-11-11): a survival player clicks the top of an
//! unlit candle, netherrack, an unlit campfire and stone at (2,-60,0) with
//! flint and steel. Each is set up as the script did, the client's packets
//! are routed at their input ticks (a packet sent after input N in the tick
//! of input N + 1), and BDS's answers are compared kind by kind: the block
//! updates in order (a repeat right after itself dropped), the sounds (name
//! and position), block actor data and updates of the held slot. BDS shows
//! the wear only in the inventory it resends after the click: each light
//! must wear ours as much.

use std::collections::BTreeMap;

use glam::IVec3;
use jolyne::valentine::{McpePacket, McpePacketData};

use crate::registry::BlockRegistry;
use crate::registry::block::StateValue;
use crate::server::game::capture_replay::{Record, Replay, records};

const BDS: &str = include_str!("testdata/bds-flint-and-steel.jsonl");
/// Where the script puts the block it lights.
const CLICKED: IVec3 = IVec3::new(2, -60, 0);

struct Scenario {
    name: &'static str,
    inputs: std::ops::Range<u64>,
    block: &'static str,
    state: Option<(&'static str, StateValue)>,
    /// The script's `/setblock ... air` inside the window: input tick and
    /// position (applied in the tick of the next input, as BDS ran them).
    clears: &'static [(u64, [i32; 3])],
}

const SCENARIOS: [Scenario; 4] = [
    Scenario {
        name: "an unlit candle",
        inputs: 295..325,
        block: "minecraft:candle",
        state: None,
        clears: &[(320, [2, -60, 0])],
    },
    Scenario {
        name: "netherrack",
        inputs: 335..365,
        block: "minecraft:netherrack",
        state: None,
        clears: &[(360, [2, -59, 0]), (361, [2, -60, 0])],
    },
    Scenario {
        name: "an unlit campfire",
        inputs: 375..405,
        block: "minecraft:campfire",
        state: Some(("extinguished", StateValue::Byte(1))),
        clears: &[(400, [2, -60, 0])],
    },
    Scenario {
        name: "stone",
        inputs: 415..445,
        block: "minecraft:stone",
        state: None,
        clears: &[(440, [2, -59, 0]), (441, [2, -60, 0])],
    },
];

/// What is compared, near the clicked block only.
#[derive(Debug, Clone, PartialEq)]
enum Event {
    Block([i32; 3], u32),
    /// Name and position in tenths.
    Sound(String, [i32; 3]),
    ActorData([i32; 3]),
    Slot(i16),
}

impl Event {
    fn kind(&self) -> &'static str {
        match self {
            Event::Block(..) => "block updates",
            Event::Sound(..) => "sounds",
            Event::ActorData(..) => "block actor data",
            Event::Slot(..) => "held slot updates",
        }
    }
}

fn near(x: i32, y: i32, z: i32) -> bool {
    (1..=3).contains(&x) && (-62..=-57).contains(&y) && (-1..=1).contains(&z)
}

fn event(packet: &McpePacketData) -> Option<Event> {
    match packet {
        McpePacketData::UpdateBlockPacket(update) => {
            let p = &update.block_position;
            near(p.x, p.y, p.z).then_some(Event::Block([p.x, p.y, p.z], update.block_runtime_id))
        }
        McpePacketData::LevelSoundEventPacket(sound) if sound.sound_event != "pop" => {
            let p = &sound.position;
            Some(Event::Sound(
                sound.sound_event.to_string(),
                [p.x, p.y, p.z].map(|v| (v * 10.0).round() as i32),
            ))
        }
        McpePacketData::BlockActorDataPacket(data) => {
            let p = &data.block_position;
            Some(Event::ActorData([p.x, p.y, p.z]))
        }
        McpePacketData::InventorySlotPacket(slot) if slot.container_id == 0 && slot.slot == 0 => {
            Some(Event::Slot(slot.item.id))
        }
        _ => None,
    }
}

/// Events by kind, without a block update repeated right after itself (a
/// no-op for the client; this server's `set_block`, which the script's
/// commands go through here, sends a change at once and with the tick's
/// batched updates).
/// The `Damage` of the flint and steel in hotbar slot 0 of BDS's resent
/// inventory, if the window has one: the user data's little-endian int after
/// the tag name.
fn bds_damage(window: &[&Record]) -> Option<i32> {
    window.iter().rev().find_map(|record| match &record.packet {
        McpePacketData::InventoryContentPacket(content) if content.container_id == 0 => {
            let user_data = &content.slots.first()?.user_data_buffer;
            let at = user_data
                .windows(6)
                .position(|name| name == b"Damage")
                .map(|start| start + 6)?;
            Some(i32::from_le_bytes(
                user_data.get(at..at + 4)?.try_into().ok()?,
            ))
        }
        _ => None,
    })
}

fn by_kind(events: impl Iterator<Item = Event>) -> BTreeMap<&'static str, Vec<Event>> {
    let mut out: BTreeMap<&'static str, Vec<Event>> = BTreeMap::new();
    for event in events {
        out.entry(event.kind()).or_default().push(event);
    }
    if let Some(blocks) = out.get_mut("block updates") {
        blocks.dedup();
    }
    out
}

#[test]
fn flint_and_steel_answers_like_bds() {
    let records = records(BDS);
    let mut h = Replay::new();
    let registry = BlockRegistry::vanilla();
    let mut failures = Vec::new();
    let mut bds_worn = 0;
    for scenario in &SCENARIOS {
        h.clear();
        h.put(
            0,
            crate::item::test_support::stack("minecraft:flint_and_steel", 1),
        );
        h.select(0);
        h.set(CLICKED + IVec3::Y, "minecraft:air");
        let block = registry.get_by_name(scenario.block).unwrap();
        let state = scenario.state.map_or(block.id, |(property, value)| {
            block.with_state_value(block.id, property, value).unwrap()
        });
        crate::server::game::blocks::set_block(h.world(), CLICKED, state, false);
        h.tick();
        h.drain();
        let window: Vec<&Record> = records
            .iter()
            .filter(|record| scenario.inputs.contains(&record.input))
            .collect();
        let bds = by_kind(
            window
                .iter()
                .filter(|record| !record.from_client)
                .filter_map(|record| event(&record.packet)),
        );
        let client: Vec<&&Record> = window.iter().filter(|record| record.from_client).collect();
        let mut ours = Vec::new();
        let mut next = 0;
        for input in scenario.inputs.clone() {
            for (at, pos) in scenario.clears {
                if *at + 1 == input {
                    h.set(IVec3::from_array(*pos), "minecraft:air");
                }
            }
            while next < client.len() && client[next].input < input {
                h.route(McpePacket::from(client[next].packet.clone()));
                next += 1;
            }
            h.tick();
            ours.extend(h.drain().iter().filter_map(event));
        }
        let ours = by_kind(ours.into_iter());
        let player = h.player;
        let held = h
            .world()
            .get::<crate::entity::components::MainInventory>(player)
            .unwrap()
            .hotbar(0)
            .cloned()
            .unwrap();
        let ours_worn = crate::item::enchant::durability_damage(&held);
        let bds_now = bds_damage(&window).expect("BDS resent the inventory");
        if bds_now - bds_worn != ours_worn {
            failures.push(format!(
                "{}: BDS wore it by {}, ours by {ours_worn}",
                scenario.name,
                bds_now - bds_worn
            ));
        }
        bds_worn = bds_now;
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

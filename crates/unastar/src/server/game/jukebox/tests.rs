//! Jukeboxes against BDS 1.26.51.
//!
//! `testdata/bds-jukebox.jsonl` is cut (`tools/bds-sweep/combat_fixture.py
//! --no-inputs`) from a `bds-sweep-client` capture of
//! `tools/bds-sweep/scripts/records.txt` played against plain BDS 1.26.51
//! (`tools/bds-sweep/script.py`, 2026-11-10). The script's commands are applied
//! here at their input ticks, and the client's clicks and hotbar selections
//! one tick after the input they followed. BDS's answers are compared with
//! ours kind by kind, window by window: popups, the jukebox's block actor
//! data, block updates and the discs that drop.
//!
//! Windows: all 22 discs in and out; a disc used on a playing jukebox, then
//! an empty hand; creative; breaking a playing jukebox; disc 11 to its end
//! and out. Block updates count within 3 blocks of the jukebox (BDS also
//! sends grass far away turning to dirt), and not in the ticks where a
//! command set the jukebox or air. The stick used on the empty jukebox is
//! left out: BDS answers nothing, while our block click resends the block
//! on the clicked face (a block-click correction, not jukebox behaviour).

use std::collections::{BTreeMap, BTreeSet};

use bevy_ecs::prelude::*;
use glam::IVec3;
use jolyne::valentine::types::{TextPacketBody, TextPacketPayloadMessageAndParamsMessageType};
use jolyne::valentine::{McpePacket, McpePacketData};
use zuri_nbt::NBTTag;

use crate::entity::components::{DroppedItem, GameMode};
use crate::server::game::capture_replay::{Replay, records};
use crate::world::block_entity::BlockEntity;

const BDS: &str = include_str!("testdata/bds-jukebox.jsonl");
const JUKEBOX: IVec3 = IVec3::new(2, -60, 0);
const DISCS: [&str; 22] = [
    "11",
    "13",
    "5",
    "blocks",
    "bounce",
    "cat",
    "chirp",
    "creator",
    "creator_music_box",
    "far",
    "lava_chicken",
    "mall",
    "mellohi",
    "otherside",
    "pigstep",
    "precipice",
    "relic",
    "stal",
    "strad",
    "tears",
    "wait",
    "ward",
];

const WINDOWS: [(&str, std::ops::Range<u64>); 5] = [
    ("every disc in and out", 38..919),
    ("a disc on a playing jukebox, an empty hand", 919..968),
    ("creative", 978..1018),
    ("breaking a playing jukebox", 1018..1048),
    ("disc 11 to its end", 1053..2535),
];

fn disc(name: &str) -> String {
    format!("minecraft:music_disc_{name}")
}

/// The script's commands at their input ticks.
fn step(h: &mut Replay, input: u64) {
    let put = |h: &mut Replay, slot: usize, item: &str| {
        h.put(slot, crate::item::test_support::stack(item, 1));
    };
    match input {
        36 => h.set(JUKEBOX, "minecraft:jukebox"),
        40..=919 if (input - 40).is_multiple_of(40) => {
            put(h, 0, &disc(DISCS[((input - 40) / 40) as usize]))
        }
        40..=919 if (input - 40) % 40 == 35 => kill_items(h),
        40..=919 if (input - 40) % 40 == 36 => h.clear(),
        920 => {
            put(h, 0, &disc("cat"));
            put(h, 1, &disc("cat"));
        }
        955 | 964 | 1015 | 1045 | 2530 => kill_items(h),
        957 | 975 | 1017 => h.clear(),
        965 => put(h, 0, "minecraft:stick"),
        980 => set_mode(h, GameMode::Creative),
        982 => put(h, 0, &disc("ward")),
        1016 => set_mode(h, GameMode::Survival),
        1020 => put(h, 0, &disc("mall")),
        1035 => h.set(JUKEBOX, "minecraft:air"),
        1050 => h.set(JUKEBOX, "minecraft:jukebox"),
        1055 => put(h, 0, &disc("11")),
        _ => {}
    }
}

fn set_mode(h: &mut Replay, mode: GameMode) {
    let player = h.player;
    *h.world().get_mut::<GameMode>(player).unwrap() = mode;
}

/// `/kill @e[type=item]`.
fn kill_items(h: &mut Replay) {
    let world = h.world();
    let items: Vec<Entity> = world
        .query_filtered::<Entity, With<DroppedItem>>()
        .iter(world)
        .collect();
    for item in items {
        world.despawn(item);
    }
}

/// What the comparison looks at, by kind.
#[derive(Debug, Clone, PartialEq)]
enum Event {
    Popup(String),
    /// The jukebox's actor data: its disc, `FinishedRecording`,
    /// `TicksPlaying`.
    Data(Option<String>, u8, i64),
    /// A block update: position and wire block id.
    Update([i32; 3], u32),
    /// A dropped music disc (item network id).
    Drop(i16),
}

impl Event {
    fn kind(&self) -> &'static str {
        match self {
            Event::Popup(_) => "popups",
            Event::Data(..) => "actor data",
            Event::Update(..) => "block updates",
            Event::Drop(_) => "drops",
        }
    }
}

fn event(packet: &McpePacketData, discs: &BTreeSet<i16>) -> Option<Event> {
    match packet {
        McpePacketData::TextPacket(text) => match &text.body {
            TextPacketBody::MessageAndParams(popup)
                if popup.message_type
                    == TextPacketPayloadMessageAndParamsMessageType::Jukeboxpopup =>
            {
                Some(Event::Popup(format!(
                    "{} {:?}",
                    popup.message,
                    popup
                        .parameter_list
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                )))
            }
            _ => None,
        },
        McpePacketData::BlockActorDataPacket(data) => {
            let p = &data.block_position;
            if [p.x, p.y, p.z] != JUKEBOX.to_array() {
                return None;
            }
            let compound = BlockEntity::from_network_nbt(data.actor_data_tags.clone())
                .ok()?
                .bedrock_compound()?
                .ok()?;
            let record = match compound.0.get("RecordItem") {
                Some(NBTTag::Compound(record)) => match record.0.get("Name") {
                    Some(NBTTag::String(name)) => Some(name.0.clone()),
                    _ => None,
                },
                _ => None,
            };
            let finished = match compound.0.get("FinishedRecording") {
                Some(NBTTag::Byte(value)) => value.0,
                _ => 255,
            };
            let ticks = match compound.0.get("TicksPlaying") {
                Some(NBTTag::Long(value)) => value.0,
                _ => -1,
            };
            Some(Event::Data(record, finished, ticks))
        }
        McpePacketData::UpdateBlockPacket(update) if update.layer == 0 => {
            let p = &update.block_position;
            let near = (IVec3::new(p.x, p.y, p.z) - JUKEBOX).abs().max_element() <= 3;
            near.then_some(Event::Update([p.x, p.y, p.z], update.block_runtime_id))
        }
        McpePacketData::AddItemActorPacket(item) if discs.contains(&item.item.id) => {
            Some(Event::Drop(item.item.id))
        }
        _ => None,
    }
}

fn window_of(input: u64) -> Option<usize> {
    WINDOWS
        .iter()
        .position(|(_, inputs)| inputs.contains(&input))
}

#[test]
fn jukeboxes_take_and_give_discs_like_bds() {
    let records = records(BDS);
    let discs: BTreeSet<i16> = DISCS
        .iter()
        .map(|name| {
            crate::item::test_support::stack(&disc(name), 1)
                .item()
                .unwrap()
                .network_id() as i16
        })
        .collect();
    let mut bds: Vec<BTreeMap<&'static str, Vec<Event>>> = vec![BTreeMap::new(); WINDOWS.len()];
    for record in records.iter().filter(|record| !record.from_client) {
        if let (Some(window), Some(event)) =
            (window_of(record.input), event(&record.packet, &discs))
        {
            bds[window].entry(event.kind()).or_default().push(event);
        }
    }
    let client: Vec<_> = records.iter().filter(|record| record.from_client).collect();
    let mut ours: Vec<BTreeMap<&'static str, Vec<Event>>> = vec![BTreeMap::new(); WINDOWS.len()];
    let mut h = Replay::new();
    let mut next = 0;
    for input in 35..WINDOWS[WINDOWS.len() - 1].1.end {
        step(&mut h, input);
        // The commands' own block updates are not part of the comparison.
        h.drain();
        // A packet sent after input N reaches the server's tick of input
        // N + 1.
        while next < client.len() && client[next].input < input {
            h.route(McpePacket::from(client[next].packet.clone()));
            next += 1;
        }
        h.tick();
        let packets = h.drain();
        // The commands' block changes reach viewers during the tick.
        let command_set_block = matches!(input, 36 | 1035 | 1050);
        if let Some(window) = window_of(input) {
            for event in packets.iter().filter_map(|packet| event(packet, &discs)) {
                if command_set_block && matches!(event, Event::Update(..)) {
                    continue;
                }
                ours[window].entry(event.kind()).or_default().push(event);
            }
        }
    }
    let mut failures = Vec::new();
    for (index, (name, _)) in WINDOWS.iter().enumerate() {
        let kinds: BTreeSet<_> = bds[index]
            .keys()
            .chain(ours[index].keys())
            .copied()
            .collect();
        for kind in kinds {
            if bds[index].get(kind) != ours[index].get(kind) {
                failures.push(format!(
                    "{name}, {kind}:\n  BDS  {:?}\n  ours {:?}",
                    bds[index].get(kind),
                    ours[index].get(kind)
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn survival_inserts_use_up_the_disc_and_creative_keeps_it() {
    let mut h = Replay::new();
    h.set(JUKEBOX, "minecraft:jukebox");
    let player = h.player;
    for (mode, left) in [(GameMode::Survival, 0), (GameMode::Creative, 1)] {
        set_mode(&mut h, mode);
        h.put(0, crate::item::test_support::stack(&disc("cat"), 1));
        assert!(super::use_jukebox(h.world(), player, JUKEBOX, 4));
        let held = crate::server::game::item_use::held(h.world(), player);
        assert_eq!(held.map_or(0, |(_, stack)| stack.count), left, "{mode:?}");
        // Taking it out again pops the disc above the jukebox.
        assert!(super::use_jukebox(h.world(), player, JUKEBOX, 4));
        let world = h.world();
        let drops: Vec<f64> = world
            .query_filtered::<&crate::entity::components::Position, With<DroppedItem>>()
            .iter(world)
            .map(|position| position.0.y)
            .collect();
        assert_eq!(drops.len(), 1);
        // The centre within 0.35 of (2.5, -58.5, 0.5); the feet 0.125 lower.
        assert!((-58.98..=-58.27).contains(&drops[0]), "{drops:?}");
        kill_items(&mut h);
    }
}

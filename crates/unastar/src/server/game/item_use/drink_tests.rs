//! The spyglass, goat horns, potions and ominous bottles against BDS 1.26.51.
//!
//! `testdata/bds-use-items.jsonl` is cut (`tools/bds-sweep/combat_fixture.py
//! --no-inputs`) from a `bds-sweep-client` capture of
//! `tools/bds-sweep/scripts/use-items.txt` played against plain BDS 1.26.51
//! (`tools/bds-sweep/script.py`, 2026-11-10). It holds the client's item uses
//! and releases and BDS's answers. Each scenario is set up as the script's
//! commands set it up (the item in hotbar slot 0, effects cleared between
//! drinks). The client's packets are routed at their input ticks, the server
//! ticking once per input, and BDS's answers are compared with ours, kind by
//! kind (a client packet sent after input N is applied in the tick of input
//! N + 1, with the script's commands of that input):
//! - the using flag;
//! - item cooldowns;
//! - sounds and where they play;
//! - `MobEffect` adds (effect, amplifier, duration, particles) and removes;
//! - `CompletedUsingItem`;
//! - the player's hurt events;
//! - what the used slot holds afterwards.
//!
//! Scenarios: the spyglass held 20 inputs; goat horns 0-7, horn 0 used again
//! inside its cooldown; potions 0-46; ominous bottles 0-4.

use std::collections::BTreeMap;

use jolyne::valentine::types::{
    DataItemEntryPayload, EnumsActorEvent, EnumsMobEffectPacketPayloadEvent,
    InventoryTransactionPacketTransaction,
};
use jolyne::valentine::{McpePacket, McpePacketData};

use crate::server::game::capture_replay::{BDS_PLAYER, Record, Replay, SESSION, records};

const BDS: &str = include_str!("testdata/bds-use-items.jsonl");

/// One scenario of the script: its window of client input ticks.
struct Scenario {
    name: &'static str,
    inputs: std::ops::Range<u64>,
}

const SCENARIOS: [Scenario; 4] = [
    Scenario {
        name: "the spyglass",
        inputs: 38..75,
    },
    Scenario {
        name: "goat horns",
        inputs: 78..1400,
    },
    Scenario {
        name: "potions",
        inputs: 1405..3525,
    },
    Scenario {
        name: "ominous bottles",
        inputs: 3525..3760,
    },
];

/// The script's commands inside a scenario, at their input ticks: the item
/// put in slot 0, and `/effect clear`.
fn step(h: &mut Replay, index: usize, input: u64) {
    let put = |h: &mut Replay, item: &str, data: i16| {
        h.put(
            0,
            crate::item::test_support::stack(item, 1).with_damage(data),
        );
    };
    match index {
        0 if input == 40 => put(h, "minecraft:spyglass", 0),
        1 => {
            let starts = [80, 280, 440, 600, 760, 920, 1080, 1240];
            if let Some(data) = starts.iter().position(|start| *start == input) {
                put(h, "minecraft:goat_horn", data as i16);
            }
        }
        2 | 3 => {
            let (first, item) = if index == 2 {
                (1410, "minecraft:potion")
            } else {
                (3525, "minecraft:ominous_bottle")
            };
            let Some(offset) = input.checked_sub(first) else {
                return;
            };
            match offset % 45 {
                0 => put(h, item, (offset / 45) as i16),
                42 => {
                    let player = h.player;
                    crate::server::game::effects::clear_effects(h.world(), player);
                }
                _ => {}
            }
        }
        _ => {}
    }
}

/// What the comparison looks at, by kind.
#[derive(Debug, Clone, PartialEq)]
enum Event {
    Using(bool),
    Cooldown(String, i32),
    /// Name and position in hundredths of a block.
    Sound(String, [i32; 3]),
    /// Added: effect, amplifier, duration, particles.
    EffectAdded(i32, i32, i32, bool),
    /// Removed: effect, amplifier.
    EffectRemoved(i32, i32),
    Completed(i16, i32),
    Hurt,
    /// The item id the used slot holds afterwards.
    Slot(i16),
}

impl Event {
    fn kind(&self) -> &'static str {
        match self {
            Event::Using(_) => "using flag",
            Event::Cooldown(..) => "cooldowns",
            Event::Sound(..) => "sounds",
            Event::EffectAdded(..) | Event::EffectRemoved(..) => "effects",
            Event::Completed(..) => "completed uses",
            Event::Hurt => "hurt",
            Event::Slot(_) => "slot",
        }
    }
}

/// Sounds of the world rather than of the scenario.
fn ambient(sound: &str) -> bool {
    matches!(sound, "pop" | "spawn" | "ambient" | "mob.warning")
}

fn events(packet: &McpePacketData, player: u64, using: &mut bool, from_bds: bool) -> Option<Event> {
    match packet {
        McpePacketData::SetActorDataPacket(data)
            if data.target_runtime_id.actor_runtime_id == player =>
        {
            let flags = data
                .actor_data
                .data
                .iter()
                .find_map(|entry| match &entry.payload {
                    DataItemEntryPayload::DataItemInt64Payload(flags) if entry.id == 0 => {
                        Some(flags.value)
                    }
                    _ => None,
                })?;
            let now = flags & (1 << jolyne::valentine::actor_flags::USINGITEM) != 0;
            (now != std::mem::replace(using, now)).then_some(Event::Using(now))
        }
        McpePacketData::PlayerStartItemCooldownPacket(cooldown) => Some(Event::Cooldown(
            cooldown.item_category.to_string(),
            cooldown.duration_ticks,
        )),
        McpePacketData::LevelSoundEventPacket(sound) => {
            let name = sound.sound_event.to_string();
            let p = &sound.position;
            (!ambient(&name)).then(|| {
                Event::Sound(
                    name,
                    [p.x, p.y, p.z].map(|v| (f64::from(v) * 100.0).round() as i32),
                )
            })
        }
        McpePacketData::MobEffectPacket(effect)
            if effect.target_runtime_id.actor_runtime_id == player =>
        {
            match effect.event_id {
                EnumsMobEffectPacketPayloadEvent::Add => Some(Event::EffectAdded(
                    effect.effect_id,
                    effect.effect_amplifier,
                    effect.effect_duration_ticks,
                    effect.show_particles,
                )),
                EnumsMobEffectPacketPayloadEvent::Remove => Some(Event::EffectRemoved(
                    effect.effect_id,
                    effect.effect_amplifier,
                )),
                _ => None,
            }
        }
        McpePacketData::CompletedUsingItemPacket(done) => {
            Some(Event::Completed(done.item_id, done.item_use_method))
        }
        McpePacketData::ActorEventPacket(event)
            if event.target_runtime_id.actor_runtime_id == player
                && event.event_id == EnumsActorEvent::Hurt =>
        {
            Some(Event::Hurt)
        }
        // BDS echoes a finished use as an item release transaction whose
        // action turns the used stack into what is left.
        McpePacketData::InventoryTransactionPacket(transaction) if from_bds => {
            match &transaction.transaction {
                InventoryTransactionPacketTransaction::ItemReleaseInventoryTransaction(release) => {
                    release
                        .actions
                        .actions
                        .first()
                        .map(|action| Event::Slot(action.to_item.id))
                }
                _ => None,
            }
        }
        McpePacketData::InventorySlotPacket(slot)
            if !from_bds && slot.container_id == 0 && slot.slot == 0 =>
        {
            Some(Event::Slot(slot.item.id))
        }
        _ => None,
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
fn spyglass_horns_potions_and_ominous_bottles_answer_like_bds() {
    let records = records(BDS);
    let mut h = Replay::new();
    let mut failures = Vec::new();
    for (index, scenario) in SCENARIOS.iter().enumerate() {
        h.clear();
        h.select(0);
        let window: Vec<&Record> = records
            .iter()
            .filter(|record| scenario.inputs.contains(&record.input))
            .collect();
        let mut using = false;
        let bds = by_kind(
            window
                .iter()
                .filter(|record| !record.from_client)
                .filter_map(|record| events(&record.packet, BDS_PLAYER, &mut using, true))
                .collect(),
        );
        let client: Vec<&&Record> = window.iter().filter(|record| record.from_client).collect();
        let mut ours = Vec::new();
        let mut using = false;
        let mut next = 0;
        for input in scenario.inputs.clone() {
            step(&mut h, index, input);
            // A packet sent after input N reaches the server's tick of
            // input N + 1, like the script's commands at N + 1.
            while next < client.len() && client[next].input < input {
                h.route(McpePacket::from(client[next].packet.clone()));
                next += 1;
            }
            h.tick();
            for packet in h.drain() {
                ours.extend(events(&packet, SESSION, &mut using, false));
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

#[test]
fn harming_hurts_six_and_twelve() {
    // BDS 1.26.51 item-use capture: drinking harming (data 23) took the
    // player from 20 to 14 health, strong harming (24) from 17 to 5.
    let mut h = Replay::new();
    let player = h.player;
    for (data, before, after) in [(23i16, 20.0, 14.0), (24, 17.0, 5.0)] {
        h.world()
            .get_mut::<crate::entity::components::Health>(player)
            .unwrap()
            .current = before;
        h.world()
            .entity_mut(player)
            .remove::<crate::entity::mob::combat::HurtTimer>();
        for &(effect, amplifier, duration) in super::food::potion_effects(data) {
            crate::server::game::effects::add_effect(
                h.world(),
                player,
                effect,
                amplifier,
                duration,
            );
        }
        let health = h
            .world()
            .get::<crate::entity::components::Health>(player)
            .unwrap()
            .current;
        assert_eq!(health, after, "potion {data}");
    }
}

/// Where the drink sound plays for other facings: `testdata/bds-drink-facing.jsonl`
/// is cut from the capture of `tools/bds-sweep/scripts/elytra.txt` (scenario
/// 5: a water bottle drunk facing south, north, and east looking 60 degrees
/// down).
#[test]
fn the_drink_sound_follows_the_yaw_but_not_the_pitch() {
    let records = records(include_str!("testdata/bds-drink-facing.jsonl"));
    let mut using = false;
    let bds: Vec<Event> = records
        .iter()
        .filter(|record| !record.from_client)
        .filter_map(|record| events(&record.packet, BDS_PLAYER, &mut using, true))
        .filter(|event| matches!(event, Event::Sound(..) | Event::Completed(..)))
        .collect();
    let client: Vec<&Record> = records.iter().filter(|record| record.from_client).collect();
    let mut h = Replay::new();
    let player = h.player;
    let mut ours = Vec::new();
    let mut using = false;
    let mut next = 0;
    for input in 718..880 {
        let facing = match input {
            722 => Some((0.0, 0.0)),
            770 => Some((180.0, 0.0)),
            820 => Some((-90.0, 60.0)),
            _ => None,
        };
        if let Some((yaw, pitch)) = facing {
            let mut rotation = h
                .world()
                .get_mut::<crate::entity::components::Rotation>(player)
                .unwrap();
            rotation.yaw = yaw;
            rotation.head_yaw = yaw;
            rotation.pitch = pitch;
        }
        if matches!(input, 725 | 775 | 825) {
            h.put(0, crate::item::test_support::stack("minecraft:potion", 1));
        }
        while next < client.len() && client[next].input < input {
            h.route(McpePacket::from(client[next].packet.clone()));
            next += 1;
        }
        h.tick();
        for packet in h.drain() {
            ours.extend(
                events(&packet, SESSION, &mut using, false)
                    .filter(|event| matches!(event, Event::Sound(..) | Event::Completed(..))),
            );
        }
    }
    assert_eq!(ours, bds);
}

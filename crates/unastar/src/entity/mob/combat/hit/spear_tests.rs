//! Spears against BDS 1.26.51, packet for packet.
//!
//! `testdata/bds-spears.jsonl` is cut (`tools/bds-sweep/combat_fixture.py`)
//! from a `bds-sweep-client` capture of `tools/bds-sweep/scripts/spears.txt`
//! played against plain BDS 1.26.51 (`tools/bds-sweep/script.py`,
//! 2026-11-10). It holds the client's inputs, attacks, item uses, swings and
//! hotbar selections, and BDS's answers: hurt events, health updates,
//! `PlayerStartItemCooldown`, swing sounds and the player's using flag.
//!
//! Each scenario of the script is set up here as its commands set it up
//! (pigs held still, items in the hotbar). Its captured client packets are
//! routed through the server, one server tick per input, with BDS runtime ids
//! mapped to the pigs' name tags. Then the answers are compared with BDS's in
//! order. The scenarios cover:
//! - plain hits and the attack cooldown: a hit 7 inputs after a hit and two
//!   more attacks in the same input are refused, and so is the diamond spear
//!   after an iron spear hit; a hit 19 inputs after a hit lands, as does a
//!   sword's hit during a spear's cooldown;
//! - the jab without a target (`UseAsAttack`) of every tier, also packed into
//!   the input, and the sword's jab, which does nothing;
//! - missed swings: the sound comes from the `MissedSwing` input flag, not
//!   from the `Animate` swing;
//! - holding a spear (the using flag);
//! - a sword's melee reach.

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use bytes::BytesMut;
use glam::{DVec3, IVec3};
use jolyne::valentine::types::{DataItemEntryPayload, EnumsActorEvent};
use jolyne::valentine::{
    BorrowedMcpePacket, InventoryTransactionPacketTransaction, McpePacket, McpePacketArgs,
    McpePacketData,
};
use tokio::sync::mpsc::Receiver;

use crate::entity::components::{GameMode, HeldSlot, MainInventory, Position, RuntimeId, Velocity};
use crate::entity::mob::components::LootConfig;
use crate::entity::mob::{SpawnCause, spawn_mob};
use crate::entity::systems::BlockSource;
use crate::item::ItemStack;
use crate::registry::BlockRegistry;
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};

const BDS: &str = include_str!("testdata/bds-spears.jsonl");
const SESSION: u64 = 7;
/// The scripted player's feet: facing east along the pigs' walk.
const FEET: DVec3 = DVec3::new(0.5, -60.0, 0.5);
/// The runtime id BDS gave the scripted player.
const BDS_PLAYER: u64 = 1;

/// A capture record: who sent it, the client tick of the latest input and
/// the packet.
struct Record {
    from_client: bool,
    input: u64,
    packet: McpePacketData,
}

fn records() -> Vec<Record> {
    BDS.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            Record {
                from_client: row["dir"] == "c2s",
                input: row["input"].as_u64().unwrap_or(0),
                packet: decode(row["hex"].as_str().unwrap()),
            }
        })
        .collect()
}

/// A recorded packet (hex, without its batch length prefix).
fn decode(hex: &str) -> McpePacketData {
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();
    let mut framed = Vec::with_capacity(bytes.len() + 5);
    let mut len = bytes.len() as u32;
    while len >= 0x80 {
        framed.push(len as u8 | 0x80);
        len >>= 7;
    }
    framed.push(len as u8);
    framed.extend_from_slice(&bytes);
    McpePacketData::decode_inner_bytes(&mut bytes::Bytes::from(framed), McpePacketArgs)
        .unwrap()
        .1
}

/// The name tag in an actor's synched data.
fn name_tag(data: &jolyne::valentine::types::SynchedActorDataCopyableDataList) -> Option<String> {
    data.data
        .iter()
        .find_map(|entry| match (&entry.id, &entry.payload) {
            (4, DataItemEntryPayload::DataItemStringPayload(name)) if !name.value.is_empty() => {
                Some(name.value.to_string())
            }
            _ => None,
        })
}

/// BDS runtime ids of the named actors the script summoned.
fn bds_names(records: &[Record]) -> HashMap<u64, String> {
    records
        .iter()
        .filter_map(|record| match &record.packet {
            McpePacketData::AddActorPacket(actor) => name_tag(&actor.actor_data)
                .map(|name| (actor.target_runtime_id.actor_runtime_id, name)),
            _ => None,
        })
        .collect()
}

/// One scenario of the script: its window of client input ticks.
struct Scenario {
    name: &'static str,
    inputs: std::ops::Range<u64>,
}

const SCENARIOS: [Scenario; 6] = [
    Scenario {
        name: "plain hits, cooldown and three attacks in one input",
        inputs: 40..160,
    },
    Scenario {
        name: "the cooldown's last input and its category",
        inputs: 170..275,
    },
    Scenario {
        name: "jabs without a target",
        inputs: 280..520,
    },
    Scenario {
        name: "missed swings",
        inputs: 528..588,
    },
    Scenario {
        name: "holding a spear",
        inputs: 588..628,
    },
    Scenario {
        name: "a sword's reach",
        inputs: 628..670,
    },
];

/// What the comparison looks at.
#[derive(Debug, Clone, PartialEq)]
enum Event {
    Hurt(String),
    Death(String),
    Health(String, i32),
    Cooldown(String, i32),
    Sound(String),
    Using(bool),
}

/// Sounds of the world rather than of the scenario.
fn ambient(sound: &str) -> bool {
    matches!(sound, "pop" | "spawn" | "ambient" | "mob.warning")
}

/// Turns a packet into compared events; `names` maps runtime ids to name
/// tags, `player` is the scripted player's runtime id, `using` the last
/// using flag seen.
fn event(
    packet: &McpePacketData,
    names: &HashMap<u64, String>,
    player: u64,
    using: &mut bool,
) -> Option<Event> {
    match packet {
        McpePacketData::ActorEventPacket(event) => {
            let name = names
                .get(&event.target_runtime_id.actor_runtime_id)?
                .clone();
            match event.event_id {
                EnumsActorEvent::Hurt => Some(Event::Hurt(name)),
                EnumsActorEvent::Death => Some(Event::Death(name)),
                _ => None,
            }
        }
        McpePacketData::UpdateAttributesPacket(update) => {
            let name = names.get(&update.target_runtime_id.actor_runtime_id)?;
            let health = update
                .attribute_list
                .iter()
                .find(|attribute| attribute.name == "minecraft:health")?;
            (health.current_value < health.max_value)
                .then(|| Event::Health(name.clone(), health.current_value.round() as i32))
        }
        McpePacketData::PlayerStartItemCooldownPacket(cooldown) => Some(Event::Cooldown(
            cooldown.item_category.to_string(),
            cooldown.duration_ticks,
        )),
        McpePacketData::LevelSoundEventPacket(sound) => {
            let name = sound.sound_event.to_string();
            (!ambient(&name)).then_some(Event::Sound(name))
        }
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
        _ => None,
    }
}

struct Harness {
    server: GameServer,
    player: Entity,
    rx: Receiver<McpePacket>,
    /// Our pigs by name tag, with where they stand.
    pigs: HashMap<String, (Entity, DVec3)>,
}

impl Harness {
    fn new() -> Self {
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..ServerConfig::default()
        });
        server
            .ecs
            .world_mut()
            .insert_resource(crate::entity::EntityRandom::seeded(11));
        let (outbound_tx, rx) = tokio::sync::mpsc::channel(1 << 16);
        let player = server
            .spawn_player(PlayerSpawnData {
                session_id: SESSION,
                display_name: "BDSSweep".into(),
                xuid: None,
                uuid: Some(format!("00000000-0000-0000-0000-{SESSION:012}")),
                runtime_id: SESSION as i64,
                position: FEET + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT,
                outbound_tx,
                saved_data: None,
                chunk_radius: 2,
            })
            .expect("valid player spawn");
        let mut h = Self {
            server,
            player,
            rx,
            pigs: HashMap::new(),
        };
        for _ in 0..40 {
            if [(-1, -1), (-1, 1), (1, -1), (1, 1)]
                .iter()
                .all(|&(x, z)| h.world().block_at(IVec3::new(x, -61, z)).is_some())
            {
                break;
            }
            h.server.tick();
        }
        *h.world().get_mut::<GameMode>(player).unwrap() = GameMode::Survival;
        for x in -4..=12 {
            for z in -4..=4 {
                h.set(IVec3::new(x, -61, z), "minecraft:stone");
                for y in -60..=-55 {
                    let walk = (1..=9).contains(&x) && (0..=1).contains(&z) && y == -60;
                    h.set(
                        IVec3::new(x, y, z),
                        if walk {
                            "minecraft:stone"
                        } else {
                            "minecraft:air"
                        },
                    );
                }
            }
        }
        h.server.tick();
        h.drain();
        h
    }

    fn world(&mut self) -> &mut World {
        self.server.ecs.world_mut()
    }

    fn set(&mut self, pos: IVec3, name: &str) {
        let state = BlockRegistry::vanilla()
            .default_state_id_by_name(name)
            .unwrap();
        crate::server::game::blocks::set_block(self.world(), pos, state, false);
    }

    /// The hotbar: an iron spear, a diamond spear and an iron sword, then
    /// an empty slot (the script's `/give`s).
    fn hotbar(&mut self, selected: u8) {
        let player = self.player;
        let mut inventory = self.world().get_mut::<MainInventory>(player).unwrap();
        for slot in 0..36 {
            let _ = inventory.0.set_item(slot, ItemStack::empty());
        }
        for (slot, item) in [
            "minecraft:iron_spear",
            "minecraft:diamond_spear",
            "minecraft:iron_sword",
        ]
        .into_iter()
        .enumerate()
        {
            inventory
                .0
                .set_item(slot, crate::item::test_support::stack(item, 1))
                .unwrap();
        }
        *self.world().get_mut::<HeldSlot>(player).unwrap() = HeldSlot(selected);
    }

    /// `/replaceitem entity @s slot.weapon.mainhand 0 <item>` with slot 0
    /// selected.
    fn main_hand(&mut self, item: &str) {
        let player = self.player;
        let mut inventory = self.world().get_mut::<MainInventory>(player).unwrap();
        inventory
            .0
            .set_item(0, crate::item::test_support::stack(item, 1))
            .unwrap();
    }

    /// An adult pig with this name tag standing at `(x, -59, z)`, held in
    /// place (no AI, and [`Harness::tick`] puts it back after knockback).
    /// BDS's pigs, held by Slowness 255, are knocked back by hits; where they
    /// slide only matters for the reach of later hits, which landed in BDS.
    fn pig(&mut self, name: &str, x: f64, z: f64) {
        let feet = DVec3::new(x, -59.0, z);
        let world = self.world();
        let pig = loop {
            let pig = spawn_mob(world, "minecraft:pig", feet, 0.0, SpawnCause::Spawned).unwrap();
            if world.get::<LootConfig>(pig).is_some() {
                break pig;
            }
            world.despawn(pig);
        };
        world
            .entity_mut(pig)
            .remove::<crate::entity::mob::ai::MobBrain>();
        self.pigs.insert(name.to_owned(), (pig, feet));
    }

    fn clear_pigs(&mut self) {
        for (_, (pig, _)) in std::mem::take(&mut self.pigs) {
            if self.world().get_entity(pig).is_ok() {
                self.world().despawn(pig);
            }
        }
    }

    /// One server tick, then the pigs back where they stand.
    fn tick(&mut self) {
        self.server.tick();
        let pigs: Vec<_> = self.pigs.values().copied().collect();
        for (pig, feet) in pigs {
            if let Some(mut position) = self.world().get_mut::<Position>(pig) {
                position.0 = feet;
            }
            if let Some(mut velocity) = self.world().get_mut::<Velocity>(pig) {
                velocity.0 = DVec3::ZERO;
            }
        }
    }

    /// Our runtime ids of the named pigs.
    fn names(&mut self) -> HashMap<u64, String> {
        let pigs: Vec<_> = self
            .pigs
            .iter()
            .map(|(n, (e, _))| (n.clone(), *e))
            .collect();
        pigs.into_iter()
            .filter_map(|(name, pig)| {
                self.world()
                    .get::<RuntimeId>(pig)
                    .map(|id| (id.0 as u64, name))
            })
            .collect()
    }

    fn route(&mut self, packet: McpePacket) {
        let mut buf = BytesMut::new();
        packet.data.encode_inner(&mut buf, 0, 0).unwrap();
        let borrowed = BorrowedMcpePacket::decode_inner(&mut buf.freeze()).unwrap();
        self.server.route_packet(SESSION, McpePacketArgs, borrowed);
    }

    fn drain(&mut self) -> Vec<McpePacketData> {
        std::iter::from_fn(|| self.rx.try_recv().ok().map(|packet| packet.data)).collect()
    }
}

/// The script's set-up before scenario `index`.
fn set_up(h: &mut Harness, index: usize) {
    h.clear_pigs();
    let player = h.player;
    h.world()
        .entity_mut(player)
        .remove::<crate::item::cooldown::ItemCooldowns>();
    match index {
        0 => {
            h.hotbar(0);
            for (name, x) in [("a1", 1.5), ("a2", 2.5), ("a3", 3.5), ("a4", 4.5)] {
                h.pig(name, x, 0.5);
            }
        }
        1 => {
            h.hotbar(0);
            for (name, x, z) in [
                ("b2", 2.5, 0.5),
                ("b3", 3.5, 0.5),
                ("b4", 4.5, 0.5),
                ("b5", 2.5, 1.5),
            ] {
                h.pig(name, x, z);
            }
        }
        2 => {
            h.hotbar(0);
            h.pig("c3", 3.5, 0.5);
        }
        // The sword selected at the end of the jabs, a netherite spear in
        // slot 0.
        3 => {
            h.hotbar(2);
            h.main_hand("minecraft:netherite_spear");
        }
        4 => {
            h.hotbar(0);
            h.main_hand("minecraft:netherite_spear");
        }
        _ => {
            h.hotbar(0);
            for (name, x) in [("r5", 5.5), ("r6", 6.5), ("r7", 7.5), ("r8", 8.5)] {
                h.pig(name, x, 0.5);
            }
        }
    }
    // Viewers see the pigs before the scenario starts.
    for _ in 0..3 {
        h.tick();
    }
    h.drain();
}

/// The script's `/replaceitem` steps inside the jab scenario.
fn in_window_step(h: &mut Harness, index: usize, input: u64) {
    let tier = match (index, input) {
        (2, 350) => "wooden",
        (2, 380) => "stone",
        (2, 410) => "copper",
        (2, 440) => "golden",
        (2, 470) => "netherite",
        _ => return,
    };
    h.main_hand(&format!("minecraft:{tier}_spear"));
}

/// A client packet with BDS runtime ids replaced by ours.
fn remapped(
    packet: &McpePacketData,
    ours: &HashMap<String, u64>,
    bds: &HashMap<u64, String>,
) -> McpePacket {
    let mut packet = packet.clone();
    if let McpePacketData::InventoryTransactionPacket(transaction) = &mut packet
        && let InventoryTransactionPacketTransaction::ItemUseOnActorInventoryTransaction(attack) =
            &mut transaction.transaction
    {
        let name = &bds[&attack.runtime_id.actor_runtime_id];
        attack.runtime_id.actor_runtime_id = ours[name];
    }
    McpePacket::from(packet)
}

#[test]
fn spear_hits_jabs_and_swings_answer_like_bds() {
    let records = records();
    let bds_names = bds_names(&records);
    let mut h = Harness::new();
    let mut failures = Vec::new();
    for (index, scenario) in SCENARIOS.iter().enumerate() {
        set_up(&mut h, index);
        let ours_by_name: HashMap<String, u64> =
            h.names().into_iter().map(|(id, name)| (name, id)).collect();
        let our_names = h.names();
        let window: Vec<&Record> = records
            .iter()
            .filter(|record| scenario.inputs.contains(&record.input))
            .collect();
        let mut using = false;
        let bds: Vec<Event> = window
            .iter()
            .filter(|record| !record.from_client)
            .filter_map(|record| event(&record.packet, &bds_names, BDS_PLAYER, &mut using))
            .collect();
        let mut ours = Vec::new();
        let mut using = false;
        for record in window.iter().filter(|record| record.from_client) {
            if let McpePacketData::PlayerAuthInputPacket(_) = &record.packet {
                in_window_step(&mut h, index, record.input);
                h.route(McpePacket::from(record.packet.clone()));
                h.tick();
                for packet in h.drain() {
                    ours.extend(event(&packet, &our_names, SESSION, &mut using));
                }
            } else {
                h.route(remapped(&record.packet, &ours_by_name, &bds_names));
            }
        }
        h.tick();
        for packet in h.drain() {
            ours.extend(event(&packet, &our_names, SESSION, &mut using));
        }
        if bds != ours {
            failures.push(format!(
                "{}:\n  BDS  {bds:?}\n  ours {ours:?}",
                scenario.name
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

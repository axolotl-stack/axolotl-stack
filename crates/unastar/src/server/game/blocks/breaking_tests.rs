//! Survival block breaking against BDS 1.26.51, input by input.
//!
//! `testdata/bds-breaking.jsonl` is cut (`tools/bds-sweep/breaking_fixture.py`)
//! from a `bds-sweep-client` capture of `tools/bds-sweep/scripts/breaking.txt`
//! played against plain BDS 1.26.51 (`tools/bds-sweep/script.py`,
//! 2026-11-10): every `PlayerAuthInput` of twelve scenarios and the server's
//! `LevelEvent`, `UpdateBlock`, `AddItemActor`, `InventorySlot` and
//! `LevelSoundEvent` packets. Each scenario is set up here as the script set
//! it up, its captured inputs are routed through the server one per tick
//! (ten more ticks where the client froze, and inputs the client sent at once
//! in one tick), and the replies are compared with BDS's: the crack and
//! destroy events in order, and the block updates, drops and tool wear in
//! order. BDS sometimes repeats the block and block-above update pair of a
//! break in the same tick; the repeat is not compared.

use bytes::BytesMut;
use glam::{DVec3, IVec3};
use jolyne::valentine::{
    BorrowedMcpePacket, McpePacket, McpePacketArgs, McpePacketData, PlayerAuthInputPacket,
};
use tokio::sync::mpsc::Receiver;

use crate::entity::components::{EffectType, GameMode, MainInventory};
use crate::entity::systems::BlockSource;
use crate::registry::BlockRegistry;
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};

const BDS: &str = include_str!("testdata/bds-breaking.jsonl");
const SESSION: u64 = 7;
/// The sweep arena: the player's feet, facing east at the west face of the
/// mined block.
const FEET: DVec3 = DVec3::new(0.5, -60.0, 0.5);

/// A capture record: who sent it, the client tick of the latest input, when
/// it was recorded and the packet.
struct Record {
    from_client: bool,
    input: u64,
    t_ms: u64,
    packet: McpePacketData,
}

fn records() -> Vec<Record> {
    BDS.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            Record {
                from_client: row["dir"] == "c2s",
                input: row["input"].as_u64().unwrap(),
                t_ms: row["t_ms"].as_u64().unwrap(),
                packet: crate::server::game::inventory::capture::decode(
                    row["hex"].as_str().unwrap(),
                ),
            }
        })
        .collect()
}

/// One scenario of the script: its window of client input ticks.
struct Scenario {
    name: &'static str,
    inputs: std::ops::Range<u64>,
}

const SCENARIOS: [Scenario; 12] = [
    Scenario {
        name: "dirt by hand",
        inputs: 60..100,
    },
    Scenario {
        name: "stone with a diamond pickaxe",
        inputs: 120..150,
    },
    Scenario {
        name: "short grass, instant",
        inputs: 160..180,
    },
    Scenario {
        name: "dirt, aborted",
        inputs: 190..210,
    },
    Scenario {
        name: "dirt, frozen client",
        inputs: 220..270,
    },
    Scenario {
        name: "dirt with Haste II",
        inputs: 280..310,
    },
    Scenario {
        name: "dirt under stone",
        inputs: 320..360,
    },
    Scenario {
        name: "dirt, predicted on time",
        inputs: 370..410,
    },
    Scenario {
        name: "dirt, predicted early",
        inputs: 420..455,
    },
    Scenario {
        name: "dirt, released after 5 inputs, then put back",
        inputs: 470..510,
    },
    Scenario {
        name: "air of the position still being broken, dirt put back",
        inputs: 510..550,
    },
    Scenario {
        name: "dirt, 14 inputs at once",
        inputs: 570..640,
    },
];

/// What the comparison looks at, positions near the arena only.
#[derive(Debug, Clone, PartialEq)]
enum Event {
    /// `LevelEvent` id, position doubled (the crack events sit on block
    /// corners and centres), data.
    Level(i32, [i32; 3], i32),
    Block([i32; 3], u32, u32),
    Drop(i16, u16),
    Wear(u32, i16, u16),
    Sound(String),
}

/// Near the arena, or the origin (where a `StopDestroyBlock`'s default
/// position puts its crack stop).
fn near(x: f32, y: f32, z: f32) -> bool {
    (-1.0..=3.0).contains(&x) && (-60.0..=-57.0).contains(&y) && (-1.0..=3.0).contains(&z)
        || (x, y, z) == (0.0, 0.0, 0.0)
}

/// The compared events of `packet`; `tool` is the network id of a held tool
/// whose wear counts (other slot updates are pickups).
fn event(packet: &McpePacketData, tool: Option<i16>) -> Option<Event> {
    match packet {
        McpePacketData::LevelEventPacket(event) => {
            let p = &event.position;
            near(p.x, p.y, p.z).then(|| {
                let doubled = [p.x, p.y, p.z].map(|v| (v * 2.0).round() as i32);
                Event::Level(event.event_id, doubled, event.data)
            })
        }
        McpePacketData::UpdateBlockPacket(update) => {
            let p = &update.block_position;
            near(p.x as f32, p.y as f32, p.z as f32).then_some(Event::Block(
                [p.x, p.y, p.z],
                update.block_runtime_id,
                update.flags,
            ))
        }
        McpePacketData::AddItemActorPacket(item) => {
            Some(Event::Drop(item.item.id, item.item.stacksize))
        }
        McpePacketData::InventorySlotPacket(slot)
            if slot.container_id == 0 && Some(slot.item.id) == tool =>
        {
            Some(Event::Wear(slot.slot, slot.item.id, slot.item.stacksize))
        }
        McpePacketData::LevelSoundEventPacket(sound) => {
            Some(Event::Sound(sound.sound_event.to_string()))
        }
        _ => None,
    }
}

/// `(crack and destroy events, everything else)`; the else list without a
/// repeated block-and-above update pair and without a block update repeated
/// right after itself (both no-ops for the client).
fn split(events: Vec<Event>) -> (Vec<Event>, Vec<Event>) {
    let (levels, mut rest): (Vec<_>, Vec<_>) = events
        .into_iter()
        .partition(|event| matches!(event, Event::Level(..)));
    rest.dedup_by(|a, b| matches!(a, Event::Block(..)) && a == b);
    let mut i = 0;
    while i + 4 <= rest.len() {
        let pair = matches!(
            (&rest[i], &rest[i + 1]),
            (Event::Block(a, ..), Event::Block(b, ..)) if b[1] == a[1] + 1
        );
        if pair && rest[i..i + 2] == rest[i + 2..i + 4] {
            rest.drain(i + 2..i + 4);
        } else {
            i += 1;
        }
    }
    (levels, rest)
}

struct Harness {
    server: GameServer,
    player: bevy_ecs::entity::Entity,
    rx: Receiver<McpePacket>,
}

impl Harness {
    fn new() -> Self {
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..ServerConfig::default()
        });
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
        let mut h = Self { server, player, rx };
        for _ in 0..40 {
            if [(-1, -1), (-1, 1), (1, -1), (1, 1)]
                .iter()
                .all(|&(x, z)| h.world().block_at(IVec3::new(x, -61, z)).is_some())
            {
                break;
            }
            h.tick();
        }
        *h.world().get_mut::<GameMode>(player).unwrap() = GameMode::Survival;
        for x in -4..=4 {
            for z in -4..=4 {
                h.set(IVec3::new(x, -61, z), "minecraft:stone");
            }
        }
        h.tick();
        h.drain();
        h
    }

    fn world(&mut self) -> &mut bevy_ecs::world::World {
        self.server.ecs.world_mut()
    }

    fn set(&mut self, pos: IVec3, name: &str) {
        let state = BlockRegistry::vanilla()
            .default_state_id_by_name(name)
            .unwrap();
        super::set_block(self.world(), pos, state, false);
    }

    /// The held hotbar slot 0 (the script's `/clear` and `/give`).
    fn hold(&mut self, item: Option<&str>) {
        let player = self.player;
        let stack = item.map_or_else(crate::item::ItemStack::empty, |name| {
            crate::item::test_support::stack(name, 1)
        });
        let mut inventory = self.world().get_mut::<MainInventory>(player).unwrap();
        for slot in 0..36 {
            let _ = inventory.0.set_item(slot, crate::item::ItemStack::empty());
        }
        inventory.0.set_item(0, stack).unwrap();
    }

    fn route(&mut self, input: PlayerAuthInputPacket) {
        let packet = McpePacket::from(input);
        let mut buf = BytesMut::new();
        packet.data.encode_inner(&mut buf, 0, 0).unwrap();
        let borrowed = BorrowedMcpePacket::decode_inner(&mut buf.freeze()).unwrap();
        self.server.route_packet(SESSION, McpePacketArgs, borrowed);
    }

    fn drain(&mut self) -> Vec<McpePacketData> {
        std::iter::from_fn(|| self.rx.try_recv().ok().map(|packet| packet.data)).collect()
    }

    /// One server tick, then the plugin manager's default for the tick's
    /// events when no plugin cancels them: block changes commit.
    fn tick(&mut self) {
        self.server.tick();
        let events = self.world().resource_mut::<super::EventBuffer>().drain();
        for event in events {
            if matches!(
                event,
                super::ServerEvent::BlockBreak { .. } | super::ServerEvent::BlockPlace { .. }
            ) {
                super::commit_block_event(self.world(), &event);
            }
        }
    }
}

/// The script's set-up before scenario `index`, and its steps inside the
/// window (by client input tick).
fn set_up(h: &mut Harness, index: usize) {
    let player = h.player;
    let mined = IVec3::new(1, -59, 0);
    h.hold(None);
    h.set(IVec3::new(1, -58, 0), "minecraft:air");
    match index {
        1 => {
            h.set(mined, "minecraft:stone");
            h.hold(Some("minecraft:diamond_pickaxe"));
        }
        2 => h.set(mined, "minecraft:short_grass"),
        5 => {
            h.set(mined, "minecraft:dirt");
            crate::server::game::effects::add_effect(
                h.world(),
                player,
                EffectType::HASTE,
                1,
                20000,
            );
        }
        6 => {
            h.set(IVec3::new(1, -58, 0), "minecraft:stone");
            h.set(mined, "minecraft:dirt");
        }
        8 => h.set(IVec3::new(1, -59, 1), "minecraft:dirt"),
        // The break of the scenario before goes on over this air.
        10 => {}
        11 => h.set(IVec3::new(1, -59, -1), "minecraft:dirt"),
        _ => h.set(mined, "minecraft:dirt"),
    }
    h.tick();
    h.drain();
}

fn in_window_step(h: &mut Harness, index: usize, input: u64) {
    match (index, input) {
        (5, 301) => {
            let player = h.player;
            crate::server::game::effects::clear_effects(h.world(), player);
        }
        (6, 351) => h.set(IVec3::new(1, -58, 0), "minecraft:air"),
        (9, 488) | (10, 512) => h.set(IVec3::new(1, -59, 0), "minecraft:dirt"),
        _ => {}
    }
}

#[test]
fn survival_breaking_answers_every_input_like_bds() {
    let records = records();
    let mut h = Harness::new();
    let tool = crate::item::test_support::stack("minecraft:diamond_pickaxe", 1)
        .item()
        .unwrap()
        .network_id() as i16;
    let mut failures = Vec::new();
    for (index, scenario) in SCENARIOS.iter().enumerate() {
        set_up(&mut h, index);
        let window: Vec<&Record> = records
            .iter()
            .filter(|record| scenario.inputs.contains(&record.input))
            .collect();
        let bds: Vec<Event> = window
            .iter()
            .filter(|record| !record.from_client)
            .filter_map(|record| event(&record.packet, Some(tool)))
            .collect();
        let inputs: Vec<(u64, u64, &PlayerAuthInputPacket)> = window
            .iter()
            .filter(|record| record.from_client)
            .map(|record| match &record.packet {
                McpePacketData::PlayerAuthInputPacket(input) => {
                    (record.input, record.t_ms, &**input)
                }
                _ => panic!("client records are inputs"),
            })
            .collect();
        let mut ours = Vec::new();
        let mut last_input_at: Option<u64> = None;
        let mut next = 0;
        while next < inputs.len() {
            // A frozen client: server ticks without input.
            if last_input_at.is_some_and(|at| inputs[next].1 > at + 300) {
                for _ in 0..10 {
                    h.tick();
                }
            }
            // Inputs sent at once reach the server in one tick.
            let mut end = next + 1;
            while end < inputs.len() && inputs[end].1 <= inputs[end - 1].1 + 10 {
                end += 1;
            }
            for &(tick, _, input) in &inputs[next..end] {
                in_window_step(&mut h, index, tick);
                h.route(input.clone());
            }
            last_input_at = Some(inputs[end - 1].1);
            next = end;
            h.tick();
            ours.extend(
                h.drain()
                    .iter()
                    .filter_map(|packet| event(packet, Some(tool))),
            );
        }
        let (bds_levels, bds_rest) = split(bds);
        let (our_levels, our_rest) = split(ours);
        if bds_levels != our_levels {
            failures.push(format!(
                "{}: crack and destroy events\n  BDS  {bds_levels:?}\n  ours {our_levels:?}",
                scenario.name
            ));
        }
        if bds_rest != our_rest {
            failures.push(format!(
                "{}: block updates, drops, wear and sounds\n  BDS  {bds_rest:?}\n  ours {our_rest:?}",
                scenario.name
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

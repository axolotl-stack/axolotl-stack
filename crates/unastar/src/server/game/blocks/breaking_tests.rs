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
//!
//! Every scenario rolls from its own seed. A drop that is random on BDS
//! (short grass's wheat seeds) is not compared with the one roll the capture
//! happened to make: on both sides it must be an outcome the BDS drop sweep
//! measured for that block and held item
//! (`unastar-data/data/measured/block_drops/`).
//!
//! `testdata/bds-breaking-2.jsonl` (`tools/bds-sweep/scripts/breaking-2.txt`,
//! played with `script.py --observer scripts/breaking-observer.txt`) adds a
//! second player standing 3 blocks west, whose packets are compared the same
//! way, and creative breaking.

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
/// The BDS drop sweep's outcomes for short grass, per held item.
const SHORT_GRASS_DROPS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../unastar-data/data/measured/block_drops/short_grass.json"
));
const BDS_2: &str = include_str!("testdata/bds-breaking-2.jsonl");
const SESSION: u64 = 7;
const OBSERVER_SESSION: u64 = 8;
/// The sweep arena: the player's feet, facing east at the west face of the
/// mined block.
const FEET: DVec3 = DVec3::new(0.5, -60.0, 0.5);
/// Where `breaking-2.txt`'s second player stands: 3 blocks west, facing east.
const OBSERVER_FEET: DVec3 = DVec3::new(-2.5, -60.0, 0.5);

/// A capture record: who sent it (the client, or the server to the player
/// or to the observer), the client tick of the latest input, when it was
/// recorded and the packet.
struct Record {
    from_client: bool,
    to_observer: bool,
    input: u64,
    t_ms: u64,
    packet: McpePacketData,
}

fn records() -> Vec<Record> {
    records_of(BDS)
}

fn records_of(capture: &str) -> Vec<Record> {
    capture
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            Record {
                from_client: row["dir"] == "c2s",
                to_observer: row["dir"] == "s2o",
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
    Pickup,
    /// A `PacketViolationWarning`'s context, without BDS's reader-failure
    /// suffix (`crate::network::violation`).
    Violation(String),
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
        McpePacketData::TakeItemActorPacket(_) => Some(Event::Pickup),
        McpePacketData::PacketViolationWarningPacket(warning) => {
            Some(Event::Violation(
                crate::network::violation::redact_violation_message(&warning.violation_context),
            ))
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
    /// A second player's packets, once [`Harness::add_observer`] ran.
    observer_rx: Option<Receiver<McpePacket>>,
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
        let mut h = Self {
            server,
            player,
            rx,
            observer_rx: None,
        };
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

    /// A second survival player at [`OBSERVER_FEET`], once its chunks are
    /// sent.
    fn add_observer(&mut self) {
        let (outbound_tx, rx) = tokio::sync::mpsc::channel(1 << 16);
        let observer = self
            .server
            .spawn_player(PlayerSpawnData {
                session_id: OBSERVER_SESSION,
                display_name: "BDSObserver".into(),
                xuid: None,
                uuid: Some(format!("00000000-0000-0000-0000-{OBSERVER_SESSION:012}")),
                runtime_id: OBSERVER_SESSION as i64,
                position: OBSERVER_FEET + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT,
                outbound_tx,
                saved_data: None,
                chunk_radius: 2,
            })
            .expect("valid observer spawn");
        *self.world().get_mut::<GameMode>(observer).unwrap() = GameMode::Survival;
        self.observer_rx = Some(rx);
        for _ in 0..10 {
            self.tick();
        }
        self.drain();
        self.drain_observer();
    }

    fn drain_observer(&mut self) -> Vec<McpePacketData> {
        let Some(rx) = self.observer_rx.as_mut() else {
            return Vec::new();
        };
        std::iter::from_fn(|| rx.try_recv().ok().map(|packet| packet.data)).collect()
    }

    fn set_mode(&mut self, mode: GameMode) {
        let player = self.player;
        *self.world().get_mut::<GameMode>(player).unwrap() = mode;
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
    // Each scenario's rolls, whatever the scenarios before it rolled.
    h.world()
        .insert_resource(crate::entity::EntityRandom::seeded(index as u64));
}

/// The drops BDS's drop sweep saw from the block scenario `index` breaks,
/// one list per outcome, if that drop is random: short grass by hand gives
/// nothing or one wheat seeds. The sweep's outcomes are `;`-separated
/// `name@data` + `x` + count stacks, `""` for nothing.
fn random_drop_outcomes(index: usize) -> Option<Vec<Vec<Event>>> {
    let (table, held) = match index {
        2 => (SHORT_GRASS_DROPS, "hand"),
        _ => return None,
    };
    let table: serde_json::Value = serde_json::from_str(table).unwrap();
    let case = table["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| {
            case["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool == held)
        })
        .unwrap_or_else(|| panic!("no measured drops with {held}"));
    let outcomes = case["observed"]
        .as_object()
        .unwrap()
        .keys()
        .map(|outcome| {
            outcome
                .split(';')
                .filter(|stack| !stack.is_empty())
                .map(|stack| {
                    let (name, data_count) = stack.rsplit_once('@').unwrap();
                    let (_, count) = data_count.split_once('x').unwrap();
                    let id = crate::item::test_support::stack(name, 1)
                        .item()
                        .unwrap()
                        .network_id() as i16;
                    Event::Drop(id, count.parse().unwrap())
                })
                .collect()
        })
        .collect();
    Some(outcomes)
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
        let (ours, _) = replay(&mut h, &inputs, Some(tool), |h, tick| {
            in_window_step(h, index, tick)
        });
        let (bds_levels, mut bds_rest) = split(bds);
        let (our_levels, mut our_rest) = split(ours);
        if let Some(outcomes) = random_drop_outcomes(index) {
            for (who, rest) in [("BDS", &mut bds_rest), ("ours", &mut our_rest)] {
                let drops: Vec<Event> = rest
                    .iter()
                    .filter(|event| matches!(event, Event::Drop(..)))
                    .cloned()
                    .collect();
                rest.retain(|event| !matches!(event, Event::Drop(..)));
                if !outcomes.contains(&drops) {
                    failures.push(format!(
                        "{}: {who} dropped {drops:?}, which the BDS drop sweep never did \
                         ({outcomes:?})",
                        scenario.name
                    ));
                }
            }
        }
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

/// Routes a scenario's captured inputs, one server tick per input (ten more
/// ticks where the client froze; inputs sent at once in one tick), with
/// `step` before each input; the compared events the player and the
/// observer were sent.
fn replay(
    h: &mut Harness,
    inputs: &[(u64, u64, &PlayerAuthInputPacket)],
    tool: Option<i16>,
    step: impl Fn(&mut Harness, u64),
) -> (Vec<Event>, Vec<Event>) {
    let mut ours = Vec::new();
    let mut observed = Vec::new();
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
            step(h, tick);
            h.route(input.clone());
        }
        last_input_at = Some(inputs[end - 1].1);
        next = end;
        h.tick();
        // The player's captures keep no pickups.
        ours.extend(
            h.drain()
                .iter()
                .filter_map(|packet| event(packet, tool))
                .filter(|event| *event != Event::Pickup),
        );
        observed.extend(
            h.drain_observer()
                .iter()
                .filter_map(|packet| event(packet, tool)),
        );
    }
    (ours, observed)
}

/// The scenarios of `breaking-2.txt`.
const SCENARIOS_2: [Scenario; 13] = [
    Scenario {
        name: "survival: stone with a diamond pickaxe",
        inputs: 80..110,
    },
    Scenario {
        name: "survival: dirt by hand, aborted",
        inputs: 120..130,
    },
    Scenario {
        name: "survival: the same dirt, released (the break goes on)",
        inputs: 130..150,
    },
    Scenario {
        name: "survival: the air the released break left",
        inputs: 150..190,
    },
    Scenario {
        name: "creative: start, then predict",
        inputs: 200..220,
    },
    Scenario {
        name: "creative: start and predict in one input",
        inputs: 230..280,
    },
    Scenario {
        name: "creative: start, continue, abort",
        inputs: 290..300,
    },
    Scenario {
        name: "creative with a diamond sword: start, then predict",
        inputs: 310..330,
    },
    Scenario {
        name: "creative: dirt under stone",
        inputs: 350..370,
    },
    Scenario {
        name: "creative: short grass",
        inputs: 380..391,
    },
    Scenario {
        name: "creative: a torch",
        inputs: 400..420,
    },
    Scenario {
        name: "survival: start and predict in one input, by hand",
        inputs: 430..450,
    },
    Scenario {
        name: "creative: a CreativeDestroyBlock action",
        inputs: 460..480,
    },
];

/// `events` without the block updates that repeat what their position was
/// last sent. BDS sends the block changes of a tick before it reads the next
/// inputs, this server after them: a creative break's update for every viewer
/// comes after the corrections of the next input's refused prediction here,
/// before them on BDS, and the corrections leave it a repeat.
fn changes_only(events: Vec<Event>) -> Vec<Event> {
    let mut last = std::collections::HashMap::new();
    events
        .into_iter()
        .filter(|event| match event {
            Event::Block(position, block, _) => last.insert(*position, *block) != Some(*block),
            _ => true,
        })
        .collect()
}

/// `testdata/bds-creative-break-items.json`: per item, whether a creative
/// start broke stone on BDS (`tools/bds-sweep/scripts/creative-break-items.txt`).
#[test]
fn creative_breaking_items_match_bds() {
    #[derive(serde::Deserialize)]
    struct Table {
        breaks: std::collections::BTreeMap<String, bool>,
    }
    let table: Table =
        serde_json::from_str(include_str!("testdata/bds-creative-break-items.json")).unwrap();
    let wrong: Vec<_> = table
        .breaks
        .iter()
        .filter(|(item, breaks)| {
            super::item_breaks_in_creative(&format!("minecraft:{item}")) != **breaks
        })
        .collect();
    assert!(wrong.is_empty(), "{wrong:?}");
    assert!(table.breaks.values().any(|breaks| !breaks));
}

/// `breaking-2.txt`'s set-up before scenario `index`.
fn set_up_2(h: &mut Harness, index: usize) {
    let mined = IVec3::new(1, -59, 0);
    match index {
        // The aborted dirt is mined again and released; the script's dirt
        // for the next scenario came while that break still went on (a
        // no-op), and the break ended in the window before. No tick
        // between these windows.
        2 | 3 => {}
        0 => {
            h.set_mode(GameMode::Survival);
            h.set(mined, "minecraft:stone");
            h.hold(Some("minecraft:diamond_pickaxe"));
        }
        1 => {
            h.hold(None);
            h.set(mined, "minecraft:dirt");
        }
        4..=6 | 12 => {
            h.set_mode(GameMode::Creative);
            h.hold(None);
            h.set(mined, "minecraft:stone");
        }
        7 => {
            h.set(mined, "minecraft:stone");
            h.hold(Some("minecraft:diamond_sword"));
        }
        8 => {
            h.hold(None);
            h.set(mined, "minecraft:dirt");
            h.set(IVec3::new(1, -58, 0), "minecraft:stone");
        }
        9 => h.set(mined, "minecraft:short_grass"),
        10 => {
            h.set(IVec3::new(1, -60, 0), "minecraft:dirt");
            // BDS's `/setblock ... torch` stands it on the block below.
            let registry = BlockRegistry::vanilla();
            let torch = registry.get_by_name("minecraft:torch").unwrap();
            let top = torch
                .with_state_value(
                    torch.id,
                    "torch_facing_direction",
                    crate::registry::block::StateValue::Str("top"),
                )
                .unwrap();
            super::set_block(h.world(), mined, top, false);
        }
        11 => {
            h.set_mode(GameMode::Survival);
            h.set(mined, "minecraft:stone");
        }
        _ => unreachable!(),
    }
    if !matches!(index, 2 | 3) {
        h.tick();
        h.drain();
        h.drain_observer();
    }
    // Each scenario's rolls, whatever the scenarios before it rolled.
    h.world()
        .insert_resource(crate::entity::EntityRandom::seeded(index as u64));
}

fn in_window_step_2(h: &mut Harness, index: usize, input: u64) {
    match (index, input) {
        (8, 361) => h.set(IVec3::new(1, -58, 0), "minecraft:air"),
        (10, 411) => h.set(IVec3::new(1, -60, 0), "minecraft:air"),
        (11, 440) => h.set(IVec3::new(1, -59, 0), "minecraft:air"),
        _ => {}
    }
}

#[test]
fn breaking_seen_by_another_player_and_in_creative_answers_like_bds() {
    let records = records_of(BDS_2);
    let mut h = Harness::new();
    h.add_observer();
    let tool = crate::item::test_support::stack("minecraft:diamond_pickaxe", 1)
        .item()
        .unwrap()
        .network_id() as i16;
    let mut failures = Vec::new();
    for (index, scenario) in SCENARIOS_2.iter().enumerate() {
        set_up_2(&mut h, index);
        let window: Vec<&Record> = records
            .iter()
            .filter(|record| scenario.inputs.contains(&record.input))
            .collect();
        let sent = |to_observer: bool| -> Vec<Event> {
            window
                .iter()
                .filter(|record| !record.from_client && record.to_observer == to_observer)
                .filter_map(|record| event(&record.packet, Some(tool)))
                .collect()
        };
        let (bds, bds_observed) = (sent(false), sent(true));
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
        let (ours, observed) = replay(&mut h, &inputs, Some(tool), |h, tick| {
            in_window_step_2(h, index, tick)
        });
        // The last scenario ends the player's session on BDS, which the
        // observer hears as the player leaving; only the player's packets
        // are compared there.
        let compared = if index + 1 == SCENARIOS_2.len() { 1 } else { 2 };
        for (who, theirs, mine) in [("player", bds, ours), ("observer", bds_observed, observed)]
            .into_iter()
            .take(compared)
        {
            // Sounds on their own: BDS sends the hit sound of a tick after
            // the block updates and drops the tick before left.
            let sounds = |events: &[Event]| -> Vec<Event> {
                events
                    .iter()
                    .filter(|event| matches!(event, Event::Sound(_)))
                    .cloned()
                    .collect()
            };
            let (bds_sounds, our_sounds) = (sounds(&theirs), sounds(&mine));
            let others = |events: Vec<Event>| -> Vec<Event> {
                events
                    .into_iter()
                    .filter(|event| !matches!(event, Event::Sound(_)))
                    .collect()
            };
            let (bds_levels, bds_rest) = split(others(theirs));
            let (our_levels, our_rest) = split(others(mine));
            let (bds_rest, our_rest) = (changes_only(bds_rest), changes_only(our_rest));
            for (what, theirs, ours) in [
                ("crack and destroy events", bds_levels, our_levels),
                ("block updates, drops and pickups", bds_rest, our_rest),
                ("sounds", bds_sounds, our_sounds),
            ] {
                if theirs != ours {
                    failures.push(format!(
                        "{}, {who}: {what}\n  BDS  {theirs:?}\n  ours {ours:?}",
                        scenario.name
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

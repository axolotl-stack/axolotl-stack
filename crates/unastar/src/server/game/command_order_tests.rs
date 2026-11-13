//! A player's command requests among that player's inputs, against BDS 1.26.51.
//!
//! `testdata/bds-command-order.jsonl` is cut (`tools/bds-sweep/command_order_fixture.py`)
//! from a `bds-sweep-client --script` capture of
//! `tools/bds-sweep/scripts/command-order.txt` played against plain BDS
//! 1.26.51 (2026-11-10): the client's `CommandRequest` and `PlayerAuthInput`
//! packets of four scenarios and the server's crack starts (`LevelEvent`
//! 3600). In three scenarios the client sends a block command (`/setblock`,
//! `/fill`, `/clone`) and, in the same flush right after it, the input that
//! starts breaking that block; in the fourth the start comes one input later.
//! BDS answers each start with the crack speed of the block the command put
//! there. The captured packets are routed through the server in capture
//! order, a tick after each input, and the first crack start of each scenario
//! is compared with BDS's.

use bytes::BytesMut;
use glam::{DVec3, IVec3};
use jolyne::valentine::{BorrowedMcpePacket, McpePacket, McpePacketArgs, McpePacketData};
use tokio::sync::mpsc::Receiver;

use crate::entity::components::GameMode;
use crate::entity::systems::BlockSource;
use crate::registry::BlockRegistry;
use crate::server::game::inventory::capture::decode;
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};

const BDS: &str = include_str!("testdata/bds-command-order.jsonl");
const SESSION: u64 = 7;
const NAME: &str = "BDSSweep";
/// The script's arena: the player's feet, facing east at the west face of
/// the mined block.
const FEET: DVec3 = DVec3::new(0.5, -60.0, 0.5);
const MINED: IVec3 = IVec3::new(1, -59, 0);
const CRACK_START: i32 = 3600;

/// One scenario of the script: its window of latest client input ticks.
const SCENARIOS: [(&str, std::ops::Range<u64>); 4] = [
    ("/setblock, the start in the same flush", 39..47),
    ("/setblock, the start one input later", 59..68),
    ("/fill, the start in the same flush", 79..87),
    ("/clone, the start in the same flush", 99..118),
];

/// A capture record: who sent it, the client tick of the latest input sent
/// before it and the packet.
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
                input: row["input"].as_u64().unwrap(),
                packet: decode(row["hex"].as_str().unwrap()),
            }
        })
        .collect()
}

/// The data of a crack start at the mined block.
fn crack_start(packet: &McpePacketData) -> Option<i32> {
    match packet {
        McpePacketData::LevelEventPacket(event)
            if event.event_id == CRACK_START
                && [event.position.x, event.position.y, event.position.z]
                    == [MINED.x, MINED.y, MINED.z].map(|v| v as f32) =>
        {
            Some(event.data)
        }
        _ => None,
    }
}

struct Harness {
    server: GameServer,
    rx: Receiver<McpePacket>,
}

impl Harness {
    /// The script's arena: an operator in survival at [`FEET`] on a stone
    /// floor at y=-61, air above.
    fn new() -> Self {
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            operators: vec![NAME.into()],
            ..ServerConfig::default()
        });
        let (outbound_tx, rx) = tokio::sync::mpsc::channel(1 << 16);
        let player = server
            .spawn_player(PlayerSpawnData {
                session_id: SESSION,
                display_name: NAME.into(),
                xuid: None,
                uuid: Some(format!("00000000-0000-0000-0000-{SESSION:012}")),
                runtime_id: SESSION as i64,
                position: FEET + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT,
                outbound_tx,
                saved_data: None,
                chunk_radius: 2,
            })
            .expect("valid player spawn");
        let mut h = Self { server, rx };
        for _ in 0..40 {
            if [(-4, -4), (-4, 4), (4, -4), (4, 4)]
                .iter()
                .all(|&(x, z)| h.world().block_at(IVec3::new(x, -61, z)).is_some())
            {
                break;
            }
            h.server.tick();
        }
        *h.world().get_mut::<GameMode>(player).unwrap() = GameMode::Survival;
        let stone = BlockRegistry::vanilla()
            .default_state_id_by_name("minecraft:stone")
            .unwrap();
        for x in -4..=4 {
            for z in -4..=4 {
                super::blocks::set_block(h.world(), IVec3::new(x, -61, z), stone, false);
            }
        }
        h.server.tick();
        h.drain();
        h
    }

    fn world(&mut self) -> &mut bevy_ecs::world::World {
        self.server.ecs.world_mut()
    }

    /// Routes a captured client packet as the network task does: decoded as
    /// a borrowed packet.
    fn route(&mut self, packet: &McpePacketData) {
        let mut buf = BytesMut::new();
        packet.encode_inner(&mut buf, 0, 0).unwrap();
        let borrowed = BorrowedMcpePacket::decode_inner(&mut buf.freeze()).unwrap();
        self.server.route_packet(SESSION, McpePacketArgs, borrowed);
    }

    /// One server tick, then the crack starts it sent.
    fn tick(&mut self) -> Vec<i32> {
        self.server.tick();
        self.drain().iter().filter_map(crack_start).collect()
    }

    fn drain(&mut self) -> Vec<McpePacketData> {
        std::iter::from_fn(|| self.rx.try_recv().ok().map(|packet| packet.data)).collect()
    }

    fn mined_block(&mut self) -> Option<u32> {
        self.world().block_at(MINED)
    }
}

fn is_input(packet: &McpePacketData) -> bool {
    matches!(packet, McpePacketData::PlayerAuthInputPacket(_))
}

#[test]
fn a_block_command_is_in_place_for_the_input_sent_after_it_like_bds() {
    let records = records();
    let mut h = Harness::new();
    let mut failures = Vec::new();
    for (name, inputs) in SCENARIOS {
        let window: Vec<&Record> = records
            .iter()
            .filter(|record| inputs.contains(&record.input))
            .collect();
        let bds = window
            .iter()
            .filter(|record| !record.from_client)
            .find_map(|record| crack_start(&record.packet));
        let mut ours = Vec::new();
        for record in window.iter().filter(|record| record.from_client) {
            h.route(&record.packet);
            if is_input(&record.packet) {
                ours.extend(h.tick());
            }
        }
        // Commands sent after the window's last input.
        ours.extend(h.tick());
        if bds.is_none() || ours.first().copied() != bds {
            failures.push(format!(
                "{name}: first crack start BDS {bds:?}, ours {:?}",
                ours.first()
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The other half of arrival order: a request that arrives after an input
/// of the same tick runs after it, so the start finds the old block.
#[test]
fn a_command_sent_after_an_input_waits_for_it() {
    let records = records();
    let sent: Vec<&McpePacketData> = records
        .iter()
        .filter(|record| record.from_client && SCENARIOS[0].1.contains(&record.input))
        .map(|record| &record.packet)
        .collect();
    // The scenario's `/setblock` and the start sent right after it.
    let at = sent
        .iter()
        .position(|packet| matches!(packet, McpePacketData::CommandRequestPacket(_)))
        .unwrap();
    let (command, start) = (sent[at], sent[at + 1]);
    assert!(is_input(start));

    let mut h = Harness::new();
    let air = h.mined_block();
    h.route(start);
    let on_air = h.tick();

    let mut h = Harness::new();
    h.route(start);
    h.route(command);
    assert_eq!(
        h.tick(),
        on_air,
        "the start found the block the command set"
    );
    assert_ne!(h.mined_block(), air, "the command ran in the same tick");
}

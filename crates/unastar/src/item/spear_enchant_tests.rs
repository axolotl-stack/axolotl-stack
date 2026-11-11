//! Which enchantments `/enchant` puts on each spear, against BDS 1.26.51.
//!
//! `testdata/bds-enchant-spears.jsonl` is cut
//! (`tools/bds-sweep/combat_fixture.py --commands enchant`) from a
//! `bds-sweep-client` capture of `tools/bds-sweep/scripts/enchant-spears.txt`
//! played against plain BDS 1.26.51 (`tools/bds-sweep/script.py`,
//! 2026-11-10). For every spear, wooden to netherite, the script put a fresh
//! spear in the main hand and ran `/enchant @s <enchantment> 1` for all 42
//! enchantments, then Lunge at levels 3 and 4: 44 commands per spear. The
//! fixture keeps each `/enchant` request and the `CommandOutput` BDS answered
//! it with. Each is replayed here with the same fresh spear held, and the
//! outputs must match: Sharpness, Smite, Bane of Arthropods, Knockback, Fire
//! Aspect, Looting, Unbreaking, Mending, Curse of Vanishing and Lunge (up to
//! level 3) succeed; every other enchantment cannot go on a spear.

use bytes::BytesMut;
use glam::DVec3;
use jolyne::valentine::{BorrowedMcpePacket, McpePacket, McpePacketArgs, McpePacketData};

use crate::entity::components::MainInventory;
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};

const BDS: &str = include_str!("testdata/bds-enchant-spears.jsonl");
const SESSION: u64 = 9;
const TIERS: [&str; 7] = [
    "wooden",
    "stone",
    "copper",
    "iron",
    "golden",
    "diamond",
    "netherite",
];
/// `/enchant` commands per spear in the script.
const PER_SPEAR: usize = 44;

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

/// One output message: its id, success and parameters.
type Message = (String, bool, Vec<String>);

/// What a `CommandOutput` says: the success count and its messages.
fn output(packet: &McpePacketData) -> Option<(u32, Vec<Message>)> {
    let McpePacketData::CommandOutputPacket(output) = packet else {
        return None;
    };
    Some((
        output.output.success_count,
        output
            .output
            .output_messages
            .iter()
            .map(|message| {
                (
                    message.message_id.to_string(),
                    message.successful,
                    message.parameters.iter().map(ToString::to_string).collect(),
                )
            })
            .collect(),
    ))
}

#[test]
fn spears_take_the_enchantments_bds_puts_on_them() {
    let records: Vec<(bool, McpePacketData)> = BDS
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            (row["dir"] == "c2s", decode(row["hex"].as_str().unwrap()))
        })
        .collect();
    let requests: Vec<&McpePacketData> = records
        .iter()
        .filter(|(from_client, _)| *from_client)
        .map(|(_, packet)| packet)
        .collect();
    let answers: Vec<_> = records
        .iter()
        .filter(|(from_client, _)| !from_client)
        .filter_map(|(_, packet)| output(packet))
        .collect();
    assert_eq!(requests.len(), TIERS.len() * PER_SPEAR);
    assert_eq!(answers.len(), requests.len());

    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        operators: vec!["BDSSweep".into()],
        ..ServerConfig::default()
    });
    let (outbound_tx, mut rx) = tokio::sync::mpsc::channel(1 << 16);
    let player = server
        .spawn_player(PlayerSpawnData {
            session_id: SESSION,
            display_name: "BDSSweep".into(),
            xuid: None,
            uuid: Some(format!("00000000-0000-0000-0000-{SESSION:012}")),
            runtime_id: SESSION as i64,
            position: DVec3::new(0.5, 17.0, 0.5),
            outbound_tx,
            saved_data: None,
            chunk_radius: 2,
        })
        .expect("valid player spawn");
    server.tick();
    while rx.try_recv().is_ok() {}

    let mut wrong = Vec::new();
    for (index, (request, bds)) in requests.iter().zip(&answers).enumerate() {
        let spear = format!("minecraft:{}_spear", TIERS[index / PER_SPEAR]);
        server
            .ecs
            .world_mut()
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, crate::item::test_support::stack(&spear, 1))
            .unwrap();
        let mut buf = BytesMut::new();
        McpePacket::from((*request).clone())
            .data
            .encode_inner(&mut buf, 0, 0)
            .unwrap();
        let borrowed = BorrowedMcpePacket::decode_inner(&mut buf.freeze()).unwrap();
        server.route_packet(SESSION, McpePacketArgs, borrowed);
        server.tick();
        let ours: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
            .filter_map(|packet| output(&packet.data))
            .collect();
        let McpePacketData::CommandRequestPacket(command) = request else {
            panic!("client records are command requests");
        };
        if ours.as_slice() != std::slice::from_ref(bds) {
            wrong.push(format!(
                "{spear} {}: BDS {bds:?}, ours {ours:?}",
                command.command
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

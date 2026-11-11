//! The client packets against the BDS 1.26.51 protocol probe fixtures
//! (`crates/bds-extractor/captures/1.26.51/protocol/`, README there): what
//! the other players received, what the sender received, and the timeline
//! showing what BDS did not send.

use super::*;
use crate::entity::components::{Position, SpatialChunk};
use crate::server::broadcast::EntityGrid;
use crate::server::game::GameServer;
use glam::DVec3;
use jolyne::valentine::types::EnumsAnimatePacketPayloadAction;
use jolyne::valentine::{AnimatePacket, McpePacketData};
use tokio::sync::mpsc;
use valentine::bedrock::codec::BedrockCodec;

pub(crate) const PROTOCOL: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../bds-extractor/captures/1.26.51/protocol"
);

/// Server packets that arrive whatever the client does: world and actor
/// traffic, clock and chunk updates.
const AMBIENT: [&str; 21] = [
    "ActorEventPacket",
    "AddActorPacket",
    "AddItemActorPacket",
    "CurrentStructureFeaturePacket",
    "LevelChunkPacket",
    "LevelEventPacket",
    "LevelSoundEventPacket",
    "MobArmorEquipmentPacket",
    "MobEquipmentPacket",
    "MoveActorAbsolutePacket",
    "MoveActorDeltaPacket",
    "NetworkChunkPublisherUpdatePacket",
    "NetworkStackLatencyPacket",
    "RemoveActorPacket",
    "SetActorDataPacket",
    "SetActorMotionPacket",
    "SubChunkPacket",
    "SyncWorldClocksPacket",
    "UpdateAttributesPacket",
    "UpdateBlockPacket",
    "UpdateSubChunkBlocksPacket",
];

/// The runs whose guest sent the relay script (two trials).
const RELAY_RUNS: [&str; 2] = ["relay", "relay-first"];

fn read(relative: &str) -> Vec<u8> {
    let path = format!("{PROTOCOL}/{relative}");
    std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

/// A fixture's payload, without its varuint packet ID header.
pub(crate) fn payload(relative: &str) -> Vec<u8> {
    let bytes = read(relative);
    let header = bytes.iter().position(|byte| byte & 0x80 == 0).unwrap() + 1;
    bytes[header..].to_vec()
}

pub(crate) fn decode<T: BedrockCodec<Args = ()>>(relative: &str) -> T {
    let payload = payload(relative);
    let mut input = payload.as_slice();
    let packet = T::decode(&mut input, ()).unwrap();
    assert!(input.is_empty(), "{relative} has trailing bytes");
    packet
}

pub(crate) fn encode<T: BedrockCodec>(packet: &T) -> Vec<u8> {
    let mut out = Vec::new();
    packet.encode(&mut out).unwrap();
    out
}

/// The fixtures of one packet in a directory, in recording order.
pub(crate) fn files(directory: &str, packet: &str) -> Vec<String> {
    let path = format!("{PROTOCOL}/{directory}");
    let mut found: Vec<String> = std::fs::read_dir(&path)
        .unwrap_or_else(|error| panic!("{path}: {error}"))
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(&format!("-{packet}.bin")))
        .map(|name| format!("{directory}/{name}"))
        .collect();
    found.sort();
    found
}

/// The recording sequence number of a fixture (`<seq>-<Packet>.bin`).
pub(crate) fn seq(file: &str) -> u64 {
    let name = file.rsplit('/').next().unwrap();
    name.split('-').next().unwrap().parse().unwrap()
}

/// The server packets BDS sent after the client packet `seq`, up to the
/// client's next scripted packet, other than [`AMBIENT`] traffic.
fn answers(run: &str, session: &str, sent: u64) -> Vec<String> {
    let text = String::from_utf8(read(&format!("{run}/{session}/timeline.json"))).unwrap();
    let timeline: Vec<(u64, f64, String, String)> = serde_json::from_str(&text).unwrap();
    timeline
        .into_iter()
        .skip_while(|(seq, ..)| *seq <= sent)
        .take_while(|(_, _, dir, name)| {
            dir == "s2c" || name == "NetworkStackLatencyPacket" || name == "PlayerAuthInputPacket"
        })
        .map(|(_, _, _, name)| name)
        .filter(|name| !AMBIENT.contains(&name.as_str()))
        .collect()
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).unwrap())
        .collect()
}

fn player(world: &mut World, runtime_id: i64, x: f64) -> (Entity, mpsc::Receiver<McpePacket>) {
    let (tx, rx) = mpsc::channel(8);
    let position = Position(DVec3::new(x, 64.0, 0.0));
    let entity = world
        .spawn((
            RuntimeEntityId(runtime_id),
            position,
            SpatialChunk::from_position(&position),
            PlayerSession::new(runtime_id as u64, "p".into(), None, None, tx),
        ))
        .id();
    (entity, rx)
}

/// A world with the sender (`runtime_id`) and one other player beside it.
fn pair(
    runtime_id: u64,
) -> (
    World,
    Entity,
    mpsc::Receiver<McpePacket>,
    mpsc::Receiver<McpePacket>,
) {
    let mut world = World::new();
    world.insert_resource(EntityGrid::default());
    let (sender, own) = player(&mut world, runtime_id as i64, 0.5);
    let (_other, other) = player(&mut world, 1000, 2.5);
    (world, sender, own, other)
}

fn sent_payload(packet: &McpePacket) -> Vec<u8> {
    match &packet.data {
        McpePacketData::EmotePacket(p) => encode(p),
        McpePacketData::EmoteListPacket(p) => encode(p),
        McpePacketData::AnimatePacket(p) => encode(p),
        McpePacketData::ClientCacheMissResponsePacket(p) => encode(p),
        McpePacketData::PacketViolationWarningPacket(p) => encode(p),
        other => panic!("unexpected {:?}", other.packet_id()),
    }
}

#[test]
fn emotes_reach_the_other_players_with_the_server_flag_like_bds() {
    for run in RELAY_RUNS {
        let sent = files(&format!("{run}/guest/sent"), "EmotePacket");
        let received = files(&format!("{run}/host"), "EmotePacket");
        assert_eq!(sent.len(), 2, "{run}");
        assert_eq!(
            received.len(),
            sent.len(),
            "{run}: every emote reached the host"
        );
        for (sent, received) in sent.iter().zip(&received) {
            let emote: EmotePacket = decode(sent);
            let (world, sender, mut own, mut other) = pair(emote.actor_runtime_id.actor_runtime_id);
            relay_emote(&world, sender, &emote);
            assert_eq!(
                sent_payload(&other.try_recv().unwrap()),
                payload(received),
                "{received}"
            );
            assert!(own.try_recv().is_err(), "not echoed to the sender");
            assert_eq!(
                answers(run, "guest", seq(sent)),
                Vec::<String>::new(),
                "{sent}"
            );
        }
    }
}

#[test]
fn emote_lists_reach_the_other_players_unchanged_like_bds() {
    for run in RELAY_RUNS {
        let [sent] = &files(&format!("{run}/guest/sent"), "EmoteListPacket")[..] else {
            panic!("{run}: one emote list");
        };
        let [received] = &files(&format!("{run}/host"), "EmoteListPacket")[..] else {
            panic!("{run}: the host got the emote list");
        };
        let list: EmoteListPacket = decode(sent);
        let (world, sender, mut own, mut other) = pair(list.runtimeid.actor_runtime_id);
        relay_emote_list(&world, sender, &list);
        assert_eq!(sent_payload(&other.try_recv().unwrap()), payload(received));
        assert!(own.try_recv().is_err());
        assert_eq!(answers(run, "guest", seq(sent)), Vec::<String>::new());
    }
}

#[test]
fn only_client_swings_are_relayed_like_bds() {
    for run in RELAY_RUNS {
        let sent = files(&format!("{run}/guest/sent"), "AnimatePacket");
        let [received] = &files(&format!("{run}/host"), "AnimatePacket")[..] else {
            panic!("{run}: the host got exactly one Animate");
        };
        // swing, wake-up, critical hit, magic critical hit
        assert_eq!(sent.len(), 4, "{run}");
        for file in &sent {
            let animate: AnimatePacket = decode(file);
            let (mut world, sender, mut own, mut other) =
                pair(animate.target_actor_runtime_id.actor_runtime_id);
            crate::server::broadcast::relay_player_swing(&mut world, sender, &animate);
            if animate.action == EnumsAnimatePacketPayloadAction::Swing {
                assert_eq!(sent_payload(&other.try_recv().unwrap()), payload(received));
            } else {
                assert!(other.try_recv().is_err(), "{file}: {:?}", animate.action);
            }
            assert!(own.try_recv().is_err());
            assert_eq!(
                answers(run, "guest", seq(file)),
                Vec::<String>::new(),
                "{file}"
            );
        }
    }
}

#[test]
fn blob_status_gets_an_empty_miss_response_like_bds() {
    for run in [RELAY_RUNS[0], RELAY_RUNS[1], "cache"] {
        let session = if run == "cache" { "host" } else { "guest" };
        let [sent] = &files(
            &format!("{run}/{session}/sent"),
            "ClientCacheBlobStatusPacket",
        )[..] else {
            panic!("{run}: one blob status");
        };
        let [received] = &files(&format!("{run}/{session}"), "ClientCacheMissResponsePacket")[..]
        else {
            panic!("{run}: one miss response");
        };
        assert!(seq(received) > seq(sent));
        let status: ClientCacheBlobStatusPacket = decode(sent);
        let (world, sender, mut own, _) = pair(1);
        answer_blob_status(&world, sender, &status);
        assert_eq!(sent_payload(&own.try_recv().unwrap()), payload(received));
        assert_eq!(
            answers(run, session, seq(sent)),
            ["ClientCacheMissResponsePacket"]
        );
    }
}

#[test]
fn unknown_screen_close_reasons_end_the_connection_like_bds() {
    let [sent] = &files(
        "relay-refused/guest/sent",
        "ServerboundDataDrivenScreenClosedPacket",
    )[..] else {
        panic!("one refused close");
    };
    let [violation] = &files("relay-refused/guest", "PacketViolationWarningPacket")[..] else {
        panic!("one violation");
    };
    let closed: ServerboundDataDrivenScreenClosedPacket = decode(sent);
    let (world, sender, mut own, mut other) = pair(1);
    screen_closed(&world, sender, &closed);
    let McpePacket {
        data: McpePacketData::PacketViolationWarningPacket(warning),
        ..
    } = own.try_recv().unwrap()
    else {
        panic!("a violation");
    };
    assert_eq!(
        warning.violation_severity,
        EnumsPacketViolationSeverity::Terminatingconnection
    );
    assert_eq!(encode(&warning), payload(violation));
    assert!(other.try_recv().is_err());

    // One session per reason: BDS refused exactly the reasons unastar does,
    // with the same violation.
    let text = String::from_utf8(read("ddui/ddui.json")).unwrap();
    let runs: Vec<serde_json::Value> = serde_json::from_str(&text).unwrap();
    assert_eq!(runs.len(), 15);
    for run in runs {
        let reason = run["reason"].as_str().unwrap();
        let (world, sender, mut own, _) = pair(1);
        screen_closed(
            &world,
            sender,
            &ServerboundDataDrivenScreenClosedPacket {
                form_id: 7,
                close_reason: reason.into(),
            },
        );
        match run["violation_hex"].as_str() {
            None => assert!(own.try_recv().is_err(), "{reason:?} is accepted"),
            Some(hex) => {
                let bds = unhex(hex);
                let header = bds.iter().position(|byte| byte & 0x80 == 0).unwrap() + 1;
                assert_eq!(
                    sent_payload(&own.try_recv().unwrap()),
                    bds[header..],
                    "{reason:?}"
                );
            }
        }
    }
}

/// Client packets BDS answers with nothing, in both relay runs: the
/// interactions, the party and pack packets, a known screen close reason,
/// the data store and the destination cookie. unastar routes them to no
/// answer either.
#[test]
fn packets_bds_ignores_get_no_answer() {
    use jolyne::valentine::McpePacketArgs;
    const IGNORED: [&str; 7] = [
        "InteractPacket",
        "PartyChangedPacket",
        "ResourcePacksReadyForValidationPacket",
        "ServerboundDataDrivenScreenClosedPacket",
        "ServerboundDataStorePacket",
        "PartyDestinationCookieResponsePacket",
        "BookEditPacket",
    ];
    let mut checked = 0;
    for run in RELAY_RUNS {
        for packet in IGNORED {
            for file in files(&format!("{run}/guest/sent"), packet) {
                assert_eq!(
                    answers(run, "guest", seq(&file)),
                    Vec::<String>::new(),
                    "{file}"
                );
                if packet == "InteractPacket" || packet == "BookEditPacket" {
                    // Routed to the inventory systems (items lane).
                    continue;
                }
                let bytes = read(&file);
                let mut framed = Vec::with_capacity(bytes.len() + 5);
                valentine::bedrock::codec::VarUInt(bytes.len() as u32)
                    .encode(&mut framed)
                    .unwrap();
                framed.extend_from_slice(&bytes);
                let (_, data) =
                    McpePacketData::decode_inner_bytes(&mut framed.into(), McpePacketArgs).unwrap();
                let (mut world, sender, mut own, mut other) = pair(1);
                GameServer::route_owned_packet(&mut world, sender, 1, McpePacket::from(data));
                assert!(own.try_recv().is_err(), "{file}");
                assert!(other.try_recv().is_err(), "{file}");
                checked += 1;
            }
        }
    }
    // Interact x6, book edits x7 and the rest x6 per run (party changed twice).
    assert!(checked >= 12, "{checked}");
}

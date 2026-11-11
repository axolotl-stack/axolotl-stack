//! Skins, abilities and the join's per-player abilities against the BDS
//! 1.26.51 protocol probe fixtures
//! (`crates/bds-extractor/captures/1.26.51/protocol/`, README there).

use super::*;
use crate::entity::components::GameMode;
use crate::server::game::join::abilities_packet;
use jolyne::auth::client::default_client_data;
use jolyne::valentine::UpdateAbilitiesPacket;
use std::collections::HashMap;

const PROTOCOL: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../bds-extractor/captures/1.26.51/protocol"
);

fn read(relative: &str) -> Vec<u8> {
    let path = format!("{PROTOCOL}/{relative}");
    std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn payload(relative: &str) -> Vec<u8> {
    let bytes = read(relative);
    let header = bytes.iter().position(|byte| byte & 0x80 == 0).unwrap() + 1;
    bytes[header..].to_vec()
}

fn decode<T: BedrockCodec<Args = ()>>(relative: &str) -> T {
    let payload = payload(relative);
    let mut input = payload.as_slice();
    let packet = T::decode(&mut input, ()).unwrap();
    assert!(input.is_empty(), "{relative} has trailing bytes");
    packet
}

fn encode<T: BedrockCodec>(packet: &T) -> Vec<u8> {
    let mut out = Vec::new();
    packet.encode(&mut out).unwrap();
    out
}

/// The login of a probe player: jolyne's default client data for its name
/// and UUID with the probe's ClientData fields over it, as `bds-extractor
/// --client-data` builds it.
fn probe_login(relative: &str, name: &str, uuid: Uuid) -> ClientData {
    let overrides: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(&read(relative)).unwrap();
    let mut data = serde_json::to_value(default_client_data(name, uuid)).unwrap();
    data.as_object_mut().unwrap().extend(overrides);
    serde_json::from_value(data).unwrap()
}

/// A captured `PlayerList` rebuilt from its players' probe logins (by name)
/// and captured colours encodes to the captured bytes.
fn assert_rebuilds(relative: &str, logins: &HashMap<&str, &str>) {
    let native: PlayerListPacket = decode(relative);
    let entries: Vec<RosterEntry> = native
        .entries
        .iter()
        .map(|entry| {
            let PlayerListPacketEntriesItem::AddEntry(entry) = entry else {
                panic!("{relative}: a removal");
            };
            let name = entry.player_name.to_string();
            let login = probe_login(logins[name.as_str()], &name, entry.uuid);
            login.validate().unwrap();
            RosterEntry::new(
                entry.uuid,
                entry.actor_unique_id.actor_unique_id,
                &name,
                None,
                &login,
                entry.player_color.color as u32,
            )
        })
        .collect();
    assert_eq!(
        encode(&add_packet(&entries)),
        payload(relative),
        "{relative}"
    );
}

#[test]
fn custom_persona_and_float_geometry_skins_match_bds_byte_for_byte() {
    let logins = HashMap::from([
        ("BDSExtractor", "relay/client-data.json"),
        ("BDSExtractor2", "relay/guest-client-data.json"),
        ("BDSExtractor3", "relay/floats-client-data.json"),
    ]);
    for file in [
        // The host's classic skin with cape, geometry and an animated face.
        "relay/host/00012-PlayerListPacket.bin",
        "relay/host/00038-PlayerListPacket.bin",
        // A persona wearing every piece type, as itself and the host see it.
        "relay/guest/00012-PlayerListPacket.bin",
        "relay/guest/00038-PlayerListPacket.bin",
        "relay/host/01366-PlayerListPacket.bin",
        // Floats in the geometry, base64 animation data, an ARGB colour.
        "relay/floats/00038-PlayerListPacket.bin",
        "relay/host/01927-PlayerListPacket.bin",
    ] {
        assert_rebuilds(file, &logins);
    }
    let native: PlayerListPacket = decode("numbers/numbers/00012-PlayerListPacket.bin");
    let PlayerListPacketEntriesItem::AddEntry(entry) = &native.entries[0] else {
        panic!("an addition");
    };
    let name = entry.player_name.to_string();
    assert_rebuilds(
        "numbers/numbers/00012-PlayerListPacket.bin",
        &HashMap::from([(name.as_str(), "numbers/numbers/client-data.json")]),
    );
}

/// The `skins` probe tried one login per piece and tint type (and a few odd
/// values); BDS refused exactly the logins jolyne's validation refuses.
#[test]
fn login_validation_refuses_the_piece_types_bds_refuses() {
    let runs: Vec<serde_json::Value> = serde_json::from_slice(&read("skins/skins.json")).unwrap();
    assert_eq!(runs.len(), 61);
    let base = probe_login("relay/guest-client-data.json", "Skin", Uuid::nil());
    let mut refused = Vec::new();
    for run in &runs {
        let case = run["case"].as_str().unwrap();
        let mut login = base.clone();
        if let Some(kind) = case.strip_prefix("piece-") {
            login.persona_pieces.truncate(1);
            login.persona_pieces[0].piece_type = kind.into();
            login.piece_tint_colours.clear();
        } else if let Some(kind) = case.strip_prefix("tint-") {
            login.piece_tint_colours.truncate(1);
            login.piece_tint_colours[0].piece_type = kind.into();
        } else {
            match case {
                "colour-unparsable" => login.skin_colour = "not-a-colour".into(),
                "colour-argb" => login.skin_colour = "#ffb37b62".into(),
                "arm-empty" => login.arm_size.clear(),
                "floats" | "numbers" => continue,
                other => panic!("unknown case {other}"),
            }
        }
        let bds_refused = !run["violation_hex"].is_null();
        assert_eq!(login.validate().is_err(), bds_refused, "{case}");
        if bds_refused {
            refused.push(case);
        }
    }
    assert_eq!(
        refused,
        [
            "piece-persona_hands",
            "tint-persona_hands",
            "piece-persona_bogus",
            "tint-persona_bogus",
            "tint-bogus_type"
        ]
    );
}

/// Every `UpdateAbilities` of the `abilities` run, rebuilt from the mode
/// and permission the console had given the player.
#[test]
fn abilities_match_bds_in_every_game_mode_and_permission() {
    let guest = "abilities/guest";
    let cases: [(u32, GameMode, bool); 11] = [
        // The join: the player's own abilities, then the full list's.
        (36, GameMode::Survival, true),
        (39, GameMode::Survival, true),
        (40, GameMode::Survival, true),
        // deop/op after each console game mode change.
        (1156, GameMode::Creative, false),
        (1160, GameMode::Creative, true),
        (1167, GameMode::Adventure, false),
        (1171, GameMode::Adventure, true),
        (1179, GameMode::Spectator, false),
        (1182, GameMode::Spectator, true),
        (1190, GameMode::Survival, false),
        (1194, GameMode::Survival, true),
    ];
    for (seq, mode, operator) in cases {
        let file = format!("{guest}/{seq:05}-UpdateAbilitiesPacket.bin");
        let native: UpdateAbilitiesPacket = decode(&file);
        let rebuilt = abilities_packet(native.data.target_player_raw_id, mode, operator);
        assert_eq!(encode(&rebuilt), payload(&file), "{file}");
    }
    // The host received the same bytes for each change.
    for (guest_seq, host_seq) in [(1156, 1548), (1160, 1551), (1179, 1567), (1194, 1579)] {
        assert_eq!(
            payload(&format!("{guest}/{guest_seq:05}-UpdateAbilitiesPacket.bin")),
            payload(&format!(
                "abilities/host/{host_seq:05}-UpdateAbilitiesPacket.bin"
            )),
        );
    }
}

/// A console game mode change: the target and every other player get the
/// same `UpdatePlayerGameType` and nothing about abilities (`protocol/relay`:
/// creative, adventure, spectator and survival for the guest, then
/// creative and survival for the host).
#[test]
fn game_mode_changes_reach_every_player_like_bds() {
    use crate::command::exec::set_game_mode;
    use crate::entity::components::{Player, PlayerSession, RuntimeEntityId};
    use jolyne::valentine::types::EnumsGameType;
    use jolyne::valentine::{McpePacketData, UpdatePlayerGameTypePacket};

    for (guest, host) in [
        (1044, 1395),
        (1047, 1397),
        (1051, 1400),
        (1054, 1404),
        (1058, 1410),
        (1060, 1413),
    ] {
        let guest_file = format!("relay/guest/{guest:05}-UpdatePlayerGameTypePacket.bin");
        let host_file = format!("relay/host/{host:05}-UpdatePlayerGameTypePacket.bin");
        assert_eq!(payload(&guest_file), payload(&host_file));
        let native: UpdatePlayerGameTypePacket = decode(&guest_file);
        let mode = match native.player_game_type {
            EnumsGameType::Survival => GameMode::Survival,
            EnumsGameType::Creative => GameMode::Creative,
            EnumsGameType::Adventure => GameMode::Adventure,
            EnumsGameType::Spectator => GameMode::Spectator,
            other => panic!("{other:?}"),
        };
        let mut world = bevy_ecs::world::World::new();
        let mut receivers = Vec::new();
        let mut target = None;
        for id in [native.targetplayer.actor_unique_id, 1] {
            let (tx, rx) = tokio::sync::mpsc::channel(8);
            let entity = world
                .spawn((
                    Player,
                    RuntimeEntityId(id),
                    GameMode::Survival,
                    PlayerSession::new(id as u64, "p".into(), None, None, tx),
                ))
                .id();
            target.get_or_insert(entity);
            receivers.push(rx);
        }
        set_game_mode(&mut world, target.unwrap(), mode);
        for mut rx in receivers {
            let McpePacketData::UpdatePlayerGameTypePacket(sent) = rx.try_recv().unwrap().data
            else {
                panic!("a game type update");
            };
            assert_eq!(encode(&sent), payload(&guest_file), "{guest_file}");
            assert!(rx.try_recv().is_err(), "nothing else, no abilities");
        }
    }
}

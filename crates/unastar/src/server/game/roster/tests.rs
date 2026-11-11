//! The roster against the BDS 1.26.51 captures: the member join fixtures and
//! the `capture.py --pair` roster fixtures
//! (`crates/bds-extractor/captures/1.26.51/roster/`, README there).

use super::*;
use jolyne::auth::client::default_client_data;
use std::collections::HashMap;

const CAPTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../bds-extractor/captures/1.26.51"
);

/// A captured packet's payload, without its varuint packet ID header.
fn payload(relative: &str) -> Vec<u8> {
    let path = format!("{CAPTURES}/{relative}");
    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
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

fn added(packet: &PlayerListPacket) -> Vec<&PlayerListPacketPayloadAddEntry> {
    packet
        .entries
        .iter()
        .filter_map(|entry| match entry {
            PlayerListPacketEntriesItem::AddEntry(entry) => Some(entry.as_ref()),
            PlayerListPacketEntriesItem::RemoveEntry(_) => None,
        })
        .collect()
}

/// The entry unastar builds for a captured player: the extractor logs in
/// with jolyne's default client data under its name and UUID.
fn rebuilt(captured: &PlayerListPacketPayloadAddEntry, colour: u32) -> RosterEntry {
    let name = captured.player_name.to_string();
    RosterEntry::new(
        captured.uuid,
        captured.actor_unique_id.actor_unique_id,
        &name,
        None,
        &default_client_data(&name, captured.uuid),
        colour,
    )
}

/// A captured `PlayerList`, rebuilt from its players' logins with the
/// colours unastar hands out, encodes to the captured bytes.
fn assert_rebuilds(relative: &str, colours: &HashMap<Uuid, u32>) {
    let native: PlayerListPacket = decode(relative);
    let entries: Vec<RosterEntry> = added(&native)
        .into_iter()
        .map(|entry| rebuilt(entry, colours[&entry.uuid]))
        .collect();
    assert_eq!(
        encode(&add_packet(&entries)),
        payload(relative),
        "{relative}"
    );
}

#[test]
fn own_entry_matches_the_bds_member_join_byte_for_byte() {
    for file in [
        "member/join/00012-PlayerListPacket.bin",
        "member/join/00038-PlayerListPacket.bin",
    ] {
        let native: PlayerListPacket = decode(file);
        let [own] = added(&native)[..] else {
            panic!("{file}: one entry expected");
        };
        let colours = HashMap::from([(own.uuid, free_colour([]))]);
        assert_rebuilds(file, &colours);
    }
}

#[test]
fn second_player_join_and_leave_match_bds_byte_for_byte() {
    let run = "roster/thunder-join-leave";
    let host: PlayerListPacket = decode(&format!("{run}/host/00012-PlayerListPacket.bin"));
    let guest: PlayerListPacket = decode(&format!("{run}/guest/00012-PlayerListPacket.bin"));
    let (host, guest) = (added(&host)[0].uuid, added(&guest)[0].uuid);
    let colours = HashMap::from([
        (host, PLAYER_COLOURS[0]),
        (guest, free_colour([PLAYER_COLOURS[0]])),
    ]);
    // The guest's own entry first, then everyone; the host sees the guest added.
    assert_rebuilds(&format!("{run}/guest/00012-PlayerListPacket.bin"), &colours);
    assert_rebuilds(&format!("{run}/guest/00039-PlayerListPacket.bin"), &colours);
    assert_rebuilds(&format!("{run}/host/01724-PlayerListPacket.bin"), &colours);
    assert_eq!(
        encode(&remove_packet(guest)),
        payload(&format!("{run}/host/01911-PlayerListPacket.bin"))
    );
}

#[test]
fn sleeping_player_counts_match_bds_byte_for_byte() {
    for (file, overworld) in [
        ("member/join/00011-LevelEventGenericPacket.bin", 1),
        (
            "roster/thunder-join-leave/guest/00011-LevelEventGenericPacket.bin",
            2,
        ),
        (
            "roster/thunder-join-leave/host/01723-LevelEventGenericPacket.bin",
            2,
        ),
        (
            "roster/thunder-join-leave/host/01910-LevelEventGenericPacket.bin",
            1,
        ),
    ] {
        assert_eq!(
            encode(&sleeping_players(overworld, 0)),
            payload(file),
            "{file}"
        );
    }
}

#[test]
fn adventure_settings_match_every_bds_join() {
    for file in [
        "member/join/00035-UpdateAdventureSettingsPacket.bin",
        "roster/thunder-join-leave/guest/00036-UpdateAdventureSettingsPacket.bin",
        "roster/thunder-join-leave/host/01726-UpdateAdventureSettingsPacket.bin",
    ] {
        assert_eq!(encode(&adventure_settings()), payload(file), "{file}");
    }
}

/// Replays every `PlayerList` one session saw and checks each newcomer got
/// the colour [`free_colour`] picks for the players online at that moment.
/// Entries up to and including the `seed` file (a join's full list, whose
/// order is not the join order) are recorded without a check.
fn replay_colours(run: &str, session: &str, seed: Option<&str>) -> usize {
    let index: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(format!("{CAPTURES}/{run}/index.json")).unwrap(),
    )
    .unwrap();
    let mut online: Vec<(Uuid, u32)> = Vec::new();
    let mut checked = 0;
    let mut seeding = seed.is_some();
    for record in index[session].as_array().unwrap() {
        if record["packet"] != "PlayerListPacket" {
            continue;
        }
        let file = format!("{run}/{session}/{}", record["file"].as_str().unwrap());
        let packet: PlayerListPacket = decode(&file);
        if seeding {
            for entry in added(&packet) {
                if !online.iter().any(|(uuid, _)| *uuid == entry.uuid) {
                    online.push((entry.uuid, entry.player_color.color as u32));
                }
            }
            seeding = record["file"].as_str() != seed;
            continue;
        }
        for entry in &packet.entries {
            match entry {
                PlayerListPacketEntriesItem::AddEntry(entry) => {
                    if online.iter().any(|(uuid, _)| *uuid == entry.uuid) {
                        continue;
                    }
                    let expected = free_colour(online.iter().map(|(_, colour)| *colour));
                    assert_eq!(
                        entry.player_color.color as u32,
                        expected,
                        "{file}: {} with {} online",
                        entry.player_name,
                        online.len()
                    );
                    online.push((entry.uuid, expected));
                    checked += 1;
                }
                PlayerListPacketEntriesItem::RemoveEntry(entry) => {
                    online.retain(|(uuid, _)| *uuid != entry.uuid);
                }
            }
        }
    }
    checked
}

#[test]
fn player_colours_follow_the_bds_palette_reuse_and_wrap() {
    // 21 players online at once: the palette, then the wrap after 13; then
    // three leave and the next joiner takes the earliest least-held colour.
    assert_eq!(
        replay_colours("roster/twenty-one-players", "host", None),
        21
    );
    // A colour freed by a leaving player goes to the next joiner: B joined
    // with the host and A online, A left, then C took A's colour.
    assert_eq!(
        replay_colours(
            "roster/colour-reuse",
            "B",
            Some("00038-PlayerListPacket.bin")
        ),
        1
    );
}

#[test]
fn build_platform_uses_the_login_device_os_numbers() {
    assert_eq!(build_platform(1), EnumsBuildPlatform::Google);
    assert_eq!(build_platform(7), EnumsBuildPlatform::Uwp);
    assert_eq!(build_platform(15), EnumsBuildPlatform::Linux);
    assert_eq!(build_platform(6), EnumsBuildPlatform::UnknownValue(6));
}

mod server {
    use super::super::PLAYER_COLOURS;
    use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};
    use glam::DVec3;
    use jolyne::valentine::{McpePacket, McpePacketData};
    use tokio::sync::mpsc::{Receiver, channel};

    fn join(server: &mut GameServer, session: u64, name: &str) -> Receiver<McpePacket> {
        let (outbound_tx, outbound_rx) = channel(256);
        let uuid = format!("00000000-0000-0000-0000-{session:012}");
        let client = jolyne::auth::client::default_client_data(name, uuid.parse().unwrap());
        server
            .spawn_player_with_login(
                PlayerSpawnData {
                    session_id: session,
                    display_name: name.into(),
                    xuid: None,
                    uuid: Some(uuid),
                    runtime_id: session as i64,
                    position: DVec3::new(0.5, 17.0, 0.5),
                    outbound_tx,
                    saved_data: None,
                    chunk_radius: 2,
                },
                Some(&client),
            )
            .unwrap();
        outbound_rx
    }

    fn drain(rx: &mut Receiver<McpePacket>) -> Vec<McpePacketData> {
        std::iter::from_fn(|| rx.try_recv().ok().map(|packet| packet.data)).collect()
    }

    /// The targets of the `UpdateAbilities` that directly follow the last
    /// `PlayerList` of a join.
    fn abilities_after_full_list(packets: &[McpePacketData]) -> Vec<i64> {
        let last = packets
            .iter()
            .rposition(|packet| matches!(packet, McpePacketData::PlayerListPacket(_)))
            .expect("a full list");
        packets[last + 1..]
            .iter()
            .map_while(|packet| match packet {
                McpePacketData::UpdateAbilitiesPacket(p) => Some(p.data.target_player_raw_id),
                _ => None,
            })
            .collect()
    }

    fn player_lists(packets: &[McpePacketData]) -> Vec<Vec<String>> {
        packets
            .iter()
            .filter_map(|packet| match packet {
                McpePacketData::PlayerListPacket(list) => Some(
                    super::added(list)
                        .iter()
                        .map(|entry| entry.player_name.to_string())
                        .collect(),
                ),
                _ => None,
            })
            .collect()
    }

    /// Two players join and one leaves: each side gets what BDS sends, in
    /// BDS order, and the first player hears of the second before its
    /// `AddPlayer`.
    #[test]
    fn joins_and_leaves_announce_the_roster_like_bds() {
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..ServerConfig::default()
        });
        let mut host = join(&mut server, 1, "Host");
        let host_join = drain(&mut host);
        assert_eq!(
            player_lists(&host_join),
            [vec!["Host".to_string()], vec!["Host".to_string()]]
        );
        assert!(matches!(
            &host_join[..2],
            [
                McpePacketData::LevelEventGenericPacket(_),
                McpePacketData::PlayerListPacket(_)
            ]
        ));
        // After the full list, each listed player's abilities in list order
        // (`protocol/abilities` joins: the host's, then the joiner's own).
        assert_eq!(abilities_after_full_list(&host_join), [1]);
        let mut guest = join(&mut server, 2, "Guest");
        let guest_join = drain(&mut guest);
        assert_eq!(abilities_after_full_list(&guest_join), [1, 2]);
        assert_eq!(
            player_lists(&guest_join),
            [
                vec!["Guest".to_string()],
                vec!["Host".to_string(), "Guest".to_string()]
            ]
        );
        let guest_entry = super::super::online(server.ecs.world())
            .into_iter()
            .map(|(_, entry)| entry.clone())
            .find(|entry| entry.0.player_name == "Guest")
            .unwrap();
        assert_eq!(guest_entry.colour(), PLAYER_COLOURS[1]);

        server.tick();
        let announced = drain(&mut host);
        let names: Vec<_> = announced.iter().map(|packet| packet.packet_id()).collect();
        let position = |name| names.iter().position(|n| *n == name);
        use jolyne::valentine::McpePacketName as N;
        let (Some(count), Some(list), Some(settings), Some(abilities), Some(add_player)) = (
            position(N::LevelEventGenericPacket),
            position(N::PlayerListPacket),
            position(N::UpdateAdventureSettingsPacket),
            position(N::UpdateAbilitiesPacket),
            position(N::AddPlayerPacket),
        ) else {
            panic!("host did not hear of the guest: {names:?}");
        };
        // `protocol/relay` host: list, adventure settings, the newcomer's
        // abilities, then its AddPlayer.
        assert!(
            count < list && list < settings && settings < abilities && abilities < add_player,
            "{names:?}"
        );
        assert!(matches!(
            &announced[abilities],
            McpePacketData::UpdateAbilitiesPacket(p) if p.data.target_player_raw_id == 2
        ));
        assert_eq!(
            announced[list],
            McpePacketData::PlayerListPacket(super::super::add_packet([&guest_entry]))
        );
        assert_eq!(
            announced[count],
            McpePacketData::LevelEventGenericPacket(super::super::sleeping_players(2, 0))
        );

        server.despawn_player(2);
        let left = drain(&mut host);
        assert_eq!(
            left,
            [
                McpePacketData::LevelEventGenericPacket(super::super::sleeping_players(1, 0)),
                McpePacketData::PlayerListPacket(super::super::remove_packet(guest_entry.uuid())),
            ]
        );
    }
}

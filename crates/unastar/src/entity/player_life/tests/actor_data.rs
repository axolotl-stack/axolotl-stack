//! The player's actor data against BDS 1.26.51
//! (`../testdata/bds-1.26.51-player-data.json`; its `sources` name the
//! captures): `AddPlayer` rows, game-mode, sneak, fire, invisibility, crawl
//! and death updates, the air across a rejoin, Respiration and bubble
//! columns, and the knockback and killing-hit order.
//!
//! Each test replays the captured phase on the void platform and compares
//! what the clients received with the decoded BDS packets. Names, runtime
//! IDs and positions differ between the two and are mapped where they
//! appear in a row (the player's name, the last death block).

use jolyne::valentine::types::{
    BlockPos, DataItemEntry, DataItemEntryPayload, EnumsGameType, PlayerInputTick,
};
use jolyne::valentine::{AddPlayerPacket, PlayerActionPacket, actor_data_i_ds as ids};

use super::*;
use crate::entity::components::{ArmourInventory, EffectType, Effects, Hunger};
use crate::item::test_support::stack;

/// What a client received about one actor that these tests compare.
#[derive(Debug, Clone, PartialEq)]
enum Wire {
    /// The rows of a `SetActorData`, in packet order.
    Data(Vec<DataItemEntry>),
    GameType(EnumsGameType),
    Event(EnumsActorEvent, i32),
    /// A `SetActorMotion` (knockback).
    Motion,
}

fn fixture() -> &'static serde_json::Value {
    static FIXTURE: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    FIXTURE.get_or_init(|| {
        serde_json::from_str(include_str!("../testdata/bds-1.26.51-player-data.json")).unwrap()
    })
}

/// Decode one recorded packet (header and payload, no batch length).
fn decode(hex: &str) -> McpePacketData {
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();
    let mut framed = Vec::with_capacity(bytes.len() + 5);
    let mut length = bytes.len() as u32;
    while length >= 0x80 {
        framed.push(length as u8 | 0x80);
        length >>= 7;
    }
    framed.push(length as u8);
    framed.extend_from_slice(&bytes);
    McpePacketData::decode_inner_bytes(&mut bytes::Bytes::from(framed), McpePacketArgs)
        .unwrap()
        .1
}

/// The packets BDS sent in `phase`.
fn bds(phase: &str) -> Vec<McpePacketData> {
    fixture()["phases"][phase]["packets"]
        .as_array()
        .unwrap_or_else(|| panic!("no phase {phase}"))
        .iter()
        .map(|hex| decode(hex.as_str().unwrap()))
        .collect()
}

/// The runtime ID of the player `phase` is about.
fn bds_subject(phase: &str) -> u64 {
    fixture()["phases"][phase]["subject_runtime_id"]
        .as_u64()
        .unwrap()
}

/// What `packets` tell about the actor `runtime_id`, as [`Wire`].
fn wire(packets: &[McpePacketData], runtime_id: u64) -> Vec<Wire> {
    let mut out = Vec::new();
    for packet in packets {
        match packet {
            McpePacketData::SetActorDataPacket(data)
                if data.target_runtime_id.actor_runtime_id == runtime_id =>
            {
                out.push(Wire::Data(data.actor_data.data.clone()));
            }
            McpePacketData::UpdatePlayerGameTypePacket(game_type) => {
                out.push(Wire::GameType(game_type.player_game_type));
            }
            McpePacketData::ActorEventPacket(event)
                if event.target_runtime_id.actor_runtime_id == runtime_id =>
            {
                out.push(Wire::Event(event.event_id, event.data));
            }
            McpePacketData::SetActorMotionPacket(motion)
                if motion.target_runtime_id.actor_runtime_id == runtime_id =>
            {
                out.push(Wire::Motion);
            }
            _ => {}
        }
    }
    out
}

/// Only the `SetActorData` rows.
fn data(wire: &[Wire]) -> Vec<Wire> {
    wire.iter()
        .filter(|wire| matches!(wire, Wire::Data(_)))
        .cloned()
        .collect()
}

/// The `AddPlayer` packets in `packets`.
fn add_players(packets: &[McpePacketData]) -> Vec<AddPlayerPacket> {
    packets
        .iter()
        .filter_map(|packet| match packet {
            McpePacketData::AddPlayerPacket(add) => Some(add.as_ref().clone()),
            _ => None,
        })
        .collect()
}

/// `rows` with the name row set to `name`.
fn named(rows: &[DataItemEntry], name: &str) -> Vec<DataItemEntry> {
    rows.iter()
        .cloned()
        .map(|mut row| {
            if let DataItemEntryPayload::DataItemStringPayload(value) = &mut row.payload
                && row.id == ids::NAME
            {
                value.value = name.into();
            }
            row
        })
        .collect()
}

/// `wire` with every last-death block row set to `pos`.
fn died_at(wire: &[Wire], pos: IVec3) -> Vec<Wire> {
    wire.iter()
        .cloned()
        .map(|mut wire| {
            if let Wire::Data(rows) = &mut wire {
                for row in rows.iter_mut() {
                    if let DataItemEntryPayload::DataItemPosPayload(value) = &mut row.payload
                        && row.id == ids::PLAYER_LAST_DEATH_POS
                    {
                        value.value = BlockPos {
                            x: pos.x,
                            y: pos.y,
                            z: pos.z,
                        };
                    }
                }
            }
            wire
        })
        .collect()
}

/// A server on the void platform with Steve joined at [`HOME`] and
/// breathing (40 ticks after the join), its join packets drained.
fn server() -> (GameServer, Joined) {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    server
        .ecs
        .world_mut()
        .insert_resource(EntityRandom::seeded(11));
    let mut steve = join(&mut server, SESSION, "Steve", HOME, None);
    for _ in 0..40 {
        server.tick();
    }
    drain(&mut steve);
    (server, steve)
}

/// Route `packet` from `session`'s client as the network task does.
fn route_as(server: &mut GameServer, session: u64, packet: impl Into<McpePacket>) {
    let packet: McpePacket = packet.into();
    let mut buf = BytesMut::new();
    packet.data.encode_inner(&mut buf, 0, 0).unwrap();
    let borrowed = BorrowedMcpePacket::decode_inner(&mut buf.freeze()).unwrap();
    server.route_packet(session, McpePacketArgs, borrowed);
}

/// A still `PlayerAuthInput` at `feet` with `flags`.
fn input(
    feet: DVec3,
    tick: u64,
    flags: &[EnumsPlayerAuthInputPacketPayloadInputData],
) -> PlayerAuthInputPacket {
    let eyes = eyes(feet);
    PlayerAuthInputPacket {
        position: Vec3 {
            x: eyes.x as f32,
            y: eyes.y as f32,
            z: eyes.z as f32,
        },
        input_data: flags.to_vec(),
        client_tick: PlayerInputTick { inputtick: tick },
        ..Default::default()
    }
}

/// Sneak (start, hold for `ticks`, stop) as the scripted BDS client did.
fn sneak(server: &mut GameServer, session: u64, feet: DVec3, ticks: u64) {
    use EnumsPlayerAuthInputPacketPayloadInputData as Flag;
    route_as(
        server,
        session,
        input(
            feet,
            1,
            &[Flag::Sneaking, Flag::Sneakdown, Flag::Startsneaking],
        ),
    );
    server.tick();
    for tick in 0..ticks {
        route_as(
            server,
            session,
            input(feet, 2 + tick, &[Flag::Sneaking, Flag::Sneakdown]),
        );
        server.tick();
    }
    route_as(
        server,
        session,
        input(feet, 2 + ticks, &[Flag::Stopsneaking]),
    );
    server.tick();
    server.tick();
}

#[test]
fn add_player_carries_the_rows_bds_sends_both_ways() {
    let breathing = add_players(&bds("add_player_breathing"));
    let joining = add_players(&bds("add_player_joining"));
    assert_eq!(breathing[0].entity_data.data.len(), 79);
    let (mut server, mut steve) = server();
    let mut alex = join(&mut server, VIEWER, "Alex", HOME + DVec3::X, None);
    server.tick();

    // Alex sees Steve: player index 1, breathing, game type default.
    let seen = add_players(&drain(&mut alex));
    let steve_seen = seen
        .iter()
        .find(|add| add.target_runtime_id.actor_runtime_id == SESSION)
        .expect("AddPlayer of Steve");
    assert_eq!(steve_seen.player_game_type, breathing[0].player_game_type);
    assert_eq!(steve_seen.player_game_type, EnumsGameType::Default);
    assert_eq!(
        steve_seen.entity_data.data,
        named(&breathing[0].entity_data.data, "Steve")
    );

    // Steve sees Alex as it joins: index 2, not breathing yet.
    let seen = add_players(&drain(&mut steve));
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].player_game_type, joining[0].player_game_type);
    assert_eq!(
        seen[0].entity_data.data,
        named(&joining[0].entity_data.data, "Alex")
    );
}

/// A player joining later sees the others as they are: Alex sneaking, and
/// Steve, its game mode set by `/gamemode`, its eyes under water for 62
/// ticks (air 238) after it sneaked once and was invisible once.
#[test]
fn add_player_shows_the_state_a_late_player_finds() {
    let bds = add_players(&bds("add_player_late"));
    let (bds_alex, bds_steve) = (&bds[0], &bds[1]);
    let (mut server, steve) = server();
    let player = steve.entity;
    let alex_feet = HOME + DVec3::X;
    let _alex = join(&mut server, VIEWER, "Alex", alex_feet, None);
    for _ in 0..40 {
        server.tick();
    }
    sneak(&mut server, SESSION, HOME, 5);
    {
        let world = server.ecs.world_mut();
        world
            .get_mut::<Effects>(player)
            .unwrap()
            .add(EffectType::INVISIBILITY, 0, 20);
    }
    for _ in 0..30 {
        server.tick();
    }
    crate::command::exec::set_game_mode(server.ecs.world_mut(), player, GameMode::Survival);
    use EnumsPlayerAuthInputPacketPayloadInputData as Flag;
    route_as(
        &mut server,
        VIEWER,
        input(
            alex_feet,
            1,
            &[Flag::Sneaking, Flag::Sneakdown, Flag::Startsneaking],
        ),
    );
    let feet = HOME.floor().as_ivec3();
    for pos in [feet, feet + IVec3::Y] {
        set_block_in(&mut server, pos, *blocks::WATER);
    }
    for _ in 0..62 {
        server.tick();
    }
    let mut late = join(&mut server, 9, "Late", HOME + DVec3::Z * 2.0, None);
    server.tick();
    let seen = add_players(&drain(&mut late));
    let of = |id: u64| {
        seen.iter()
            .find(|add| add.target_runtime_id.actor_runtime_id == id)
            .unwrap()
    };
    assert_eq!(of(VIEWER).player_game_type, bds_alex.player_game_type);
    assert_eq!(
        of(VIEWER).entity_data.data,
        named(&bds_alex.entity_data.data, "Alex")
    );
    assert_eq!(of(SESSION).player_game_type, bds_steve.player_game_type);
    assert_eq!(of(SESSION).player_game_type, EnumsGameType::Survival);
    assert_eq!(
        of(SESSION).entity_data.data,
        named(&bds_steve.entity_data.data, "Steve")
    );
}

fn set_block_in(server: &mut GameServer, pos: IVec3, block: u32) {
    let world = server.ecs.world_mut();
    let chunk = world
        .resource::<ChunkManager>()
        .get_by_coords(pos.x >> 4, pos.z >> 4)
        .expect("chunk loaded");
    world.get_mut::<ChunkData>(chunk).unwrap().inner.set_block(
        (pos.x & 15) as u8,
        pos.y as i16,
        (pos.z & 15) as u8,
        block,
    );
}

fn player_index(packet: &AddPlayerPacket) -> i32 {
    packet
        .entity_data
        .data
        .iter()
        .find_map(|row| match &row.payload {
            DataItemEntryPayload::DataItemIntPayload(index) if row.id == ids::PLAYER_INDEX => {
                Some(index.value)
            }
            _ => None,
        })
        .unwrap()
}

/// Player indices: the lowest no player in the world has. BDS gave Late 3
/// and Fourth 4 while Viewer (2) was still connected; a player rejoining
/// an empty server is 1 again (`rejoin_under_water`).
#[test]
fn player_indices_are_the_lowest_free_ones() {
    let bds = add_players(&bds("add_player_index"));
    assert_eq!(bds.iter().map(player_index).collect::<Vec<_>>(), [3, 4]);
    let (mut server, mut steve) = server();
    let _alex = join(&mut server, VIEWER, "Alex", HOME + DVec3::X, None);
    let _late = join(&mut server, 9, "Late", HOME + DVec3::Z, None);
    let _fourth = join(&mut server, 10, "Fourth", HOME + DVec3::Z * 2.0, None);
    server.tick();
    let seen = add_players(&drain(&mut steve));
    assert_eq!(seen.iter().map(player_index).collect::<Vec<_>>(), [2, 3, 4]);
    server.despawn_player(VIEWER);
    let mut fifth = join(&mut server, 11, "Fifth", HOME + DVec3::Z * 3.0, None);
    server.tick();
    let joined = add_players(&drain(&mut steve));
    assert_eq!(joined.iter().map(player_index).collect::<Vec<_>>(), [2]);
    drain(&mut fifth);
}

/// Only spectator and back change the flag word (no `HAS_COLLISION` for a
/// spectator); it follows `UpdatePlayerGameType` to the player and its
/// viewers.
#[test]
fn game_mode_changes_send_the_flag_updates_bds_sends() {
    let subject_view = wire(&bds("game_modes"), bds_subject("game_modes"));
    let viewer_view = wire(&bds("game_modes_viewer"), bds_subject("game_modes_viewer"));
    assert_eq!(subject_view, viewer_view);
    assert_eq!(data(&subject_view).len(), 2);
    let (mut server, mut steve) = server();
    let mut alex = join(&mut server, VIEWER, "Alex", HOME + DVec3::X, None);
    for _ in 0..40 {
        server.tick();
    }
    drain(&mut steve);
    drain(&mut alex);
    let player = steve.entity;
    for mode in [
        GameMode::Creative,
        GameMode::Survival,
        GameMode::Spectator,
        GameMode::Survival,
        GameMode::Adventure,
        GameMode::Survival,
    ] {
        crate::command::exec::set_game_mode(server.ecs.world_mut(), player, mode);
        for _ in 0..40 {
            server.tick();
        }
    }
    assert_eq!(wire(&drain(&mut steve), SESSION), subject_view);
    assert_eq!(wire(&drain(&mut alex), SESSION), viewer_view);
}

/// Start and stop sneaking: the three updates of each, to the player and
/// its viewers.
#[test]
fn sneaking_sends_the_updates_bds_sends() {
    let subject_view = data(&wire(&bds("sneak"), bds_subject("sneak")));
    let viewer_view = data(&wire(&bds("sneak_viewer"), bds_subject("sneak_viewer")));
    assert_eq!(subject_view, viewer_view);
    assert_eq!(subject_view.len(), 6);
    let (mut server, mut steve) = server();
    let mut alex = join(&mut server, VIEWER, "Alex", HOME + DVec3::X, None);
    for _ in 0..40 {
        server.tick();
    }
    drain(&mut steve);
    drain(&mut alex);
    sneak(&mut server, SESSION, HOME, 60);
    assert_eq!(data(&wire(&drain(&mut steve), SESSION)), subject_view);
    assert_eq!(data(&wire(&drain(&mut alex), SESSION)), viewer_view);
}

/// A fire block at the feet: `ONFIRE` turns on with the first fire hurt
/// and off when the burning ends.
#[test]
fn burning_sets_the_on_fire_flag() {
    let bds = wire(&bds("fire"), bds_subject("fire"));
    assert_eq!(bds[0], Wire::Event(EnumsActorEvent::Hurt, 6));
    assert!(matches!(bds[1], Wire::Data(_)));
    let (mut server, mut steve) = server();
    let feet = HOME.floor().as_ivec3();
    let fire = crate::world::chunk::blocks::get_block_id("minecraft:fire");
    set_block_in(&mut server, feet, fire);
    for _ in 0..60 {
        server.tick();
    }
    set_block_in(&mut server, feet, *blocks::AIR);
    for _ in 0..400 {
        server.tick();
    }
    let seen = wire(&drain(&mut steve), SESSION);
    assert_eq!(data(&seen), data(&bds));
    assert_eq!(seen[0], Wire::Event(EnumsActorEvent::Hurt, 6));
    assert_eq!(seen[1], data(&bds)[0]);
}

/// Invisibility sets `INVISIBLE`; when it ends, `INVISIBLE` is off and
/// `CAN_SHOW_NAME` on.
#[test]
fn invisibility_ends_with_can_show_name() {
    let bds = data(&wire(&bds("invisibility"), bds_subject("invisibility")));
    assert_eq!(bds.len(), 2);
    let (mut server, mut steve) = server();
    let player = steve.entity;
    server
        .ecs
        .world_mut()
        .get_mut::<Effects>(player)
        .unwrap()
        .add(EffectType::INVISIBILITY, 0, 60);
    for _ in 0..80 {
        server.tick();
    }
    assert_eq!(data(&wire(&drain(&mut steve), SESSION)), bds);
}

/// The flag words of `wire`'s updates that change it.
fn flag_words(wire: &[Wire]) -> Vec<i64> {
    wire.iter()
        .filter_map(|wire| match wire {
            Wire::Data(rows) => rows.iter().find_map(|row| match &row.payload {
                DataItemEntryPayload::DataItemInt64Payload(flags) if row.id == 0 => {
                    Some(flags.value)
                }
                _ => None,
            }),
            _ => None,
        })
        .collect()
}

/// Fire resistance sets `FIRE_IMMUNE`; clearing it (here under water, as in
/// the capture) takes it away.
#[test]
fn fire_resistance_sets_fire_immune() {
    let bds = flag_words(&wire(
        &bds("fire_resistance"),
        bds_subject("fire_resistance"),
    ));
    assert_eq!(bds.len(), 2);
    let mut h = harness(Difficulty::Easy);
    h.rules().natural_regeneration = false;
    h.ticks(5);
    h.drain();
    let player = h.player;
    h.world()
        .get_mut::<Effects>(player)
        .unwrap()
        .add(EffectType::FIRE_RESISTANCE, 0, 1200);
    h.tick();
    let mut seen = flag_words(&wire(&h.drain(), SESSION));
    let feet = HOME.floor().as_ivec3();
    for pos in [feet, feet + IVec3::Y] {
        h.set_block(pos, *blocks::WATER);
    }
    h.ticks(5);
    h.drain();
    h.world()
        .get_mut::<Effects>(player)
        .unwrap()
        .remove(EffectType::FIRE_RESISTANCE);
    h.tick();
    seen.extend(flag_words(&wire(&h.drain(), SESSION)));
    assert_eq!(seen, bds);
}

/// Stone, glass and oak leaves at eye height in turn (BDS breath capture
/// `head_in_solid`): height 0.6 with `CRAWLING` while each is there, back
/// to 1.8 without when it goes; no air or flag update.
#[test]
fn a_block_at_eye_height_makes_the_player_crawl() {
    let breath: serde_json::Value =
        serde_json::from_str(include_str!("../testdata/bds-1.26.51-breath.json")).unwrap();
    let bds: Vec<McpePacketData> = breath["phases"]["head_in_solid"]["packets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|hex| decode(hex.as_str().unwrap()))
        .collect();
    let bds = data(&wire(&bds, 1));
    assert_eq!(bds.len(), 6);
    let (mut server, mut steve) = server();
    let eyes = eyes(HOME).floor().as_ivec3();
    for block in ["minecraft:stone", "minecraft:glass", "minecraft:oak_leaves"] {
        set_block_in(
            &mut server,
            eyes,
            crate::world::chunk::blocks::get_block_id(block),
        );
        for _ in 0..60 {
            server.tick();
        }
        set_block_in(&mut server, eyes, *blocks::AIR);
        for _ in 0..60 {
            server.tick();
        }
    }
    assert_eq!(data(&wire(&drain(&mut steve), SESSION)), bds);
    // In the player-data capture the stone came between BDS's pose update
    // and its breath tick: one air point went before the crawl (and came
    // back after it). The rows of the pose are the same.
    let late_pose: Vec<Wire> = data(&wire(&bds_phase_crawl(), 1))
        .into_iter()
        .filter(|wire| !matches!(wire, Wire::Data(rows) if rows.iter().all(|row| row.id == ids::AIR_SUPPLY)))
        .collect();
    assert_eq!(late_pose.len(), 2);
}

fn bds_phase_crawl() -> Vec<McpePacketData> {
    bds("crawl")
}

/// Drowning to death inside the player tick: the air, the dead box and the
/// death rows in one update; after the respawn the living box, then the
/// breath (BDS breath capture `drowning_death_and_respawn`, every row).
#[test]
fn drowning_to_death_sends_the_air_box_and_death_rows_at_once() {
    let breath: serde_json::Value =
        serde_json::from_str(include_str!("../testdata/bds-1.26.51-breath.json")).unwrap();
    let bds: Vec<McpePacketData> = breath["phases"]["drowning_death_and_respawn"]["packets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|hex| decode(hex.as_str().unwrap()))
        .collect();
    let bds = data(&wire(&bds, 1));
    // The capture ran with natural regeneration off; food 17 heals nothing
    // either.
    let mut h = harness(Difficulty::Easy);
    h.rules().natural_regeneration = false;
    let player = h.player;
    h.world().get_mut::<Hunger>(player).unwrap().food_level = 17;
    h.tick();
    h.set_health(8.0);
    let feet = HOME.floor().as_ivec3();
    for pos in [feet, feet + IVec3::Y] {
        h.set_block(pos, *blocks::WATER);
    }
    h.ticks(380);
    assert!(h.dead());
    for pos in [feet, feet + IVec3::Y] {
        h.set_block(pos, *blocks::AIR);
    }
    h.ticks(5);
    h.route(PlayerActionPacket {
        player_runtime_id: ActorRuntimeId {
            actor_runtime_id: SESSION,
        },
        action: jolyne::valentine::types::EnumsPlayerActionType::Respawn,
        face: -1,
        ..Default::default()
    });
    h.ticks(5);
    let died = eyes(HOME).floor().as_ivec3();
    assert_eq!(data(&wire(&h.drain(), SESSION)), died_at(&bds, died));
}

/// `/kill` on land: the last death block and the died flag at once, the
/// 0.2 × 0.2 box on the next tick, the living box after the respawn; the
/// player and its viewers alike.
#[test]
fn a_kill_sends_the_death_rows_then_the_dead_box() {
    let subject_view = wire(&bds("kill_on_land"), bds_subject("kill_on_land"));
    let viewer_view = wire(
        &bds("kill_on_land_viewer"),
        bds_subject("kill_on_land_viewer"),
    );
    assert_eq!(subject_view, viewer_view);
    let (mut server, mut steve) = server();
    let mut alex = join(&mut server, VIEWER, "Alex", HOME + DVec3::X, None);
    for _ in 0..40 {
        server.tick();
    }
    drain(&mut steve);
    drain(&mut alex);
    let player = steve.entity;
    assert!(super::super::kill_player(server.ecs.world_mut(), player));
    for _ in 0..5 {
        server.tick();
    }
    route_as(
        &mut server,
        SESSION,
        PlayerActionPacket {
            player_runtime_id: ActorRuntimeId {
                actor_runtime_id: SESSION,
            },
            action: jolyne::valentine::types::EnumsPlayerActionType::Respawn,
            face: -1,
            ..Default::default()
        },
    );
    for _ in 0..5 {
        server.tick();
    }
    let died = eyes(HOME).floor().as_ivec3();
    assert_eq!(
        wire(&drain(&mut steve), SESSION),
        died_at(&subject_view, died)
    );
    assert_eq!(
        wire(&drain(&mut alex), SESSION),
        died_at(&viewer_view, died)
    );
}

/// `wire` with every name row set to `name`.
fn wire_named(wire: &[Wire], name: &str) -> Vec<Wire> {
    wire.iter()
        .map(|wire| match wire {
            Wire::Data(rows) => Wire::Data(named(rows, name)),
            other => other.clone(),
        })
        .collect()
}

/// A saved player rejoins with its air: on land the first update turns
/// `BREATHING` on with the saved air + 4 (BDS: 39 → 43); under water the
/// air goes on from the saved -1 and hits at -20. The join packet says 300
/// either way.
#[test]
fn a_rejoining_player_keeps_its_air() {
    let on_land = data(&wire(&bds("rejoin_on_land"), bds_subject("rejoin_on_land")));
    let under_water = wire(
        &bds("rejoin_under_water"),
        bds_subject("rejoin_under_water"),
    );
    for (air, submerged, bds) in [(39, false, on_land), (-1, true, under_water)] {
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..ServerConfig::default()
        });
        if submerged {
            // Load the platform with another player first (it leaves, so
            // the rejoining player is index 1 again), then fill the water.
            let mut probe = join(&mut server, 99, "Probe", HOME, None);
            for _ in 0..20 {
                server.tick();
            }
            drain(&mut probe);
            let feet = HOME.floor().as_ivec3();
            for pos in [feet, feet + IVec3::Y] {
                set_block_in(&mut server, pos, *blocks::WATER);
            }
            server.despawn_player(99);
        }
        let saved = PlayerData {
            uuid: uuid(SESSION),
            game_mode: 0,
            air,
            ..PlayerData::default()
        };
        let mut steve = join(&mut server, SESSION, "Steve", HOME, Some(saved));
        for _ in 0..(bds.len() + 30) {
            server.tick();
        }
        let seen: Vec<Wire> = wire(&drain(&mut steve), SESSION)
            .into_iter()
            .take(bds.len())
            .collect();
        assert_eq!(seen, wire_named(&bds, "Steve"), "saved air {air}");
    }
}

/// The saved record carries the air (full for a player saved dead).
#[test]
fn the_saved_record_keeps_the_air() {
    let mut h = harness(Difficulty::Easy);
    let player = h.player;
    h.world()
        .get_mut::<super::super::breath::PlayerBreath>(player)
        .unwrap()
        .air = 57;
    let (_, record) = crate::server::game::player_record(h.world(), player).unwrap();
    assert_eq!(record.air, 57);
    assert!(super::super::kill_player(h.world(), player));
    let (_, record) = crate::server::game::player_record(h.world(), player).unwrap();
    assert_eq!(record.air, 300);
}

/// Air drops under water with a Respiration helmet, counted from the
/// first tick under water to the drain: BDS dropped it on 480 of 1800
/// ticks at level III and 499 of 1001 at level I (chance 1 / (L + 1)).
#[test]
fn respiration_spares_the_air() {
    for (phase, level, ticks) in [("respiration_3", 3u8, 1800), ("respiration_1", 1u8, 1001)] {
        let bds = wire(&bds(phase), bds_subject(phase));
        // Air updates from the first tick under water until `BREATHING` is
        // back on (the drain).
        let air_updates = |wire: &[Wire]| {
            let breathing = |rows: &[DataItemEntry]| {
                rows.iter().find_map(|row| match &row.payload {
                    DataItemEntryPayload::DataItemInt64Payload(flags) if row.id == 0 => {
                        Some(flags.value & (1 << 35) != 0)
                    }
                    _ => None,
                })
            };
            let mut count = 0;
            let mut dived = false;
            for wire in wire {
                let Wire::Data(rows) = wire else { continue };
                match breathing(rows) {
                    Some(false) => dived = true,
                    Some(true) if dived => break,
                    _ => {}
                }
                if dived && rows.iter().any(|row| row.id == ids::AIR_SUPPLY) {
                    count += 1;
                }
            }
            count
        };
        let mut h = harness(Difficulty::Easy);
        h.rules().natural_regeneration = false;
        let player = h.player;
        {
            let mut health = h.world().get_mut::<Health>(player).unwrap();
            health.max = 100.0;
            health.current = 100.0;
        }
        let helmet = crate::item::enchant::with_enchantment_ids(
            &stack("minecraft:diamond_helmet", 1),
            [(6, level)],
        );
        h.world()
            .get_mut::<ArmourInventory>(player)
            .unwrap()
            .set_helmet(helmet)
            .unwrap();
        h.tick();
        h.drain();
        let feet = HOME.floor().as_ivec3();
        for pos in [feet, feet + IVec3::Y] {
            h.set_block(pos, *blocks::WATER);
        }
        h.ticks(ticks);
        let seen = wire(&h.drain(), SESSION);
        // Within four standard deviations of the expected count, BDS's
        // count included.
        let p = 1.0 / f64::from(level + 1);
        let n = f64::from(ticks as u32);
        let (mean, spread) = (n * p, 4.0 * (n * p * (1.0 - p)).sqrt());
        for (who, count) in [("unastar", air_updates(&seen)), ("BDS", air_updates(&bds))] {
            assert!(
                (count as f64 - mean).abs() <= spread,
                "{phase} {who}: {count} drops in {ticks} ticks, expected {mean} ± {spread}"
            );
        }
        // Hits land only on a drop from -19.
        let hits = |wire: &[Wire]| {
            wire.iter()
                .filter(|wire| matches!(wire, Wire::Event(EnumsActorEvent::Hurt, 9)))
                .count()
        };
        assert!(hits(&seen) > 0 && hits(&bds) > 0, "{phase}");
    }
}

/// Water over a bubble column's source: two ticks holding the breath, then
/// the column reaches the feet and the air is full at once.
#[test]
fn a_bubble_column_fills_the_air_at_once() {
    let bds = data(&wire(
        &bds("soul_sand_column"),
        bds_subject("soul_sand_column"),
    ));
    let mut h = harness(Difficulty::Easy);
    h.ticks(5);
    h.drain();
    let feet = HOME.floor().as_ivec3();
    for pos in [feet, feet + IVec3::Y] {
        h.set_block(pos, *blocks::WATER);
    }
    h.ticks(2);
    let column = crate::world::chunk::blocks::get_block_id("minecraft:bubble_column");
    h.set_block(feet, column);
    h.ticks(5);
    let seen = data(&wire(&h.drain(), SESSION));
    // BDS then pushes the player up and flags it MOVING, which the client
    // does here; only the breath updates are compared.
    assert_eq!(seen, bds[..3]);
}

/// A mob's hit: knockback, then the hurt event; the killing hit sends only
/// the death event (BDS: a slime's hits).
#[test]
fn hits_knock_back_before_the_hurt_and_a_killing_hit_sends_only_the_death() {
    let bds = wire(&bds("slime_hits"), bds_subject("slime_hits"));
    let pattern: Vec<Wire> = bds
        .iter()
        .map(|wire| match wire {
            Wire::Event(EnumsActorEvent::Hurt, _) => Wire::Event(EnumsActorEvent::Hurt, 2),
            other => other.clone(),
        })
        .collect();
    assert_eq!(
        pattern.last(),
        Some(&Wire::Event(EnumsActorEvent::Death, 0))
    );
    assert_eq!(
        pattern[..2],
        [Wire::Motion, Wire::Event(EnumsActorEvent::Hurt, 2)]
    );
    let mut h = harness(Difficulty::Normal);
    let cow = h.cow();
    h.hit_from(cow, 3.0);
    h.ticks(11);
    h.hit_from(cow, 100.0);
    let seen = wire(&h.drain(), SESSION);
    assert_eq!(
        seen.into_iter()
            .filter(|wire| !matches!(wire, Wire::Data(_)))
            .collect::<Vec<_>>(),
        [
            Wire::Motion,
            Wire::Event(EnumsActorEvent::Hurt, 2),
            Wire::Event(EnumsActorEvent::Death, 0),
        ]
    );
}

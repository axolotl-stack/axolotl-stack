use super::*;
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};
use crate::world::rules::GameRule;
use glam::DVec3;
use jolyne::valentine::types::GameRuleRuleValue;
use jolyne::valentine::{GameRulesChangedPacket, McpePacketData};
use tokio::sync::mpsc;
use valentine::bedrock::codec::BedrockCodec;

fn server() -> GameServer {
    GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    })
}

fn join(server: &mut GameServer) -> mpsc::Receiver<McpePacket> {
    let (outbound_tx, rx) = mpsc::channel(1 << 16);
    server.spawn_player(PlayerSpawnData {
        session_id: 1,
        display_name: "Clock".into(),
        xuid: None,
        uuid: Some("00000000-0000-0000-0000-000000000001".into()),
        runtime_id: 1,
        position: DVec3::new(0.5, 17.0, 0.5),
        outbound_tx,
        saved_data: None,
        chunk_radius: 2,
    });
    rx
}

fn clocks(rx: &mut mpsc::Receiver<McpePacket>) -> Vec<SyncWorldClocksPacketData> {
    let mut found = Vec::new();
    while let Ok(packet) = rx.try_recv() {
        if let McpePacketData::SyncWorldClocksPacket(packet) = packet.data {
            found.push(packet.data);
        }
    }
    found
}

fn state_time(data: &SyncWorldClocksPacketData) -> Option<i32> {
    match data {
        SyncWorldClocksPacketData::SyncStateData(state) => {
            assert_eq!(state.clock_data.len(), 1);
            assert_eq!(state.clock_data[0].clock_id, OVERWORLD_CLOCK_ID);
            assert!(!state.clock_data[0].is_paused);
            Some(state.clock_data[0].time)
        }
        _ => None,
    }
}

#[test]
fn clock_advances_every_tick_and_stops_without_the_daylight_cycle() {
    let mut server = server();
    for _ in 0..3 {
        server.tick();
    }
    assert_eq!(server.ecs.world().resource::<WorldTime>().ticks, 3);

    server
        .ecs
        .world_mut()
        .resource_mut::<GameRules>()
        .set(GameRule::DoDaylightCycle, false);
    for _ in 0..5 {
        server.tick();
    }
    assert_eq!(server.ecs.world().resource::<WorldTime>().ticks, 3);

    // BDS keeps the clock in an i32 and restarts it past the top.
    let mut time = WorldTime {
        ticks: WorldTime::MAX,
    };
    time.advance();
    assert_eq!(time.ticks, 1);
}

#[test]
fn clock_state_reaches_players_every_256_ticks() {
    let mut server = server();
    let mut rx = join(&mut server);
    let joined = clocks(&mut rx);
    assert!(
        matches!(&joined[..], [SyncWorldClocksPacketData::InitializeRegistryData(registry)]
            if registry.clock_data[0].time == 0),
        "the join carries the clock registry: {joined:?}"
    );

    let mut synced_at = Vec::new();
    for _ in 0..600 {
        let tick = server.ecs.world().resource::<TickCounter>().current;
        server.tick();
        for data in clocks(&mut rx) {
            let time = state_time(&data).expect("only clock states after the join");
            synced_at.push((tick, time));
        }
    }
    // The tick counter reaches 256 on the 257th tick, after the clock
    // advanced 257 times; then every 256 ticks.
    assert_eq!(synced_at, [(256, 257), (512, 513)]);
}

#[test]
fn set_time_broadcasts_the_clock_the_same_tick() {
    let mut server = server();
    let mut rx = join(&mut server);
    clocks(&mut rx);
    set_time(server.ecs.world_mut(), 13_000);
    server.tick();
    let synced: Vec<_> = clocks(&mut rx).iter().filter_map(state_time).collect();
    assert_eq!(synced, [13_001]);
    server.tick();
    assert!(clocks(&mut rx).is_empty(), "a change is broadcast once");
}

#[test]
fn sky_darkens_at_night() {
    let mut server = server();
    let world = server.ecs.world_mut();
    let stone = world
        .resource::<BlockRegistryResource>()
        .0
        .get_by_name("minecraft:stone")
        .unwrap()
        .default_state_id;
    let mut chunk = crate::world::chunk::Chunk::new(0, 0);
    chunk.set_block(3, 70, 3, stone);
    let chunk = world.spawn(ChunkData::new(chunk)).id();
    world
        .resource_mut::<ChunkManager>()
        .insert(crate::world::ecs::ChunkPosition::new(0, 0), chunk);
    let open = IVec3::new(5, 71, 5);
    let covered = IVec3::new(3, 69, 3);

    let at = |world: &mut World, ticks| {
        world.resource_mut::<WorldTime>().ticks = ticks;
        (
            sky_darken(world),
            sky_brightness(world, open),
            sky_brightness(world, covered),
            is_daytime(world),
        )
    };
    assert_eq!(at(world, 6_000), (0, 15, 0, true), "noon");
    assert_eq!(at(world, 18_000), (11, 4, 0, false), "midnight");
    assert_eq!(at(world, 24_000 + 6_000), (0, 15, 0, true), "the next noon");
    assert!(at(world, 1_000).3, "day begins at the day marker");
    assert!(!at(world, 13_000).3, "night begins at the night marker");
    assert_eq!(sky_brightness_with_darken(world, open, 10), 5);
    assert_eq!(sky_brightness(world, IVec3::new(5, MIN_Y - 1, 5)), 0);
}

#[test]
fn clock_registry_matches_the_bds_join_capture() {
    let bytes = include_bytes!(
        "../../../../bds-extractor/captures/1.26.51/member/join/00032-SyncWorldClocksPacket.bin"
    );
    // Packet id 344 is a two-byte varint.
    let mut input = &bytes[2..];
    let native = SyncWorldClocksPacket::decode(&mut input, ()).unwrap();
    assert!(input.is_empty());
    assert_eq!(clock_registry_packet(WorldTime { ticks: 32 }), native);
}

#[test]
fn game_rules_match_the_bds_join_capture() {
    let bytes = include_bytes!(
        "../../../../bds-extractor/captures/1.26.51/member/join/00037-GameRulesChangedPacket.bin"
    );
    let mut input = &bytes[1..];
    let native = GameRulesChangedPacket::decode(&mut input, ()).unwrap();
    assert!(input.is_empty());
    let ours = GameRules::default().packet_data().rules_list;
    let names: Vec<_> = native
        .rule_data
        .rules_list
        .iter()
        .filter(|rule| GameRule::from_name(&rule.rule_name).is_some())
        .cloned()
        .collect();
    assert_eq!(ours, names, "names, order, defaults and modifiability");
    assert!(
        ours.iter()
            .all(|rule| matches!(rule.rule_value, GameRuleRuleValue::Bool(_)))
    );
}

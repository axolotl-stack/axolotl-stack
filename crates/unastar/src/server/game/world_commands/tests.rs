use super::*;
use crate::server::game::packet_queues::{ChatAction, ChatPacketQueue};
use crate::server::game::types::ServerWorldTemplate;
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};
use crate::world::rules::StartGameWorldState;
use glam::DVec3;
use jolyne::valentine::types::{
    EnumsGameType, EnumsSharedTypesLegacyDifficulty, GameRule as WireGameRule, GameRuleRuleValue,
    SyncWorldClocksPacketData,
};
use jolyne::valentine::{McpePacketData, StartGamePacket};
use tokio::sync::mpsc;

/// Abilities bits (`UpdateAbilities` layer values).
const MAY_FLY: u32 = 1 << 10;
const INSTANT_BUILD: u32 = 1 << 11;

fn server(difficulty: Difficulty) -> GameServer {
    GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        operators: vec!["Op".into()],
        difficulty,
        ..ServerConfig::default()
    })
}

struct Joined {
    entity: Entity,
    session: u64,
    rx: mpsc::Receiver<McpePacket>,
}

fn join(server: &mut GameServer, session: u64, name: &str) -> Joined {
    let (outbound_tx, rx) = mpsc::channel(1 << 16);
    let entity = server.spawn_player(PlayerSpawnData {
        session_id: session,
        display_name: name.into(),
        xuid: None,
        uuid: Some(format!("00000000-0000-0000-0000-{session:012}")),
        runtime_id: session as i64,
        position: DVec3::new(0.5, 17.0, 0.5),
        outbound_tx,
        saved_data: None,
        chunk_radius: 2,
    });
    Joined {
        entity,
        session,
        rx,
    }
}

fn command(server: &mut GameServer, player: &Joined, line: &str) {
    server
        .ecs
        .world_mut()
        .resource_mut::<ChatPacketQueue>()
        .0
        .push((
            player.entity,
            player.session,
            ChatAction::Command(jolyne::valentine::CommandRequestPacket {
                command: line.into(),
                ..Default::default()
            }),
        ));
    server.tick();
}

fn drain(player: &mut Joined) -> Vec<McpePacketData> {
    let mut packets = Vec::new();
    while let Ok(packet) = player.rx.try_recv() {
        packets.push(packet.data);
    }
    packets
}

fn texts(packets: &[McpePacketData]) -> Vec<String> {
    packets
        .iter()
        .filter(|packet| matches!(packet, McpePacketData::TextPacket(_)))
        .map(|packet| format!("{packet:?}"))
        .collect()
}

fn difficulties(packets: &[McpePacketData]) -> Vec<u32> {
    packets
        .iter()
        .filter_map(|packet| match packet {
            McpePacketData::SetDifficultyPacket(packet) => Some(packet.difficulty),
            _ => None,
        })
        .collect()
}

fn start_game(server: &GameServer) -> StartGamePacket {
    let mut start = StartGamePacket::default();
    server
        .ecs
        .world()
        .resource::<StartGameWorldState>()
        .apply(&mut start);
    start
}

#[test]
fn configured_difficulty_reaches_start_game_and_the_join() {
    let mut server = server(Difficulty::Hard);
    assert_eq!(
        server
            .ecs
            .world()
            .resource::<ServerWorldTemplate>()
            .0
            .start_game_template
            .settings
            .game_difficulty,
        EnumsSharedTypesLegacyDifficulty::Hard
    );
    assert_eq!(
        start_game(&server).settings.game_difficulty,
        EnumsSharedTypesLegacyDifficulty::Hard
    );
    let mut player = join(&mut server, 1, "Op");
    assert_eq!(difficulties(&drain(&mut player)), [3]);
}

#[test]
fn difficulty_command_broadcasts_and_reaches_start_game() {
    let mut server = server(Difficulty::Easy);
    let mut op = join(&mut server, 1, "Op");
    let mut other = join(&mut server, 2, "Other");
    drain(&mut op);
    drain(&mut other);

    command(&mut server, &other, "/difficulty hard");
    assert_eq!(
        *server.ecs.world().resource::<Difficulty>(),
        Difficulty::Easy
    );
    let refused = drain(&mut other);
    assert!(difficulties(&refused).is_empty());
    assert!(
        texts(&refused)
            .iter()
            .any(|text| text.contains("permission"))
    );

    command(&mut server, &op, "/difficulty normal");
    assert_eq!(
        *server.ecs.world().resource::<Difficulty>(),
        Difficulty::Normal
    );
    let to_op = drain(&mut op);
    assert_eq!(difficulties(&to_op), [2]);
    assert!(
        texts(&to_op)
            .iter()
            .any(|text| text.contains("Set game difficulty to NORMAL"))
    );
    assert_eq!(difficulties(&drain(&mut other)), [2]);
    assert_eq!(
        start_game(&server).settings.game_difficulty,
        EnumsSharedTypesLegacyDifficulty::Normal
    );
}

#[test]
fn gamerule_keepinventory_changes_the_rule_and_broadcasts_it() {
    let mut server = server(Difficulty::Easy);
    let mut op = join(&mut server, 1, "Op");
    let mut other = join(&mut server, 2, "Other");
    drain(&mut op);
    drain(&mut other);

    command(&mut server, &op, "/gamerule keepinventory true");
    assert!(server.ecs.world().resource::<GameRules>().keep_inventory);
    let changed = WireGameRule {
        rule_name: "keepInventory".into(),
        rule_can_be_modified: true,
        rule_value: GameRuleRuleValue::Bool(true),
    };
    for player in [&mut op, &mut other] {
        let announced: Vec<_> = drain(player)
            .into_iter()
            .filter_map(|packet| match packet {
                McpePacketData::GameRulesChangedPacket(packet) => Some(packet.rule_data.rules_list),
                _ => None,
            })
            .collect();
        assert_eq!(announced, [vec![changed.clone()]]);
    }
    assert!(
        start_game(&server)
            .settings
            .rule_data
            .rules_list
            .contains(&changed)
    );

    command(&mut server, &op, "/gamerule KeepInventory");
    assert!(
        texts(&drain(&mut op))
            .iter()
            .any(|text| text.contains("keepInventory = true"))
    );
    command(&mut server, &op, "/gamerule keepinventory maybe");
    assert!(server.ecs.world().resource::<GameRules>().keep_inventory);
}

#[test]
fn time_set_night_moves_the_clock_and_syncs_players() {
    let mut server = server(Difficulty::Easy);
    let mut op = join(&mut server, 1, "Op");
    let mut other = join(&mut server, 2, "Other");
    server.tick();
    drain(&mut op);
    drain(&mut other);

    command(&mut server, &op, "/time set night");
    // Set to 13000 by the command, then this tick's clock advance.
    assert_eq!(server.ecs.world().resource::<WorldTime>().ticks, 13_001);
    assert!(!clock::is_daytime(server.ecs.world()));
    for player in [&mut op, &mut other] {
        let synced: Vec<_> = drain(player)
            .into_iter()
            .filter_map(|packet| match packet {
                McpePacketData::SyncWorldClocksPacket(packet) => match packet.data {
                    SyncWorldClocksPacketData::SyncStateData(state) => {
                        Some(state.clock_data[0].time)
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(synced, [13_001]);
    }
    assert_eq!(start_game(&server).level_current_time, 13_001);

    command(&mut server, &op, "/time query daytime");
    assert!(
        texts(&drain(&mut op))
            .iter()
            .any(|text| text.contains("Daytime is 13001"))
    );
    command(&mut server, &op, "/time add 1000");
    assert_eq!(server.ecs.world().resource::<WorldTime>().ticks, 14_003);
}

#[test]
fn gamemode_creative_switches_mode_with_the_bds_packets() {
    let mut server = server(Difficulty::Easy);
    let mut op = join(&mut server, 1, "Op");
    let mut other = join(&mut server, 2, "Other");
    drain(&mut op);
    drain(&mut other);

    command(&mut server, &op, "/gamemode creative");
    assert_eq!(
        *server.ecs.world().get::<GameMode>(op.entity).unwrap(),
        GameMode::Creative
    );
    assert_eq!(
        *server.ecs.world().get::<GameMode>(other.entity).unwrap(),
        GameMode::Survival
    );
    let to_op = drain(&mut op);
    let to_other = drain(&mut other);
    assert!(to_op.iter().any(|packet| matches!(packet,
        McpePacketData::SetPlayerGameTypePacket(p) if p.player_game_type == EnumsGameType::Creative)));
    assert!(
        !to_other
            .iter()
            .any(|packet| matches!(packet, McpePacketData::SetPlayerGameTypePacket(_))),
        "only the target's own game type is set"
    );
    for packets in [&to_op, &to_other] {
        assert!(packets.iter().any(|packet| matches!(packet,
            McpePacketData::UpdatePlayerGameTypePacket(p)
                if p.player_game_type == EnumsGameType::Creative
                    && p.targetplayer.actor_unique_id == 1)));
        let abilities = packets
            .iter()
            .find_map(|packet| match packet {
                McpePacketData::UpdateAbilitiesPacket(p) if p.data.target_player_raw_id == 1 => {
                    Some(p.data.layers[0].ability_values)
                }
                _ => None,
            })
            .expect("the new abilities reach every player");
        assert_eq!(
            abilities & (MAY_FLY | INSTANT_BUILD),
            MAY_FLY | INSTANT_BUILD
        );
    }
    assert!(
        texts(&to_op)
            .iter()
            .any(|text| text.contains("Set own game mode to Creative"))
    );

    command(&mut server, &op, "/gamemode a other");
    assert_eq!(
        *server.ecs.world().get::<GameMode>(other.entity).unwrap(),
        GameMode::Adventure
    );
    assert!(drain(&mut other).iter().any(|packet| matches!(packet,
        McpePacketData::SetPlayerGameTypePacket(p) if p.player_game_type == EnumsGameType::Adventure)));
}

#[test]
fn kill_command_kills_the_named_player_once() {
    let mut server = server(Difficulty::Easy);
    let mut op = join(&mut server, 1, "Op");
    let other = join(&mut server, 2, "Other");
    drain(&mut op);

    command(&mut server, &op, "/kill other");
    let world = server.ecs.world();
    assert!(
        world
            .get::<crate::entity::player_life::Dead>(other.entity)
            .is_some()
    );
    assert!(
        world
            .get::<crate::entity::player_life::Dead>(op.entity)
            .is_none()
    );
    assert!(
        texts(&drain(&mut op))
            .iter()
            .any(|text| text.contains("Killed Other"))
    );

    command(&mut server, &op, "/kill Other");
    assert!(
        texts(&drain(&mut op))
            .iter()
            .any(|text| text.contains("No targets matched selector"))
    );
    command(&mut server, &op, "/kill @e");
    assert!(
        texts(&drain(&mut op))
            .iter()
            .any(|text| text.contains("Unsupported target selector: @e"))
    );
}

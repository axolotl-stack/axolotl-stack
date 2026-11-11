//! End-to-end replays of the PvP race captures through the server: the acceptance test of
//! the `ordered-packet-apply` change.
//!
//! Each trial's client packets are routed through [`GameServer::route_packet`] as the
//! network task routes them, one server tick per client tick, and the hurt events,
//! health updates and deaths the bystander C receives are compared with BDS's most
//! frequent outcome (`testdata/pvp-race-*.jsonl`, see `packet_order_tests.rs`). They can
//! only pass once the server applies packets in BDS's order (decision 0045) and players
//! can hit players (the PvP lane, decision 0046), so they are ignored until then.

use bytes::BytesMut;
use glam::{DVec3, IVec3};
use jolyne::valentine::{
    BorrowedMcpePacket, EnumsActorEvent, McpePacket, McpePacketArgs, McpePacketData,
};
use tokio::sync::mpsc::Receiver;

use super::tests::{Fixture, fixture};
use crate::entity::components::GameMode;
use crate::registry::BlockRegistry;
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};

/// Where every trial starts (`tools/bds-sweep/pvp_race_scripts.py`).
const PLAYERS: [(char, &str, DVec3); 3] = [
    ('A', "RaceA", DVec3::new(0.5, -60.0, 0.5)),
    ('B', "RaceB", DVec3::new(2.5, -60.0, 0.5)),
    ('C', "RaceC", DVec3::new(2.5, -60.0, 2.5)),
];

struct Arena {
    server: GameServer,
    sessions: Vec<(char, u64)>,
    bystander: Receiver<McpePacket>,
    runtime_ids: Vec<(i64, char)>,
}

/// The capture's arena: a flat stone floor, the three players where every trial starts,
/// joined in the capture's order with BDS's runtime ids, all operators in survival.
fn arena(race: &Fixture) -> Arena {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        operators: PLAYERS.iter().map(|(_, name, _)| (*name).into()).collect(),
        ..ServerConfig::default()
    });
    let join = race.header["join_order"].as_str().unwrap().to_owned();
    let mut sessions = Vec::new();
    let mut runtime_ids = Vec::new();
    let mut bystander = None;
    for label in join.chars() {
        let (_, name, feet) = PLAYERS.iter().find(|(l, ..)| *l == label).unwrap();
        let runtime_id = race.header["runtime_ids"][label.to_string()]
            .as_i64()
            .unwrap();
        let session = 100 + runtime_id as u64;
        let (outbound_tx, rx) = tokio::sync::mpsc::channel(1 << 16);
        let player = server
            .spawn_player(PlayerSpawnData {
                session_id: session,
                display_name: (*name).into(),
                xuid: None,
                uuid: Some(format!("00000000-0000-0000-0000-{session:012}")),
                runtime_id,
                position: *feet + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT,
                outbound_tx,
                saved_data: None,
                chunk_radius: 2,
            })
            .expect("arena player spawns");
        *server.ecs.world_mut().get_mut::<GameMode>(player).unwrap() = GameMode::Survival;
        sessions.push((label, session));
        runtime_ids.push((runtime_id, label));
        if label == 'C' {
            bystander = Some(rx);
        }
    }
    let stone = BlockRegistry::vanilla()
        .default_state_id_by_name("minecraft:stone")
        .unwrap();
    for x in -4..=8 {
        for z in -4..=6 {
            super::super::blocks::set_block(
                server.ecs.world_mut(),
                IVec3::new(x, -61, z),
                stone,
                false,
            );
        }
    }
    server.tick();
    let mut arena = Arena {
        server,
        sessions,
        bystander: bystander.unwrap(),
        runtime_ids,
    };
    arena.drain();
    arena
}

impl Arena {
    fn route(&mut self, player: &str, packet: &McpePacketData) {
        let label = player.chars().next().unwrap();
        let session = self.sessions.iter().find(|(l, _)| *l == label).unwrap().1;
        let mut buf = BytesMut::new();
        packet.encode_inner(&mut buf, 0, 0).unwrap();
        let borrowed = BorrowedMcpePacket::decode_inner(&mut buf.freeze()).unwrap();
        self.server.route_packet(session, McpePacketArgs, borrowed);
    }

    fn drain(&mut self) -> Vec<McpePacketData> {
        std::iter::from_fn(|| self.bystander.try_recv().ok().map(|packet| packet.data)).collect()
    }

    fn label(&self, runtime_id: u64) -> Option<char> {
        self.runtime_ids
            .iter()
            .find(|(id, _)| *id as u64 == runtime_id)
            .map(|(_, label)| *label)
    }

    /// What the bystander saw in the flush of pass `pass`, in the words of
    /// [`bds_outcome`]: `+2 hurt B`, `+2 death A`, `health B 19`. A pass's flush reached
    /// BDS's scripted clients during the next client tick, hence `pass + 1`.
    fn outcome(&self, pass: i64, packets: &[McpePacketData]) -> Vec<String> {
        let mut out = Vec::new();
        for packet in packets {
            match packet {
                McpePacketData::ActorEventPacket(event) => {
                    let Some(who) = self.label(event.target_runtime_id.actor_runtime_id) else {
                        continue;
                    };
                    match event.event_id {
                        EnumsActorEvent::Hurt => out.push(format!("{:+} hurt {who}", pass + 1)),
                        EnumsActorEvent::Death => out.push(format!("{:+} death {who}", pass + 1)),
                        _ => {}
                    }
                }
                McpePacketData::UpdateAttributesPacket(update) => {
                    let Some(who) = self.label(update.target_runtime_id.actor_runtime_id) else {
                        continue;
                    };
                    for attribute in &update.attribute_list {
                        if &*attribute.name == "minecraft:health" && attribute.current_value < 20.0
                        {
                            out.push(format!("health {who} {}", attribute.current_value));
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }
}

/// BDS's most frequent outcome for a variant: hurts and deaths with the client tick they
/// arrived in, and health below full, from the race tick to fifteen ticks after.
fn bds_outcome(race: &Fixture, variant: &str) -> Vec<String> {
    race.majority(variant)
        .iter()
        .filter_map(|event| {
            let mut words = event.split_whitespace();
            let tick: i64 = words.next()?.parse().ok()?;
            if !(0..=15).contains(&tick) {
                return None;
            }
            match words.next()? {
                "hurt" => Some(format!("{tick:+} hurt {}", words.next()?)),
                "death" if event.contains(" Death ") => {
                    Some(format!("{tick:+} death {}", words.next()?))
                }
                "health" => {
                    let who = words.next()?;
                    let value: f32 = words.next()?.parse().ok()?;
                    (value < 20.0).then(|| format!("health {who} {value}"))
                }
                _ => None,
            }
        })
        .collect()
}

/// Replays every variant of a capture; returns the variants whose outcome differs.
fn replay(text: &str) -> Vec<String> {
    let race = fixture(text);
    let mut failures = Vec::new();
    for (variant, sent) in &race.sent {
        let mut arena = arena(&race);
        let first = sent.first().map_or(0, |packet| packet.rel);
        let mut seen = Vec::new();
        for rel in first..=15 {
            for packet in sent.iter().filter(|packet| packet.rel == rel) {
                arena.route(&packet.player, &packet.packet);
            }
            arena.server.tick();
            let packets = arena.drain();
            if rel >= 0 {
                seen.extend(arena.outcome(rel, &packets));
            }
        }
        let bds = bds_outcome(&race, variant);
        if seen != bds {
            failures.push(format!("{variant}: BDS {bds:?}, ours {seen:?}"));
        }
    }
    failures
}

fn assert_replays(texts: &[&str]) {
    let failures: Vec<String> = texts.iter().flat_map(|text| replay(text)).collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "needs ordered packet apply (decision 0045) and player hits (0046)"]
fn held_attacks_replay_like_bds() {
    assert_replays(&[
        include_str!("testdata/pvp-race-queue.jsonl"),
        include_str!("testdata/pvp-race-model.jsonl"),
    ]);
}

#[test]
#[ignore = "needs ordered packet apply (decision 0045) and player hits (0046)"]
fn sprint_crit_and_strict_facing_replay_like_bds() {
    assert_replays(&[
        include_str!("testdata/pvp-race-sprint.jsonl"),
        include_str!("testdata/pvp-race-crit.jsonl"),
        include_str!("testdata/pvp-race-model-strict.jsonl"),
    ]);
}

#[test]
#[ignore = "needs ordered packet apply (decision 0045) and player hits (0046)"]
fn mutual_hits_and_victim_inputs_replay_like_bds() {
    assert_replays(&[
        include_str!("testdata/pvp-race-mutual.jsonl"),
        include_str!("testdata/pvp-race-victim-input.jsonl"),
    ]);
}

#[test]
#[ignore = "needs ordered packet apply (decision 0045) and player hits (0046)"]
fn kill_races_replay_like_bds() {
    assert_replays(&[
        include_str!("testdata/pvp-race-both-low.jsonl"),
        include_str!("testdata/pvp-race-kill.jsonl"),
        include_str!("testdata/pvp-race-pairs-abc.jsonl"),
        include_str!("testdata/pvp-race-pairs-cba.jsonl"),
        include_str!("testdata/pvp-race-pairs-bca.jsonl"),
        include_str!("testdata/pvp-race-pairs-acb.jsonl"),
    ]);
}

#[test]
#[ignore = "needs ordered packet apply (decision 0045) and player hits (0046)"]
fn bursts_lag_windows_and_two_attackers_replay_like_bds() {
    assert_replays(&[
        include_str!("testdata/pvp-race-burst.jsonl"),
        include_str!("testdata/pvp-race-lag.jsonl"),
        include_str!("testdata/pvp-race-window.jsonl"),
        include_str!("testdata/pvp-race-two-attackers.jsonl"),
        include_str!("testdata/pvp-race-follow-up.jsonl"),
    ]);
}

#[test]
#[ignore = "needs ordered packet apply (decision 0045) and player hits (0046)"]
fn reach_and_teleports_replay_like_bds() {
    assert_replays(&[
        include_str!("testdata/pvp-race-reach-order.jsonl"),
        include_str!("testdata/pvp-race-pending.jsonl"),
    ]);
}

/// The harness itself runs on today's server, which keeps the ignored replays compiling
/// and runnable: with no player-versus-player hits yet, every variant that lands a hit on
/// BDS differs.
#[test]
fn the_replay_harness_runs_a_capture_on_todays_server() {
    let failures = replay(include_str!("testdata/pvp-race-burst.jsonl"));
    assert_eq!(failures.len(), 3, "{failures:#?}");
}

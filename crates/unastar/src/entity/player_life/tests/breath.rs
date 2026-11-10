//! The player's air supply and `BREATHING` actor flag against BDS 1.26.51
//! (`../testdata/bds-1.26.51-breath.json`; its `sources` name the captures).
//!
//! Each test replays one captured phase tick for tick: the console's
//! `/fill` that put water over the player's eyes or drained it, a game-mode
//! or effect change, a death. It then compares what the player's client (or
//! a viewer's) received about the player's breath with the decoded BDS
//! packets. Under water the air changes every tick, so the number of ticks
//! between two commands is the length of the run of updates between two
//! flag changes ([`runs`]).

use jolyne::valentine::types::{DataItemEntryPayload, PlayerInputTick};
use jolyne::valentine::{PlayStatusPacketStatus, PlayerActionPacket};

use super::*;
use crate::entity::components::{EffectType, Effects};

/// The actor-data keys of a player's breath: the flag word, the air supply
/// and the maximum air supply.
const BREATH_KEYS: [u32; 3] = [0, 7, 42];
/// Actor flags `CANCLIMB`, `HAS_COLLISION` and `HAS_GRAVITY`, without and
/// with `BREATHING`, as BDS sends them for a survival player.
const NOT_BREATHING: i64 = (1 << 19) | (1 << 48) | (1 << 49);
const BREATHING: i64 = NOT_BREATHING | (1 << 35);

/// What a packet about a player tells a client about the player's breath.
#[derive(Debug, Clone, PartialEq)]
enum Seen {
    /// The [`BREATH_KEYS`] rows of a `SetActorData`, in packet order.
    Data(Vec<(u32, i64)>),
    /// An `ActorEvent` and its data.
    Event(EnumsActorEvent, i32),
    /// The `minecraft:health` of an `UpdateAttributes`.
    Health(f32),
}

fn seen(packets: &[McpePacketData], runtime_id: u64) -> Vec<Seen> {
    let mut seen = Vec::new();
    for packet in packets {
        match packet {
            McpePacketData::SetActorDataPacket(data)
                if data.target_runtime_id.actor_runtime_id == runtime_id =>
            {
                let rows: Vec<(u32, i64)> = data
                    .actor_data
                    .data
                    .iter()
                    .filter(|row| BREATH_KEYS.contains(&row.id))
                    .map(|row| {
                        let value = match &row.payload {
                            DataItemEntryPayload::DataItemInt64Payload(long) => long.value,
                            DataItemEntryPayload::DataItemShortPayload(short) => {
                                i64::from(short.value)
                            }
                            other => panic!("breath key {} carries {other:?}", row.id),
                        };
                        (row.id, value)
                    })
                    .collect();
                if !rows.is_empty() {
                    seen.push(Seen::Data(rows));
                }
            }
            McpePacketData::ActorEventPacket(event)
                if event.target_runtime_id.actor_runtime_id == runtime_id =>
            {
                seen.push(Seen::Event(event.event_id, event.data));
            }
            McpePacketData::UpdateAttributesPacket(update)
                if update.target_runtime_id.actor_runtime_id == runtime_id =>
            {
                seen.extend(
                    update
                        .attribute_list
                        .iter()
                        .filter(|attribute| &*attribute.name == "minecraft:health")
                        .map(|attribute| Seen::Health(attribute.current_value)),
                );
            }
            _ => {}
        }
    }
    seen
}

fn fixture() -> &'static serde_json::Value {
    static FIXTURE: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    FIXTURE.get_or_init(|| {
        serde_json::from_str(include_str!("../testdata/bds-1.26.51-breath.json")).unwrap()
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

/// What BDS sent the client of `phase` about the subject (runtime ID 1).
fn captured(phase: &str) -> Vec<Seen> {
    let fixture = fixture();
    assert_eq!(fixture["subject_runtime_id"], 1);
    let packets: Vec<McpePacketData> = fixture["phases"][phase]["packets"]
        .as_array()
        .unwrap_or_else(|| panic!("no phase {phase}"))
        .iter()
        .map(|hex| decode(hex.as_str().unwrap()))
        .collect();
    seen(&packets, 1)
}

/// Only the `SetActorData` rows.
fn data(seen: &[Seen]) -> Vec<Seen> {
    seen.iter()
        .filter(|seen| matches!(seen, Seen::Data(_)))
        .cloned()
        .collect()
}

/// Only the actor events.
fn events(seen: &[Seen]) -> Vec<Seen> {
    seen.iter()
        .filter(|seen| matches!(seen, Seen::Event(..)))
        .cloned()
        .collect()
}

/// Only the health updates.
fn health(seen: &[Seen]) -> Vec<Seen> {
    seen.iter()
        .filter(|seen| matches!(seen, Seen::Health(_)))
        .cloned()
        .collect()
}

/// The lengths of the runs of `SetActorData` updates that each start with a
/// flag change (key 0): one update per tick under water, one per tick until
/// the air is full on the surface.
fn runs(seen: &[Seen]) -> Vec<usize> {
    let mut runs: Vec<usize> = Vec::new();
    for rows in seen.iter().filter_map(|seen| match seen {
        Seen::Data(rows) => Some(rows),
        _ => None,
    }) {
        match runs.last_mut() {
            Some(run) if rows[0].0 != 0 => *run += 1,
            _ => runs.push(1),
        }
    }
    runs
}

impl Harness {
    /// Water over the player's feet and eyes at [`HOME`], or air again.
    fn submerge(&mut self, water: bool) {
        let feet = HOME.floor().as_ivec3();
        let block = if water { *blocks::WATER } else { *blocks::AIR };
        self.set_block(feet, block);
        self.set_block(feet + IVec3::Y, block);
    }

    fn effect(&mut self, effect: EffectType, on: bool) {
        let player = self.player;
        let mut effects = self.world().get_mut::<Effects>(player).unwrap();
        if on {
            effects.add(effect, 0, 200);
        } else {
            effects.remove(effect);
        }
    }

    fn seen(&mut self) -> Vec<Seen> {
        seen(&self.drain(), SESSION)
    }
}

/// A survival player on the platform that does not heal: the captures ran
/// with `naturalregeneration` off; here the food stays below the 18 that
/// natural regeneration needs.
fn subject() -> Harness {
    let mut h = harness(Difficulty::Easy);
    let player = h.player;
    h.world()
        .get_mut::<crate::entity::components::Hunger>(player)
        .unwrap()
        .food_level = 17;
    h.tick();
    h.drain();
    h
}

#[test]
fn joining_sends_breathing_one_tick_after_the_spawn_status() {
    let bds = captured("spawn");
    assert_eq!(
        bds,
        [
            Seen::Data(vec![(0, NOT_BREATHING), (7, 300), (42, 300)]),
            Seen::Data(vec![(0, BREATHING)]),
        ]
    );
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let mut joined = join(&mut server, SESSION, "Steve", HOME, None);
    let mut packets = drain(&mut joined);
    for _ in 0..40 {
        server.tick();
        packets.extend(drain(&mut joined));
    }
    assert_eq!(data(&seen(&packets, SESSION)), bds);
    // `PlayStatus(PlayerSpawn)`, then the flag.
    let spawned = packets
        .iter()
        .position(|packet| {
            matches!(packet, McpePacketData::PlayStatusPacket(status)
                if status.status == PlayStatusPacketStatus::Playerspawn)
        })
        .expect("spawn status");
    let breathing = packets
        .iter()
        .rposition(|packet| {
            matches!(packet, McpePacketData::SetActorDataPacket(data)
                if data.target_runtime_id.actor_runtime_id == SESSION)
        })
        .unwrap();
    assert!(spawned < breathing);
}

#[test]
fn a_short_dive_matches_the_bds_capture() {
    let bds = captured("short_dive");
    assert_eq!(runs(&bds), [99, 25]);
    let mut h = subject();
    let mut viewer = h.viewer();
    h.submerge(true);
    h.ticks(99);
    h.submerge(false);
    h.ticks(40);
    let packets = h.drain();
    assert_eq!(seen(&packets, SESSION), bds);
    assert_eq!(
        seen(&packets, SESSION)[0],
        Seen::Data(vec![(0, NOT_BREATHING), (7, 299)])
    );
    // A player standing next to it receives the same updates.
    assert_eq!(captured("short_dive_viewer"), bds);
    assert_eq!(seen(&drain(&mut viewer), SESSION), bds);
}

#[test]
fn drowning_hits_every_twenty_ticks_and_surfacing_refills_the_air() {
    let bds = captured("drown_and_surface");
    assert_eq!(runs(&bds), [420, 75]);
    let hits: Vec<&Seen> = bds
        .iter()
        .filter(|seen| matches!(seen, Seen::Event(..)))
        .collect();
    assert_eq!(hits, [&Seen::Event(EnumsActorEvent::Hurt, 9); 6]);
    let mut h = subject();
    h.submerge(true);
    h.ticks(420);
    h.submerge(false);
    h.ticks(80);
    assert_eq!(h.seen(), bds);
    assert_eq!(h.health(), 8.0);
}

#[test]
fn surfacing_with_negative_air_resets_it_to_zero() {
    let bds = captured("surface_at_negative_air");
    assert_eq!(runs(&bds), [309, 76]);
    assert_eq!(bds[309], Seen::Data(vec![(0, BREATHING), (7, 0)]));
    let mut h = subject();
    h.submerge(true);
    h.ticks(309);
    h.submerge(false);
    h.ticks(80);
    assert_eq!(h.seen(), bds);
}

/// Under water at air 200, the player is made able to breathe there, then
/// unable again before the water is drained.
fn breathes_mid_dive(phase: &str, protect: impl Fn(&mut Harness, bool)) {
    let bds = captured(phase);
    let runs = runs(&bds);
    assert_eq!(runs.len(), 4, "{phase}: dive, protected, dive, surface");
    let mut h = subject();
    h.submerge(true);
    h.ticks(runs[0]);
    protect(&mut h, true);
    h.ticks(runs[1] + 20);
    protect(&mut h, false);
    h.ticks(runs[2]);
    h.submerge(false);
    h.ticks(runs[3] + 20);
    assert_eq!(h.seen(), bds, "{phase}");
}

#[test]
fn creative_and_spectator_players_breathe_under_water() {
    breathes_mid_dive("creative_mid_dive", |h, on| {
        h.set_mode(if on {
            GameMode::Creative
        } else {
            GameMode::Survival
        });
    });
    breathes_mid_dive("spectator_mid_dive", |h, on| {
        h.set_mode(if on {
            GameMode::Spectator
        } else {
            GameMode::Survival
        });
    });
    // A spectator has no collision.
    assert!(
        captured("spectator_mid_dive")
            .contains(&Seen::Data(vec![(0, BREATHING & !(1 << 48)), (7, 204)]))
    );
}

#[test]
fn water_breathing_and_conduit_power_breathe_under_water() {
    breathes_mid_dive("water_breathing_mid_dive", |h, on| {
        h.effect(EffectType::WATER_BREATHING, on);
    });
    breathes_mid_dive("conduit_power_mid_dive", |h, on| {
        h.effect(EffectType::CONDUIT_POWER, on);
    });
}

#[test]
fn adventure_players_hold_their_breath() {
    let bds = captured("adventure_dive");
    let runs = runs(&bds);
    assert_eq!(runs.len(), 2);
    let mut h = subject();
    h.set_mode(GameMode::Adventure);
    h.submerge(true);
    h.ticks(runs[0]);
    h.submerge(false);
    h.ticks(runs[1] + 20);
    assert_eq!(h.seen(), bds);
}

/// The killing hit sends the death event without a hurt event and the air
/// with the death, and the respawned player breathes at full air.
#[test]
fn drowning_to_death_and_respawning_refill_the_air() {
    let bds = captured("drowning_death_and_respawn");
    let runs = runs(&bds);
    assert_eq!(runs, [380, 1]);
    assert_eq!(
        health(&bds)[..4],
        [
            Seen::Health(6.0),
            Seen::Health(4.0),
            Seen::Health(2.0),
            Seen::Health(0.0)
        ]
    );
    let mut h = subject();
    h.set_health(8.0);
    h.submerge(true);
    h.ticks(380);
    assert!(h.dead());
    h.submerge(false);
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
    let seen = h.seen();
    assert_eq!(data(&seen), data(&bds));
    assert_eq!(health(&seen), health(&bds));
    assert_eq!(events(&seen), events(&bds));
    assert_eq!(
        events(&bds),
        [
            Seen::Event(EnumsActorEvent::Hurt, 9),
            Seen::Event(EnumsActorEvent::Hurt, 9),
            Seen::Event(EnumsActorEvent::Hurt, 9),
            Seen::Event(EnumsActorEvent::Death, 0),
            Seen::Event(EnumsActorEvent::SpawnAlive, 0),
        ]
    );
}

/// A solid block at eye height does not stop the breath (BDS makes the
/// player crawl instead, which the captured packets show but this server
/// does not implement).
#[test]
fn a_block_at_the_eyes_does_not_drain_the_air() {
    let captured = captured("head_in_solid");
    assert!(data(&captured).is_empty());
    let mut h = subject();
    let eyes = eyes(HOME).floor().as_ivec3();
    for block in [*blocks::STONE, *blocks::AIR] {
        h.set_block(eyes, block);
        h.ticks(60);
    }
    assert!(data(&h.seen()).is_empty());
}

#[test]
fn dying_on_land_sends_no_breath_update() {
    assert!(captured("land_death_and_respawn").is_empty());
    let mut h = subject();
    let player = h.player;
    assert!(super::super::kill_player(h.world(), player));
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
    assert!(!h.dead());
    assert!(data(&h.seen()).is_empty());
}

/// The hurt event carries the damage cause's ID (`/damage 1` with each
/// cause in the capture).
#[test]
fn the_hurt_event_carries_the_damage_cause() {
    let bds = captured("hurt_causes");
    let causes = [
        ActorDamageCause::Fall,
        ActorDamageCause::Lava,
        ActorDamageCause::EntityAttack,
        ActorDamageCause::Magic,
        ActorDamageCause::Drowning,
        ActorDamageCause::Void,
    ];
    let mut h = subject();
    let player = h.player;
    for cause in causes {
        super::super::hurt_player(
            h.world(),
            player,
            &Hit {
                damage: 1.0,
                cause,
                attacker: None,
                by_player: false,
                knockback_from: None,
                knockback_power: KnockbackPower::DEFAULT,
            },
        );
        h.ticks(15);
    }
    let events: Vec<Seen> = h
        .seen()
        .into_iter()
        .filter(|seen| matches!(seen, Seen::Event(..)))
        .collect();
    assert_eq!(events, bds);
    assert_eq!(
        bds.iter()
            .map(|seen| match seen {
                Seen::Event(_, data) => *data,
                _ => unreachable!(),
            })
            .collect::<Vec<_>>(),
        [5, 8, 2, 14, 9, 12]
    );
}

/// The air and flag updates carry the player's latest input tick.
#[test]
fn breath_updates_carry_the_input_tick() {
    let mut h = subject();
    h.route(PlayerAuthInputPacket {
        position: Vec3 {
            x: HOME.x as f32,
            y: eyes(HOME).y as f32,
            z: HOME.z as f32,
        },
        input_data: vec![EnumsPlayerAuthInputPacketPayloadInputData::Verticalcollision],
        client_tick: PlayerInputTick { inputtick: 77 },
        ..Default::default()
    });
    h.submerge(true);
    h.tick();
    let ticks: Vec<u64> = h
        .drain()
        .iter()
        .filter_map(|packet| match packet {
            McpePacketData::SetActorDataPacket(data)
                if data.target_runtime_id.actor_runtime_id == SESSION =>
            {
                Some(data.tick.inputtick)
            }
            _ => None,
        })
        .collect();
    assert_eq!(ticks, [77]);
}

//! A moving player against BDS 1.26.51
//! (`../testdata/bds-1.26.51-movement.json`; its `sources` name the
//! captures): `MOVING`, the sprint, sneak, swim and glide updates and their
//! refusals, the movement attribute, the food exhaustion of each move, and
//! falls onto each landing block.
//!
//! Each test replays the inputs the moving `bds-sweep-client` sent in a
//! phase (positions moved onto the void platform, client ticks kept), from
//! the health and exhaustion the subject had before it, and compares what
//! the player and its viewer received with what BDS sent, tick by tick:
//! both stamp a packet with the input tick it answers.

use jolyne::valentine::actor_data_i_ds as ids;
use jolyne::valentine::actor_flags;
use jolyne::valentine::types::DataItemEntryPayload;

use super::*;
use crate::entity::components::{ArmourInventory, Hunger, PlayerState};
use crate::entity::player_life::authority::{Judgement, PlayerMotion};
use crate::entity::player_life::environment::PlayerFall;
use crate::item::test_support::stack;

/// BDS's floor top is y -60, the platform's 16.
const DY: f64 = GROUND + 60.0;

fn fixture() -> &'static serde_json::Value {
    static FIXTURE: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    FIXTURE.get_or_init(|| {
        serde_json::from_str(include_str!("../testdata/bds-1.26.51-movement.json")).unwrap()
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

fn decoded(phase: &str, field: &str) -> Vec<McpePacketData> {
    fixture()["phases"][phase][field]
        .as_array()
        .unwrap_or_else(|| panic!("no phase {phase}"))
        .iter()
        .map(|hex| decode(hex.as_str().unwrap()))
        .collect()
}

/// The packets BDS sent the recording client in `phase`.
fn bds(phase: &str) -> Vec<McpePacketData> {
    decoded(phase, "packets")
}

/// The subject's inputs in `phase`.
fn inputs(phase: &str) -> Vec<PlayerAuthInputPacket> {
    decoded(phase, "inputs")
        .into_iter()
        .map(|packet| match packet {
            McpePacketData::PlayerAuthInputPacket(input) => *input,
            other => panic!("not an input: {other:?}"),
        })
        .collect()
}

/// The subject's attribute `name` before `phase`.
fn before(phase: &str, name: &str) -> f32 {
    fixture()["phases"][phase]["attributes_before"][name]
        .as_f64()
        .unwrap_or_else(|| panic!("{phase}: no {name} before it")) as f32
}

/// BDS's runtime ID of the subject (1 in every capture).
const BDS_SUBJECT: u64 = 1;

/// Where a BDS position lands on the platform.
#[derive(Debug, Clone, Copy)]
struct Map(DVec3);

impl Map {
    /// The platform position of BDS block `pos`.
    fn block(self, pos: IVec3) -> IVec3 {
        (pos.as_dvec3() + self.0).floor().as_ivec3()
    }

    fn eyes(self, input: &PlayerAuthInputPacket) -> DVec3 {
        DVec3::new(
            f64::from(input.position.x),
            f64::from(input.position.y),
            f64::from(input.position.z),
        ) + self.0
    }
}

/// The map that puts BDS's (x, z) at the platform's (x + dx, z + dz).
fn map(dx: f64, dz: f64) -> Map {
    Map(DVec3::new(dx, DY, dz))
}

/// Put the player (and the server's copy of it) where `input` says, at
/// rest and not falling: the start of a phase.
fn place(h: &mut Harness, input: &PlayerAuthInputPacket, map: Map) {
    put(h, map.eyes(input), false);
}

/// BDS's teleports in `phase`: the input tick each followed and the eyes
/// of its destination.
fn teleports(phase: &str) -> Vec<(u64, DVec3)> {
    fixture()["phases"][phase]["teleports"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|t| {
                    let v = |i: usize| t[i].as_f64().unwrap();
                    (t[0].as_u64().unwrap(), DVec3::new(v(1), v(2), v(3)))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Put the player and its copy at `eyes`: at rest, or teleported there (the
/// copy then rests through the next input, as BDS's does).
fn put(h: &mut Harness, eyes: DVec3, teleport: bool) {
    let player = h.player;
    h.world().get_mut::<Position>(player).unwrap().0 = eyes;
    if let Some(mut motion) = h.world().get_mut::<PlayerMotion>(player) {
        if teleport {
            motion.teleported(eyes);
        } else {
            motion.placed(eyes);
        }
    }
    if let Some(mut fall) = h.world().get_mut::<PlayerFall>(player) {
        *fall = PlayerFall::default();
    }
}

/// Replay `phase`'s inputs; returns what the player (and the viewer)
/// received during them.
fn replay(
    h: &mut Harness,
    phase: &str,
    map: Map,
    viewer: Option<&mut Joined>,
) -> (Vec<McpePacketData>, Vec<McpePacketData>) {
    replay_with(h, phase, map, viewer, |_, _| {})
}

/// BDS's corrections in `phase`: the input tick and the eyes and velocity
/// it put the client at.
fn bds_corrections(phase: &str) -> Vec<(u64, DVec3, DVec3)> {
    fixture()["phases"][phase]["corrections"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|c| {
                    let v = |i: usize| c[i].as_f64().unwrap();
                    (
                        c[0].as_u64().unwrap(),
                        DVec3::new(v(1), v(2), v(3)),
                        DVec3::new(v(4), v(5), v(6)),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The corrections the player was sent: tick, eyes and velocity.
fn our_corrections(packets: &[McpePacketData]) -> Vec<(u64, DVec3, DVec3)> {
    packets
        .iter()
        .filter_map(|packet| match packet {
            McpePacketData::CorrectPlayerMovePredictionPacket(c) => Some((
                c.tick.inputtick,
                DVec3::new(f64::from(c.pos.x), f64::from(c.pos.y), f64::from(c.pos.z)),
                DVec3::new(
                    f64::from(c.pos_delta.x),
                    f64::from(c.pos_delta.y),
                    f64::from(c.pos_delta.z),
                ),
            )),
            _ => None,
        })
        .collect()
}

/// The replayed client is corrected exactly where BDS corrected it, to the
/// state BDS sent: never where BDS was satisfied, and wherever the server's
/// copy judged the input. Near a block the simulation does not reproduce
/// the copy trusts the client and takes its state, so it can miss a
/// correction BDS sent there or up to [`TRUST_CARRIES`] inputs later; the
/// recorded client then jumped to BDS's state, which this server never
/// asked for, so a correction of ours in the inputs after such a miss is
/// the replay's artefact and not checked either.
fn assert_corrections_like_bds(
    phase: &str,
    map: Map,
    ours: &[(u64, DVec3, DVec3)],
    trusted: &[u64],
) {
    let theirs = bds_corrections(phase);
    let within = |ticks: &[u64], tick: u64| {
        ticks
            .iter()
            .any(|from| (*from..=from + TRUST_CARRIES).contains(&tick))
    };
    let mut missed = Vec::new();
    for (tick, ..) in &theirs {
        if ours.iter().any(|(ours, ..)| ours == tick) {
            continue;
        }
        assert!(
            within(trusted, *tick) || BDS_KEPT_ITS_COPY.contains(&(phase, *tick)),
            "{phase}: BDS corrected input {tick} where the copy judged and did not"
        );
        missed.push(*tick);
    }
    for (tick, eyes, velocity) in ours {
        let Some((_, bds_eyes, bds_velocity)) = theirs.iter().find(|(bds, ..)| bds == tick) else {
            assert!(
                within(&missed, *tick),
                "{phase}: corrected input {tick} that BDS did not correct ({eyes}, {velocity})"
            );
            continue;
        };
        let bds_eyes = *bds_eyes + map.0;
        assert!(
            eyes.distance(bds_eyes) < 2.0e-4 && velocity.distance(*bds_velocity) < 2.0e-4,
            "{phase} {tick}: corrected to {eyes} {velocity}, BDS to {bds_eyes} {bds_velocity}"
        );
    }
}

/// Inputs a trusted client's state carries on in the server's copy (a
/// bounce's velocity, say) before its own simulation would differ.
const TRUST_CARRIES: u64 = 20;

/// Corrections BDS sent a recording client this server's copy does not:
/// the copy takes a position within a tenth of a block where BDS kept its
/// own (`docs/quirks/bds.md`, Entities / player movement pull: the rule is
/// not known). In `slime_10` the round-3 client bounced off slime a little
/// higher than BDS (its slime physics was fixed since); the copy, which
/// bounces as BDS does, took each of its positions, BDS kept its copy and
/// corrected the eleventh disagreement.
const BDS_KEPT_ITS_COPY: [(&str, u64); 1] = [("slime_10", 713)];

/// [`replay`], calling `after` with each input's tick once the server has
/// ticked with it. Asserts the corrections match BDS's
/// ([`assert_corrections_like_bds`]).
fn replay_with(
    h: &mut Harness,
    phase: &str,
    map: Map,
    mut viewer: Option<&mut Joined>,
    mut after: impl FnMut(&mut Harness, u64),
) -> (Vec<McpePacketData>, Vec<McpePacketData>) {
    let inputs = inputs(phase);
    let teleports = teleports(phase);
    place(h, &inputs[0], map);
    h.tick();
    h.drain();
    if let Some(viewer) = viewer.as_deref_mut() {
        drain(viewer);
    }
    let mut trusted = Vec::new();
    for input in inputs {
        let eyes = map.eyes(&input);
        let tick = input.client_tick.inputtick;
        h.route(PlayerAuthInputPacket {
            position: Vec3 {
                x: eyes.x as f32,
                y: eyes.y as f32,
                z: eyes.z as f32,
            },
            ..input
        });
        h.tick();
        let player = h.player;
        if matches!(
            h.world().get::<PlayerMotion>(player).unwrap().last,
            Some(Judgement::Trusted(_))
        ) {
            trusted.push(tick);
        }
        // BDS's teleports after this input (the inputs the client had sent
        // before it saw one rest at the destination).
        for (_, to) in teleports.iter().filter(|(after, _)| *after == tick) {
            put(h, *to + map.0, true);
        }
        after(h, tick);
    }
    let player = h.drain();
    assert_corrections_like_bds(phase, map, &our_corrections(&player), &trusted);
    let seen = viewer.map(drain).unwrap_or_default();
    (player, seen)
}

/// A survival player that does not heal, as in the captures, with the
/// health and exhaustion the subject had before `phase`.
fn subject(phase: &str) -> Harness {
    subject_in(harness(Difficulty::Easy), phase)
}

/// [`subject`] on a platform 4 chunks around the origin, all of it in view:
/// for phases that cross more of it.
fn wide_subject(phase: &str) -> Harness {
    subject_in(harness_sized(Difficulty::Easy, 4, 4), phase)
}

fn subject_in(mut h: Harness, phase: &str) -> Harness {
    h.rules().natural_regeneration = false;
    let player = h.player;
    h.world().get_mut::<Hunger>(player).unwrap().exhaustion =
        before(phase, "minecraft:player.exhaustion");
    h.set_health(before(phase, "minecraft:health"));
    h.tick();
    h.drain();
    h
}

/// Replay the sneak of the first capture: BDS keeps `TRANSITION_BLOCKING`
/// in the second flag word after a sneak, so the phases after it carry it.
fn after_the_sneak(h: &mut Harness) {
    replay(h, "sneak", map(4.0, -8.0), None);
}

/// Put an elytra in `h`'s chest slot.
fn wear_elytra(h: &mut Harness) {
    let player = h.player;
    h.world()
        .get_mut::<ArmourInventory>(player)
        .unwrap()
        .set_chestplate(stack("minecraft:elytra", 1))
        .unwrap();
}

/// Fill the first capture's pool (water x 14..26, y -63..-56, z -6..6).
fn pool(h: &mut Harness, at: Map) {
    for x in 14..=26 {
        for y in -63..=-56 {
            for z in -6..=6 {
                h.set_block(at.block(IVec3::new(x, y, z)), *blocks::WATER);
            }
        }
    }
}

/// A comparable row value.
fn value(payload: &DataItemEntryPayload) -> String {
    match payload {
        DataItemEntryPayload::DataItemInt64Payload(v) => v.value.to_string(),
        DataItemEntryPayload::DataItemFloatPayload(v) => v.value.to_string(),
        DataItemEntryPayload::DataItemShortPayload(v) => v.value.to_string(),
        other => format!("{other:?}"),
    }
}

/// One `SetActorData`: its tick and the rows of the keys asked for.
type Update = (u64, Vec<(u32, String)>);

/// The `SetActorData` updates about `runtime_id`: tick and the rows of
/// `keys`, in packet order (updates without such rows left out).
fn updates(packets: &[McpePacketData], runtime_id: u64, keys: &[u32]) -> Vec<Update> {
    packets
        .iter()
        .filter_map(|packet| match packet {
            McpePacketData::SetActorDataPacket(data)
                if data.target_runtime_id.actor_runtime_id == runtime_id =>
            {
                let rows: Vec<(u32, String)> = data
                    .actor_data
                    .data
                    .iter()
                    .filter(|row| keys.contains(&row.id))
                    .map(|row| (row.id, value(&row.payload)))
                    .collect();
                (!rows.is_empty()).then_some((data.tick.inputtick, rows))
            }
            _ => None,
        })
        .collect()
}

/// The movement keys: the flag words, the box and key 139.
const MOVEMENT_KEYS: [u32; 5] = [
    ids::RESERVED_0,
    ids::RESERVED_053,
    ids::RESERVED_054,
    ids::RESERVED_092,
    ids::RESERVED_139,
];

const MOVING: i64 = 1 << actor_flags::MOVING;

/// Each change of the first-word flag `flag` in `updates`: tick and the new
/// value.
fn flag_edges(updates: &[Update], flag: u32) -> Vec<(u64, bool)> {
    let mut edges = Vec::new();
    let mut set = false;
    for (tick, rows) in updates {
        for (id, value) in rows {
            if *id == ids::RESERVED_0 {
                let now = value.parse::<i64>().unwrap() & (1 << flag) != 0;
                if now != set {
                    edges.push((*tick, now));
                    set = now;
                }
            }
        }
    }
    edges
}

/// Each change of `MOVING` in `updates`: tick and the new value.
fn moving_edges(updates: &[Update]) -> Vec<(u64, bool)> {
    flag_edges(updates, actor_flags::MOVING)
}

/// `updates` without `MOVING`: the bit cleared from the first flag word and
/// the updates that only changed it left out.
fn without_moving(updates: &[Update]) -> Vec<Update> {
    let mut out: Vec<Update> = Vec::new();
    let mut flags: Option<String> = None;
    for (tick, rows) in updates {
        let rows: Vec<(u32, String)> = rows
            .iter()
            .map(|(id, value)| {
                if *id == ids::RESERVED_0 {
                    (*id, (value.parse::<i64>().unwrap() & !MOVING).to_string())
                } else {
                    (*id, value.clone())
                }
            })
            .collect();
        let only_moving =
            rows.len() == 1 && rows[0].0 == ids::RESERVED_0 && flags.as_ref() == Some(&rows[0].1);
        if let Some((_, value)) = rows.iter().find(|(id, _)| *id == ids::RESERVED_0) {
            flags = Some(value.clone());
        }
        if !only_moving {
            out.push((*tick, rows));
        }
    }
    out
}

/// `name`'s values in the `UpdateAttributes` about `runtime_id`, with tick
/// and modifier count.
fn attribute(packets: &[McpePacketData], runtime_id: u64, name: &str) -> Vec<(u64, f32, usize)> {
    let mut out = Vec::new();
    for packet in packets {
        let McpePacketData::UpdateAttributesPacket(update) = packet else {
            continue;
        };
        if update.target_runtime_id.actor_runtime_id != runtime_id {
            continue;
        }
        if let Some(attribute) = update
            .attribute_list
            .iter()
            .find(|attribute| &*attribute.name == name)
        {
            out.push((
                update.tick.inputtick,
                attribute.current_value,
                attribute.modifiers.len(),
            ));
        }
    }
    out
}

/// The exhaustion updates about `runtime_id`: tick and value.
fn exhaustion(packets: &[McpePacketData], runtime_id: u64) -> Vec<(u64, f32)> {
    attribute(packets, runtime_id, "minecraft:player.exhaustion")
        .into_iter()
        .map(|(tick, value, _)| (tick, value))
        .collect()
}

/// The movement values about `runtime_id` (tick, value, modifiers), each
/// change once.
fn movement(packets: &[McpePacketData], runtime_id: u64) -> Vec<(u64, f32, usize)> {
    let mut values = attribute(packets, runtime_id, "minecraft:movement");
    values.dedup_by(|a, b| a.1 == b.1);
    values
}

/// `got` and `expected` hold the same ticks, their values within
/// `tolerance` (BDS sums the exhaustion in its own float order).
fn assert_series(got: &[(u64, f32)], expected: &[(u64, f32)], tolerance: f32, what: &str) {
    let ticks = |series: &[(u64, f32)]| series.iter().map(|(tick, _)| *tick).collect::<Vec<_>>();
    assert_eq!(
        ticks(got),
        ticks(expected),
        "{what}: {got:?} vs {expected:?}"
    );
    for ((tick, value), (_, bds)) in got.iter().zip(expected) {
        assert!(
            (value - bds).abs() <= tolerance,
            "{what} at {tick}: {value} vs {bds}"
        );
    }
}

/// `MOVING` follows each input's move: on at the first move of a walk, off
/// when an input moves the player less than √0.001 blocks; the viewer gets
/// the same updates.
#[test]
fn moving_follows_each_input() {
    let bds_subject = updates(&bds("walk"), BDS_SUBJECT, &MOVEMENT_KEYS);
    assert_eq!(moving_edges(&bds_subject), [(347, true), (370, false)]);
    assert_eq!(bds_subject.len(), 2);
    assert_eq!(
        updates(&bds("walk_viewer"), BDS_SUBJECT, &MOVEMENT_KEYS),
        bds_subject
    );
    let mut h = subject("walk");
    let mut viewer = h.viewer();
    let (seen, viewer_seen) = replay(&mut h, "walk", map(4.0, 4.0), Some(&mut viewer));
    assert_eq!(updates(&seen, SESSION, &MOVEMENT_KEYS), bds_subject);
    assert_eq!(updates(&viewer_seen, SESSION, &MOVEMENT_KEYS), bds_subject);
}

/// A jump turns `MOVING` on, off at the apex and on again; it costs 0.05
/// exhaustion (0.2 sprinting, the sprint-jump phase) instead of the
/// distance of its tick, and nothing is charged in the air.
#[test]
fn jumps_toggle_moving_and_cost_their_exhaustion() {
    for phase in ["jump", "sprint_jump"] {
        let bds = bds(phase);
        let mut h = subject(phase);
        let (seen, _) = replay(&mut h, phase, map(4.0, -10.0), None);
        assert_eq!(
            updates(&seen, SESSION, &MOVEMENT_KEYS),
            updates(&bds, BDS_SUBJECT, &MOVEMENT_KEYS),
            "{phase}"
        );
        let expected = exhaustion(&bds, BDS_SUBJECT);
        assert!(!expected.is_empty(), "{phase}");
        assert_series(&exhaustion(&seen, SESSION), &expected, 1.0e-5, phase);
    }
}

/// Holding the sprint key from rest and pressing forward starts the sprint
/// at once (flags, then the 0.13 movement with BDS's modifier, then
/// `MOVING`); releasing forward ends it. Sprinting costs 0.1 per horizontal
/// block every tick; the viewer gets the flags and the attributes too.
#[test]
fn a_sprint_starts_moving_forward_and_ends_with_it() {
    let bds_subject = bds("sprint");
    let bds_viewer = bds("sprint_viewer");
    let mut h = subject("sprint");
    let mut viewer = h.viewer();
    let (seen, viewer_seen) = replay(&mut h, "sprint", map(4.0, 0.0), Some(&mut viewer));
    for (who, bds, seen) in [
        ("player", &bds_subject, &seen),
        ("viewer", &bds_viewer, &viewer_seen),
    ] {
        assert_eq!(
            updates(seen, SESSION, &MOVEMENT_KEYS),
            updates(bds, BDS_SUBJECT, &MOVEMENT_KEYS),
            "{who}"
        );
        assert_eq!(movement(seen, SESSION), movement(bds, BDS_SUBJECT), "{who}");
        let expected = exhaustion(bds, BDS_SUBJECT);
        assert!(expected.len() > 30, "{who}");
        assert_series(&exhaustion(seen, SESSION), &expected, 1.0e-5, who);
    }
    let modifiers: Vec<usize> = movement(&seen, SESSION)
        .iter()
        .map(|(_, _, modifiers)| *modifiers)
        .collect();
    assert_eq!(modifiers, [1, 0]);
}

/// Start and stop sneaking with the vanilla flags: the flag word with the
/// box height, then the second flag word; `MOVING` while sneak-walking. The
/// viewer gets the same.
#[test]
fn sneaking_sends_the_updates_bds_sends() {
    let bds_subject = updates(&bds("sneak"), BDS_SUBJECT, &MOVEMENT_KEYS);
    assert_eq!(bds_subject.len(), 6);
    let mut h = subject("sneak");
    let mut viewer = h.viewer();
    let (seen, viewer_seen) = replay(&mut h, "sneak", map(4.0, -8.0), Some(&mut viewer));
    assert_eq!(updates(&seen, SESSION, &MOVEMENT_KEYS), bds_subject);
    assert_eq!(
        updates(&viewer_seen, SESSION, &MOVEMENT_KEYS),
        updates(&bds("sneak_viewer"), BDS_SUBJECT, &MOVEMENT_KEYS)
    );
}

/// A sneak that another update carries first (an item use, a game-mode
/// change in the same tick) still sends the sneak's two updates and keeps
/// `TRANSITION_BLOCKING` afterwards.
#[test]
fn a_sneak_sent_by_another_update_keeps_its_sequence() {
    let bds: Vec<Vec<(u32, String)>> =
        without_moving(&updates(&bds("sneak"), BDS_SUBJECT, &MOVEMENT_KEYS))
            .into_iter()
            .map(|(_, rows)| rows)
            .collect();
    assert_eq!(bds.len(), 4);
    let mut h = subject("sneak");
    let player = h.player;
    for sneaking in [true, false] {
        h.world().get_mut::<PlayerState>(player).unwrap().sneaking = sneaking;
        super::super::actor_data::send_changes(
            h.world(),
            player,
            super::super::actor_data::Keys::All,
        );
        h.tick();
        h.tick();
    }
    let seen: Vec<Vec<(u32, String)>> = updates(&h.drain(), SESSION, &MOVEMENT_KEYS)
        .into_iter()
        .map(|(_, rows)| rows)
        .collect();
    assert_eq!(seen, bds);
}

/// Sprinting in water starts the swim with the sprint (height 0.6, movement
/// 0.13); letting go ends the swim at once and the sprint a tick later, each
/// with the second flag word and key 139.
#[test]
fn a_swim_starts_in_water_and_ends_in_two_steps() {
    let bds = bds("swim");
    // BDS's pool sits at x 14..26: put it at x 2..14 of the platform.
    let at = map(-12.0, 9.0);
    let mut h = subject("swim");
    after_the_sneak(&mut h);
    pool(&mut h, at);
    let (seen, _) = replay(&mut h, "swim", at, None);
    let keys = [
        ids::RESERVED_0,
        ids::RESERVED_054,
        ids::RESERVED_092,
        ids::RESERVED_139,
    ];
    let (ours, theirs) = (
        updates(&seen, SESSION, &keys),
        updates(&bds, BDS_SUBJECT, &keys),
    );
    assert_eq!(without_moving(&ours), without_moving(&theirs));
    // `MOVING`: the same edges, but the last off comes a tick late: the
    // client's move at 861 is 0.001022, just over the limit, where BDS's own
    // simulation of it (which it nudges toward the client) was under it.
    let (ours, theirs) = (moving_edges(&ours), moving_edges(&theirs));
    assert_eq!(theirs, [(814, true), (861, false)]);
    assert_eq!(ours, [(814, true), (862, false)]);
    let values = |packets: &[McpePacketData], runtime_id| -> Vec<(u64, f32)> {
        movement(packets, runtime_id)
            .into_iter()
            .map(|(tick, value, _)| (tick, value))
            .collect()
    };
    assert_eq!(values(&seen, SESSION), values(&bds, BDS_SUBJECT));
}

/// A swim asked for on land and a glide asked for on the ground are
/// refused with the unchanged flag words and key 139.
#[test]
fn swims_on_land_and_glides_on_the_ground_are_refused() {
    for phase in ["swim_dry", "glide_ground"] {
        let bds = bds(phase);
        let mut h = subject(phase);
        after_the_sneak(&mut h);
        wear_elytra(&mut h);
        let (seen, _) = replay(&mut h, phase, map(4.0, 4.0), None);
        let keys = [ids::RESERVED_0, ids::RESERVED_092, ids::RESERVED_139];
        let bds_rows = updates(&bds, BDS_SUBJECT, &keys);
        assert_eq!(bds_rows.len(), 1, "{phase}: {bds_rows:?}");
        assert_eq!(updates(&seen, SESSION, &keys), bds_rows, "{phase}");
    }
}

/// A glide starts in the air with an elytra (`GLIDING`, height 0.6) and
/// ends on landing, back to 1.8; slowed under 0.5 blocks a tick, the
/// 30-block glide lands unhurt.
#[test]
fn a_glide_needs_the_air_and_an_elytra() {
    let bds = bds("glide");
    // The glide crosses 66 blocks of BDS's superflat floor.
    let mut h = wide_subject("glide");
    wear_elytra(&mut h);
    let player = h.player;
    let (seen, _) = replay(&mut h, "glide", map(4.0, 4.0), None);
    let keys = [ids::RESERVED_0, ids::RESERVED_054];
    assert_eq!(
        updates(&seen, SESSION, &keys),
        updates(&bds, BDS_SUBJECT, &keys)
    );
    assert!(!h.world().get::<PlayerState>(player).unwrap().gliding);
    let start = before("glide", "minecraft:health");
    assert_eq!(harm(&bds, BDS_SUBJECT, start), (vec![], vec![]));
    assert_eq!(harm(&seen, SESSION, start), (vec![], vec![]));
}

/// The hurt events of a phase and the health each hurt left, from `start`
/// down (BDS healed the subject between falls).
fn harm(packets: &[McpePacketData], runtime_id: u64, start: f32) -> (Vec<i32>, Vec<f32>) {
    let hurts = packets
        .iter()
        .filter_map(|packet| match packet {
            McpePacketData::ActorEventPacket(event)
                if event.target_runtime_id.actor_runtime_id == runtime_id
                    && event.event_id == EnumsActorEvent::Hurt =>
            {
                Some(event.data)
            }
            _ => None,
        })
        .collect();
    let mut health: Vec<f32> = Vec::new();
    for (_, value, _) in attribute(packets, runtime_id, "minecraft:health") {
        if value >= health.last().copied().unwrap_or(start) {
            break;
        }
        health.push(value);
    }
    (hurts, health)
}

/// Falls from 3 to 12.5 blocks onto stone, 10 blocks into water, onto hay,
/// slime (sneaking too), a bed and into powder snow: BDS's hurt events and
/// the health they leave; a fall's hurt event carries the eyes where the
/// player landed.
#[test]
fn falls_hurt_like_bds() {
    // The falls ran at x 40..64, z 0.5; the landing blocks at z 0.
    let at = map(-36.0, 4.0);
    let landing = |name: &str| crate::world::chunk::blocks::get_block_id(name);
    let cases: [(&str, &[(IVec3, &str)]); 12] = [
        ("fall_3", &[]),
        ("fall_3_5", &[]),
        ("fall_4", &[]),
        ("fall_5", &[]),
        ("fall_10", &[]),
        ("fall_12_5", &[]),
        (
            "water_10",
            &[
                (IVec3::new(44, -62, 0), "minecraft:water"),
                (IVec3::new(44, -61, 0), "minecraft:water"),
            ],
        ),
        ("hay_10", &[(IVec3::new(48, -61, 0), "minecraft:hay_block")]),
        ("slime_10", &[(IVec3::new(52, -61, 0), "minecraft:slime")]),
        ("bed_10", &[(IVec3::new(56, -60, 0), "minecraft:bed")]),
        (
            "powder_snow_10",
            &[
                (IVec3::new(60, -62, 0), "minecraft:powder_snow"),
                (IVec3::new(60, -61, 0), "minecraft:powder_snow"),
            ],
        ),
        (
            "slime_sneak_10",
            &[(IVec3::new(64, -61, 0), "minecraft:slime")],
        ),
    ];
    for (phase, blocks) in cases {
        let bds = bds(phase);
        let mut h = subject(phase);
        for (pos, name) in blocks {
            h.set_block(at.block(*pos), landing(name));
        }
        let (seen, _) = replay(&mut h, phase, at, None);
        let start = before(phase, "minecraft:health");
        assert_eq!(
            harm(&seen, SESSION, start),
            harm(&bds, BDS_SUBJECT, start),
            "{phase}"
        );
        let landed = |packets: &[McpePacketData], runtime_id, shift: DVec3| -> Vec<DVec3> {
            packets
                .iter()
                .filter_map(|packet| match packet {
                    McpePacketData::ActorEventPacket(event)
                        if event.target_runtime_id.actor_runtime_id == runtime_id
                            && event.event_id == EnumsActorEvent::Hurt =>
                    {
                        let at = event.fire_at_position.as_ref().expect("a fall's position");
                        Some(DVec3::new(f64::from(at.x), f64::from(at.y), f64::from(at.z)) + shift)
                    }
                    _ => None,
                })
                .collect()
        };
        let (ours, theirs) = (
            landed(&seen, SESSION, DVec3::ZERO),
            landed(&bds, BDS_SUBJECT, at.0),
        );
        assert_eq!(ours.len(), theirs.len(), "{phase}");
        for (ours, theirs) in ours.iter().zip(&theirs) {
            assert!(
                ours.distance(*theirs) < 1.0e-4,
                "{phase}: {ours} vs {theirs}"
            );
        }
    }
}

/// Feather Falling IV boots take 48 % off a 10-block fall (7 to 3.64).
#[test]
fn feather_falling_softens_a_fall() {
    let bds = bds("feather_falling_10");
    let (hurts, health) = harm(&bds, BDS_SUBJECT, 20.0);
    assert_eq!(
        (hurts.as_slice(), health.as_slice()),
        (&[5][..], &[16.36][..])
    );
    let mut h = subject("feather_falling_10");
    let player = h.player;
    let boots = crate::item::enchant::with_enchantments(
        &stack("minecraft:diamond_boots", 1),
        &[(crate::item::enchant::Enchantment::FeatherFalling, 4)],
    );
    h.world()
        .get_mut::<ArmourInventory>(player)
        .unwrap()
        .set_boots(boots)
        .unwrap();
    let (seen, _) = replay(&mut h, "feather_falling_10", map(-36.0, 4.0), None);
    assert_eq!(harm(&seen, SESSION, 20.0), (hurts, health));
}

/// Each exhaustion update's increase, from `base`.
fn steps(series: &[(u64, f32)], base: f32) -> Vec<(u64, f32)> {
    let mut last = base;
    series
        .iter()
        .map(|(tick, value)| {
            let step = (*tick, value - last);
            last = *value;
            step
        })
        .collect()
}

/// Ticks of the exhaustion phases the replay cannot charge as BDS did:
/// - after the sprint-swim stop the client kept the sprint's water drag a
///   tick longer than BDS's copy (608 to the swim's end at 619, up to 0.016
///   apart); the server's copy takes the client's position that close where
///   BDS kept its own, so it charges the client's moves;
/// - the water walk's window starts mid-walk (619), where the replay's copy
///   starts at rest.
const BDS_OWN_MOVES: [(&str, std::ops::RangeInclusive<u64>); 2] = [
    ("swim_exhaustion", 608..=619),
    ("water_walk_exhaustion", 619..=619),
];

/// Exhaustion of each kind of move, tick by tick: none walking or
/// sneak-walking, 0.1 per horizontal block sprinting on the ground, 0.05 a
/// jump, 0.2 a sprint-jump, 0.01 per block moved in water.
#[test]
fn exhaustion_matches_bds_tick_by_tick() {
    let pool_at = map(-12.0, 4.0);
    for (phase, at, water) in [
        ("walk_exhaustion", map(4.0, 4.0), false),
        ("sprint_exhaustion", map(4.0, 4.0), false),
        ("jumps_exhaustion", map(4.0, 8.0), false),
        ("sprint_jumps_exhaustion", map(4.0, 8.0), false),
        ("sneak_walk_exhaustion", map(4.0, -4.0), false),
        ("swim_exhaustion", pool_at, true),
        ("water_walk_exhaustion", pool_at, true),
    ] {
        let bds = bds(phase);
        let mut h = subject(phase);
        if water {
            pool(&mut h, at);
        }
        let (seen, _) = replay(&mut h, phase, at, None);
        let base = before(phase, "minecraft:player.exhaustion");
        let compared = |series: &[(u64, f32)]| -> Vec<(u64, f32)> {
            steps(series, base)
                .into_iter()
                .filter(|(tick, _)| {
                    !BDS_OWN_MOVES
                        .iter()
                        .any(|(name, ticks)| *name == phase && ticks.contains(tick))
                })
                .collect()
        };
        let expected = compared(&exhaustion(&bds, BDS_SUBJECT));
        let walking = matches!(phase, "walk_exhaustion" | "sneak_walk_exhaustion");
        assert_eq!(expected.is_empty(), walking, "{phase}");
        assert_series(
            &compared(&exhaustion(&seen, SESSION)),
            &expected,
            1.0e-6,
            phase,
        );
    }
}

/// A knocked-back player is `MOVING` from the input after the hit (BDS
/// moves it with the push before the client reports it), while it flies and
/// slides, then not.
#[test]
fn a_pushed_player_is_moving() {
    let bds = bds("push");
    let keys = [ids::RESERVED_0];
    let mut h = subject("push");
    // `/damage 1 entity_attack` from the viewer 3 blocks east, processed
    // after the inputs of ticks 1290 and 1330.
    let (seen, _) = replay_with(&mut h, "push", map(4.0, 4.0), None, |h, tick| {
        if tick == 1290 || tick == 1330 {
            let player = h.player;
            let eyes = h.world().get::<Position>(player).unwrap().0;
            crate::entity::player_life::hurt_player(
                h.world(),
                player,
                &Hit {
                    damage: 1.0,
                    cause: ActorDamageCause::EntityAttack,
                    attacker: None,
                    by_player: false,
                    knockback_from: Some(eyes + DVec3::X * 3.0),
                    knockback_power: KnockbackPower::DEFAULT,
                },
            );
        }
    });
    let theirs = moving_edges(&updates(&bds, BDS_SUBJECT, &keys));
    assert_eq!(
        theirs,
        [(1291, true), (1304, false), (1331, true), (1344, false)]
    );
    // The server's copy flies, lands and is pulled towards the client (a
    // tick late with each push) exactly as BDS's, and corrects it at 1301
    // and 1341 as BDS did (the replay checks the corrections). On the input
    // after the second correction BDS kept its own copy where this server
    // takes the client's position (inside a tenth of a block), so the
    // copy's slide stops a tick later.
    let ours = moving_edges(&updates(&seen, SESSION, &keys));
    assert_eq!(ours[..3], theirs[..3]);
    assert_eq!(ours[3], (1345, false));
}

/// Each hurt about `runtime_id`: the tick of the health update that
/// follows it, its cause and the health it left.
fn hurts(packets: &[McpePacketData], runtime_id: u64) -> Vec<(u64, i32, f32)> {
    let mut out = Vec::new();
    let mut cause = None;
    for packet in packets {
        match packet {
            McpePacketData::ActorEventPacket(event)
                if event.target_runtime_id.actor_runtime_id == runtime_id
                    && event.event_id == EnumsActorEvent::Hurt =>
            {
                cause = Some(event.data);
            }
            McpePacketData::UpdateAttributesPacket(update)
                if update.target_runtime_id.actor_runtime_id == runtime_id =>
            {
                let health = update
                    .attribute_list
                    .iter()
                    .find(|attribute| &*attribute.name == "minecraft:health");
                if let (Some(health), Some(cause)) = (health, cause) {
                    out.push((update.tick.inputtick, cause, health.current_value));
                }
                if health.is_some() {
                    cause = None;
                }
            }
            _ => {}
        }
    }
    out
}

/// A fire block at the feet for 4 s, then gone: a hurt every 10 ticks in
/// it (the hurt timer), then a 1-point `fire_tick` burn 20, 40, ... ticks
/// before the fire's 8 s run out, and `ONFIRE` off when they have.
#[test]
fn a_fire_block_hurts_then_burns_out_like_bds() {
    const FIRE: i32 = 6;
    const FIRE_TICK: i32 = 7;
    let bds = bds("fire");
    // The fire block stood at x 40, z 0, on netherrack.
    let at = map(-36.0, 4.0);
    let block = |name: &str| crate::world::chunk::blocks::get_block_id(name);
    let mut h = subject("fire");
    h.set_block(
        at.block(IVec3::new(40, -61, 0)),
        block("minecraft:netherrack"),
    );
    // BDS's commands placed the fire in the tick of input 711 and took it
    // away after input 790.
    let fire = at.block(IVec3::new(40, -60, 0));
    let (seen, _) = replay_with(&mut h, "fire", at, None, |h, tick| match tick {
        710 => h.set_block(fire, block("minecraft:fire")),
        790 => h.set_block(fire, block("minecraft:air")),
        _ => {}
    });
    let (ours, theirs) = (hurts(&seen, SESSION), hurts(&bds, BDS_SUBJECT));
    assert_eq!(theirs.len(), 15);
    assert_eq!(ours.len(), theirs.len(), "{ours:?}");
    // BDS stamps a packet with the input it last handled, which runs up to
    // three ticks behind its own clock here; the replay handles one input
    // a tick.
    for ((tick, cause, health), (bds_tick, bds_cause, bds_health)) in ours.iter().zip(&theirs) {
        assert_eq!(health, bds_health, "at {bds_tick}");
        assert!(tick.abs_diff(*bds_tick) <= 3, "{tick} vs {bds_tick}");
        if *bds_tick > 790 {
            // Out of the fire only the burn hurts. In it, BDS's causes
            // alternate between the block's `fire` and the burn's
            // `fire_tick` with its input timing; ours are the block's but
            // the last.
            assert_eq!(
                (cause, bds_cause),
                (&FIRE_TICK, &FIRE_TICK),
                "at {bds_tick}"
            );
        } else {
            assert!([FIRE, FIRE_TICK].contains(cause), "at {tick}");
        }
    }
    let onfire = |packets: &[McpePacketData], runtime_id| {
        flag_edges(
            &updates(packets, runtime_id, &[ids::RESERVED_0]),
            actor_flags::ONFIRE,
        )
    };
    let (ours, theirs) = (onfire(&seen, SESSION), onfire(&bds, BDS_SUBJECT));
    assert_eq!(theirs, [(711, true), (947, false)]);
    assert_eq!(ours.len(), 2);
    for ((tick, set), (bds_tick, bds_set)) in ours.iter().zip(&theirs) {
        assert_eq!(set, bds_set);
        assert!(tick.abs_diff(*bds_tick) <= 3, "{tick} vs {bds_tick}");
    }
}

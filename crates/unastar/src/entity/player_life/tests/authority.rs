//! The server's movement authority against BDS 1.26.51
//! (`../testdata/bds-1.26.51-authority.json`, distilled by
//! `tools/bds-sweep/authority_fixture.py`; its `sources` name the probes):
//! a sweep client reporting positions offset from its own simulation, too
//! fast, through a wall, up a step, hovering, claiming the ground while
//! falling, or ignoring teleports, respawns and corrections, and BDS's own
//! copy of it as a second client saw it.
//!
//! Each phase is replayed input by input on the void platform (BDS's floor
//! top at -60 is the platform's 16). After every input the server's copy
//! must stand where BDS's did, and the corrections must be BDS's: the same
//! inputs, the copy's position and velocity.

use serde_json::Value;

use super::*;
use crate::entity::player_life::authority::PlayerMotion;

/// BDS's floor top is y -60, the platform's 16.
const DY: f64 = GROUND + 60.0;

/// The respawns in the probes: the phase and the input each followed. BDS
/// sent no `MovePlayer` for them; its copy stood at the spawn point from
/// the next input on.
const RESPAWNS: [(&str, u64); 2] = [("respawn_ignored", 963), ("respawn", 1203)];

fn fixture() -> &'static Value {
    static FIXTURE: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    FIXTURE.get_or_init(|| {
        serde_json::from_str(include_str!("../testdata/bds-1.26.51-authority.json")).unwrap()
    })
}

fn rows(phase: &str, list: &str) -> &'static [Value] {
    fixture()["phases"][phase][list]
        .as_array()
        .unwrap_or_else(|| panic!("no {list} in {phase}"))
}

fn f(value: &Value) -> f64 {
    value.as_f64().unwrap()
}

fn vec3(row: &[Value], at: usize) -> DVec3 {
    DVec3::new(f(&row[at]), f(&row[at + 1]), f(&row[at + 2]))
}

/// A correction: its input tick, the eyes, the velocity and the ground
/// contact it carries.
type Correction = (u64, DVec3, DVec3, bool);

/// How a replay went: the inputs where the copy stood elsewhere than BDS's
/// (tick, ours, BDS's) and the corrections sent.
struct Replayed {
    apart: Vec<(u64, DVec3, DVec3)>,
    corrections: Vec<Correction>,
}

/// Replay `phase`, BDS's (x, z) moved by `shift` on the platform.
fn replay(h: &mut Harness, phase: &str, shift: DVec3) -> Replayed {
    use EnumsPlayerAuthInputPacketPayloadInputData as Flag;
    let shift = shift + DVec3::Y * DY;
    let inputs = rows(phase, "inputs");
    let relays = rows(phase, "relays");
    let teleports = rows(phase, "teleports");
    // The copy starts where BDS's stood after the first input, at rest.
    let first = relays[0].as_array().unwrap();
    let player = h.player;
    let start = vec3(first, 1) + shift;
    let feet = start - DVec3::Y * f64::from(unastar_rules::movement::NETWORK_OFFSET);
    h.world().get_mut::<Position>(player).unwrap().0 = eyes(feet);
    h.world()
        .get_mut::<PlayerMotion>(player)
        .unwrap()
        .placed(eyes(feet));
    h.tick();
    h.drain();
    let mut apart = Vec::new();
    for input in inputs {
        let row = input.as_array().unwrap();
        let tick = row[0].as_u64().unwrap();
        let claim = vec3(row, 1) + shift;
        let flags: Vec<Flag> = row[8]
            .as_array()
            .unwrap()
            .iter()
            .map(|flag| match flag.as_str().unwrap() {
                "Jumping" => Flag::Jumping,
                "Sneaking" => Flag::Sneaking,
                "Sprintdown" => Flag::Sprintdown,
                "Startgliding" => Flag::Startgliding,
                "Handledteleport" => Flag::Handledteleport,
                "Verticalcollision" => Flag::Verticalcollision,
                "Startjumping" => Flag::Startjumping,
                other => panic!("flag {other}"),
            })
            .collect();
        h.route(PlayerAuthInputPacket {
            position: Vec3 {
                x: claim.x as f32,
                y: claim.y as f32,
                z: claim.z as f32,
            },
            player_rotation: jolyne::valentine::types::Vec2 {
                x: f(&row[4]) as f32,
                y: f(&row[5]) as f32,
            },
            player_head_rotation: f(&row[5]) as f32,
            move_vector: jolyne::valentine::types::Vec2 {
                x: f(&row[6]) as f32,
                y: f(&row[7]) as f32,
            },
            input_data: flags,
            client_tick: jolyne::valentine::types::PlayerInputTick { inputtick: tick },
            ..Default::default()
        });
        h.tick();
        if tick > first[0].as_u64().unwrap()
            && let Some(relay) = relays.iter().find(|r| r[0].as_u64() == Some(tick))
        {
            let bds = vec3(relay.as_array().unwrap(), 1) + shift;
            let bds_feet = bds - DVec3::Y * f64::from(unastar_rules::movement::NETWORK_OFFSET);
            let ours = h.feet();
            // The server's positions are `f64`, BDS's `f32`.
            if ours.distance(bds_feet) > 1.0e-4 {
                apart.push((tick, ours, bds_feet));
            }
        }
        for teleport in teleports.iter().filter(|t| t[0].as_u64() == Some(tick)) {
            let to = vec3(teleport.as_array().unwrap(), 1) + shift;
            let feet = to - DVec3::Y * f64::from(unastar_rules::movement::NETWORK_OFFSET);
            h.world().get_mut::<Position>(player).unwrap().0 = eyes(feet);
            h.world()
                .get_mut::<PlayerMotion>(player)
                .unwrap()
                .teleported(eyes(feet));
        }
        if RESPAWNS.contains(&(phase, tick)) {
            // The copy stands still at the spawn point after the next input.
            let next = relays
                .iter()
                .find(|r| r[0].as_u64() == Some(tick + 1))
                .unwrap();
            let spawn = vec3(next.as_array().unwrap(), 1) + shift;
            let feet = spawn - DVec3::Y * f64::from(unastar_rules::movement::NETWORK_OFFSET);
            h.world().get_mut::<Position>(player).unwrap().0 = eyes(feet);
            h.world()
                .get_mut::<PlayerMotion>(player)
                .unwrap()
                .respawned(eyes(feet));
        }
    }
    let corrections = h
        .drain()
        .into_iter()
        .filter_map(|packet| match packet {
            McpePacketData::CorrectPlayerMovePredictionPacket(c) => {
                // The fields every correction in the probes carried,
                // whatever the input.
                let fields = &fixture()["correction_fields"];
                assert_eq!(
                    format!("{:?}", c.prediction_type),
                    fields["prediction_type"].as_str().unwrap()
                );
                assert_eq!(
                    [f64::from(c.rotation.x), f64::from(c.rotation.y)],
                    [f(&fields["rotation"][0]), f(&fields["rotation"][1])]
                );
                assert_eq!(
                    c.vehicle_angular_velocity.is_none(),
                    fields["vehicle_angular_velocity"].is_null()
                );
                Some((
                    c.tick.inputtick,
                    DVec3::new(f64::from(c.pos.x), f64::from(c.pos.y), f64::from(c.pos.z)) - shift,
                    DVec3::new(
                        f64::from(c.pos_delta.x),
                        f64::from(c.pos_delta.y),
                        f64::from(c.pos_delta.z),
                    ),
                    c.on_ground,
                ))
            }
            _ => None,
        })
        .collect();
    Replayed { apart, corrections }
}

/// BDS's corrections in `phase`.
fn bds_corrections(phase: &str) -> Vec<Correction> {
    rows(phase, "corrections")
        .iter()
        .map(|c| {
            let c = c.as_array().unwrap();
            (
                c[0].as_u64().unwrap(),
                vec3(c, 1),
                vec3(c, 4),
                c[7].as_bool().unwrap(),
            )
        })
        .collect()
}

fn same_corrections(ours: &[Correction], theirs: &[Correction]) -> bool {
    ours.len() == theirs.len()
        && ours.iter().zip(theirs).all(|(ours, theirs)| {
            ours.0 == theirs.0
                && ours.1.distance(theirs.1) < 2.0e-4
                && ours.2.distance(theirs.2) < 2.0e-4
                && ours.3 == theirs.3
        })
}

/// A harness whose platform has the probe's blocks: BDS's stone wall at
/// x 12, y -60..-57, z -3..3 and step at x 20, y -60, z -3..3. The platform
/// reaches past the falls at x 30 and 34.
fn arena(shift: DVec3) -> Harness {
    let mut h = harness_sized(Difficulty::Easy, 4, 4);
    h.rules().natural_regeneration = false;
    for z in -3..=3 {
        for y in -60..=-57 {
            let pos = (DVec3::new(12.0, f64::from(y), f64::from(z)) + shift + DVec3::Y * DY)
                .floor()
                .as_ivec3();
            h.set_block(pos, *blocks::STONE);
        }
        let step = (DVec3::new(20.0, -60.0, f64::from(z)) + shift + DVec3::Y * DY)
            .floor()
            .as_ivec3();
        h.set_block(step, *blocks::STONE);
    }
    h
}

/// The phases whose every input the server's copy follows as BDS's did,
/// with BDS's corrections: teleports followed, late, ignored; respawns
/// followed and ignored (the input after one is simulated from the spawn
/// point, so its correction carries that input's gravity); positions
/// reported inside and outside the threshold, corrections followed and
/// ignored; too fast, through a wall, hovering, a no-fall.
#[test]
fn the_copy_moves_and_corrects_like_bds() {
    let shift = DVec3::new(4.0, 0.0, 4.0);
    for phase in [
        "tp_follow",
        "tp_late_5_walking",
        "tp_ignore_uncorrected",
        "respawn_ignored",
        "respawn",
        "offset_z_0_2",
        "offset_xz_0_4",
        "offset_1_uncorrected",
        "offset_0_3_uncorrected",
        "speed",
        "phase_wall",
        "fly",
        "nofall",
        "slow_descent_uncorrected",
        "offset_z_0_09",
        "offset_z_0_11",
        "offset_xz_0_08",
        "offset_y_minus_0_09",
        "tp_0_05",
        "tp_0_05_ignored",
        "tp_0_3_ignored",
    ] {
        let mut h = arena(shift);
        let replayed = replay(&mut h, phase, shift);
        assert!(
            replayed.apart.is_empty(),
            "{phase}: the copy left BDS's at {:?}",
            &replayed.apart[..replayed.apart.len().min(3)]
        );
        let theirs = bds_corrections(phase);
        assert!(
            same_corrections(&replayed.corrections, &theirs),
            "{phase}: corrected {:?}, BDS {theirs:?}",
            replayed.corrections
        );
    }
}

/// Where BDS keeps its own copy although the client is inside a tenth of a
/// block, this server takes the client's position (the rule BDS follows
/// there is not known): after a walk the client reported stopped on a step
/// (`step_hack`, inputs 519 to 521), a client hovering 0.09 above the floor
/// (`offset_y_0_09`: BDS took ten inputs, then about one in two) and
/// positions drifting 0.05 or 0.095 a tick ahead (BDS took about one input
/// in two, and stopped after seven at 0.095). The corrections are BDS's but
/// for the 0.095 drift, which this server corrects three inputs later, once
/// it is out of the threshold.
#[test]
fn close_positions_bds_keeps_are_taken() {
    let shift = DVec3::new(4.0, 0.0, 4.0);
    for (phase, apart, corrected) in [
        ("step_hack", &[519, 520, 521][..], &[523, 530, 536][..]),
        ("offset_y_0_09", &[550, 551, 553, 555, 556][..], &[][..]),
        ("drift_z_0_05", &[703, 705, 707, 709][..], &[][..]),
        ("drift_z_0_095", &[818, 819, 820, 821][..], &[829][..]),
    ] {
        let mut h = arena(shift);
        let replayed = replay(&mut h, phase, shift);
        let ticks: Vec<u64> = replayed.apart.iter().map(|(tick, ..)| *tick).collect();
        assert_eq!(&ticks[..apart.len()], apart, "{phase}");
        let ours: Vec<u64> = replayed.corrections.iter().map(|c| c.0).collect();
        assert_eq!(ours, corrected, "{phase}");
    }
}

/// Falls hurt by the server's copy, not by the client's word: BDS's health
/// updates in the cheat probes say 2 for a client hovering 5 blocks up
/// (`fly`, its copy landing at input 570), and 7 for one claiming the
/// ground through a 10-block fall (`nofall`, input 801) or descending
/// slowly through it (`slow_descent_uncorrected`, input 884).
#[test]
fn cheated_falls_hurt_like_bds() {
    let shift = DVec3::new(4.0, 0.0, 4.0);
    for (phase, damage) in [
        ("fly", 2.0),
        ("nofall", 7.0),
        ("slow_descent_uncorrected", 7.0),
    ] {
        let mut h = arena(shift);
        replay(&mut h, phase, shift);
        assert_eq!(20.0 - h.health(), damage, "{phase}");
    }
}

/// Flight is not simulated, so a flying player is trusted: its copy takes
/// the position it reports, without a correction, unless that is more than
/// 64 blocks from the tracked one.
#[test]
fn flight_is_trusted_within_64_blocks() {
    let mut h = harness(Difficulty::Easy);
    let player = h.player;
    h.world()
        .get_mut::<crate::entity::components::PlayerState>(player)
        .unwrap()
        .flying = true;
    let start = h.feet();
    for (to, taken) in [
        (start + DVec3::new(0.0, 10.0, 0.0), true),
        (start + DVec3::new(100.0, 10.0, 0.0), false),
    ] {
        let before = h.feet();
        let position = reported(to);
        h.route(PlayerAuthInputPacket {
            position: Vec3 {
                x: position.x as f32,
                y: position.y as f32,
                z: position.z as f32,
            },
            ..Default::default()
        });
        h.tick();
        let corrections = h
            .drain()
            .into_iter()
            .filter(|packet| matches!(packet, McpePacketData::CorrectPlayerMovePredictionPacket(_)))
            .count();
        assert_eq!(corrections, 0, "{to}");
        let expected = if taken { to } else { before };
        assert!(
            h.feet().distance(expected) < 1.0e-4,
            "{to}: at {}",
            h.feet()
        );
    }
}

/// A position 100 blocks from the copy is not taken: the copy moves as the
/// input's keys say (here, not at all) and the client is corrected to it.
#[test]
fn a_teleport_sized_jump_is_corrected() {
    let mut h = harness(Difficulty::Easy);
    let start = h.feet();
    h.step(start, true);
    h.drain();
    let position = reported(start + DVec3::X * 100.0);
    h.route(PlayerAuthInputPacket {
        position: Vec3 {
            x: position.x as f32,
            y: position.y as f32,
            z: position.z as f32,
        },
        input_data: vec![EnumsPlayerAuthInputPacketPayloadInputData::Verticalcollision],
        ..Default::default()
    });
    h.tick();
    let corrections: Vec<_> = h
        .drain()
        .into_iter()
        .filter_map(|packet| match packet {
            McpePacketData::CorrectPlayerMovePredictionPacket(c) => Some(c),
            _ => None,
        })
        .collect();
    let at = reported(start);
    assert_eq!(
        corrections
            .iter()
            .map(|c| (c.pos.x, c.pos.y, c.pos.z))
            .collect::<Vec<_>>(),
        [(at.x as f32, at.y as f32, at.z as f32)]
    );
    assert!(h.feet().distance(start) < 1.0e-4, "at {}", h.feet());
}

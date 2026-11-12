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

use jolyne::valentine::types::EnumsRewindType;

use super::*;
use crate::entity::components::PlayerState;
use crate::entity::player_life::authority::{Judgement, PlayerMotion, Trust};
use EnumsPlayerAuthInputPacketPayloadInputData as Flag;
use unastar_rules::movement::authority::Verdict;

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
                let expected = match fields["prediction_type"].as_str() {
                    Some("Player") => EnumsRewindType::Player,
                    other => panic!("prediction type {other:?}"),
                };
                assert_eq!(c.prediction_type, expected);
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
/// ignored; too fast, through a wall, hovering, a no-fall; the horizontal
/// position and the height off independently (a close horizontal position
/// is taken however far off the height is).
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
        "split_x_up_3",
        "split_xz_up_3",
        "split_x_0_11_up_3",
        "split_x_down_3",
        "split_x_0_3_up_3",
        "split_walking_x_up_3",
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

/// One input standing at `feet` with `flags`; returns the corrections sent.
fn stand(h: &mut Harness, feet: DVec3, flags: Vec<Flag>) -> usize {
    let position = reported(feet);
    h.route(PlayerAuthInputPacket {
        position: Vec3 {
            x: position.x as f32,
            y: position.y as f32,
            z: position.z as f32,
        },
        input_data: flags,
        ..Default::default()
    });
    h.tick();
    corrections(h.drain()).len()
}

/// The feet of the server's copy of the player.
fn copy_feet(h: &mut Harness) -> DVec3 {
    let player = h.player;
    let [x, y, z] = h.world().get::<PlayerMotion>(player).unwrap().body.feet;
    DVec3::new(f64::from(x), f64::from(y), f64::from(z))
}

/// How the copy took the last input.
fn judgement(h: &mut Harness) -> Judgement {
    let player = h.player;
    h.world().get::<PlayerMotion>(player).unwrap().last.unwrap()
}

/// Flight is not simulated, so a player flying in a game mode that can fly
/// is trusted: its copy takes the position it reports, without a
/// correction, unless that is more than 64 blocks from the copy. Such an
/// input is refused whole and leaves the copy where it was, so repeating
/// it does not move the player either.
#[test]
fn flight_is_trusted_within_64_blocks() {
    let mut h = harness(Difficulty::Easy);
    h.set_mode(GameMode::Creative);
    let player = h.player;
    h.world().get_mut::<PlayerState>(player).unwrap().flying = true;
    let start = h.feet();
    let up = start + DVec3::new(0.0, 10.0, 0.0);
    let far = start + DVec3::new(100.0, 10.0, 0.0);
    let taken = Judgement::Trusted(Trust::Flight);
    let refused = Judgement::Refused(Trust::Flight);
    for (to, expected, judged) in [(up, up, taken), (far, up, refused), (far, up, refused)] {
        assert_eq!(stand(&mut h, to, Vec::new()), 0, "{to}");
        assert!(
            h.feet().distance(expected) < 1.0e-4,
            "{to}: at {}",
            h.feet()
        );
        assert!(
            copy_feet(&mut h).distance(expected) < 1.0e-4,
            "{to}: copy at {}",
            copy_feet(&mut h)
        );
        assert_eq!(judgement(&mut h), judged, "{to}");
    }
}

/// A trusted input's velocity is bounded: a flying client reporting one of
/// 1e30 blocks a tick leaves the copy at the fastest a player moves.
#[test]
fn a_trusted_velocity_is_bounded() {
    let mut h = harness(Difficulty::Easy);
    h.set_mode(GameMode::Creative);
    let player = h.player;
    h.world().get_mut::<PlayerState>(player).unwrap().flying = true;
    let start = h.feet();
    h.put(start);
    let position = reported(start + DVec3::Y);
    h.route(PlayerAuthInputPacket {
        position: Vec3 {
            x: position.x as f32,
            y: position.y as f32,
            z: position.z as f32,
        },
        pos_delta: Vec3 {
            x: 0.0,
            y: 1.0e30,
            z: -1.0e30,
        },
        ..Default::default()
    });
    h.tick();
    assert_eq!(judgement(&mut h), Judgement::Trusted(Trust::Flight));
    let velocity = h.world().get::<PlayerMotion>(player).unwrap().body.velocity;
    assert_eq!(velocity, [0.0, 4.0, -4.0]);
}

/// A client cannot make itself trusted by claiming flight: in a game mode
/// that cannot fly its `StartFlying` is ignored, and hovering 5 blocks up
/// is corrected like any other position out of reach.
#[test]
fn a_survival_client_cannot_fly() {
    let mut h = harness(Difficulty::Easy);
    let player = h.player;
    let start = h.feet();
    h.step(start, true);
    h.drain();
    let corrections = stand(&mut h, start + DVec3::Y * 5.0, vec![Flag::Startflying]);
    assert_eq!(corrections, 1);
    assert_eq!(judgement(&mut h), Judgement::Judged(Verdict::Correct));
    assert!(!h.world().get::<PlayerState>(player).unwrap().flying);
    assert!(h.feet().distance(start) < 1.0e-4, "at {}", h.feet());
}

/// A sprint the copy picks up after trusted inputs keeps its speed: a
/// client that starts sprinting under an effect the copy trusts goes on
/// sprinting once the effect ends, and the copy follows it exactly,
/// without a correction.
#[test]
fn a_sprint_goes_on_after_trusted_inputs() {
    use crate::entity::components::{EffectType, Effects};
    let mut h = harness_sized(Difficulty::Easy, 4, 4);
    let player = h.player;
    let start = h.feet();
    h.put(start);
    let mut effects = Effects::new();
    effects.add(EffectType::JUMP_BOOST, 0, 100_000);
    h.world().entity_mut(player).insert(effects);
    let mut client = unastar_rules::movement::Body::at(start.as_vec3().to_array());
    client.on_ground = true;
    let controls = unastar_rules::movement::Controls {
        forward: 1.0,
        sprint: true,
        ..Default::default()
    };
    let mut sent = 0;
    for tick in 1..=40 {
        if tick == 10 {
            h.world().entity_mut(player).insert(Effects::new());
        }
        let input = h.predict(&mut client, &controls, tick);
        h.route(input);
        h.tick();
        let judged = judgement(&mut h);
        if tick < 10 {
            assert_eq!(judged, Judgement::Trusted(Trust::Effect), "input {tick}");
        } else {
            assert!(
                matches!(judged, Judgement::Judged(_)),
                "input {tick}: {judged:?}"
            );
        }
        sent += corrections(h.drain()).len();
    }
    assert_eq!(sent, 0);
    let [x, y, z] = client.feet;
    let client_feet = DVec3::new(f64::from(x), f64::from(y), f64::from(z));
    assert!(client_feet.distance(start) > 8.0, "the client sprinted");
    assert!(
        copy_feet(&mut h).distance(client_feet) < 1.0e-4,
        "copy at {}, client at {client_feet}",
        copy_feet(&mut h)
    );
}

/// A client only slower than the copy's simulation is taken: one eating
/// while it walks (it scales its move, the copy, which does not model that,
/// does not), and one holding the sprint key at 6 food (it cannot sprint,
/// the copy would). Neither is corrected, and the copy ends where it does.
#[test]
fn a_slower_client_is_taken() {
    for (case, forward, sprint_key) in [("eating", 0.2, false), ("hungry", 1.0, true)] {
        let mut h = harness_sized(Difficulty::Easy, 4, 4);
        let start = h.feet();
        h.put(start);
        let mut client = unastar_rules::movement::Body::at(start.as_vec3().to_array());
        client.on_ground = true;
        let controls = unastar_rules::movement::Controls {
            forward,
            ..Default::default()
        };
        let (mut sent, mut slowed) = (0, 0);
        for tick in 1..=30 {
            let mut input = h.predict(&mut client, &controls, tick);
            // The keys as pressed: the slowdown is the client's own.
            input.move_vector.y = 1.0;
            if sprint_key {
                input.input_data.push(Flag::Sprintdown);
            }
            h.route(input);
            h.tick();
            if judgement(&mut h) == Judgement::Trusted(Trust::Slowed) {
                slowed += 1;
            }
            sent += corrections(h.drain()).len();
        }
        assert_eq!(sent, 0, "{case}");
        assert!(slowed > 0, "{case}");
        let [x, y, z] = client.feet;
        let client_feet = DVec3::new(f64::from(x), f64::from(y), f64::from(z));
        assert!(
            client_feet.distance(start) > 1.0,
            "{case}: the client walked"
        );
        assert!(
            copy_feet(&mut h).distance(client_feet) < 1.0e-4,
            "{case}: copy at {}, client at {client_feet}",
            copy_feet(&mut h)
        );
    }
}

/// A client that does not follow a knockback is not taken as slower: the
/// push plays out on the copy and the client is corrected, as BDS corrects
/// a client that applies its knockback late.
#[test]
fn a_knockback_must_be_followed() {
    let mut h = harness(Difficulty::Easy);
    let player = h.player;
    let start = h.feet();
    h.step(start, true);
    h.drain();
    h.world()
        .get_mut::<PlayerMotion>(player)
        .unwrap()
        .pushed(DVec3::new(0.6, 0.0, 0.0));
    let mut sent = 0;
    for _ in 0..12 {
        sent += stand(&mut h, start, vec![Flag::Verticalcollision]);
        assert!(
            matches!(judgement(&mut h), Judgement::Judged(_)),
            "{:?}",
            judgement(&mut h)
        );
    }
    assert!(sent > 0);
}

/// The trusts a client can seek out reach about one tick of movement, and
/// not through blocks the simulation reproduces: next to a ladder, a claim
/// 10 blocks away or inside the wall beside it is judged and corrected, and
/// so is a claim in a crawl space the copy is not in.
#[test]
fn near_trusts_do_not_reach_far() {
    fn ladder(h: &mut Harness) -> DVec3 {
        let id = h
            .world()
            .resource::<crate::server::game::types::BlockRegistryResource>()
            .0
            .default_state_id_by_name("minecraft:ladder")
            .unwrap();
        h.set_block(HOME.floor().as_ivec3() + IVec3::X, id);
        DVec3::new(10.0, 0.0, 0.0)
    }
    fn crawl_space(h: &mut Harness) -> DVec3 {
        let away = DVec3::new(5.0, 0.0, 0.0);
        h.set_block((HOME + away).floor().as_ivec3() + IVec3::Y, *blocks::STONE);
        away
    }
    /// A ladder on a wall: the claim steps into the wall beside it.
    fn into_a_wall(h: &mut Harness) -> DVec3 {
        ladder(h);
        for y in 0..2 {
            h.set_block(
                HOME.floor().as_ivec3() + IVec3::new(0, y, 1),
                *blocks::STONE,
            );
        }
        DVec3::new(0.0, 0.0, 1.0)
    }
    type Setup = fn(&mut Harness) -> DVec3;
    let cases: [(&str, Setup); 3] = [
        ("a ladder", ladder),
        ("a crawl space", crawl_space),
        ("a wall by a ladder", into_a_wall),
    ];
    for (case, setup) in cases {
        let mut h = harness(Difficulty::Easy);
        let start = h.feet();
        h.put(start);
        let away = setup(&mut h);
        let corrections = stand(&mut h, start + away, vec![Flag::Verticalcollision]);
        assert_eq!(corrections, 1, "{case}");
        assert_eq!(
            judgement(&mut h),
            Judgement::Judged(Verdict::Correct),
            "{case}"
        );
        assert!(
            copy_feet(&mut h).distance(start) < 0.1,
            "{case}: copy at {}",
            copy_feet(&mut h)
        );
    }
}

/// Give `h`'s player a riptide trident in hand and still water beside it.
fn riptide_by_water(h: &mut Harness) {
    use crate::entity::components::{HeldSlot, MainInventory};
    use crate::item::enchant::{Enchantment, with_enchantments};
    use crate::item::test_support::stack;
    let player = h.player;
    let trident = with_enchantments(&stack("minecraft:trident", 1), &[(Enchantment::Riptide, 3)]);
    let slot = h.world().get::<HeldSlot>(player).unwrap().0;
    *h.world()
        .get_mut::<MainInventory>(player)
        .unwrap()
        .hotbar_mut(slot)
        .unwrap() = trident;
    h.set_block(HOME.floor().as_ivec3() + IVec3::X, *blocks::WATER);
}

/// A riptide spin started in water is trusted for at most 20 inputs.
#[test]
fn a_riptide_spin_is_trusted_for_twenty_inputs() {
    let mut h = harness(Difficulty::Easy);
    let start = h.feet();
    h.put(start);
    riptide_by_water(&mut h);
    for input in 1..=21 {
        let mut flags = vec![Flag::Verticalcollision];
        if input == 1 {
            flags.push(Flag::Startspinattack);
        }
        assert_eq!(stand(&mut h, start, flags), 0);
        let judged = judgement(&mut h);
        if input <= 20 {
            assert_eq!(judged, Judgement::Trusted(Trust::Riptide), "input {input}");
        } else {
            assert!(
                matches!(judged, Judgement::Judged(_)),
                "input {input}: {judged:?}"
            );
        }
    }
}

/// Inputs the simulation does not reproduce are trusted, each for its
/// reason, and only where it applies: a movement effect, swift sneak
/// sneaking, depth strider next to water, soul speed on soul soil, a
/// riptide trident's spin started in water, a block at eye height (a crawl
/// space), ice underfoot (its friction is unconfirmed).
#[test]
fn situations_the_simulation_does_not_cover_are_trusted() {
    use crate::entity::components::{
        ArmourInventory, EffectType, Effects, HeldSlot, MainInventory,
    };
    use crate::item::enchant::{Enchantment, with_enchantments};
    use crate::item::test_support::stack;

    fn wear(h: &mut Harness, item: &str, enchantment: Enchantment) {
        let player = h.player;
        let stack = with_enchantments(&stack(item, 1), &[(enchantment, 3)]);
        let mut armour = h.world().get_mut::<ArmourInventory>(player).unwrap();
        if item.ends_with("leggings") {
            armour.set_leggings(stack).unwrap();
        } else {
            armour.set_boots(stack).unwrap();
        }
    }
    fn block_named(h: &mut Harness, name: &str) -> u32 {
        h.world()
            .resource::<crate::server::game::types::BlockRegistryResource>()
            .0
            .default_state_id_by_name(name)
            .unwrap()
    }

    type Setup = fn(&mut Harness) -> Vec<Flag>;
    type Case = (&'static str, Setup, Option<Trust>);
    let cases: [Case; 13] = [
        ("nothing", |_| Vec::new(), None),
        (
            "jump boost",
            |h| {
                let player = h.player;
                let mut effects = Effects::new();
                effects.add(EffectType::JUMP_BOOST, 1, 1000);
                h.world().entity_mut(player).insert(effects);
                Vec::new()
            },
            Some(Trust::Effect),
        ),
        (
            "swift sneak sneaking",
            |h| {
                wear(h, "minecraft:iron_leggings", Enchantment::SwiftSneak);
                vec![Flag::Sneaking]
            },
            Some(Trust::Gear),
        ),
        (
            "swift sneak standing",
            |h| {
                wear(h, "minecraft:iron_leggings", Enchantment::SwiftSneak);
                Vec::new()
            },
            None,
        ),
        (
            "depth strider by water",
            |h| {
                wear(h, "minecraft:iron_boots", Enchantment::DepthStrider);
                h.set_block(HOME.floor().as_ivec3() + IVec3::X, *blocks::WATER);
                Vec::new()
            },
            Some(Trust::Gear),
        ),
        (
            "soul speed on soul soil",
            |h| {
                wear(h, "minecraft:iron_boots", Enchantment::SoulSpeed);
                let soil = block_named(h, "minecraft:soul_soil");
                h.set_block(HOME.floor().as_ivec3() - IVec3::Y, soil);
                Vec::new()
            },
            Some(Trust::Gear),
        ),
        (
            "soul speed on stone",
            |h| {
                wear(h, "minecraft:iron_boots", Enchantment::SoulSpeed);
                Vec::new()
            },
            None,
        ),
        (
            "a riptide spin by water",
            |h| {
                riptide_by_water(h);
                vec![Flag::Startspinattack]
            },
            Some(Trust::Riptide),
        ),
        (
            "a riptide spin on land",
            |h| {
                riptide_by_water(h);
                h.set_block(HOME.floor().as_ivec3() + IVec3::X, *blocks::AIR);
                vec![Flag::Startspinattack]
            },
            None,
        ),
        (
            "a spin without riptide",
            |h| {
                let player = h.player;
                let slot = h.world().get::<HeldSlot>(player).unwrap().0;
                *h.world()
                    .get_mut::<MainInventory>(player)
                    .unwrap()
                    .hotbar_mut(slot)
                    .unwrap() = stack("minecraft:trident", 1);
                h.set_block(HOME.floor().as_ivec3() + IVec3::X, *blocks::WATER);
                vec![Flag::Startspinattack]
            },
            None,
        ),
        (
            "a block at eye height",
            |h| {
                h.set_block(HOME.floor().as_ivec3() + IVec3::Y, *blocks::STONE);
                Vec::new()
            },
            Some(Trust::Crawl),
        ),
        (
            "ice underfoot",
            |h| {
                let ice = block_named(h, "minecraft:ice");
                h.set_block(HOME.floor().as_ivec3() - IVec3::Y, ice);
                Vec::new()
            },
            Some(Trust::Block),
        ),
        (
            "an item in use (judged: standing, nothing slows)",
            |h| {
                let player = h.player;
                h.world()
                    .entity_mut(player)
                    .insert(crate::server::game::item_use::ItemUsing {
                        slot: 0,
                        item: 0,
                        since: 0,
                    });
                Vec::new()
            },
            None,
        ),
    ];
    for (case, setup, expected) in cases {
        let mut h = harness(Difficulty::Easy);
        let start = h.feet();
        h.put(start);
        let mut flags = setup(&mut h);
        flags.push(Flag::Verticalcollision);
        let corrections = stand(&mut h, start, flags);
        let judged = judgement(&mut h);
        match expected {
            Some(trust) => assert_eq!(judged, Judgement::Trusted(trust), "{case}"),
            None => assert!(matches!(judged, Judgement::Judged(_)), "{case}: {judged:?}"),
        }
        assert_eq!(corrections, 0, "{case}");
    }
}

/// The copy reads both block layers: the water of a waterlogged block (in
/// the second layer) is water to it, as to the sweep client; flowing water
/// and lava are not supported (a current pushes), still water is.
#[test]
fn waterlogged_blocks_are_water_and_flowing_water_is_trusted() {
    use crate::entity::player_life::authority::{MotionTable, WorldTerrain};
    use crate::server::game::types::BlockRegistryResource;
    use unastar_rules::movement::Terrain;
    let mut h = harness(Difficulty::Easy);
    let registry = &h.world().resource::<BlockRegistryResource>().0;
    let seagrass = registry
        .default_state_id_by_name("minecraft:seagrass")
        .unwrap();
    let water = registry.get_by_name("minecraft:water").unwrap();
    let flowing = (water.min_state_id..=water.max_state_id)
        .find(|&id| {
            registry
                .liquid_state(id)
                .is_some_and(|liquid| liquid.depth != 0)
        })
        .unwrap();
    let lava = registry.default_state_id_by_name("minecraft:lava").unwrap();
    let pos = HOME.floor().as_ivec3() + IVec3::new(3, 0, 0);
    let chunk = h
        .world()
        .resource::<ChunkManager>()
        .get_by_coords(pos.x >> 4, pos.z >> 4)
        .unwrap();
    h.world()
        .get_mut::<ChunkData>(chunk)
        .unwrap()
        .inner
        .set_block_layers(
            (pos.x & 15) as u8,
            pos.y as i16,
            (pos.z & 15) as u8,
            seagrass,
            Some(*blocks::WATER),
        );
    let world = h.world();
    let table = world.resource::<MotionTable>();
    let terrain = WorldTerrain {
        blocks: &*world,
        table,
    };
    assert!(terrain.block(pos.to_array()).water, "waterlogged seagrass");
    assert!(!terrain.block((pos + IVec3::X).to_array()).water, "air");
    assert!(table.reproduces(*blocks::WATER), "still water");
    assert!(!table.reproduces(flowing), "flowing water");
    assert!(!table.reproduces(lava), "lava");
    assert!(table.reproduces(*blocks::STONE), "stone");
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
    let at = reported(start);
    assert_eq!(
        corrections(h.drain())
            .iter()
            .map(|c| (c.pos.x, c.pos.y, c.pos.z))
            .collect::<Vec<_>>(),
        [(at.x as f32, at.y as f32, at.z as f32)]
    );
    assert!(h.feet().distance(start) < 1.0e-4, "at {}", h.feet());
}

/// Inside the threshold BDS judges the height in a way this server does not
/// model yet. Horizontally close, it takes a position 0.3 above and lets its
/// copy fall back to the floor, again and again (never correcting a
/// standing client, correcting a walking one every 16 inputs from 1022);
/// and once the pulls bring x within reach, it takes x but never the
/// height 0.05 above, correcting every 16 inputs. This server takes a
/// height within a tenth with the rest of the position and pulls a farther
/// one: it corrects the 0.3 hover every 16 inputs (earlier than BDS, or
/// where BDS does not), and takes the 0.05 one.
#[test]
fn heights_off_inside_the_threshold_are_judged_unlike_bds() {
    let shift = DVec3::new(4.0, 0.0, 4.0);
    for (phase, first_apart, ours) in [
        ("split_x_up_0_3", 309, &[319, 335, 351][..]),
        (
            "split_walking_x_up_0_3",
            1008,
            &[1018, 1034, 1050, 1066][..],
        ),
        ("split_x_0_3_up_0_05", 434, &[418][..]),
    ] {
        let mut h = arena(shift);
        let replayed = replay(&mut h, phase, shift);
        assert_eq!(
            replayed.apart.first().map(|(tick, ..)| *tick),
            Some(first_apart),
            "{phase}"
        );
        let corrected: Vec<u64> = replayed.corrections.iter().map(|c| c.0).collect();
        assert_eq!(corrected, ours, "{phase}");
    }
}

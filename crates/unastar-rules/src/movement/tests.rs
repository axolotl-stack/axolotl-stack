use super::authority::{Authority, Verdict};
use super::*;

/// A flat floor whose top is at `top`, of one block kind, and water from
/// `water` up (if any).
struct Flat {
    top: f32,
    kind: BlockKind,
    friction: f32,
    water: Option<f32>,
}

impl Flat {
    fn stone(top: f32) -> Self {
        Self {
            top,
            kind: BlockKind::Plain,
            friction: 0.6,
            water: None,
        }
    }
}

impl Terrain for Flat {
    fn collisions(&self, area: &Aabb, out: &mut Vec<Aabb>) {
        let floor = Aabb {
            min: [-1000.0, self.top - 1.0, -1000.0],
            max: [1000.0, self.top, 1000.0],
        };
        if floor.intersects(area) {
            out.push(floor);
        }
    }
    fn loaded(&self, _: &Aabb) -> bool {
        true
    }
    fn block(&self, pos: [i32; 3]) -> Block {
        let water = self.water.is_some_and(|w| {
            let y = pos[1] as f32;
            y + 1.0 > w && y < w + 100.0
        });
        if pos[1] == self.top as i32 - 1 {
            Block {
                friction: self.friction,
                kind: self.kind,
                water,
            }
        } else {
            Block {
                water,
                ..Block::AIR
            }
        }
    }
}

fn standing(top: f32) -> Body {
    let mut body = Body::at([0.5, top, 0.5]);
    body.on_ground = true;
    body
}

fn walk(forward: f32) -> Controls {
    Controls {
        forward,
        ..Default::default()
    }
}

#[test]
fn standing_still_reports_the_ground_velocity() {
    let terrain = Flat::stone(-60.0);
    let mut body = standing(-60.0);
    // The first tick starts at rest, so it does not touch the floor yet.
    step(&mut body, &Controls::default(), &terrain);
    for _ in 0..5 {
        step(&mut body, &Controls::default(), &terrain);
        assert_eq!(body.feet, [0.5, -60.0, 0.5]);
        assert!(body.on_ground && body.vertical_collision);
        assert_eq!(body.velocity, [0.0, -0.0784, 0.0]);
    }
    assert_eq!(body.network_position()[1], -60.0 + 1.620_01);
}

#[test]
fn walking_and_sprinting_reach_their_vanilla_speeds() {
    let terrain = Flat::stone(-60.0);
    for (sprint, expected) in [(false, 0.215_85), (true, 0.280_6)] {
        let mut body = standing(-60.0);
        let controls = Controls {
            sprint,
            ..walk(1.0)
        };
        let mut last = body.feet[2];
        let mut per_tick = 0.0;
        for _ in 0..60 {
            step(&mut body, &controls, &terrain);
            per_tick = body.feet[2] - last;
            last = body.feet[2];
        }
        assert!((per_tick - expected).abs() < 2.0e-4, "{sprint}: {per_tick}");
        assert_eq!(body.feet[0], 0.5, "yaw 0 walks along +z");
    }
}

/// BDS's own copy of a client walking forward from rest, as a second
/// client saw it (the `MovePlayer` relays of the teleport probe's walk,
/// where the client agreed with every one).
#[test]
fn walking_from_rest_moves_like_the_bds_copy() {
    let terrain = Flat::stone(-60.0);
    let mut body = standing(-60.0);
    step(&mut body, &Controls::default(), &terrain);
    let start = body.feet[2];
    let bds = [
        0.098, 0.249_508, 0.430_231, 0.626_906, 0.832_291, 1.042_431, 1.255_167, 1.469_321,
    ];
    for expected in bds {
        step(&mut body, &walk(1.0), &terrain);
        assert!(
            (body.feet[2] - start - expected).abs() < 2.0e-6,
            "{} vs {expected}",
            body.feet[2] - start
        );
    }
}

#[test]
fn a_jump_peaks_at_its_vanilla_height_and_lands_after_twelve_ticks() {
    let terrain = Flat::stone(0.0);
    let mut body = standing(0.0);
    let mut peak = 0.0f32;
    let mut landed = None;
    for tick in 1..=20 {
        let controls = Controls {
            jump: tick == 1,
            ..Default::default()
        };
        let changes = step(&mut body, &controls, &terrain);
        assert_eq!(changes.jumped, tick == 1);
        peak = peak.max(body.feet[1]);
        if tick > 1 && body.on_ground && landed.is_none() {
            landed = Some(tick);
        }
    }
    assert!((peak - 1.2522).abs() < 1.0e-3, "{peak}");
    assert_eq!(landed, Some(12));
    assert_eq!(body.feet[1], 0.0);
}

#[test]
fn falling_follows_gravity_and_drag() {
    let terrain = Flat::stone(-60.0);
    let mut body = Body::at([0.5, -50.0, 0.5]);
    step(&mut body, &Controls::default(), &terrain);
    assert_eq!(body.feet[1], -50.0);
    assert!((body.velocity[1] + 0.0784).abs() < 1e-7);
    step(&mut body, &Controls::default(), &terrain);
    assert!((body.feet[1] + 50.0784).abs() < 1e-5);
    let mut ticks = 2;
    while !body.on_ground {
        step(&mut body, &Controls::default(), &terrain);
        ticks += 1;
    }
    assert_eq!(body.feet[1], -60.0);
    assert_eq!(ticks, 18, "a 10-block fall lands on its 18th tick");
}

#[test]
fn slime_bounces_a_landing_and_sneaking_does_not() {
    for (sneak, bounces) in [(false, true), (true, false)] {
        let terrain = Flat {
            kind: BlockKind::Slime,
            friction: 0.8,
            ..Flat::stone(0.0)
        };
        let mut body = Body::at([0.5, 3.0, 0.5]);
        body.sneaking = sneak;
        let controls = Controls {
            sneak,
            ..Default::default()
        };
        while !body.on_ground {
            step(&mut body, &controls, &terrain);
        }
        assert_eq!(body.velocity[1] > 0.0, bounces, "{:?}", body.velocity);
    }
}

#[test]
fn sneaking_stops_at_an_edge() {
    struct Ledge;
    impl Terrain for Ledge {
        fn collisions(&self, area: &Aabb, out: &mut Vec<Aabb>) {
            let ledge = Aabb {
                min: [-10.0, -1.0, -10.0],
                max: [1.0, 0.0, 10.0],
            };
            if ledge.intersects(area) {
                out.push(ledge);
            }
        }
        fn loaded(&self, _: &Aabb) -> bool {
            true
        }
        fn block(&self, _: [i32; 3]) -> Block {
            Block {
                kind: BlockKind::Plain,
                ..Block::AIR
            }
        }
    }
    let mut body = standing(0.0);
    body.sneaking = true;
    let controls = Controls {
        yaw: -90.0,
        sneak: true,
        ..walk(1.0)
    };
    for _ in 0..80 {
        step(&mut body, &controls, &Ledge);
    }
    assert!(body.on_ground);
    assert!(body.feet[0] < 1.3 && body.feet[0] > 1.0, "{:?}", body.feet);
}

#[test]
fn unloaded_terrain_freezes_the_body() {
    struct Void;
    impl Terrain for Void {
        fn collisions(&self, _: &Aabb, _: &mut Vec<Aabb>) {}
        fn loaded(&self, _: &Aabb) -> bool {
            false
        }
        fn block(&self, _: [i32; 3]) -> Block {
            Block::AIR
        }
    }
    let mut body = Body::at([0.5, 70.0, 0.5]);
    let changes = step(&mut body, &walk(1.0), &Void);
    assert!(changes.frozen);
    assert_eq!(body.feet, [0.5, 70.0, 0.5]);
}

#[test]
fn idle_in_water_sinks_towards_the_water_terminal_speed() {
    let terrain = Flat {
        water: Some(-80.0),
        ..Flat::stone(-100.0)
    };
    let mut body = Body::at([0.5, -70.0, 0.5]);
    let mut last = body.feet[1];
    let mut per_tick = 0.0;
    for _ in 0..80 {
        step(&mut body, &Controls::default(), &terrain);
        per_tick = body.feet[1] - last;
        last = body.feet[1];
    }
    assert!(body.in_water);
    // v' = 0.8 v - 0.005 settles at -0.025.
    assert!((per_tick + 0.025).abs() < 1e-5, "{per_tick}");
}

#[test]
fn a_swimmer_heads_where_it_looks_and_keeps_the_sprint_drag_a_tick() {
    let terrain = Flat {
        water: Some(-80.0),
        ..Flat::stone(-100.0)
    };
    let mut body = Body::at([0.5, -70.0, 0.5]);
    body.in_water = true;
    let swim = Controls {
        sprint: true,
        pitch: 30.0,
        ..walk(1.0)
    };
    let changes = step(&mut body, &swim, &terrain);
    assert!(changes.started_sprinting && changes.started_swimming);
    let mut last = body.feet[1];
    let mut per_tick = 0.0;
    for _ in 0..60 {
        step(&mut body, &swim, &terrain);
        per_tick = body.feet[1] - last;
        last = body.feet[1];
    }
    // Looking 30 degrees down: d' = 0.8 (1 - 0.085) d - 0.0425.
    let steady = -0.0425 / (1.0 - 0.8 * 0.915);
    assert!((per_tick - steady).abs() < 1e-4, "{per_tick} vs {steady}");
    // Letting go: the sprint (and its 0.9 drag) still moves this tick.
    let changes = step(&mut body, &Controls::default(), &terrain);
    assert!(changes.stopped_sprinting && changes.stopped_swimming);
    assert!(!body.sprinting && !body.swimming && body.moved_sprinting);
}

#[test]
fn a_sprint_lasts_while_moving_forward_without_the_key() {
    let terrain = Flat::stone(0.0);
    let mut body = standing(0.0);
    let sprint = Controls {
        sprint: true,
        ..walk(1.0)
    };
    assert!(step(&mut body, &sprint, &terrain).started_sprinting);
    let changes = step(&mut body, &walk(1.0), &terrain);
    assert!(body.sprinting && !changes.stopped_sprinting);
    let changes = step(&mut body, &walk(0.0), &terrain);
    assert!(changes.stopped_sprinting && !body.sprinting);
    assert_eq!(body.movement_speed, WALK_SPEED);
}

#[test]
fn sprinting_needs_forward_input_and_no_sneak() {
    let mut body = standing(0.0);
    let mut changes = Changes::default();
    update_state(
        &mut body,
        &Controls {
            sprint: true,
            ..Default::default()
        },
        &mut changes,
    );
    assert!(!body.sprinting && !changes.started_sprinting);
    update_state(
        &mut body,
        &Controls {
            sprint: true,
            ..walk(1.0)
        },
        &mut changes,
    );
    assert!(body.sprinting && changes.started_sprinting);
    assert!((body.movement_speed - 0.13).abs() < 1e-7);
}

/// A standing copy and a client that reports `claim` every input: what
/// BDS's copy does, input by input (`z` only, ground friction 0.546).
fn judge_standing(claims: &[f32]) -> Vec<(f32, Verdict)> {
    let terrain = Flat::stone(0.0);
    let mut copy = standing(0.0);
    step(&mut copy, &Controls::default(), &terrain);
    let mut authority = Authority::default();
    claims
        .iter()
        .map(|&z| {
            let pull = authority.take_pull();
            for (velocity, pull) in copy.velocity.iter_mut().zip(pull) {
                *velocity += pull;
            }
            let before = copy.feet;
            step(&mut copy, &Controls::default(), &terrain);
            let moved = (copy.feet[0] - before[0]).abs() + (copy.feet[2] - before[2]).abs() > 0.0;
            let verdict = authority.judge(&mut copy, [0.5, 0.0, z], moved, true);
            (copy.feet[2], verdict)
        })
        .collect()
}

/// BDS 1.26.51, a client reporting itself 0.2 blocks ahead of BDS's copy
/// while standing still: the copy is pulled 0.005 a tick (with the ground
/// friction), and the eleventh disagreement is corrected
/// (`offset-z-0.2` of the correction probe).
#[test]
fn a_small_offset_is_pulled_then_corrected_like_bds() {
    let out = judge_standing(&[0.7; 11]);
    let bds = [
        0.5, 0.505, 0.512_73, 0.521_95, 0.531_99, 0.542_46, 0.553_19, 0.564_04, 0.574_97, 0.585_93,
        0.596_92,
    ];
    for (tick, ((z, verdict), bds)) in out.iter().zip(bds).enumerate() {
        assert!((z - bds).abs() < 2.0e-5, "input {tick}: {z} vs {bds}");
        let expected = if tick == 10 {
            Verdict::Correct
        } else {
            Verdict::Pulled
        };
        assert_eq!(*verdict, expected, "input {tick}");
    }
}

/// Within 0.1 on every axis the copy takes the client's position at once
/// (`offset-z-0.09`); 0.11 away it is pulled twice, then taken
/// (`offset-z-0.11` of the acceptance probe).
#[test]
fn a_position_within_a_tenth_is_taken() {
    let out = judge_standing(&[0.59]);
    assert_eq!(out[0], (0.59, Verdict::Accepted));
    let out = judge_standing(&[0.61; 3]);
    assert_eq!(out[0].1, Verdict::Pulled);
    assert!((out[1].0 - 0.505).abs() < 1e-6);
    assert_eq!(out[2], (0.61, Verdict::Accepted));
}

/// Too far is corrected at once, and again every sixth input while the
/// client stays there.
#[test]
fn far_positions_are_corrected_every_sixth_input() {
    let out = judge_standing(&[1.5; 13]);
    let corrected: Vec<usize> = out
        .iter()
        .enumerate()
        .filter(|(_, (_, verdict))| *verdict == Verdict::Correct)
        .map(|(tick, _)| tick)
        .collect();
    assert_eq!(corrected, [0, 6, 12]);
    assert!(out.iter().all(|(z, _)| *z == 0.5), "never pulled that far");
}

/// After a teleport the first input rests at the destination: a client
/// still reporting its old position far away is corrected, a near one is
/// pulled, and the near one's position is not taken.
#[test]
fn the_input_after_a_teleport_rests_and_is_judged_against_the_destination() {
    let mut authority = Authority::default();
    authority.teleported();
    assert!(authority.resting());
    assert_eq!(
        authority.judge_rest([0.5, 0.0, 8.5], [0.5, 0.0, 0.5]),
        Verdict::Correct
    );
    assert!(!authority.resting());

    let mut authority = Authority::default();
    authority.teleported();
    assert_eq!(
        authority.judge_rest([0.5, 0.0, 0.6], [0.5, 0.0, 0.55]),
        Verdict::Pulled
    );
    let mut copy = standing(0.0);
    copy.feet = [0.5, 0.0, 0.6];
    let pull = authority.take_pull();
    assert!((pull[2] + 0.005).abs() < 1e-7);
    copy.feet[2] += pull[2];
    assert_eq!(
        authority.judge(&mut copy, [0.5, 0.0, 0.55], true, true),
        Verdict::Pulled,
        "a teleported client's position is not taken"
    );
}

/// An input the copy did not judge (it took the client's state) leaves
/// nothing to pull and no disagreement counted: ten pulls, then a synced
/// input, and the next disagreement is the first again.
#[test]
fn a_synced_input_clears_the_pull_and_the_count() {
    let mut authority = Authority::default();
    let mut copy = Body::at([0.0, 0.0, 0.5]);
    for _ in 0..10 {
        copy.feet = [0.0, 0.0, 0.5];
        assert_eq!(
            authority.judge(&mut copy, [0.0, 0.0, 0.7], false, true),
            Verdict::Pulled
        );
    }
    authority.synced();
    assert_eq!(authority.take_pull(), [0.0; 3]);
    copy.feet = [0.0, 0.0, 0.5];
    assert_eq!(
        authority.judge(&mut copy, [0.0, 0.0, 0.7], false, true),
        Verdict::Pulled,
        "the count started again"
    );
}

//! The movement corpus (`tools/movement-diff/corpus.json`) through
//! `unastar_rules::movement`, against BDS 1.26.51's own simulation of it
//! (`testdata/bds-1.26.51-movement-diff.json`, extracted by
//! `movement-diff extract` from the run `tools/movement-diff/bds_run.py`
//! made).
//!
//! Every scenario must match BDS bit for bit: the wire position on every
//! tick, ground contact on every tick, and the velocity on the ticks BDS
//! corrected (about one in six). The scenarios unastar does not match yet
//! are listed in [`KNOWN_GAPS`] with the tick they first differ at; they
//! must still differ there, so a fix shows up as a failure until the list
//! is updated.

use bds_sweep::scenario::{
    Corpus, DenseVelocities, DenseVelocity, HarnessCheck, HarnessRuns, TickState, Traces, simulate,
};

/// Scenarios unastar does not simulate like BDS yet: the first tick that
/// differs, and why.
const KNOWN_GAPS: &[(&str, u32, &str)] = &[
    (
        "bed_bounce",
        12,
        "a bed's bounce is not BDS's (0.5324 after the landing, unfitted)",
    ),
    (
        "bed_drop_1",
        6,
        "a bed's bounce is not BDS's (measured from five heights, unfitted)",
    ),
    (
        "bed_drop_2",
        9,
        "a bed's bounce is not BDS's (measured from five heights, unfitted)",
    ),
    (
        "bed_drop_3",
        11,
        "a bed's bounce is not BDS's (measured from five heights, unfitted)",
    ),
    (
        "bed_drop_4",
        12,
        "a bed's bounce is not BDS's (measured from five heights, unfitted)",
    ),
    (
        "bed_drop_7",
        16,
        "a bed's bounce is not BDS's (measured from five heights, unfitted)",
    ),
    ("bubble_column_down", 1, "bubble columns are not simulated"),
    ("bubble_column_up", 1, "bubble columns are not simulated"),
    (
        "elytra_glide_yaw",
        6,
        "a glide at yaw 37.5 is 3 ULP off on its first tick: BDS's sine table entries are a few ULP off the correctly rounded sines",
    ),
    (
        "powder_snow_walk_into",
        18,
        "powder snow's freezing slowdown (a movement modifier growing each tick) is not simulated",
    ),
    (
        "random_14",
        54,
        "4 ULP on x velocity late in a random scenario (soul sand, strafing sprint jumps)",
    ),
    (
        "random_172",
        6,
        "a sprint jump at yaw 60 is 3 ULP off on z: BDS's sine table entry for cos 60 is 3 ULP under the correctly rounded one",
    ),
    (
        "random_186",
        32,
        "a sneak let go under a top slab keeps BDS's copy in the 1.49 box, so a jump rises 0.01; unastar stands it up",
    ),
    (
        "random_291",
        72,
        "a slime bounce an ULP off after the tick's gravity; no f32 order found",
    ),
    (
        "random_66",
        60,
        "2 ULP on x after a strafing sprint jump on ice",
    ),
    (
        "random_a141",
        58,
        "a sneak let go under a ceiling keeps BDS's copy in the 1.49 box; unastar stands it up",
    ),
    (
        "water_climb_out",
        21,
        "jumping out of water onto a ledge: the lift is not fitted",
    ),
    (
        "water_sprint_swim",
        39,
        "a swimmer surfacing: BDS holds its height at the surface for four ticks",
    ),
    ("water_swim_deep", 46, "1 ULP on x in the tick a swim stops"),
];

/// The first tick and field where `ours` differs from BDS's `theirs`.
fn first_difference(ours: &[TickState], theirs: &[TickState]) -> Option<(u32, &'static str)> {
    assert_eq!(ours.len(), theirs.len(), "tick counts differ");
    ours.iter().zip(theirs).find_map(|(ours, theirs)| {
        assert_eq!(ours.tick, theirs.tick);
        let bits = |v: [f32; 3]| v.map(f32::to_bits);
        if bits(ours.position.0) != bits(theirs.position.0) {
            return Some((ours.tick, "position"));
        }
        if let (Some(a), Some(b)) = (ours.velocity, theirs.velocity)
            && bits(a.0) != bits(b.0)
        {
            return Some((ours.tick, "velocity"));
        }
        if let (Some(a), Some(b)) = (ours.on_ground, theirs.on_ground)
            && a != b
        {
            return Some((ours.tick, "on_ground"));
        }
        None
    })
}

/// The committed BDS traces, with BDS's velocity after every input where
/// the shifted runs showed it (`movement-diff velocities`).
fn bds_traces() -> Traces {
    let mut bds: Traces =
        serde_json::from_str(include_str!("../testdata/bds-1.26.51-movement-diff.json")).unwrap();
    let dense: DenseVelocities = serde_json::from_str(include_str!(
        "../testdata/bds-1.26.51-movement-diff-velocities.json"
    ))
    .unwrap();
    for (name, outcome) in dense.scenarios {
        let DenseVelocity::Known { velocities } = outcome else {
            continue;
        };
        let trace = bds.scenarios.get_mut(&name).unwrap();
        assert_eq!(trace.ticks.len(), velocities.len(), "{name}");
        for (state, velocity) in trace.ticks.iter_mut().zip(velocities) {
            if let Some(velocity) = velocity {
                assert!(
                    state
                        .velocity
                        .is_none_or(|v| v.0.map(f32::to_bits) == velocity.0.map(f32::to_bits)),
                    "{name} tick {}: the shifted runs disagree with the committed one",
                    state.tick
                );
                state.velocity = Some(velocity);
            }
        }
    }
    bds
}

#[test]
fn unastar_moves_like_bds_in_every_scenario() {
    let corpus = Corpus::parse(include_str!("../../../tools/movement-diff/corpus.json")).unwrap();
    let bds = bds_traces();
    let mut failures = Vec::new();
    for scenario in &corpus.scenarios {
        let theirs = &bds
            .scenarios
            .get(&scenario.name)
            .unwrap_or_else(|| panic!("no BDS trace of {}", scenario.name))
            .ticks;
        let ours = simulate(scenario).unwrap().ticks;
        let difference = first_difference(&ours, theirs);
        let gap = KNOWN_GAPS
            .iter()
            .find(|(name, ..)| *name == scenario.name)
            .map(|(_, tick, _)| *tick);
        match (difference, gap) {
            (None, None) => {}
            (Some((tick, _)), Some(expected)) if tick == expected => {}
            (Some((tick, field)), _) => failures.push(format!(
                "{}: {field} differs from BDS at tick {tick} (known gap: {gap:?})",
                scenario.name
            )),
            (None, Some(tick)) => failures.push(format!(
                "{}: matches BDS now; remove its known gap (tick {tick})",
                scenario.name
            )),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_scenario_has_a_bds_trace() {
    let corpus = Corpus::parse(include_str!("../../../tools/movement-diff/corpus.json")).unwrap();
    let bds: Traces =
        serde_json::from_str(include_str!("../testdata/bds-1.26.51-movement-diff.json")).unwrap();
    for scenario in &corpus.scenarios {
        let trace = &bds.scenarios[&scenario.name];
        assert_eq!(
            trace.ticks.len() as u32,
            scenario.ticks(),
            "{}",
            scenario.name
        );
    }
    for (name, ..) in KNOWN_GAPS {
        assert!(
            corpus.get(name).is_some(),
            "known gap {name} is not a scenario"
        );
    }
}

/// What a known-answer scenario must read from BDS after each input, worked
/// out here from first principles, not from `unastar_rules::movement`: the
/// wire position (`f32`) and ground contact, and the velocity after it.
/// A known answer after one input: the wire position, ground contact and
/// velocity.
type Known = ([f32; 3], bool, [f32; 3]);

fn known_answer(name: &str, ticks: u32) -> Option<Vec<Known>> {
    const OFFSET: f32 = 1.620_01;
    let (x, mut feet, z) = match name {
        "known_still" => (0.5f32, 100.0f32, 0.5f32),
        "known_free_fall" => (0.5, 119.0, 0.5),
        _ => return None,
    };
    let falls = name == "known_free_fall";
    let mut velocity = 0.0f32;
    let mut out = Vec::new();
    for tick in 1..=ticks {
        // BDS's teleport leaves the body on the ground for the first input:
        // it moves nothing; a standing body is stopped by the floor.
        if falls && tick > 1 {
            feet += velocity;
        }
        velocity = (velocity - 0.08) * 0.98;
        let grounded = !falls || tick == 1;
        out.push(([x, feet + OFFSET, z], grounded, [0.0, velocity, 0.0]));
        if !falls {
            velocity = 0.0;
        }
    }
    Some(out)
}

#[test]
fn bds_reads_the_known_answers_bit_for_bit() {
    let corpus = Corpus::parse(include_str!("../../../tools/movement-diff/corpus.json")).unwrap();
    let bds: Traces =
        serde_json::from_str(include_str!("../testdata/bds-1.26.51-movement-diff.json")).unwrap();
    let mut checked = 0;
    for scenario in &corpus.scenarios {
        let Some(expected) = known_answer(&scenario.name, scenario.ticks()) else {
            continue;
        };
        checked += 1;
        let trace = &bds.scenarios[&scenario.name].ticks;
        for (state, (position, on_ground, velocity)) in trace.iter().zip(expected) {
            let bits = |v: [f32; 3]| v.map(f32::to_bits);
            assert_eq!(
                bits(state.position.0),
                bits(position),
                "{} tick {}: position {:?}, known answer {position:?}",
                scenario.name,
                state.tick,
                state.position.0
            );
            assert_eq!(
                state.on_ground,
                Some(on_ground),
                "{} tick {}",
                scenario.name,
                state.tick
            );
            if let Some(read) = state.velocity {
                assert_eq!(
                    bits(read.0),
                    bits(velocity),
                    "{} tick {}: velocity {:?}, known answer {velocity:?}",
                    scenario.name,
                    state.tick,
                    read.0
                );
            }
        }
    }
    assert_eq!(checked, 2, "the corpus has both known-answer scenarios");
}

/// The other BDS runs of the corpus (`movement-diff check`): a second
/// session, other reported offsets and flags from the keys alone must give
/// the committed traces bit for bit, but in the scenarios each lists with
/// its reason.
#[test]
fn other_bds_runs_give_the_committed_traces() {
    let bds: Traces =
        serde_json::from_str(include_str!("../testdata/bds-1.26.51-movement-diff.json")).unwrap();
    let checks: HarnessRuns = serde_json::from_str(include_str!(
        "../testdata/bds-1.26.51-movement-diff-checks.json"
    ))
    .unwrap();
    for kind in [
        HarnessCheck::Repeat,
        HarnessCheck::Offset,
        HarnessCheck::Flags,
    ] {
        assert!(
            checks.runs.iter().any(|run| run.check == kind),
            "no {kind:?} run is recorded"
        );
    }
    let mut failures = Vec::new();
    for run in &checks.runs {
        assert!(
            run.digests.len() >= 225,
            "{} did not play the whole corpus",
            run.run
        );
        for (name, digest) in &run.digests {
            let trace = bds
                .scenarios
                .get(name)
                .unwrap_or_else(|| panic!("{name} of {} is not in the committed run", run.run));
            let committed = trace.velocities();
            let velocities_agree = run.velocities.get(name).is_some_and(|theirs| {
                theirs.iter().all(|(tick, v)| {
                    committed
                        .get(tick)
                        .is_none_or(|ours| ours.0.map(f32::to_bits) == v.0.map(f32::to_bits))
                })
            });
            let same = *digest == trace.digest() && velocities_agree;
            let listed = run.differs.contains_key(name);
            if same == listed {
                failures.push(format!(
                    "{} ({:?}): {name} {}",
                    run.run,
                    run.check,
                    if same {
                        "matches now; drop it from `differs`"
                    } else {
                        "differs"
                    }
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

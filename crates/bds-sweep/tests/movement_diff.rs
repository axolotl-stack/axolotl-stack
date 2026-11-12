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

use bds_sweep::scenario::{Corpus, TickState, Traces, simulate};

/// Scenarios unastar does not simulate like BDS yet: the first tick that
/// differs, and why.
const KNOWN_GAPS: &[(&str, u32, &str)] = &[
    (
        "elytra_glide",
        6,
        "the glide differs from its first tick (bedsim too); not fitted yet",
    ),
    ("lava_walk", 6, "lava is not simulated"),
    (
        "levitation_float",
        1,
        "BDS reports a levitating player off the ground on the first input; positions and velocities match",
    ),
    ("scaffolding_climb", 6, "scaffolding is not simulated"),
    (
        "water_sprint_swim",
        6,
        "a swimmer's pitch steering differs from the first input (bedsim matches BDS to tick 10)",
    ),
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

#[test]
fn unastar_moves_like_bds_in_every_scenario() {
    let corpus = Corpus::parse(include_str!("../../../tools/movement-diff/corpus.json")).unwrap();
    let bds: Traces =
        serde_json::from_str(include_str!("../testdata/bds-1.26.51-movement-diff.json")).unwrap();
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

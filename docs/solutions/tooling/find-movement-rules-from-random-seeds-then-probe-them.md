---
title: Find movement rules from where random seeds first diverge, then probe them; re-measure after every rule
area: tooling
tags: [movement, movement-diff, bds-sweep, random, mutation, rounding, flags]
date: 2026-11-12
origin: movement round 6 (OpenSpec change movement-input-rules)
status: current
---

## Context

Round 5 left unastar bit-exact on 135 of 147 corpus scenarios but on only 10 of 102 random seeds.
Round 6 took the seeds to 207 of 230 (two batches, 90 %) with seven rules, each found and confirmed in
the same way.

## What we learned

- **Classify the first difference by the key change before it.** For each failing seed, print
  the tick it first differs at and the keys of the segment before and after it
  (`J.R f1 s0.3 -> ... f0 s1`). 49 of 92 first differed on the input that let go of a sneak.
  The next large groups were a sprint whose forward input dropped, and strafing sprint jumps.
  The table points at the rule; one scenario's numbers then fit it.
- **The subject's flags come from our own simulation, so an old run goes stale when a rule
  changes.** `bds-sweep-client` derives `StartSprinting`, `StopSprinting` and the rest from
  unastar's simulation, as a vanilla client derives them from its prediction. In round 5's
  random run, unastar kept a sprint going at half forward and so never sent `StopSprinting`.
  BDS then walked at half forward, and sprinted again at full forward without the key. Replaying
  old seeds after a fix is fine for proposing a rule. To score it, play the seeds on BDS again
  with the new simulation. With the same rules, round 6's replay of round 5's run matched 81 of
  102 valid seeds; a fresh BDS run of the same seeds matched 86 of 101.
- **Probe each rule before trusting it.** Small scenarios that change one thing at a time all
  matched on the first BDS run once the rule was right: 31 for the sneak release and sprint
  stops, 20 for the double tap (gaps of 1 to 9 inputs pin the 7-input window), and 27 for
  sprinting into walls (yaws 5 to 75 pin the 45-degree turn). A probe that fails points at what
  the rule misses: `sr_toggle` found the double tap, `sr_air` its ground-only condition, and
  `sw_ground_5` that only a wall already touched ends a sprint.
- **Find rounding by enumerating ULPs offline.** Emulate the few ticks before a corrected
  velocity in numpy `f32` and reproduce unastar's bits first. Then move one constant or
  intermediate at a time by ±k ULPs until BDS's bits come out. The air acceleration of a
  sprinter is 0.025999999, not 0.026, and the strafe normalisation and the friction product were
  never the cause. When no `f32` intermediate gives BDS's bits, the structure is wrong. Do not
  switch to `f64`: a slime bounce fitted three seeds in `f64` and broke 30 others.
- **Search for mutation catchers offline, against what BDS shows.** Build each mutation (as
  `mutate.py` does) and simulate candidate scenarios with and without it. Keep the scenarios that
  differ in what a BDS trace shows: the position on every tick, ground contact, and the velocity
  on every sixth tick. Wire positions near the origin have the smallest ULPs, so a cobweb or
  powder-snow factor shows when the move ends near x = 0. One ULP of 0.085 (swim steering) or of
  0.01 (slow falling) never moves a product past an `f32` rounding step at the speeds they act on.
  No scenario can catch such a mutation; use the smallest change that shows.
- **Validate the knockback too.** A `/damage` command once ran an input early: BDS set the
  velocity after input 11 instead of 12, and every later position differed. The setup, input and
  relay checks all passed. `movement-diff extract` now compares the velocities BDS set with the
  corpus's events.

## Where it lives

- `crates/unastar-rules/src/movement/mod.rs` (`update_state`, `wall_stops_sprint`,
  `honey_slide`, `AIR_SPEED_SPRINTING`).
- `tools/movement-diff/corpus.json`: the `sr_*`, `ss_*`, `dt_*` and `sw_*` probes, the
  `random_a*` and `random_2xx` seeds, and the mutation catchers.
- `crates/bds-sweep/src/movement_diff.rs` (`extract`, the events check);
  `tools/movement-diff/mutate.py`.
- [Decision 0067](../../decisions/0067-find-movement-rules-from-random-seeds-and-probes.md).

---
status: proposed
date: 2026-11-12
decision-makers: [project owner]
---

# Find movement rules from random seeds and targeted probes, and lay the fixtures out a tick a line

## Context and Problem Statement

After round 5 the shared movement simulation matched BDS 1.26.51 bit for bit in 135 of 147
hand-written scenarios but in only 10 of 102 random ones
([0060](0060-validate-the-movement-harness-against-bds-itself.md)). The hand-written corpus
tests what its authors thought of; random key sequences found that a sneak release, a sprint's
forward threshold, double taps, walls and honey's side all moved the player unlike unastar. We
needed a way to find such rules, to score them, and to keep the corpus that confirms them
without the fixtures outgrowing review.

## Decision Drivers

- Copy BDS exactly ([0013](0013-copy-bds-behaviour-exactly.md)); black-box only.
- The subject derives its input flags from unastar's simulation
  ([0054](0054-measure-bds-movement-through-a-far-reporting-client.md)), so a run measures BDS's
  answer to the flags our simulation implies.
- Every rule must be confirmed by scenarios that keep failing if it regresses.

## Considered Options

1. **Classify where random seeds first diverge, fit the rule on one seed, confirm it with
   targeted probes, and score it with a fresh BDS run of the seeds.**
2. Replay the committed random run after each fix and count matches.
3. Write more hand-made scenarios by guess.

For the fixtures: (a) one tick a line in plain JSON; (b) delta-encoded positions; (c) binary
traces with a JSON index.

## Decision Outcome

Chosen: option 1, and fixture layout (a).

- **Scoring needs a fresh run.** After a rule change the subject would send other flags, and
  BDS answers flags as well as keys (in round 5's run, a sprint unastar never stopped went on
  through a half-forward stretch). Replays only propose rules.
- **Each rule gets probes.** `sr_*` (sneak release), `ss_*` (sprint stops), `dt_*` (double
  tap) and `sw_*` (walls) vary one thing each, and the seeds that showed the rule stay as
  regression scenarios (`random_a*` from the first batch, `random_2xx` from the second).
- **Mutations are caught by what BDS shows.** Catchers are searched offline: the mutated
  simulation must differ from the plain one in positions, ground contact or the velocity on a
  corrected input. A mutation smaller than any observation (one ULP of the swim rate or of slow
  falling's gravity) is replaced by the smallest that shows.
- **Fixtures, one tick a line.** `bds_sweep::json` lays out the outer levels and writes each
  tick as compact JSON on one line. The files stay readable and diffable at about a third of
  their size, and need no new reader. Delta or binary encodings would save more, but neither can
  be read in a diff.

### Consequences

- Good: seven rules found in one round. Random seeds went from 10 of 102 to 207 of 230 (two
  batches, played on BDS with the final simulation), and every rule has probes.
- Good: the mutation check covers the new rules and catches 38 of 41 mutations; the three it misses act only where the simulation is not BDS's yet (beds, powder snow's freeze) or below the wire's precision.
- Bad: each scored change costs a BDS run of the seeds (about 15 minutes for 150).
- Bad: the corpus grew to 226 scenarios, and a full run with its validation reruns takes about
  an hour and a half.

### Confirmation

`crates/bds-sweep/tests/movement_diff.rs` (`unastar_moves_like_bds_in_every_scenario` with its
known gaps); `tools/movement-diff/mutations.json`; `tools/movement-diff/report.md`;
`docs/solutions/tooling/find-movement-rules-from-random-seeds-then-probe-them.md`.

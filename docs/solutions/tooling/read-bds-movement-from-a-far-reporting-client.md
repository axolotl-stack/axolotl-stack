---
title: Read BDS's own movement from a client that reports a far position; a zero-correction run hides ULP errors
area: tooling
tags: [movement, bds-sweep, movement-diff, bedsim, MovePlayer, correction, f32, fixture]
date: 2026-11-12
origin: commits "feat(movement-diff): a three-way movement harness" and the unastar-rules movement fixes on top of it
status: current
---

## Context

`unastar_rules::movement` ran on BDS 1.26.51 in the sweep client without a correction, so it was
believed to match. It was not: walking on stone accelerated by 0.09999999 instead of 0.1, and a
diagonal walk was 2 % too fast. BDS takes any reported position within 0.1 of its copy, so a
client that reports its own simulation never sees errors that small. The movement-diff harness
(`tools/movement-diff/`) reads BDS's own simulation instead and compares three implementations
bit for bit.

## What we learned

- **Report a position 3 blocks off to see BDS simulate.** Past the acceptance threshold, BDS
  never takes or pulls towards the reported position. It corrects at most every sixth input,
  and its copy moves by its simulation alone. A viewer client gets the copy after every input in
  the `MovePlayer` relay (exact `f32`, stamped with the input). The corrections add the velocity
  on one input in six. Check each correction against the relay of the same input: in the first
  run all 470 matched, which validates the method on every run.
- **Script the setup from the subject with `CommandRequest`.** Commands then land between known
  inputs. A command sent after input N runs before BDS simulates N, and its `SetActorMotion` is
  stamped N-1. The input after a `/tp` rests. Inputs already in flight when the teleport arrives
  go out idle, so every scenario starts with 3 idle inputs.
- **BDS's `/tp` leaves the copy on the ground**, even in the air, until the first move.
- **Compare in wire space.** BDS sends the feet plus 1.62001; adding that in `f32` on our side
  compares the feet bit for bit too, except where two feet values round to the same `y`.
- **Fit the formula offline in numpy `f32`.** Brute-forcing operation orders and neighbouring
  ULPs against one BDS value found each fix in minutes:
  - the acceleration is `((speed × r) × r) × r` with `r = 0.546 / friction`;
  - the strafe and forward impulses are scaled before the sine and cosine;
  - BDS uses the exact sine and cosine for the input, the table only for the sprint jump.

  `tools/movement-diff/compare.py --scenario NAME --ticks` gives the first differing tick and
  its ULP distance.
- **Keep the collision box.** The feet are the centre of a box translated each move. Rebuilding
  the box from the rounded feet moves it by an ULP, and a walk drifts after two ticks.
- **bedsim is a good lead, not ground truth.** It matched BDS where unastar failed on ladders,
  vines, soul sand, honey, slime walking, slow falling and the acceleration. It is wrong on:
  - the yaw trigonometry (`math32` sine), off by an ULP;
  - the cross-term order of a turning walk;
  - a signed zero;
  - scaffolding and bubble columns (it pushes the player out of a one-block column at once);
  - water from tick 11 of a swim.

  Read its MIT code for the rule, write it in your own words, and re-verify against BDS.
- **Effect speeds are not per input.** With speed or slowness, the first inputs of a sprint
  accelerate irregularly on BDS. The attribute changes on BDS's tick. Neither simulation can
  copy that per input.
- **Shared-machine traps.** Other lanes' cargo runs hold the target lock, so a "fast" rebuild
  may still be waiting and a report silently uses the old binary. Write command output to a
  file and check the rebuild finished before trusting a report.

## Where it lives

- `tools/movement-diff/`: the corpus, `bds_run.py`, the comparator, `bedsim-runner/` and the
  README with the precision limits.
- `crates/bds-sweep/src/scenario.rs` and `src/movement_diff.rs`: the corpus types, the unastar
  runner and the extraction.
- `crates/bds-sweep/tests/movement_diff.rs`: the replay test and its known gaps.
- `crates/bds-sweep/testdata/bds-1.26.51-movement-diff.json`: the BDS traces.
- Decision [0054](../../decisions/0054-measure-bds-movement-through-a-far-reporting-client.md).

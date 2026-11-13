---
title: Check that BDS played the scenario you wrote; a clean-looking movement run can measure the wrong world
area: tooling
tags: [movement, movement-diff, bds-sweep, validation, corpus, fill, bubble-column, fixture]
date: 2026-11-12
origin: change movement-harness-validation (movement round 5)
status: current
---

## Context

The movement differential compares unastar with BDS scenario by scenario. Round 5 added a check
that reads the world BDS sent the subject client back from its capture and compares it with the
corpus at each scenario's first input. The first time it ran it failed the committed run.

## What we learned

- **`/fill` does not always place what it says.**
  - A filled bubble column with nothing under it became air: `bubble_column_up` had matched
    "BDS" for a round while testing nothing.
  - Rows of fences, panes, bars and walls get their connections from their neighbours.
  - Scaffolding gets `stability_check`, and a bed's halves land somewhere else than filled.
  - A `/tp` to a whole coordinate lands on the block's centre.
  - Spell out the states BDS computes, and declare the blocks it forms (`formed` in the corpus).
- **Scenarios contaminate each other.** A bubble column's water stays in the second block layer
  after `/fill … air`, then flows. Every scenario after it was played in water at one spot. Fill
  stone before air, and play such scenarios last.
- **Fill order matters for liquids.** Water filled before its walls flows out during the
  following setup inputs, and only sometimes (the run that leaked and the one that did not used
  the same corpus). Place liquids last.
- **Plants grow during a run.** Twisting vines grew a block in the middle of a scenario. Cap
  them.
- **Repeat runs differ in where BDS corrects.** Positions repeat bit for bit, but one session
  corrects inputs 6, 12, 18…, another 1, 7, 13…. Compare velocities only where both runs have
  one. For the same reason, a knockback command can land an input later; take the knockback's
  timing and velocity from the run itself.
- **Fit offline in numpy `f32`, scenario by scenario.**
  - The glide matched only once the sine table's angles were computed in `f32`.
  - The swim matched only with the look vector's y.
  - The water box needed 0.401, not 0.4.
  Each came from one corrected velocity.

## Where it lives

- `crates/bds-sweep/src/validation.rs`: the setup, input and relay checks.
- `movement-diff check`, `world`, `shift`, `velocities` and `random`.
- `tools/movement-diff/mutate.py`.
- `tools/movement-diff/README.md`, "Harness validation".
- [Decision 0060](../../decisions/0060-validate-the-movement-harness-against-bds-itself.md).

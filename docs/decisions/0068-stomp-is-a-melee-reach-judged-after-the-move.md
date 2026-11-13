---
status: proposed
date: 2026-11-12
decision-makers: [project owner (delegated)]
---

# The stomp attack is a reach of the one melee goal, judged after the move

## Context and Problem Statement

The polar bear's `minecraft:behavior.stomp_attack` chases its target like
`behavior.melee_box_attack`, but hits by a distance built from its width, rears up
(`STANDING`) before each hit and restarts its cooldown when the cooldown runs out with the
target close but out of reach. `sweep.py polar-bear` measured six hostile bears tick by tick.
Their rear-ups, stand-downs and hits line up only when the bear's position is read after
that tick's move and the target's before it. The melee box goal decides inside the AI tick,
before the physics step. The question was where the stomp's rules live.

## Decision Drivers

- Copy BDS exactly (decision 0013).
- The code-shape rules: one canonical type per concept, data before code.

## Considered Options

1. **A separate stomp goal type** with its own chase code.
2. **One melee goal with a reach enum:** `MeleeAttackGoal` (formerly `MeleeBoxAttackGoal`)
   carries `MeleeReach::Box` or `MeleeReach::Stomp`. The chase, re-pathing and look are
   shared. The stomp reach's cooldown is a `StompClock` that a system steps after the physics
   step.
3. **One melee goal, the stomp judged inside the AI tick** from the position before the
   move.

## Decision Outcome

Chosen option: 2. Option 1 would copy the chase and re-path rules that BDS shares between
both goals. Option 3 put one of the five clean runs a tick off at every stand-down. The reach
formulas come from the authored multipliers: `(width × reach_multiplier)² + target width`,
and that times `no_damage_range_multiplier`.

### Consequences

- Good, because a behaviour pack mob with `stomp_attack` gets the measured rules with no
  per-mob code.
- Good, because five of the six measured runs replay exactly.
- Bad, because the stomp hit is dealt after the physics step, not in the AI's deferred
  work. Two places deal melee hits, and a stomp's knockback moves its target on the next
  tick.
- Bad, because only player targets were measured. A mob target is read after its own move.
- Bad, because when the cooldown first starts after the bear turns hostile is not modelled.
  BDS started it nine ticks after the hurt.

### Confirmation

`crates/unastar/src/entity/mob/tests/polar_bear.rs`: `the_stomp_clock_rears_and_hits_on_bds_ticks`
and `a_hostile_polar_bear_rears_up_then_stomps_its_target`, against
`mob/ai/testdata/bds-1.26.51-polar_bear.json`.

## More Information

- Copied quirks: the Entities / polar bear rows of `docs/quirks/bds.md`.
- Numbered 0068 to avoid collisions with records on open branches.

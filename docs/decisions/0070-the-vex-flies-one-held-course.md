---
status: proposed
date: 2026-11-13
decision-makers: [project owner (delegated)]
---

# The vex flies one held course, steered before its goals

## Context and Problem Statement

The vex authors `minecraft:navigation.walk` and `minecraft:movement.basic`, yet it flies
through walls with no gravity, and it wanders although its definition authors no wander goal.
`sweep.py vex` measured BDS 1.26.51's vexes tick by tick. Every push was 0.0125 (wandering)
or 0.05 (charging) along the unit vector from the feet to one point. The point was held until
the feet came within the box size; on that tick the vex kept half its velocity. A charge's
first push came two samples after `CHARGING` went up, and a wander could begin on the sample
after an arrival. The question was where this flight lives next to the bat's float control
(decision 0069).

## Decision Drivers

- Copy BDS exactly (decision 0013).
- The code-shape rules: one canonical type per concept, data before code.
- Reuse of `mob::ai::fly`.

## Considered Options

1. **Treat the vex as a float flyer:** give it `FloatNavigation` and a wander goal that sets a
   point every tick, as the bat's does.
2. **One held course in the shared flight state, steered by a native vex class:** `FlyState`
   gains a `Course` (point and push). `FlyState::steer` pushes until arrival and slows on it.
   It runs before the goals in the AI tick; the native wander and `behavior.charge_attack` set
   the course.
3. **A vex-only goal** that moves the vex itself.

## Decision Outcome

Chosen option: 2. Option 1 cannot hold a point across ticks or slow on arrival, and the vex
authors no float navigation. Option 3 would duplicate the flight state the bat already has.
What the definition does not describe is native (`NativeTraits::vex`, `VexClass`): the
flight, the 0.05 push scale, the wander (1 in 7, ±7 / ±5 / ±7 blocks, speed 0.25) and the
arrival slowdown. The charge is an authored goal (`ChargeAttackGoal`). Its hit is judged
after the physics step (`tick_charge_hits`), as the stomp's is (decision 0068).

### Consequences

- Good, because wanders and charges replay BDS's flight within a thousandth of a block every
  tick, and wait lengths match by distribution.
- Good, because `damage_over_time` now works for any mob.
- Bad, because `behavior.charge_attack` compiles for any mob, but only the vex class flies
  the charge. On another mob the goal never starts. No other vanilla mob authors it.
- Bad, because the steering runs before the goals for the vex, and after them for every other
  mob. The order is measured for the vex only.
- Bad, because a mob target's aim point (0.1 below its eyes) is extrapolated from players.

### Confirmation

`crates/unastar/src/entity/mob/tests/vex.rs`: `a_wandering_vex_flies_its_course_like_bds`,
`vex_wanders_match_bds_by_distribution`, `a_charging_vex_flies_at_its_target_like_bds`,
`a_vex_charges_and_hits_a_survival_player`, against `mob/ai/testdata/bds-1.26.51-vex.json`.

## More Information

- Copied quirks: the Entities / vex rows of `docs/quirks/bds.md`.
- Measurements: `docs/research/tadpole-and-vex-bds-1.26.51.md`.
- Numbered 0070 to avoid collisions with records on open branches.

---
status: proposed
date: 2026-11-12
decision-makers: [project owner (delegated)]
---

# Float-navigating flyers push straight at their point, with no path and no gravity

## Context and Problem Statement

The bat (`minecraft:navigation.float`, `minecraft:behavior.float_wander`, `minecraft:can_fly`)
is the first flying mob. Every other mob moves by goals that hand waypoints to the path
follower and the land or swim move control, with gravity. `sweep.py bat` measured six BDS
1.26.51 bats tick by tick. On every tick it pushed, the bat's velocity was
`(0.91, 0.546, 0.91) × v + 0.1 × û`. Here û was the unit vector from its feet before the move
to a fixed point, to within a thousandth of a degree. Points beyond glass walls and below the
floor were flown at. `sweep.py ghast` showed a second float flyer keeping 0.91 a tick on
every axis. The bat also rests hanging under blocks, which its definition does not author.
The question was where flight lives, so that other flyers can reuse it.

## Decision Drivers

- Copy BDS exactly (decision 0013).
- The code-shape rules: one canonical type per concept, data before code.
- Reuse: the ghast and the happy ghast author the same navigation and goal.

## Considered Options

1. **A 3D path search for flyers:** the path follower and a fly move control over air cells.
   This is the shape the `can_fly` description suggests (the prior art,
   `Jviguy/stab-ent-m5` `503bf672`, only compiled the components).
2. **A float control:** `navigation.float` marks the mob (`FloatNavigation`). The AI tick takes
   a float branch, as hopping mobs take a hop branch (decision 0062). Goals write the point,
   and the control adds `movement × û` to the velocity. `mob::derive` removes gravity and sets
   the vertical drag to 0.91 times a native class factor (the bat's 0.6).
3. **The bat as a native class:** its whole flight in bat-only code.

## Decision Outcome

Chosen option: 2, a float control (`mob::ai::fly`). Option 1 would route around walls BDS's
bat bumps into, and it would spend path searches that BDS does not make. Option 3 would have
to be copied for the ghast, whose drag and push follow the same rule with its own movement
speed. What the definitions do not describe is native: the bat's 0.6 vertical factor
(`NativeTraits::flight_vertical_factor`) and its resting (`NativeTraits::bat`,
`mob::ai::roost`).

### Consequences

- Good, because the bat's pushes replay tick by tick, and the wander lengths and idle draws
  match BDS by distribution.
- Good, because the ghast and the happy ghast only need their own goal options measured.
- Bad, because the AI tick has a third movement branch. Travel is not shared with the
  other two branches: a flyer has no jump and no ground acceleration.
- Bad, because `behavior.float`'s jump is ignored for a float-navigating mob. A bat in
  water is not measured.

### Confirmation

`crates/unastar/src/entity/mob/tests/bat.rs`: `a_flying_bat_pushes_at_its_point_like_bds`,
`bat_wanders_match_bds_by_distribution` and `bat_settling_matches_bds_by_distribution`,
against `mob/ai/testdata/bds-1.26.51-bat.json`.

## More Information

- Copied quirks: the Entities / bat rows of `docs/quirks/bds.md`.
- Numbered 0069 to avoid collisions with records on open branches.

# Proposal: the vex's flight, charge and decay

## Why

Entities round 10 takes on the tadpole, the vex and the goat. A full blocker dump gave the
vex only `minecraft:behavior.charge_attack`, `minecraft:behavior.look_at_entity` and
`minecraft:damage_over_time`. The round 9 sweeps had measured its speeds and its 9-damage
hits, but its decay showed nothing. The tadpole's bobbing and the goat are measured here and
left for later changes: the tadpole's pitch is not understood, and the goat has four
blockers and no horn measurement.

Prior work: decision 0069 (float flyers) and `mob::ai::fly`; decision 0068 (a hit judged
after the move); `docs/solutions/server/subtract-the-drag-to-find-what-a-steering-mob-aims-at.md`.
Prior art: neither PR #71 nor the `Jviguy/*` branches implement the vex.

## What Changes

- The vex class (`NativeTraits::vex`) flies whatever its walk navigation says. It has no
  gravity and 0.91 drag on every axis. Its move control (`FlyState::steer`) holds one point,
  pushes `0.05 × speed × movement` a tick at it, and keeps half its velocity on the tick its
  feet come within its box size. Its body faces its target, or else its motion, at most
  `max_turn` a tick. The control steers before the goals think.
- Its native wander: with no point, a 1 in 7 draw a tick picks a block centre within ±7 / ±5
  / ±7 blocks (not a solid block) at speed 0.25.
- `minecraft:behavior.charge_attack` (`ChargeAttackGoal`): starts with `success_rate` while
  the vex has no point and the target is farther than `min_distance`. It flies at 0.1 below
  the target's eyes and raises `CHARGING`. A post-physics system (`tick_charge_hits`) hits
  when the boxes touch after the move. A mob without a melee goal can now deal melee hits.
- `minecraft:damage_over_time` (`DamageOverTime`): hurts by `damage_per_hurt` with no cause
  every `time_between_hurt` seconds, the first one interval after the component arrives.
- `minecraft:behavior.look_at_entity` (`LookAtEntityGoal`): the look-at-player rules over
  nearby mobs. Filters are rejected.
- `minecraft:physics` with `has_collision: false` moves the body through blocks
  (`PhysicsBody::collides`).
- `sweep.py vex` gains `look` and a `decay` that keeps the vex in view. `sweep.py goat` is new.
  `mob_fixture.py --events-only` keeps an item without its per-tick rows. The committed fixture
  is `mob/ai/testdata/bds-1.26.51-vex.json`.

Out of scope: the vex's head and pitch, the `MOVING` flag, evoker summoning (the evoker
cannot spawn), the tadpole (its pitch is not fitted), and the goat (ram, jump, horn,
genetics).

## Capabilities

- `mob-runtime` (modified): the vex flies through one held course.

## Impact

`crates/unastar` (components, the AI tick, physics, native traits, combat), `tools/bds-sweep`,
`docs/parity`, `docs/quirks`, `docs/decisions` (0070), `docs/research`.

### Parity rows

- entities: `minecraft:vex` to `verified`. Its `behavior.charge_attack` and `damage_over_time`
  to `verified`; its `attack`, `collision_box`, `equipment`, `movement`, `movement.basic`,
  `physics` and `timer` to `done`; `behavior.look_at_entity` and `navigation.walk` to
  `partial`.
- entities: the axolotl's and the dolphin's `damage_over_time` to `partial`. The evoker's and
  the ravager's `behavior.look_at_entity` stay `missing`, as their filters are rejected.
- entities: the tadpole and the goat stay `missing`, with notes pointing at the research.

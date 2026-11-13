# Proposal: the bat's flight and resting

## Why

Entities round 9 starts with flight. A full blocker dump (every component of every group)
showed that the bat lacks only `minecraft:behavior.float_wander`, `minecraft:can_fly` and
`minecraft:navigation.float`. The ghast and the happy ghast author the same navigation and
goal, so the flight is built once for every float-navigating flyer.

Prior work: decision 0062 (hop control) is the model for a movement branch outside the path
follower, and decision 0065 (swim navigation) is the model for a body without gravity while
it moves. `docs/solutions/server/a-sinking-mob-cannot-tell-gravity-before-drag-from-after.md`
covers drag order. Nothing covers flight. Prior art: `Jviguy/stab-ent-m5` (`503bf672`) only
compiles the flyer components and has no AI.

## What Changes

- `minecraft:navigation.float` (`FloatNavigation`): the float control. It pushes the mob at
  the point its goal wants by its movement speed, with no path search. The body has no
  gravity and 0.91 air drag on every axis.
- `minecraft:behavior.float_wander` (`FloatWanderGoal`): wanders of `float_duration` × 20
  ticks cut to whole ticks, plus one. Each wander draws points in the authored box until one
  is not in a solid block. Options no capture shows are rejected.
- `minecraft:can_fly`: the `CANFLY` actor flag.
- The bat class (`NativeTraits`): a 0.6 vertical air drag factor, and resting. It is born
  resting, settles under a solid block with a 1 in 100 chance per tick, and wakes when its
  roof goes, when a player's eyes come within 4 blocks of its feet, or when it is hurt. Its
  `FALL_DAMAGE_MULTIPLIER` starts at 0.
- `sweep.py bat` (`cave`, `wake`, `low`, `near`, `unroof`, `canopy`), `ghast`, `vex` and
  `tadpole --items grow`. `mob_fixture.py` reads `AddActor` flags from anywhere in the
  capture. The committed bat fixture is `mob/ai/testdata/bds-1.26.51-bat.json`.

Out of scope: the bat's natural spawning replay, its sounds, its head turns while resting, a
bat in water, the ghast's and the happy ghast's goal options (`must_reach`,
`navigate_around_surface`), and the tadpole, vex and goat (later changes).

## Capabilities

- `mob-runtime` (modified): a float-navigating mob moves only through its float control.

## Impact

`crates/unastar` (components, AI tick, derived physics, native traits, the hurt path),
`tools/bds-sweep`, `docs/parity`, `docs/quirks`, `docs/decisions` (0069).

### Parity rows

- entities: `minecraft:bat` to `verified`. Its `behavior.float_wander`, `navigation.float`,
  `can_fly`, `physics` and `movement` to `verified`, `collision_box` and `health` to `done`,
  and the rest to `partial`.
- entities: `can_fly`, `navigation.float` and `behavior.float_wander` of the allay, bee,
  ghast, happy ghast, parrot and wither stay `partial`, with gaps naming what they still lack.

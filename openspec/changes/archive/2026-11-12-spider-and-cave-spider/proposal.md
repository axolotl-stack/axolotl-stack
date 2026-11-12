# Proposal: the spider and the cave spider

## Why

Entities round 6a targets the spider, the cave spider and the enderman. Neither spider could
spawn: `navigation.climb`, `behavior.leap_at_target`, `addrider`,
`block_movement_slowdown_immunity` and `renders_when_invisible` had no runtime kind, and their
events test `is_brightness`, `is_daytime` and `is_snow_covered`, which the mob filters could
not answer. The enderman was measured in the same round (`sweep.py enderman`) and lands in a
later change.

Prior work: PR #71 (`fb1291c3`: wall climbing as a destination block plus a direct sweep, a
leap goal, day and night sensors and poison), used as a feature checklist and checked against
new measurements, not copied; its constants were not black-box measured.

## What Changes

- Wall climbing: a `WallClimbing` state written by the physics step, the `WALLCLIMBING`
  flag, and a direct approach for a climber without a walk path (decision 0061, proposed).
- Collision counts an overlap below 1e-7 as touching, so a box pressed against a wall stays
  out of it.
- `behavior.leap_at_target` (the ocelot's `target_dist` is rejected).
- `renders_when_invisible` sets its actor flag; `addrider` and
  `block_movement_slowdown_immunity` are accepted without behaviour.
- Filters `is_brightness`, `is_daytime` and `is_snow_covered` (the enderman's
  `in_contact_with_water` and `is_riding` wait for its change).
- `sweep.py spider` and `sweep.py enderman`, and the shared distiller `mob_fixture.py`.

Out of scope: riding (spider jockeys), the enderman's components, cobweb slowdown.

## Capabilities

- `mob-runtime` (modified): wall climbers and touching boxes.

## Impact

`crates/unastar` (physics, collision, mob components, AI, filters), `crates/unastar-data`
(the `navigation.climb` body), `tools/bds-sweep`, `docs/parity`, `docs/quirks`,
`docs/research`, `docs/decisions`.

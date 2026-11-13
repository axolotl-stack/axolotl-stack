# Proposal: the polar bear's stomp and water drag

## Why

Entities round 8 targets the cheapest unspawnable mobs. A fresh blocker dump (every component
of every group, not only the first failure) showed that the polar bear lacked only
`minecraft:water_movement` and `minecraft:behavior.stomp_attack`. The parity gap text also
listed `follow_range`, `freezing_immune` and `on_friendly_anger`, which other rounds had
already implemented.

Prior art: `Jviguy/stab-ent-m5` (`503bf672`) and PR #71 do not touch the polar bear. The
flyer component compile in `503bf672` is noted for the bat, which this change does not
cover.

## What Changes

- `minecraft:water_movement`: `drag_factor` is the horizontal drag of a living body in water.
- `minecraft:behavior.stomp_attack`: a stomp reach on the one melee goal
  (`MeleeAttackGoal`, formerly `MeleeBoxAttackGoal`). The bear rears up ten ticks before
  each hit, and a cooldown that runs out in the no-damage range restarts. The reach is judged
  after the move.
- `sweep.py bat`, `polar-bear` and `tadpole`, and the generic `mob_fixture.py`. Only the
  polar bear fixture is committed. The bat and tadpole sweeps are measured for a later round.

Out of scope: when the stomp cooldown first starts, the stomp's area damage
(`stomp_range_multiplier`, which BDS did not show), and `require_complete_path`.

## Capabilities

- `mob-runtime` (modified): a stomp reach is judged after the mob moved.

## Impact

`crates/unastar` (melee goal, physics body, a post-physics system), `crates/unastar-data`
(the `water_movement`, `movement.sway` and `navigation.float` bodies), `tools/bds-sweep`,
`docs/parity`, `docs/quirks`, `docs/decisions` (0068).

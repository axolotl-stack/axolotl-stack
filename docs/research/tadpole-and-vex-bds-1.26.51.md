# The tadpole and the vex on BDS 1.26.51: first measurements

These are findings from sweeps that are measured but not yet modelled (entities round 9). The
raw runs are not committed. Re-run them with `tools/bds-sweep/sweep.py tadpole --items all`
and `sweep.py vex --items all`, with `--client-capture --difficulty normal`, and distil them
with `mob_fixture.py`.

## Tadpole

Blockers (full dump): `behavior.random_swim` and `movement.sway`.

`sweep.py tadpole --items swim` ran four tadpoles in the glass pool for 600 ticks. They moved
on 268 to 533 of the ticks.

- **Turning.** A swim starts with the body turning 30° a tick (the default `max_turn`) to
  the new heading.
- **Horizontal motion.** In water the horizontal motion keeps 0.8 a tick. Every moving tick
  adds 0.0098 along the yaw, times the cosine of the pitch (0.00976 at a pitch of 4.9°,
  0.0098 at 0°). That is the underwater speed 0.1 × the movement speed 0.1 × 0.98. The top
  speed is 0.049 a tick (0.0098 / 0.2).
- **Vertical motion (the bobbing).** It does not fit the drowned's swim. With the 0.8 drag
  taken out, the vertical push is constant for stretches (−0.0122 a tick for 14 ticks), then
  drifts. Every 7 to 12 ticks it jumps by about +0.02. This looks like the 0.02 water gravity
  switching off and on, with a small upward push between 0 and 0.011. The pitch stays within
  ±5°. It is not modelled.
- **Growth.** A tadpole summoned with `ageable_grow_up` (`--items grow`) is never sent to the
  client. The frog's `AddActor` and the positional `convert_to_frog` sound come in the same
  batch, before the first sampled tick. So the transformation has no delay. In unastar the
  frog is not spawnable (8 blockers, including `breedable` love filters, `movement.amphibious`,
  `behavior.croak`, `behavior.eat_mob`, `behavior.jump_to_block`, `behavior.lay_egg`,
  `behavior.move_to_land` and `behavior.move_to_water`).

## Vex

Blockers (full dump): `behavior.charge_attack`, `behavior.look_at_entity` and
`damage_over_time`. Its idle wandering is not authored. Its physics authors no gravity and no
collision.

- **Idle** (`--items idle`, the glass room): it flies at up to 0.139 a tick (0.10 on average)
  and leaves the room through its walls and ceiling.
- **Charge** (`--items charge`, the survival player 8 blocks away): it reaches up to 0.53 a
  tick. Each hit dealt 9 (entity_attack): its `attack` is 3, and it spawns holding an iron
  sword from `vex_gear`. The hits came 53 to 98 ticks apart.
- **Decay** (`--items decay`, summoned with `minecraft:add_damage_timer`): its health did
  not change in 2,800 ticks, although the timer is 30 to 119 seconds. Either the summon event
  did not add the timer group, or the vex had flown out of range. Re-measure it with the vex
  kept in view (a barrier box), and read its actor data for the group.

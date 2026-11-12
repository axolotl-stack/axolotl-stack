# Proposal: mob ranged attacks (the skeleton family)

## Why

The skeleton, stray, bogged and parched could not spawn: `behavior.avoid_mob_type` (alone
it blocks 28 actors), `behavior.ranged_attack`, `minecraft:shooter` and `freezing_immune` had
no runtime kind, and their definitions use the `has_ranged_weapon`, `in_block` and
`is_underground` filters, an item-tag descriptor (`q.all_tags('minecraft:is_spear')`) and, on
the skeleton, a damage-sensor filter keyed `other_with_families`, which is no filter key.

Prior work: the ranged sweep of entities round 3 (`sweep.py ranged`, easy, normal and hard,
player kept alive); the unmerged integration branch (PR #71, commits `566bcfab`, `61fcbad2`,
`59216af7`, `14e35e06`) implemented an earlier version, used here as prior art and checked
against new measurements, not copied. Lessons: `docs/solutions/server/count-packet-batches-from-the-cause-not-from-the-addactor.md`,
`docs/solutions/server/pin-a-measured-position-with-three-geometries.md`; decision 0042 (one
effect system), which the tipped arrows and the slowness freeze go through.

## What Changes

- `behavior.ranged_attack` and `minecraft:shooter`: the goal, the shot (start, aim, spread,
  sounds, owner, aux value) and the arrows' difficulty damage and tipped effects, measured on
  BDS 1.26.51 and replayed against it.
- `behavior.avoid_mob_type`, general for every mob that authors only its common fields; the
  rest are rejected.
- Filters `has_ranged_weapon`, `in_block` and `is_underground`; item descriptors with item-tag
  Molang; a filter whose only fault is an unknown key never matches.
- `freezing_immune` accepted without behaviour (nothing freezes on the server).
- Mob AI walks at the movement attribute's value, so slowness slows it.
- New sweep sets for `sweep.py ranged` and a distiller (`ranged_fixture.py`).

Out of scope: charged and burst ranged attacks (pillager, shulker, blaze), the witch's thrown
potions, players' tipped arrows, skeleton-horse traps.

## Capabilities

- `mob-runtime` (new): contracts of the mob component runtime this change relies on.

## Impact

`crates/unastar` (mob components, AI, combat, projectiles, filters), `crates/unastar-data`
(the `minecraft:shooter` body), `crates/unastar-rules` (`Filter::never`), `tools/bds-sweep`,
parity rows of the skeleton family and of every actor `avoid_mob_type` unblocks.

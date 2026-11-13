# Proposal: the enderman

## Why

Entities round 6b. The enderman could not spawn: `looked_at`, `teleport`,
`behavior.take_block`, `behavior.place_block` and `variable_max_auto_step` had no runtime kind,
and its definition tests `in_contact_with_water` and `is_riding`, which the mob filters could not
answer. Round 6a measured it once (`docs/research/enderman-bds-1.26.51.md`), but its endermen
often teleported away before they were measured; two more rounds of `sweep.py enderman` keep them
in place.

Prior work: PR #71 (`fb1291c3`, `913417fb`) and PR #61 (a fork), used only as a checklist for
gaze, anger and block carrying. Nothing was copied: every rule here is measured again.

## What Changes

- `minecraft:looked_at`: the stare cone (`1 - cos` of half the field of view over the
  distance), aimed 2.55 up the enderman; the minimum look duration; `stare`; pumpkin, creative
  and wall exemptions. `has_equipment` answers players' armour slots.
- `minecraft:teleport`: one attempt (a uniform target in the cube, dropped onto ground with room
  and no liquid). The random timer starts expired, plus light and dark rolls. Two attempts per
  hurt. Dodges projectiles; teleports toward a far target. The forced move, trail and level
  events on the wire.
- `behavior.take_block` / `behavior.place_block`: the carried block is the main-hand item
  (decision 0066, proposed). The summon's old carried-block actor data is air.
- `variable_max_auto_step`: the body's step height.
- Filters `in_contact_with_water` (water overlapping the box; no rain) and `is_riding` (never).
- `sweep.py enderman` rounds two and three; `mob_fixture.py` keeps teleports, carried blocks,
  arrows and the player's turns.

Out of scope: rain and weather, riding, endermites (its target filter is accepted, not measured).

## Capabilities

- `mob-runtime` (modified): teleport landings, projectile dodges.

## Impact

`crates/unastar` (mob components, AI goals, life systems, filters, projectile flight, mob
network), `crates/unastar-data` (the `looked_at`, `teleport` and `variable_max_auto_step`
bodies), `tools/bds-sweep`, `docs/parity`, `docs/quirks`, `docs/research`, `docs/decisions`.

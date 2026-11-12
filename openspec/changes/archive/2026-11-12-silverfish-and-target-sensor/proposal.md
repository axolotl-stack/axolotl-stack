# Proposal: the silverfish, and the creeper's fuse sensor

## Why

Entities round 5 targets the creeper first and the silverfish among the closest free mobs.
The silverfish could not spawn: `behavior.silverfish_merge_with_stone` and
`behavior.silverfish_wake_up_friends` had no runtime kind, and nothing knew which blocks a
silverfish hides in. The creeper could not spawn either: `minecraft:explode` (the fuse and the
blast), `behavior.melee_attack`, `behavior.swell`, `minecraft:target_nearby_sensor` and
`minecraft:is_charged` had no runtime kind. Blocks round 4 (PR #105) implements
`minecraft:explode` and the explosion, but it forks from main and does not merge cleanly into
this lane, so the creeper's own parts land here and the creeper becomes spawnable once both
are in.

Prior work: `docs/research/silverfish-bds-1.26.51.md` (round 4 measurements); the unmerged
branch `Jviguy/stab-ent-m5` (`503bf672`: a merge goal modelled as a random stroll, infested
blocks as a hand-written name table) and PR #71 (`fb1291c3`: a creeper with a hand-written
swell and sensor), used as leads and checked against new measurements, not copied.

## What Changes

- `behavior.silverfish_merge_with_stone`, `behavior.silverfish_wake_up_friends` and the
  infested-block table (`data/measured/infested_blocks.json`, from a BDS sweep) with the
  silverfish a woken block lets out; the silverfish becomes spawnable.
- `minecraft:target_nearby_sensor` (general), `behavior.swell` and `minecraft:is_charged`.
- Sweeps `sweep.py creeper` and new `sweep.py silverfish` item sets; distillers
  `creeper_fixture.py` and `silverfish_fixture.py`.

Out of scope: `minecraft:explode` and the explosion (blocks round 4), `behavior.melee_attack`
(only the creeper uses it; its chase is measured with the creeper once it can spawn),
players breaking infested blocks (blocks lane; `mob::life::infested::release` is the hook).

## Capabilities

- `mob-runtime` (modified): built-in facts a component needs come from measured data,
  resolved at load.

## Impact

`crates/unastar` (mob components, AI goals, life, sensors), `crates/unastar-data` (measured
infested blocks, the `target_nearby_sensor` body), `tools/bds-sweep`, `docs/parity`,
`docs/quirks`, `docs/research`.

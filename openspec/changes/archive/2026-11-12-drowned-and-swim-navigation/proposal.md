# Proposal: the drowned and swim navigation

## Why

Entities round 7 targets the water mobs. The drowned could not spawn: `navigation.generic`,
`movement.generic`, `underwater_movement`, the immersed-target knockback preset, the
`is_daytime` and `is_riding` filters, the rider group's actor `spawn_entity` and its thrown
trident (no retained AddActor) were missing. Without it, the zombie's and husk's underwater
conversions could not run, and transformations with a delay, a sound, a spawn event or kept
equipment had no behaviour.

Prior work: `Jviguy/stab-ent-m5` (`503bf672`: a 709-line swim navigation, the drowned, husk,
transformation and witch) and PR #71. Both were used as leads and checked against new BDS
measurements, not merged.

## What Changes

- Swim navigation (`navigation::swim`): paths over water cells, pulled straight; a swimming mob
  pushes along the line to its waypoint with no gravity.
- The drowned becomes spawnable; its trident throws, daytime targeting and spawn equipment
  replay BDS.
- `minecraft:transformation` runs with a delay, a sound, a spawn event and kept equipment; the
  zombie and husk convert under water when BDS converts them.
- `sweep.py drowned` and `sweep.py witch`, `drowned_fixture.py` and the committed fixture; the
  witch's measurements as a research note.

Out of scope: the witch itself (its splash potion actor needs the items lane), riding, the
drowned's natural spawning.

## Capabilities

- `mob-runtime` (modified): a swimming mob moves through its swim path.

## Impact

`crates/unastar` (navigation, mob AI, physics, components, transformation, spawn entity),
`crates/unastar-data` (three component bodies, the `spawns` capture role),
`crates/bds-extractor/captures`, `tools/bds-sweep`, `docs/parity`, `docs/quirks`,
`docs/research`, `docs/decisions`.

# Proposal: the slime and the magma cube

## Why

Entities round 6b targets the hopping mobs, among the remaining free hostile mobs. Neither the
slime nor the magma cube could spawn: `minecraft:movement.jump`, `minecraft:area_attack`,
`behavior.slime_float`, `behavior.slime_attack`, `behavior.slime_random_direction`,
`behavior.slime_keep_on_jumping` and the magma cube's `minecraft:freezing_vulnerable` had no
runtime kind, nothing split a dying slime, and nothing let a slime spawn only in slime chunks.

Prior work: PR #71 (`0abaa4d1`: hop goals, area attack, splitting, slime chunks and swamp
spawning) and `Jviguy/stab-ent-m5` (`503bf672`: the magma cube). Both were used as leads and
checked against new BDS measurements, not merged; their constants were not measured
black-box and are replaced by measured ones where they disagree.

## What Changes

- `minecraft:movement.jump` (the hop control) and the four slime goals; the slime and the
  magma cube become spawnable.
- `minecraft:area_attack` (contact damage).
- Splitting on death into two to four children of half the size.
- The slime's native spawn check: slime chunks only (the swamp surface rule is not modelled).
- `sweep.py slime` and `sweep.py slime-spawn`; `slime_fixture.py` and the committed fixture.

Out of scope: the swamp surface spawning rule and the superflat chance (not measured), lava
and water floating beyond the goal itself, the magma cube's nether spawning.

## Capabilities

- `mob-runtime` (modified): a hopping mob moves only through its hop control.

## Impact

`crates/unastar` (mob components, AI, death, spawning), `crates/unastar-data` (the
`movement.jump` and `area_attack` bodies), `tools/bds-sweep`, `docs/parity`, `docs/quirks`,
`docs/research`.

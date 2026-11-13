---
status: proposed
date: 2026-11-12
decision-makers: [project owner (delegated)]
---

# The enderman carries blocks in its main hand

## Context and Problem Statement

`behavior.take_block` and `behavior.place_block` let the enderman pick a block up and put it
down. unastar needs somewhere to keep the carried block, a way to show it to viewers, and a rule
for what happens to it when the enderman dies. Actor data 23 is named for a carried block, and
the BDS 1.26.51 summon sends it (as air), so it looked like the natural carrier.

## Decision Drivers

- Copy what BDS sends (decision 0013), measured: `sweep.py enderman --items carry`.
- One canonical type per concept: a mob's held items already live in `MobEquipment`, which is
  replicated, saved and dropped on death.
- No parallel state that could disagree with what viewers see.

## Considered Options

1. **A carried-block component** that writes the block's network id into actor data 23.
2. **The main-hand slot of `MobEquipment`**: the block's item, carrying the block state, as a
   picked-up item. Actor data 23 stays air.

## Decision Outcome

Chosen option: 2. BDS 1.26.51 never changes actor data 23 when the enderman takes a block. It
sends a `MobEquipment` for the main hand with the block's item and state, one batch before the
block update, and an empty one when the block is put down or the enderman dies, when the block
drops with its loot. `mob::ai::carry` reads the carried state back from the main-hand stack.

### Consequences

- Good, because what the enderman carries is exactly what viewers see, and it is saved and
  dropped by the code that already handles every mob's equipment.
- Good, because the summon still carries air in actor data 23, as BDS's does (a native trait of
  the enderman class).
- Bad, because a block with no item cannot be carried. No block in the enderman's list lacks
  one.
- Neutral: the carried block always drops on death (it counts as picked up), as BDS's did in
  the one measured kill; that drop is not replayed.

## More Information

Evidence: `crates/unastar/src/entity/mob/ai/testdata/bds-1.26.51-enderman.json` (`carry-*`
items), `crates/unastar/src/entity/mob/tests/enderman.rs`
(`a_carried_block_shows_in_the_hand_and_is_put_down`,
`an_enderman_takes_the_blocks_bds_takes`), `docs/research/enderman-bds-1.26.51.md`. Prior art:
PR #61 (a fork) listed block carrying as a feature only; nothing was taken from it.

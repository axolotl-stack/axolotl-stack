---
title: Replay pressure plates from the actor positions BDS recorded, not from a physics model
area: testing
tags: [pressure plates, bds-sweep, actors, timeline, replay, items]
date: 2026-11-12
origin: lane BLOCKS round 4 (feat(blocks) commit adding server/game/redstone/plates.rs)
status: current
---

## Context

Whether a plate is pressed depends on where actors are, and dropped items scatter at random,
bounce and merge. A replay that simulated the actors would test unastar's item physics, not the
plate rules.

## What we learned

- Record the actors in the cell every tick (the timeline's actor box) and feed the plate rules
  those positions: all 92 plate rows replay exactly, items included, with no physics.
- Tick t of the server sees the actors that existed when it began (read t) where its own
  movement left them (read t + 1): an item pressing a plate is the one that fell below a quarter
  block during that tick. A mob removed in the tick still counts; merged items do not.
- Releases are not immediate: a plate checks itself every 20 ticks (10 weighted) from its press,
  so an armour stand killed 30 ticks after pressing a stone plate releases it 10 ticks later.
- A stone plate stores `redstone_signal` 1 but powers 15: compare stored states in the replay
  and the power through the circuit.
- In a live-world test, set a dropped item's velocity to zero; a drop scatters at random and the
  test otherwise passes or fails by chance.

## Where it lives

`tools/bds-sweep/plates.py`, `crates/unastar-data/data/measured/redstone/plates_*.json`,
`crates/unastar/src/server/game/redstone/plates.rs` and `redstone/tests.rs`
(`plates_press_like_bds_under_the_measured_actors`).

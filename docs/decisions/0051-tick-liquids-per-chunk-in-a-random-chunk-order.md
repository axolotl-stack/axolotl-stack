---
status: proposed
date: 2026-11-11
decision-makers: [lane LIQUIDS]
---

# Tick liquids per chunk in a random chunk order, same-tick ticks in scheduling order

## Context and Problem Statement

unastar had no liquid flow. BDS 1.26.51 liquids, measured tick by tick
(`crates/unastar-data/data/measured/liquids/`), tick 5 (water) or 30 (lava) game ticks after
being woken. A tick scheduled into another chunk lands one tick early in about half the
crossings, at random from tick to tick, and ticks due in the same game tick inside a chunk run in
an order that depends on how they were scheduled but was not reconstructed. The engine needs a
structure that reproduces the measured traces ([0013](0013-copy-bds-behaviour-exactly.md)).

## Considered Options

- Reuse the global scheduled-block queue (`block_use::ScheduledBlocks`) with fixed delays.
- A dedicated liquid engine with per-chunk queues; every game tick it draws a random chunk order,
  and a tick scheduled into a chunk ranked after the scheduling chunk is due one tick sooner.
  Inside a chunk, due ticks run in the order they were scheduled.
- The same engine with a reconstructed same-tick order (searched: first-in-first-out,
  last-in-first-out and a standard binary heap, with every spread and neighbour order). The best
  model explains 7 of the 10 clean cells.

## Decision Outcome

Chosen option: the dedicated engine with a random chunk order and scheduling-order ticks
(`crates/unastar/src/server/game/liquids/`). It reproduces every measured cell where the order
of same-tick ticks does not matter: exactly inside a chunk, and within the crossing tolerance
across chunks. The crossings land early as often as on BDS. The global queue cannot express the
chunk effect, and no searched same-tick model fits.

### Consequences

- Good, because the replays compare every liquid sweep cell with BDS, and the engine runs in
  memory behind a `LiquidTarget`, as redstone does behind its circuit trait.
- Bad, because cells decided by the same-tick order (sources made between two sources, receding
  spreads) can differ from BDS; they are skipped in the replays with that reason, and the
  `world.liquids` parity row lists the gap.

### Confirmation

`cargo test -p unastar --lib liquids` replays every row; `chunk_crossings_land_early_as_often_as_on_bds`
compares the crossing timing by distribution.

---
title: Measure neighbour-dependent block states with wave-based Endstone scenarios, and keep each job file small
area: tooling
tags: [bds-sweep, endstone, scenarios, block updates, connections, stairs, support]
date: 2026-11-10
origin: lane BLOCKS block-update work (feat(blocks) commit adding server/game/block_update)
status: current
---

## Context

Stair corners, fence/pane/wall connections, wall heights and whether attached blocks fall are
computed by BDS from neighbours and ship as no data. Measuring them one scenario at a time would
take hours of shared BDS time, and reading back right after a change can miss updates BDS applies
a tick or two later.

## What we learned

- The `scenarios` job (`tools/bds-sweep/scenarios.py`, plugin `jobs.py`) runs up to 64 scenarios
  side by side, one cell each, 6 blocks apart; items with the same step signature share a wave,
  so one wait covers 64 scenarios. 33,292 scenarios took 215 s of BDS time; sort items by step
  signature or every wave holds one item.
- Endstone `Block.set_data(data, physics)` separates the two halves of a placement: with
  `physics=false` the placed block still takes its own neighbour-dependent state (a fence set
  next to stone connects), but its neighbours are not updated and nothing is survival-checked;
  with `physics=true` neighbours react as on a player placement. `/setblock` behaves like
  `physics=true` and also places a door's upper half; `set_data` does not.
- BDS never removes a block for lack of support when the block itself is set into the world
  (a dandelion set on stone stays); only a neighbour update pops it. Measure support by
  changing a neighbour, not by placing. A player's placement is different: BDS refuses it where
  the block could not stay (an oak sapling clicked onto stone stays in hand; see
  `docs/solutions/testing/replay-bds-click-captures-to-test-placement.md`).
- Some updates lag: walls settle their connections a tick after placement, carpets and lanterns
  fall a tick or two after losing support. Read after a wait of 2-3 ticks, not immediately.
- Read back the *stored* states and replay those in tests: BDS normalises some requested states
  (a stair's requested corner is recomputed, `torch_facing_direction=unknown` may be kept), and
  a test that places the requested state measures something else.
- A job file of ~90,000 items (80 MB with `indent=1`) crashed BDS with an access violation as the
  scripted player connected; 33,000 items (51 MB) ran fine. Split big families into several
  runs under one lock hold.

## Where it lives

`tools/bds-sweep/scenarios.py`, `tools/bds-sweep/endstone-sweep/src/endstone_bds_sweep/jobs.py`
(`scenarios`), `tools/bds-sweep/summarize.py` (`summarize_scenarios`),
`crates/unastar-data/data/measured/block_updates/`, replays in
`crates/unastar/src/server/game/block_update/tests.rs`.

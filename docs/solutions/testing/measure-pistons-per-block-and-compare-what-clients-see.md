---
title: Measure pistons per block on the circuit layout, and compare what a client is sent field by field
area: testing
tags: [pistons, bds-sweep, scenarios, circuits, block actors, watcher, slime, replay]
date: 2026-11-11
origin: lane BLOCKS round 4 (feat(blocks) commit adding server/game/redstone/pistons.rs)
status: current
---

## Context

Pistons had no measured data: which blocks move, break or hold, how a piston is powered, its
timing, and what a client is sent while blocks move. Older piston code (#71, `a8732b6c`) took
its block lists from public behaviour and turned out wrong in several places (beds and chests).

## What we learned

- Guess nothing about the facing: a pilot with every `facing_direction` powered from one side
  showed the horizontal values are not the usual order (2 south, 3 north, 4 east, 5 west). The
  same pilot in round 3 had pushed toward its own power source and looked like "pistons do
  nothing".
- One per-block family (every default state in front of a sticky piston, powered 20 ticks)
  classifies the whole palette in about three minutes: the head never comes out (holds), the
  block lands one further (moves), lands back (pulled) or vanishes (breaks, `items` gives its
  drops).
- Slime and honey stick to the superflat floor under them: in the per-block family they "hold"
  because the floor line exceeds 12 blocks. Raise glue structures off the floor (`pistons_slime`
  runs at y=2) and keep the per-block result as measured, with the override in code.
- Endstone `set_data` on a lever changes the lever's look but not its power: a "lever on the
  piston" row measured nothing. Use redstone blocks as inputs, or real clicks.
- Rows whose block does its own thing in the cell (wall signs facing down that unastar's support
  rules drop, double plants Endstone completes, coral blocks dying 45 ticks after they were set)
  are skipped by name with the reason; the piston rule is not what differs there.
- A `placements` case that clicks a lever gives the watcher's full packet stream for a move. Compare
  it per kind (actors, block changes, sounds) after decoding: block actor compounds are hash maps,
  so print them with sorted keys, and drop consecutive repeats (BDS sends some moving block actors
  twice). The test world needs the superflat floor, or the lever pops off.
- unastar on main sent every block change twice (`write_block` sent it and also queued it for
  the batch system). A piston packet comparison makes that visible immediately.

## Where it lives

`tools/bds-sweep/pistons.py`, `placements.py` (`pistons`), `summarize.py round4`,
`crates/unastar-data/data/measured/pistons/`, `crates/unastar/src/server/game/redstone/pistons.rs`,
`redstone/tests.rs` (piston families) and `redstone/piston_packets.rs`.

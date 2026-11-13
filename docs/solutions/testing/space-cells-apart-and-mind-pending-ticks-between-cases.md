---
title: Space scenario cells apart when they bring water, and mind block ticks a previous case left pending
area: testing
tags: [bds-sweep, scenarios, placements, coral, scheduled ticks, water]
date: 2026-11-11
origin: lane BLOCKS round 3 leftovers (coral death, tools/bds-sweep/coral.py)
status: current
---

## Context

Round 2 concluded that a live coral block set by the scenario job never dies and that a
player-placed one dies at a random tick within 60 (two of 12 by tick 10). Both were wrong.

## What we learned

- Endstone `set_data` fills a coral plant or fan with water, and that water flows: on the
  6-block scenario grid it reached the coral blocks of neighbouring cells and kept them alive.
  Cells 16 blocks apart with a 15 x 15 box refilled after each wave (`coral.py` `LAYOUT`)
  showed every coral block set out of water dying, 45 ticks later.
- The `placements` job runs its cases one after another on one spot. A block tick a case
  leaves pending (a coral block's 45-tick check) fires on the next case's block: the two
  "dead by tick 10" coral blocks were placed about 45 ticks after the case before them. A
  replay that starts each case in a fresh world cannot see this; skip such cases with the
  reason, and measure timings in separate cells.
- Read every tick (`timeline`) before modelling a delay: reads every 10 ticks made a fixed
  45-tick check and a random 54 to 184 tick one look alike.
- A click from the scripted player inside a `timeline` gets its own wave (the wave signature
  holds the item key), so clicked cells need no unique commands to stay apart.

## Where it lives

`tools/bds-sweep/coral.py`, `crates/unastar-data/data/measured/block_updates/coral_*.json`,
`crates/unastar/src/server/game/block_use.rs` (`coral_changed`) and
`placement_capture_tests.rs` (`coral_blocks_die_at_the_measured_ticks`).

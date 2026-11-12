---
title: Liquids set by a command stay still, chunk crossings land early at random, and the liquid layer needs per-tick capture cuts
area: testing
tags: [liquids, bds-sweep, scenarios, timeline, capture, chunks, scheduled ticks, waterlogging]
date: 2026-11-11
origin: lane LIQUIDS (feat(liquids) commit adding server/game/liquids/)
status: current
---

## Context

Measuring BDS 1.26.51 water and lava tick by tick with the `scenarios` timeline step, and
replaying the traces through unastar's liquid engine.

## What we learned

- A water or lava source set by `/setblock` or Endstone `set_data` (even with a block update)
  never flows. The first pilot showed no flow anywhere, and so did a plain BDS probe. A liquid
  flows only once a neighbour changes. The `start` helper in `liquids.py` sets the liquid
  without an update, then sets a stone on it with one. Round 3's note that "Endstone fills a
  coral plant with water, and that water flows" did not hold here: Endstone-set coral held no
  water and died, but kelp and seagrass did hold water.
- Endstone reads only the block layer. The liquid layer (waterlogged water) is visible only in the
  client capture. A `tick_marks` layout makes the timeline send `bds-sweep:tick <wave> <tick>`
  before each tick's events, and `summarize.py liquids` cuts the capture per tick and per cell.
  The packets between two markers are that tick's packets (one subchunk update per subchunk).
- Liquid ticks that cross a chunk border land a tick early about half the time, at random from
  tick to tick for the same pair of chunks. Read naively, this looks like noise in otherwise
  5-tick steps. Lay cells out at chunk centres (`liquids_chunks` layout) when exact timing
  matters; compare crossings by distribution.
- Ticks due in the same game tick inside one chunk run in an order that depends on the order the
  liquids were woken. It is not first-in-first-out, last-in-first-out, or a standard binary heap
  over the reconstructed push order (`liquids_order2.json` holds the clean cells). Cells where it
  matters (two sources making a third, receding spreads) are skipped in the replays with that
  reason.
- `liquid_holders` (set a block into a still source with a block update, wake it, read the four
  sides) runs 64 blocks per wave and finds every block that keeps water, plus the sides it lets
  water out through. Bucket clicks on six of them agreed with it, at one wave per click.

## Where it lives

`tools/bds-sweep/liquids.py`, `endstone-sweep/.../jobs.py` (`tick_marks`, `dimension`, `setup`),
`tools/bds-sweep/summarize.py` (`summarize_liquids`), `crates/unastar-data/data/measured/liquids/`,
`crates/unastar/src/server/game/liquids/` (engine, `tests.rs` replays).

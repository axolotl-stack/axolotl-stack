---
title: A plain BDS console can build and check sub-chunk fixtures, but the End only saves edited chunks
area: tooling
tags: [bds, console, tickingarea, testforblock, fixtures, leveldb, subchunk]
date: 2026-11-10
origin: tools/bds-sweep/subchunk_fixture.py (openspec/changes/archive/2026-11-11-persist-block-states-by-name, read side)
status: current
---

## Context

The sub-chunk decoding fix needed BDS-written records of every palette shape, and BDS's
own answer for what each cell holds. No client was involved: everything went through the
console of a plain `bedrock_server` started by `bds_runtime.Bds(..., endstone=False)`.

## What we learned

- `tickingarea add <from> <to> <name> true` loads a few chunks within about 5 s with no
  player online; `execute in nether|the_end run …` reaches the other dimensions for both
  ticking areas and `fill`/`setblock`/`testforblock`. Poll with `testforblock` until it stops
  answering "Cannot test for block outside of the world".
- Console feedback carries no command id. Sync by sending a `testforblock … air` in a known
  loaded air cell with a unique position after a batch and waiting for its "at X,Y,Z" line.
  Through stdin BDS ran about 15 `fill` and 6 `testforblock` commands per second.
- `fill` on cells already holding the block logs `ERROR … 0 blocks filled`; that is not a
  failure. A misspelt block name is: `minecraft:bricks` does not exist (it is `brick_block`),
  and the fill fails with a syntax error while the cells stay air.
- `/testforblock` compares the first layer only, with full states when given; a
  mismatch answers "did not match the expected block state" without saying which state.
- In a default world, generated overworld and nether chunks were saved on `stop`, but End
  chunks around the island were not until a block in each was changed (one glass block
  high up is enough). Uniform zero-bit storages only showed up there. In a flat world no
  chunk without a changed block was saved, in any dimension, so set one block in every
  column a later step reads.
- BDS writes an edited chunk either through its periodic save (while the chunk stays
  loaded) or when the chunk unloads, and the two write different palettes; at `stop` they
  race. For deterministic records, run `tickingarea remove_all` in each dimension after
  the edits and wait about 90 s before `stop` (see
  `docs/solutions/storage/bds-compacts-palettes-only-when-a-chunk-unloads.md`). Ticking
  areas are stored in the world, so a copy of a world reloads them.
- Read worlds from a copy with a reader that does not open LevelDB for writing
  (`tools/bds-sweep/leveldb_read.py` reads tables and logs directly); to edit records for
  edge cases, write them into a copy before BDS starts, and check afterwards that BDS
  left them untouched.

## Where it lives

`tools/bds-sweep/subchunk_fixture.py` (build, probe, natural, natural-probe, fixture
steps), `tools/bds-sweep/subchunk_roundtrip.py`, `tools/bds-sweep/leveldb_read.py`,
`tools/bds-sweep/bds_runtime.py`.

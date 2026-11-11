---
title: Compare random ticks with BDS by distribution, with seeded replays and an envelope for spreading fields
area: testing
tags: [random ticks, bds-sweep, statistics, chi-square, crops, copper, leaves, grass]
date: 2026-11-11
origin: lane BLOCKS round 3 (feat(blocks) commit adding server/game/random_ticks.rs)
status: current
---

## Context

Random ticks are random: a single BDS run of a crop or copper field cannot be matched cell by
cell. The task was to measure BDS's random tick rules and check unastar's against them with
tests that stay deterministic.

## What we learned

- Measure fields, not cells: 16 x 16 fields of identical cells (`tools/bds-sweep/random_ticks.py`),
  `randomtickspeed` raised per wave so 3000 ticks give tens of random ticks per block, every cell
  read every 100 ticks. Waves of different speeds must not share a wave (the `cmd` step that sets
  the speed is part of the wave signature).
- Blocks with a deterministic step per random tick are the clock: sugar cane age and bare
  farmland moisture gave the per-block rate (`speed / 2048` per tick, Poisson-shaped counts),
  which every other rule is then measured against.
- Replay with a seeded generator and compare histograms per read with a two-sample chi-square
  (categories with small expected counts pooled, Bonferroni over the reads). Several replicate
  fields on unastar's side make its distribution precise; the seed makes the test deterministic.
- Cells that influence each other (grass spreading from cell to cell, farmland turning to dirt
  and then to grass) are one draw of the whole field: the chi-square over cells is far too
  sharp there (one seed gave p = 4e-11, another passed). Compare each BDS count with the spread
  of 16 unastar fields instead (mean +- 4 SD).
- Check that the tests can fail: halving the tick rate fails the crop, cane and farmland tests;
  the leaf test is weak (its fields decay too fast to tell the rates apart).
- Debug test builds are slow: a field at speed 256 takes millions of picks. A dense block array,
  one 32-bit draw per pick (12 bits for the position) and a small SplitMix generator brought
  the suite from over 40 minutes to about a minute.
- Endstone accepts states a block does not have (`dirt_type` on dirt); the replay ignores them.

## Where it lives

`tools/bds-sweep/random_ticks.py`, `summarize.py random-ticks`,
`crates/unastar-data/data/measured/random_ticks/`, `crates/unastar/src/server/game/random_ticks.rs`
and `random_ticks/tests.rs`.

---
title: To identify a mining multiplier from BDS, measure it on a break that takes a hundred ticks or more
area: tooling
tags: [bds sweep, mining, haste, mining fatigue, measurement design]
date: 2026-11-10
origin: tools/bds-sweep 68753e30 (`sweep.py mining --variants effects`)
status: current
---

## Context

The first mining sweep measured Haste I-II and Mining Fatigue I with a diamond tool on the slice
blocks. Those breaks take 2-10 ticks, and a sweep cell is only good to ±1 tick.

## What we learned

At 2-10 ticks, ±1 tick is ±10-50 %, so the cells could not tell candidate formulas apart: Haste
`(1 + 0.2L)²`, `(1 + 0.2L) × 1.2^L`, `(1 + 0.2L) × 6 / (6 - L)` and `1 + 0.5L`, and Mining Fatigue
`0.21^L` against `0.3^L × (1 - 0.3L)`, all fitted every cell within a tick. The only clear signal
was that the textbook `1 + 0.2L` and `0.3^L` were wrong. Five more cells by hand settled it: Haste
I-III on stone (150 ticks unaffected: 104, 75, 54) and Mining Fatigue I-II on dirt (15 ticks
unaffected: 71, 340). Only `(1 + 0.2L) × 1.2^L` and `0.21^L` fit those and the 156 earlier cells.
Pick the block and tool so the measured break is long: the relative error is then 1 tick over the
whole break, not over a handful.

The same applies to any multiplier sweep (Efficiency on soft blocks saturates at 0-1 ticks and
says little; a tier's speed is best read where the hand needs hundreds of ticks).

## Where it lives

`tools/bds-sweep/sweep.py` (`EFFECT_CELLS`), `crates/unastar-data/data/measured/mining_ticks.json`,
`crates/unastar/src/server/game/mining.rs`.

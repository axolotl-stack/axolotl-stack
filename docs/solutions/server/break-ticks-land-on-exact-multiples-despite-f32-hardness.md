---
title: Break times that are exact tick multiples need a tolerance, because block hardness is f32
area: server
tags: [mining, break time, float, bds sweep, measured]
date: 2026-11-10
origin: items lane 2026-11-10, after tools/bds-sweep 68753e30 (mining measured against the BDS sweep)
status: current
---

## Context

`server/game/mining.rs` turns per-tick progress (`speed / hardness / 30 or 100`) into the ticks a
break takes, and `mining/measured_tests.rs` compares that with every cell of the BDS mining sweep
(`unastar-data/data/measured/mining_ticks.json`). A sweep cell is the server ticks from the click to
the break, which is our tick count minus one (the click tick already makes progress), ±1 tick.

## What we learned

Many vanilla breaks land exactly on a whole tick: a copper pickaxe on diamond ore makes 1/60 per
tick, a golden pickaxe on netherrack exactly 1. `ceil(1 / progress)` then decides the result on
rounding noise, and it rounds the wrong way in both precisions we tried: in `f32`, 5/3/100 gives
60.0000006 ticks (61 instead of 60); in `f64` the block hardness is still the registry's `f32`
(0.4 is 0.4000000059), so netherrack by hand came out at 41 ticks instead of 40 even with a 1e-9
relative tolerance. Each of these is one tick slow, which together with the sweep's own ±1 jitter
failed six cells. Computing in `f64` and snapping to the nearest whole tick when within 1e-4 fixes
all 569 compared cells; nothing in vanilla data lands that close to a tick without being exact.

When comparing with the sweep, compare `break_ticks - 1` with the median and allow ±1; a trial
list like `[59, 58, 58]` for a 60-tick break is normal.

## Where it lives

`crates/unastar/src/server/game/mining.rs` (`break_ticks`),
`crates/unastar/src/server/game/mining/measured_tests.rs`.

---
title: Measure random BDS goals on a quiet machine, and interleave the cases you compare
area: tooling
tags: [bds-sweep, statistics, silverfish, load, interleaving, random goals, distribution]
date: 2026-11-12
origin: entities round 5 (silverfish merge goal), branch t3/c6f1368f
status: current
---

## Context

The silverfish's merge goal is a per-tick chance. Two sweeps of 1 × 1 stone pits (`sweep.py
silverfish --items pit`, 20 runs each) waited 16.3 and 14.6 ticks on average before merging. A
later sweep over seven kinds of host block waited 7 ticks, which looked like stone merging at
less than half the rate of cobblestone or stone bricks.

## What we learned

- **It was not the block.** A sweep that interleaves the cases (`--items ab`: stone,
  cobblestone, stone, cobblestone, … 20 of each, twice) gave 7.5 and 9.4, then 10.6 and 10.2 ticks: the same. The slow
  stone sweeps had run while the machine was compiling the server (`cargo test` builds of
  several minutes); the quick ones had not. BDS's sampled tick counter showed no skipped ticks,
  so the effect is suspected, not explained.
- **Interleave what you compare.** Running all of case A, then all of case B, confounds the
  cases with whatever changed on the machine in between. Alternate them in one sweep.
- **Keep heavy builds off the machine while a random goal is measured.** Deterministic
  measurements (positions, packet order, block changes) were unaffected; chance-per-tick rates
  were not. A sweep that fits a rate should be run when no cargo build is going, and its
  results checked against an interleaved control.
- **Fit the rate by likelihood, compare by mean ± 3 standard errors.** A geometric wait has a
  standard error of about its mean over √n, so 20 runs pin a mean of 8 only to ±1.8: enough to
  reject a roll of 1 in 5 or 1 in 10 per tick together with the face draw, not to pick between
  1 in 7 and 1 in 8.

## How to apply

For any goal with a random start (strolls, merges, ambient sounds), run its sweep on a quiet
machine, interleave the cases it compares, and record the conditions next to the fitted value
(`docs/research/mob-runtime-constants.md`). Replays compare distributions, never single runs
(`mob/tests/silverfish.rs`).

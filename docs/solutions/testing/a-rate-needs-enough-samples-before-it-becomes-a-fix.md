---
title: A drop rate needs enough samples before it becomes a fix, and a test's roll can be a kill
area: testing
tags: [loot, sampling, short_grass, fortune, flaky, crit, loopback]
date: 2026-11-12
origin: items round 3 (grass seeds, item_use_loopback)
status: current
---

## Context

Round 2 read the 400-sample drop sweep of short grass as "seeds about 11%, not 12.5%, and
Fortune adds nothing", and handed the rate on as a fix to make. A 20000-sample sweep per tool
(480000 samples that can drop a seed) gave 12.48% ± 0.09%: the table's 1/8 was right and only
the Fortune bonus was wrong.

Separately, `tests/item_use_loopback.rs` started failing about half the time after thrown
projectiles began moving in the tick after the throw. The change was innocent.

## What we learned

- At 400 samples a 1-in-8 rate has a standard error of 1.7 points, so 11% is less than one
  error away. Before changing a rate, size the sweep so the difference you expect is several
  standard errors: `sqrt(p(1-p)/n)`. At n = 20000 per cell it is 0.23 points.
- A replay that checks a random outcome must compare it with the measured distribution, not one
  roll: `block_drops/grass_tests.rs` rolls each cell 20000 times from a seeded RNG and allows four
  standard errors of the difference of two proportions.
- The loopback failure was a roll too. A fully drawn bow's critical bonus (up to 5 on 6) can kill
  a 10-health cow, and a killing hit sends only the death event, never `Hurt`. The server in that
  test is entropy-seeded, so the test waited for `Hurt` that, some runs, never came. It now
  accepts `Hurt` or `Death`. Debug output (`eprintln!`) slowed the server enough to change the
  rolls, so the failure vanished whenever it was being watched.

## Where it lives

- `crates/unastar/src/server/game/block_drops/grass_tests.rs`, `testdata/bds-grass-seeds.json`.
- `crates/unastar/tests/item_use_loopback.rs` (the arrow and the cow).
- Related: [replays compare random rolls with measured outcomes](replays-compare-random-rolls-with-measured-outcomes.md).

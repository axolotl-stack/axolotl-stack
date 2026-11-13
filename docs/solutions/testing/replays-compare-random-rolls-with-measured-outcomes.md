---
title: A capture replay must not compare a random roll with the one roll BDS happened to make
area: testing
tags: [replay, capture, random, loot, flaky, seed, EntityRandom]
date: 2026-11-11
origin: commit fd00f7d1 (fix(items): breaking replay no longer depends on a lucky loot roll)
status: current
---

## Context

The survival breaking replay (`breaking_tests.rs`) compared every drop with BDS's capture,
including breaking short grass by hand. Wheat seeds from short grass are a chance roll, and the
capture happened to roll none. The replay's server rolls from `EntityRandom`, whose default
is seeded from entropy. The test failed in about one run in eight, passed locally most of the
time, and first failed on CI (PR #77, test-windows) after a docs-only change.

## What we learned

- A server built for a test rolls from entropy unless the test seeds it:
  `EntityRandom::default()` is `StdRng::from_entropy()`. A replay or sweep test that compares
  anything a roll decides must insert `EntityRandom::seeded(..)` itself.
- A seed that happens to reproduce BDS's roll is not a fix. The first fix seeded the whole
  replay with 3, because seeds 1 and 2 rolled seeds. Any change in what draws from the RNG
  before that roll (one more drop, a reordered scenario, a merge that adds a system) flips it
  back, and then the failure is deterministic.
- What holds up has two parts:
  1. Seed per scenario, after its set-up (`seeded(index)`). The run is deterministic, and a
     scenario's rolls don't depend on the scenarios before it.
  2. Compare a random outcome with the set of outcomes BDS's drop sweep measured for that
     block and held item (`unastar-data/data/measured/block_drops/<block>.json`: the keys of
     `observed`, `;`-separated `name@data` + `x` + count, `""` for nothing). Check BDS's side
     against the same set. Every other drop is still compared exactly.
- To prove such a fix, read a seed offset from an environment variable in a temporary copy
  of the test. One build then serves every seed: run the test binary itself
  (`target/debug/deps/unastar-*.exe <filter>`) under many seeds and in a loop. Here, 40 seeds
  rolled nothing 39 times and one wheat seeds once, every run passed, and 200 runs in a row
  passed.
- The other replays already seed their RNG: melee 7, spears 11, elytra 3, and 13 for
  `capture_replay.rs` (jukebox, drinks). Mob RNGs are seeded from deterministic seeds, so
  seeding `EntityRandom` covers them too.

## Where it lives

- `crates/unastar/src/server/game/blocks/breaking_tests.rs`: `set_up` and `set_up_2` seeding,
  `random_drop_outcomes`.
- `crates/unastar/src/entity/item_drop.rs`: `EntityRandom`.
- `crates/unastar/src/server/game/capture_replay.rs`: the shared replay harness, seeded.

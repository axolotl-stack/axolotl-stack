---
title: Measure per-tick player state with a console script, then replay its tick counts in the harness
area: tooling
tags: [bds-sweep, bds-sweep-client, capture, console, harness, breathing, drowning]
date: 2026-11-10
origin: commit "fix(unastar): replicate the player's air supply and breathing flag like BDS"
status: current
---

## Context

Drowning needs the player under water for 15+ seconds, and the bot client cannot swim. The
question was what BDS sends every tick (air, flags, hurt events) and how to compare unastar with
it exactly.

## What we learned

- Plain BDS on a slot is enough: a superflat world, `bds-sweep-client --capture-dir` as the
  player (it sends `PlayerAuthInput` every 50 ms, so BDS ticks it), and console commands on BDS's
  stdin. A glass shell (`fill -2 -61 -2 2 -57 2 glass hollow`) around the player at
  `(0.5,-60,0.5)` lets `fill -1 -60 -1 1 -58 1 water` / `air` submerge and drain it without moving
  it. Game mode, `/effect`, `/damage <cause>` and `/kill` drive the other cases.
- While the state changes every tick (air under water), the number of updates between two flag
  changes is the number of ticks between two commands. A harness test can replay the phase exactly
  (`runs()` in `player_life/tests/breath.rs`) and compare every decoded packet, not just the
  shape.
- unastar also reads console commands from stdin, so the same script runs against
  `target/debug/unastar.exe` for a side-by-side capture with the same client. Its super-flat
  surface is y=4 (BDS: −60), it has `/spawnpoint` but no `/setworldspawn` or `/damage`, and the
  `super_flat` generator rejects a `seed` key.
- Traps: unastar's hunger regeneration reads `naturalregeneration` from the announced world
  template, not from `/gamerule`, so a test that needs no healing keeps food below 18 instead.
  `cargo run -p parity -- generate` with `UNASTAR_BDS_DIR` set rewrites
  `docs/parity/source/bedrock-worldgen.tsv` line endings; restore it if its content did not change.

## Where it lives

`tools/bds-sweep/` (slots, `bds_runtime.py`), `crates/bds-sweep` (`bds-sweep-client`),
`crates/unastar/src/entity/player_life/tests/breath.rs`, fixture
`crates/unastar/src/entity/player_life/testdata/bds-1.26.51-breath.json` (its `sources` give the
setup of each capture).

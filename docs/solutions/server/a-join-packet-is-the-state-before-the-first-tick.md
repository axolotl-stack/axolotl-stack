---
title: A copied join packet is the state before the first tick; capture what follows it too
area: server
tags: [actor-data, breathing, air-supply, join, set-actor-data, capture]
date: 2026-11-10
origin: commit "fix(unastar): replicate the player's air supply and breathing flag like BDS"
status: current
---

## Context

Survival players on unastar always saw the air-bubble bar. The join `SetActorData` matched the
BDS 1.26.51 capture exactly (flags `CANCLIMB|HAS_COLLISION|HAS_GRAVITY`, air 300/300, no
`BREATHING`), and a regression test pinned it. The client shows the bubbles while `BREATHING` is
off or the air is short of full, so the pinned packet was the bug.

## What we learned

- The join packet is the player's state before its first tick. One tick after
  `PlayStatus(PlayerSpawn)` BDS sends a flags-only `SetActorData` that turns `BREATHING` on, then
  one update per change (air every tick under water). unastar had copied the packet but had no
  system that ticks breath, so the flag stayed off forever.
- The committed join capture held the evidence but hid it: its `join-sequence.json` lists a
  17-byte `SetActorData` about 100 ms after `PlayStatus(PlayerSpawn)` (the size of the flag
  update), yet only the first `SetActorData` was kept as a `.bin`, so nothing in the repository
  showed what it said. When a test pins an initial value from a join capture, also record a few
  seconds after spawn and check what changes it.
- A value that "matches the capture" can still be wrong if the capture's next packets change it.
  The quirk row for the initial flags said "copied" for weeks while the bug was live.

## Where it lives

`crates/unastar/src/entity/player_life/breath.rs`, `crates/unastar/src/server/game/join.rs`
(`initial_player_actor_data`), evidence `crates/unastar/src/entity/player_life/testdata/bds-1.26.51-breath.json`
(phase `spawn`), tests `crates/unastar/src/entity/player_life/tests/breath.rs`.

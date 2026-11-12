---
title: Scripted BDS player captures need a fixed identity for rejoins, a resync after /enchant, and no mobs
area: tooling
tags: [bds-sweep, bds-sweep-client, capture, rejoin, enchant, equip, slime, tellraw]
date: 2026-11-10
origin: commits "feat(bds-sweep): input, item-use, equip and quit orders for scripted players" and "fix(unastar): replicate a player's actor data like BDS"
status: current
---

## Context

The player-data captures drove `bds-sweep-client` players from the BDS console on a slot
(`tellraw <name> {"rawtext":[{"text":"bds-sweep:<order>"}]}` reaches only that player). Three
runs were lost to setup traps before the measurements were usable.

## What we learned

- A rejoin needs `--uuid`: the client otherwise logs in with a new random identity and BDS
  treats it as a new player. Terminating the client is not leaving: BDS keeps the player in the
  world (and ticking its air) until the connection times out about 10 s later, and a rejoin
  before that fails with a server-id conflict. `bds-sweep:quit` plus waiting for
  `Player disconnected` in the console is the reliable way; plan the 10 s into the phase.
- `/enchant` rewrites the held stack through a legacy inventory transaction the client does not
  track, so the client's stack net ID is stale and an `ItemStackRequest` on it fails
  (`Failedtovalidatesrcslot`). Sending `bds-sweep:use` first makes BDS reject the use and resend
  the whole inventory with fresh net IDs; `bds-sweep:equip-head` then works. Using armour in the
  air does not equip it here (BDS rejects it).
- Superflat worlds spawn slimes: one killed the subject mid-run and knocked it around outside
  the test tank. Run `gamerule domobspawning false` and `kill @e[type=!player]` first, `tp` the
  player back and heal it (`effect … instant_health`) between phases.
- `fill … air` over a bubble column leaves its water: the column re-forms. Replace the source
  block (soul sand, magma) to end it.
- `cargo test -p bds-sweep` does not rebuild `target/debug/bds-sweep-client.exe`; run
  `cargo build -p bds-sweep` before a capture that uses a new order.

## Where it lives

`crates/bds-sweep/src/main.rs` (orders), `tools/bds-sweep/README.md` (order table),
fixture `crates/unastar/src/entity/player_life/testdata/bds-1.26.51-player-data.json` (its
`sources` give each capture's setup).

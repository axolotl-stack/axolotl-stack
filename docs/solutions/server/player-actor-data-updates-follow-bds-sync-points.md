---
title: A player's SetActorData updates are grouped by when BDS syncs, not by what changed
area: server
tags: [actor-data, set-actor-data, add-player, death, respawn, sneak, moving, capture]
date: 2026-11-10
origin: commit "fix(unastar): replicate a player's actor data like BDS"
status: current
---

## Context

unastar sent a player's flag word only with item use and its air only from the breath tick;
`AddPlayer` carried no actor data at all, so a later viewer saw no air, flags or box. Copying
BDS meant copying not only the values but how they are split into `SetActorData` packets,
which the clients see and the captures pin down.

## What we learned

- Keep one record of what the clients last received (`PlayerActorData::sent`) and send the
  difference; `AddPlayer` then sends exactly that record (79 rows), so a late viewer and an old
  one agree. The join `SetActorData` is the same set without the box and four other keys.
- One update per player at the end of the tick reproduces most captures, but not all. Within
  a tick BDS sends a toggle's flags at once, then the attribute changes, then the breath's
  update, and `MOVING` in an update of its own at the end. BDS 1.26.51 also syncs right after
  some events: a game-mode change sends the flag word after
  `UpdatePlayerGameType`; a death outside the player's own tick (`/kill`, a mob) sends the
  death rows at once and the 0.2 × 0.2 box a tick later, while a drowning death sends air, box
  and death rows together; a respawn sends the living box on its own before the breath update.
  The `/kill` grouping varied between captures (once together), so a test must pick one
  captured variant, not assume a rule.
- With the vanilla input flags a sneak is two updates in its tick: the flag word with the
  height, then the second word (`BLOCKING`, `TRANSITION_BLOCKING`). The three-packet form first
  measured here (old rows resent with key 139, the sneak a tick late) was BDS answering a client
  that left out the raw sneak flags; refusals and that resync resend rows that did not change,
  so a diff-only sync is not enough there.
- Do not send anything before the player's first breath tick after the join: the first update
  must carry that tick's flag and air (a rejoining player's saved air arrives with `BREATHING`),
  and the spawn status can be cleared between the breath tick and the network send.
- A client that does not move can only measure refusals (BDS refused its `Startsprinting`
  and `Startswimming`); the moving client (`bds-sweep` move orders) measured the accepted
  toggles, `MOVING` and exhaustion (`bds-1.26.51-movement.json`). `Startcrawling` is still
  ignored: crawling comes from blocks.
- A player joining sees the others as they were at the start of the tick: build `AddPlayer`
  from a snapshot taken before the tick's updates (`announced`), not from the latest record.

## Where it lives

`crates/unastar/src/entity/player_life/actor_data.rs`, `motion.rs`, `breath.rs`, `death.rs`,
`respawn.rs`, `crates/unastar/src/server/broadcast.rs` (`AddPlayer`), evidence
`crates/unastar/src/entity/player_life/testdata/bds-1.26.51-player-data.json` and
`bds-1.26.51-movement.json`, tests `crates/unastar/src/entity/player_life/tests/actor_data.rs`
and `tests/movement.rs`.

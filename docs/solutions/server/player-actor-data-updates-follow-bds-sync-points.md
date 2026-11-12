---
title: A player's SetActorData updates are grouped by when BDS syncs, not by what changed
area: server
tags: [actor-data, set-actor-data, add-player, death, respawn, sneak, capture]
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
- One update per player at the end of the tick reproduces most captures, but not all. BDS
  1.26.51 also syncs right after some events: a game-mode change sends the flag word after
  `UpdatePlayerGameType`; a death outside the player's own tick (`/kill`, a mob) sends the
  death rows at once and the 0.2 × 0.2 box a tick later, while a drowning death sends air, box
  and death rows together; a respawn sends the living box on its own before the breath update.
  The `/kill` grouping varied between captures (once together), so a test must pick one
  captured variant, not assume a rule.
- Starting a sneak is three packets (flags, height and the old second word with key 139; then
  the second word; then all three again next tick). Rows that did not change are resent, so a
  diff-only sync is not enough there.
- Do not send anything before the player's first breath tick after the join: the first update
  must carry that tick's flag and air (a rejoining player's saved air arrives with `BREATHING`),
  and the spawn status can be cleared between the breath tick and the network send.
- The scripted client does not move, which shapes what can be measured: BDS refuses its
  `Startsprinting` and `Startswimming` (answering with the unchanged flags), so their accepted
  case is still unmeasured; its `Startcrawling` is ignored (crawling comes from blocks).

## Where it lives

`crates/unastar/src/entity/player_life/actor_data.rs`, `breath.rs`, `death.rs`, `respawn.rs`,
`crates/unastar/src/server/broadcast.rs` (`AddPlayer`), evidence
`crates/unastar/src/entity/player_life/testdata/bds-1.26.51-player-data.json`, tests
`crates/unastar/src/entity/player_life/tests/actor_data.rs`.

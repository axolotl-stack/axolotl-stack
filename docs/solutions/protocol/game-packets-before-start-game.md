---
title: Game packets before StartGame, and comparing McpePacket bytes with fixtures
area: protocol
tags: [join, start-game, jolyne, roster, fixtures]
date: 2026-11-12
origin: OpenSpec change protocol-join-order
status: current
---

## Context

BDS 1.26.51 sends game packets before `StartGame`, in the same batch. The join had to copy that
without moving server state into jolyne.

## What we learned

- BDS sends three packets there, not two: the sleeping-player `LevelEventGeneric`, the joining
  player's own `PlayerList` entry and a `SyncWorldClocks` *sync state* (clock id, time, not
  paused) (member join `00011`-`00013`). The later `SyncWorldClocks` (`00032`) is the *registry*
  form with the time markers. Every pre-`StartGame` gap in the fixture numbering is worth decoding:
  `00013` was never listed as a gap because it looked like the later clock packet.
- The jolyne client already defers game packets that arrive before `StartGame` into its play
  session, so no client change was needed; the server only had to queue them first in the
  `StartGame` group (`WorldJoinParams::before_start_game`).
- The count needs the online roster before the game loop has the player. A lock-guarded roster
  shared with the connection tasks (`OnlineRoster`) with an RAII reservation keeps it consistent:
  a failed join gives its place back on drop, a spawned one on `despawn_player`.
- `McpePacket::encode` writes a game frame (`0xFE`, length, header, body). The committed fixtures
  are header + body, so compare the typed packet's own `encode` with the fixture payload (after the
  varuint id), not the `McpePacket` bytes.

## Where it lives

`crates/jolyne/src/world.rs` (`WorldJoinParams`), `crates/jolyne/src/proto/server.rs`,
`crates/unastar/src/server/game/roster.rs` (`OnlineRoster`, `before_start_game`),
`crates/unastar/src/server/connect.rs`; decision
[0044](../../decisions/0044-send-pre-start-game-roster-packets-before-start-game.md).

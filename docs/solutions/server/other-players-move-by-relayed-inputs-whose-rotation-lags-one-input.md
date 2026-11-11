---
title: Other players move by one MovePlayer per input, whose pitch and yaw lag the input by one; BDS simulates the position itself
area: server
tags: [MovePlayer, player replication, PlayerAuthInput, head yaw, rotation, server-authoritative movement, bds-sweep, walk]
date: 2026-11-10
origin: fix(entities) player-relay commit on branch t3/44607a6a (round 3 of the replication work)
status: current
---

## Context

unastar broadcast a player's `MovePlayer` only when it moved 0.1 block or turned 1°, to a 3×3
chunk area, with `on_ground` always true and head yaw equal to yaw. The obvious alternative,
moving players like mobs (`MoveActorDelta` at the bandwidth cadence of the player definition's
empty `conditional_bandwidth_optimization`), is also wrong.

## What we learned

- **One `MovePlayer` per input, always.** BDS relays every `PlayerAuthInput` to every player in
  send range as one `MovePlayer` (mode normal), also for a player standing still and at 40–56
  blocks, and never sends players `MoveActorDelta`. Two inputs processed in one tick give two
  packets in one batch.
- **The rotation lags.** A packet carries the position, ground contact and head yaw after the
  input and the input's own tick, but the pitch and yaw the player had before it: turning 9° per
  input relays head 9, 18, 27 with yaw 0, 9, 18. The yaw is wrapped into [-180, 180) while the
  head yaw is passed on as sent (head 189 with yaw -180). The pitch lags the same way. After a
  teleport the first relayed yaw is the teleport's.
- **The position is BDS's own simulation.** A scripted client claiming 0.2 blocks per tick with
  no movement input was corrected (20 `CorrectPlayerMovePrediction`) and relayed drifting 0.03
  blocks; with the forward input it walked +z at 0.2159 blocks per tick whatever it claimed.
  Ground contact is the server's too: a client that never sends a collision flag stands on the
  ground, and a falling one 0.04 above the floor is still in the air. unastar trusts the client's
  position (a vanilla client predicts the same) but computes ground contact from the blocks
  under the feet.
- **Measuring it.** The sweep client grew a `bds-sweep:walk DX DY DZ DYAW TICKS FLAGS [DPITCH]`
  order; the sweep needs a client built from the checkout (`--client`). Decode the client's
  own `PlayerAuthInput`s (pitch, yaw, head, tick) from its capture to tie each relay to its input;
  without that the lag is invisible, since the scripted client sends yaw and head yaw equal.

## Where it lives

- `crates/unastar/src/server/player_view.rs` (`sync_player_viewers`, `RelayedMoves`),
  `crates/unastar/src/server/game/movement.rs` (records the rotation before each input)
- Evidence and test: `crates/unastar/src/server/testdata/bds-1.26.51-player-relay.json`,
  `crates/unastar/src/server/bds_player_relay.rs`
- Tooling: `crates/bds-sweep/src/main.rs` (walk order), `tools/bds-sweep/sweep.py walk`,
  `tools/bds-sweep/player_relay_fixture.py`
- Quirk row: `docs/quirks/bds.md` (Entities / player movement relay)

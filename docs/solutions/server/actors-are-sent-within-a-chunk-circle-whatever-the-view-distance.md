---
title: BDS sends actors within a 17-square chunk circle whatever the view distance; put each packet on a tick with chat marks
area: server
tags: [send range, AddActor, AddPlayer, RemoveActor, visibility, view distance, simulation distance, bds-sweep, observer, chat marks]
date: 2026-11-10
origin: fix(entities) send-range commit on branch t3/44607a6a (round 3 of the replication work)
status: current
---

## Context

unastar showed an actor to every player whose loaded chunks contained it, so a player with view
distance 10 saw mobs 10 chunks away. Round 2 noticed BDS stopped "about four chunks" out. Guessing
the shape (a block radius around the player, a square, the client's chunk radius, the simulation
distance) would each have fitted the round-2 points.

## What we learned

- **The rule is chunk to chunk.** A viewer sees an actor while `Δx² + Δz² ≤ 17` in chunk
  offsets between the chunk it stands in and the actor's chunk: (4, 0), (4, 1), (2, 3) in;
  (5, 0), (5, 1), (3, 3), (3, 4) out. Actors in one chunk switch together, and the switch comes
  exactly when the viewer crosses a chunk border. A block-distance model fails: a cow 63 blocks
  away on the diagonal was dropped while one 69.5 blocks away along x was kept.
- **Nothing else changes it.** View / simulation distance 6 / 4, 10 / 10 and 16 / 12 switch at
  the same steps; players, mobs, armor stands, items, xp orbs, arrows and minecarts alike. BDS
  had already sent the client every chunk within its view distance, so the range is not "chunks
  sent".
- **Add now, remove a tick later.** The add comes in the tick the viewer's chunk enters the
  range; the `RemoveActor` one tick after it leaves. There is no hysteresis.
- **Players too, and on leaving the game.** Two players 71 blocks apart (offset (3, 4)) got each
  other's `RemoveActor`, then `AddPlayer` again when near. unastar never sent a leaving player's
  `RemoveActor` at all (the despawn event it waited for was never emitted); players now carry
  `ActorViewers`, whose removal sends it.
- **Technique: chat marks.** Have the plugin send the observer `bds-sweep:mark <item> <step>
  <tick>` every tick; each packet in the observer's capture then belongs to the last mark
  before it. That gave every add and remove an exact step and tick with no clock alignment.
  One-block steps of ten ticks, out and back along +x, the diagonal and -x, plus big jumps,
  separate chunk granularity, shape, hysteresis and delay in one 5-minute run per config.
- **Leftover:** BDS's `AddPlayer` position is the feet; unastar still sends the eye there.

## Where it lives

- `crates/unastar/src/entity/send_range.rs` (the rule, `ViewerChanges`), used by
  `mob/net/network.rs::sync_mob_viewers`, `server/broadcast.rs::sync_actor_viewers` and
  `server/player_view.rs::sync_player_viewers`
- Evidence and test: `crates/unastar/src/entity/testdata/bds-1.26.51-send-range.json`,
  `crates/unastar/src/entity/bds_send_range.rs`
- Tooling: `tools/bds-sweep/sweep.py range --observer`, `jobs.py::send_range`,
  `tools/bds-sweep/range_fixture.py`
- Quirk row: `docs/quirks/bds.md` (Entities / actor send range)

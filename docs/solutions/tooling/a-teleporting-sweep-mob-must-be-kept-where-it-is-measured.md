---
title: A teleporting sweep mob must be kept where it is measured, in BDS and in the replay
area: tooling
tags: [bds-sweep, enderman, teleport, despawn, measurement, replay]
date: 2026-11-12
origin: entities lane round 6b, `sweep.py enderman` rounds two and three (enderman-r2, enderman-r3)
status: current
---

## Context

Round 6a's enderman sweep was mostly unusable: endermen teleported away before they were looked
at or before they took a block. Rounds two and three fixed that, and the first try at each fix
still lost data in a way that looked like behaviour.

## What we learned

- **It teleports in its spawn tick.** About half the endermen on a superflat, and nearly all on a
  platform, teleport in the tick they spawn (the random timer starts expired). Put the mob back
  with `/tp` a few ticks after the spawn (tick 5) before measuring anything, and check in the
  replay that it did not move again before the measured tick.
- **A mob far from the player despawns mid-item.** The round-two hurt cycles kept the sweep
  player 25 blocks away. A teleport took the enderman past 32 blocks of every player, BDS
  despawned it at random within a few dozen ticks, and every later `/damage` matched no target
  ("No targets matched selector" in `bds.log`). The results looked like hurts that teleport
  rarely. Keep the player within 32 − 16 blocks of the home spot (`near`), so no single teleport
  can get past 32. A creative player looking at the ground does not provoke the enderman.
- **Seal it to measure a slow goal.** Take/place needs hundreds of ticks in one place. Fill the
  mob's whole teleport cube with stone except a small room, and almost every attempt fails.
  Spawn-tick and hurt teleports land in the room only rarely.
- **Freeze the replay's mob too.** The unastar replay of the sealed room first let its enderman
  walk: it stepped onto the ring (its step height is 1.0625), and from one block higher its
  teleport cube reached the top of the stone and it escaped. BDS's enderman was frozen with
  Slowness 255. Give the replay mob the same effect (`effects::give`).
- **`--items` takes prefixes, and a forgotten one is silent.** The `anger` items never ran in
  round two because `anger` was not in the list. Diff the item count against the plan.

## How to apply

For any sweep of a mob that teleports or wanders: put it back or seal it, keep the player close
enough that it cannot despawn, check `bds.log` for "No targets matched selector", and freeze the
replay's mob exactly as the sweep froze BDS's.

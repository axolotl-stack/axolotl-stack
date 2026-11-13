---
title: Pair each actor with its own sample tick to find when BDS decides; dump every blocker, not the first
area: server
tags: [bds-sweep, measurement, melee, polar-bear, parity]
date: 2026-11-12
origin: entities round 8 (the polar bear)
status: current
---

## Context
The polar bear's stomp rules (rear up, hit, restart the cooldown) were fitted to six BDS runs
by simulating a cooldown clock over the sampled distances between the bear and the player.
With both actors read on the same sample tick, the fit missed every second hit, which came
just after the previous hit had knocked the player back. Reading both on the next tick missed
the rear-ups instead.

## What we learned
- A sweep sample at tick `t` holds the state after tick `t − 1`. A mob that moves during tick
  `t` and decides after its move is seen at sample `t + 1`. A player moved by its client (a
  knockback) is still at sample `t` when the server decides. Only bear-at-`t + 1` with
  player-at-`t` replayed five runs exactly. So BDS judged the stomp after the bear's move, and
  unastar's stomp runs after the physics step (decision 0068).
- `MobRegistry::unsupported` reports only the first failing component, and the parity `gaps`
  text for unspawnable mobs is hand-written and goes stale. A throwaway test that compiles every
  component of every group one at a time (and the events after stripping the failures) gave the
  real lists. The polar bear needed two components, not five; the tadpole two; the bat three.
- A mob frozen with Slowness 255 may never run its attack goal at all (BDS's frozen bears never
  reared). To measure reach, let the mob walk up and pin the target instead.

## How to apply
Before fitting a decision rule to tracked samples, try each actor at `t` and `t + 1`.
Self-moving mobs are usually one tick ahead of client-moved players. When picking the next
mobs to make spawnable, dump every blocker; don't trust the row text or the first error.

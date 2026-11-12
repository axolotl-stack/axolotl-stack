---
title: A sweep's cleanup must remove splitting mobs, not kill them, and BDS's AI runs after the landing tick
area: tooling
tags: [bds-sweep, slime, split, cleanup, tick order, endstone]
date: 2026-11-12
origin: entities round 6b (slime and magma cube), branch t3/7455d537
status: current
---

## Context

`sweep.py slime --items split` kills a slime on tick 10 of each item and tracks every slime for
80 ticks. The first run's large-slime items showed 7 to 18 "children" appearing on tick 1, and
the first attack run's magma cube took hits of 3 and 6 where only one size was summoned.

## What we learned

- **`kill` in a cleanup leaks into the next item.** Killing a large slime at the end of an item
  leaves its body for the 20-tick death animation; its children appear in the next item, and
  theirs in the one after. Endstone also stops listing a dying actor, so the plugin cannot see
  the body it is waiting for. The `effects` job now removes every tracked actor with
  `actor.remove()` (no death, no split) before the cleanup commands run.
- **A barrier pen can keep a big mob from ever touching the player.** The 1.04-wide slime could
  not pass between pen walls one block apart; contact damage needs only a wall behind the
  player.
- **BDS's AI runs after the physics of a tick, unastar's before.** A BDS slime counts its hop
  delay from the tick it lands (10 to 29 ground ticks for a 0.5 to 1.5 s delay); unastar's AI
  first sees the ground a tick later, so the drawn delay is shortened by one to land on the
  same distribution. Compare such timings by distribution and look for a constant one-tick
  offset before fitting anything else.

## Where it applies

Any sweep of mobs that split, spawn children or drop actors on death; any replay of a timer that
starts when a mob lands or reaches something.

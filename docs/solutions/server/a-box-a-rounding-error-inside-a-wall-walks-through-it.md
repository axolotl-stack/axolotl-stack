---
title: A box a rounding error inside a wall walks through it, and a barrier pen is climbable
area: server
tags: [physics, collision, spider, climbing, bds-sweep, measurement]
date: 2026-11-12
origin: entities lane round 6a, `sweep.py spider` (spider-normal, spider-open-normal, spider-leap-normal)
status: current
---

## Context

Replaying a BDS spider's climb up a 3-tall block with the production physics step, the spider
stopped at the wall on the first tick and walked straight through it on the second.

## What we learned

- **`feet ± width / 2` is not exact.** The clip stopped the spider with its box's min x exactly
  2.0; the feet came back as `(min + max) / 2 = 2.6999999999999997`, and the next box built from
  the feet had min x `1.9999999999999996`, inside the block. The clip only stops boxes that do
  not overlap yet, so the block no longer counted and the move went through. Any mob whose
  half-width does not round-trip can do this; nothing noticed until a mob pressed against a wall
  for many ticks. Clips now count an overlap below 1e-7 as touching (`CLIP_EPSILON` in
  `registry/collision.rs`), on every axis and for the perpendicular overlap tests too, so the
  pressed box neither passes nor gets stuck sliding along the wall.
- **Barrier pens are walls a spider climbs.** The usual sweep pen (barriers west, north and south
  of the player, open towards the mob) kept skeleton arrows' geometry fixed, but a spider
  (1.4 wide, too wide for the opening) climbed it instead of reaching the player, so the first
  `approach` run measured climbing and no hits. Spider approaches run in the open; the pen is
  kept where no hit should land (anger durations), because every swing restarts the anger.
- **Freeze, provoke and put back to measure a rare goal.** Leaps are rare while the spider
  chases (the melee goal outranks the leap and shares its controls). A frozen spider hurt every
  50 ticks and teleported back three ticks later gives clean leaps from a standing start
  (exactly 0.4 toward the target and 0.4 up) and a distance window; it still yields only one leap
  per distance, in the cycle the spider turned angry, so the roll stays provisional.

## How to apply

When a replay passes through geometry it should stop at, print the box's min/max: a value like
`1.9999999999999996` is this. For any new mob goal measured with a pen, check first whether the
mob can climb (navigation.climb) or jump the pen.

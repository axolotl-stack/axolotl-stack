---
title: Subtract the drag to find what a steering mob aims at; measure a range in clear sight
area: server
tags: [bds-sweep, measurement, flight, bat, parity]
date: 2026-11-12
origin: entities round 9 (the bat)
status: current
---

## Context
The bat's flight (`navigation.float`, `behavior.float_wander`) had to be fitted to six BDS
runs of tracked samples. Its velocity curved and sped up in bursts, which did not match the
Java bat's per-axis `signum` steering. The wake range of a resting bat was then measured with
the player behind the wall of a small stone room.

## What we learned
- Find the drag first, from the ticks with no input. Then `push = v(t) − drag × v(t − 1)` is
  the input of each tick. For the bat, every push had a length of exactly 0.1 (its movement
  speed). Two consecutive pushes meet at one point, and every later push of the same wander
  aims at that point from the feet before the move, to a thousandth of a degree. The points
  gave everything else: the draw box (their offsets from the feet), the wander length (runs
  of pushes at one point), and the rejection rule (no point ever lay inside the floor).
- The fixture keeps four decimals. That blurs a push direction by up to about a degree, so
  tests that re-derive points from the committed fixture need a looser angle (0.9995) than
  the raw samples (0.99999).
- `mob_fixture.py` used to drop every packet before an item's first mark. An item's
  `AddActor` (and a summoned mob's first flag changes) often arrive there, so the flags at
  `AddActor` were missing, and the first real change was hidden. The script now reads
  `AddActor` flags from anywhere in the capture.
- The first wake test put the player outside the room's wall: no distance woke the bat, which
  looked like a short range. In clear sight (a roof on open sides), the rule was plain: the
  player's eyes within 4 blocks of the bat's feet.

## How to apply
For a mob that steers by adding a force, solve for the drag, subtract it, and look at the
push length and direction before guessing an AI. When measuring a sensing range, keep the
line between the two actors clear unless sight is what you are testing.

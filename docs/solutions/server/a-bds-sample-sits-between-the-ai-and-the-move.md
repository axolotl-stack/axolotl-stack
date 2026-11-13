---
title: A BDS sweep sample sits between the AI and the move; keep a fast mob in view
area: server
tags: [bds-sweep, measurement, flight, vex, tadpole, parity]
date: 2026-11-13
origin: entities round 10 (the vex)
status: current
---

## Context
The vex (`sweep.py vex`) was fitted tick by tick from the generic tracked-mob job. Positions
and velocities replayed at once, with the drag subtracted as for the bat. Body yaws were a
tick off, and the order of goal and move control had to be read from the data. The round 9
decay run had shown no damage in 2,800 ticks.

## What we learned
- A tracked sample `k` shows the feet after the move of tick `k − 1`, and its velocity is
  that move. Its yaw and actor flags, though, were already set by tick `k`'s AI. So a
  sample's yaw faces the move of the *next* sample. A replay that sets the feet from sample
  `i − 1` must start the yaw from sample `i − 2`, and compare our yaw after a tick with BDS's
  sample before it.
- The shortest waits tell the order. A goal that sets a point flown from the next tick, with
  the control steering *before* the goals, allows a one-sample wait after an arrival and puts
  the first push two samples after the goal's flag. A pending point (goals first, flown a
  tick later) forbids the one-sample wait, and BDS showed three in 44.
- A hit judged on "the move that made the boxes touch" is visible as: the feet of sample `t`
  do not touch, the feet of sample `t + 1` do. Judge it after the physics step.
- The fixture's four decimals blur the direction of a slow move. Scale a yaw tolerance by
  `atan(1e-4 / horizontal speed)`.
- The vex decay run showed nothing because the vex flew more than 60 blocks off: BDS sends an
  actor only within about four chunks of a viewer, so the samples froze. Teleporting the
  player over the mob every 40 ticks (`execute at @e[...] run tp …`) kept it in view, and
  the hurts showed at once.
- For the tadpole, a per-segment regression of the vertical push against `y` found the rule
  (a slope of ±0.049 and an integer `Y`). The sign came from the pitch two samples earlier,
  and the regression was checked by counting sign agreements at lags 0 to 3.

## How to apply
Before fitting rotations or flag timings from a sweep, place each column in the tick: the
position and velocity are from the last move, while the rotation and flags come from this
tick's AI. Keep long runs of fast or noclip mobs in view by moving the player with them.

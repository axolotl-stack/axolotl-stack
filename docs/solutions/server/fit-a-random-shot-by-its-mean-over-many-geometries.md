---
title: Random mob shots are fitted by their mean over many pinned geometries, not by one capture
area: server
tags: [skeleton, ranged_attack, shooter, arrows, bds-sweep, measurement, statistics]
date: 2026-11-11
origin: entities lane round 4, `sweep.py ranged` (ent3-ranged-*, ent4-ranged-*)
status: current
---

## Context

The skeleton family's shots add a random spread to every arrow, the player gets knocked back by
every hit, and the first capture had one geometry per mob (shooter 8 blocks east, level ground).
The aim formula, the spread and the arrow damage all had to come out of that noise.

## What we learned

- **Pin the target first.** A knocked-back player changes the geometry between shots, so a
  per-shot fit mixes distances. A barrier pen west, north and south of the player (open towards
  the shooter) keeps every shot's geometry the same; then hundreds of shots can be averaged.
- **One distance fits the wrong formula.** At 8 blocks on level ground, Java's straight line
  with a `0.2 × distance` arc fit the mean climb to its standard error. Varying distance (4, 8,
  12) and height (target 3 and 6 above or below) broke it: the residual grew with the height
  difference. The drag-free parabola of the arrow's power and gravity through the centre of the
  target's box, measured from the shooter's horizontal position (one block behind where the
  arrow starts), fit every geometry with no free parameter (chi²/dof 0.44). The tell was a
  coefficient of about `h / (h + 1)` on the height term: the aim is computed from somewhere
  other than the arrow.
- **Spread: per-axis rms over the uncertainty, then look at the tails.** The gaussian and the
  triangle draw give nearly the same sd; one deviation past the triangle's bound and a kurtosis
  of 3.2 settle it.
- **Damage is interval-censored.** A hit of 4 at speed `s` only says the factor lies in
  `(3/s, 4/s]`. Maximum likelihood over those intervals per difficulty gave the mean and the
  spread; comparing a uniform and a gaussian by log-likelihood showed which shape fits better,
  and by how little (record such a fit as provisional).
- **Replay random behaviour as distributions.** The committed tests compare what is
  deterministic shot by shot (ticks, start point, owner, sounds) and the rest as statistics
  against the fixture: each geometry's mean climb within three standard errors, the spread
  within its sampling error, damage counts by chi².
- **Slowness never slowed a mob's AI.** The frozen-shooter replay walked: the AI read the
  authored `minecraft:movement`, not the movement attribute the effects modify. A replay that
  freezes a mob the way BDS's sweep does is a cheap check of such plumbing.

## How to apply

Before fitting a measured formula, plan the sweep as a design: pin what moves, then vary one
dimension at a time, with enough repeats for the noise. Keep the per-shot data in the fixture so
a different model can be refitted later without BDS.

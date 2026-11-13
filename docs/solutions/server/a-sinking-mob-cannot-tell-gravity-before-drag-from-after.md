---
title: A sinking mob cannot tell gravity-before-drag from gravity-after-drag; a pushed one can
area: server
tags: [physics, water, drowned, slime, bds-sweep, measurement]
date: 2026-11-12
origin: entities round 7 (the drowned and swim navigation)
status: current
---

## Context
unastar stepped a living body in water as `v = (v − 0.02) × 0.8`. BDS 1.26.51's idle drowned
sank 0.0866, 0.0893, 0.0914, ... a tick toward 0.1, which that step (toward 0.08) does not
reach. Switching water to `v = 0.8 v − 0.02` made the drowned replay exactly and broke two
float tests that rose too slowly.

## What we learned
- A free fall only shows the recurrence `d' = 0.8 d + c`: gravity before the drag with
  `g = 0.025` and gravity after it with `g = 0.02` give the same moves. The order only shows
  once something else pushes the body each tick.
- BDS's floating slime (slime fixture, `float` items) rises `d' = 0.8 d + 0.02`: the 0.04 float
  impulse minus 0.02, neither scaled by the drag. Only gravity after the drag fits both mobs,
  so it is the rule, and the float tests' thresholds (unmeasured) were the thing to change.
- The equipment a mob is summoned with arrives in its `AddActor` batch, which can come before
  the first `bds-sweep:mark` of its item: look it up by runtime id over the whole capture, not
  per marked item.

## How to apply
When a measured free motion disagrees with the physics, find a second measurement with an
extra known force (a jump impulse, a push, knockback) before choosing the order of terms.

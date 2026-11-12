# Design: the spider and the cave spider

## Climbing is physics

BDS spiders rise exactly 0.2 a tick against a wall, from the second blocked tick, with no
gravity, and leave the top at `(0.2 - 0.08) × 0.98` (`sweep.py spider --items wall`). The
physics step owns it: a body with `PhysicsBody::wall_climber` (set from
`navigation.climb` when the composition derives) keeps a `WallClimbing` state, "my last move
was blocked horizontally"; a blocked move while it was already set leaves the move rising at
`CLIMB_SPEED`. A system after physics mirrors the state into `WALLCLIMBING`, which BDS sets and
clears on the same ticks.

## A climber with no path walks straight

The walk search has no wall-face nodes. A climber chasing its target (the melee goal) whose walk
search fails gets a two-point path (its feet, the target's ground) and the physics climbs what is
in the way; strolls, panics and pickups still need a walk path; the alternatives (wall-face
nodes, a separate climb follower) are in decision 0061.

## Touching boxes

`feet ± width / 2` puts a spider pressed against a wall at x 2.7 a rounding error inside the
block (`1.9999999999999996`), and the clip treated it as already overlapping, so the next move
went through the wall. Clips now count an overlap below 1e-7 as touching, on every axis.

## The leap

A leap sets the velocity to 0.4 toward the target and `yd` up, replacing the travel step's
(measured from a standing start). It needs the move control, which the higher-priority melee
goal holds while it chases, so a chasing spider never leaps here; BDS spiders leapt now and then
while chasing, and that timing is a known gap.

## Filters

`is_brightness` reads the brightness ramp at the sky light of the feet block (block light is not
modelled), which puts the dusk switch where BDS's was. `is_snow_covered` uses the published
snowfall temperature and cannot be observed until riding exists.

# Design: the drowned and swim navigation

## A swim path is pulled straight

BDS's hunting drowned swam straight at its target's feet from the pool floor, 0.0135 a tick
along the line (`sweep.py drowned --items hunt`). The swim search runs best first over block
cells (water nodes, and land nodes for an amphibious mob) and the route is then pulled
straight: from each point the mob heads for the farthest node the line between the body's
centres reaches through cells without collision. The prior art's search (`503bf672`) was kept
in shape (axial steps, then diagonals of two valid axial steps) and rewritten; its node
centres at floor height match where BDS aimed.

## A swimmer has no gravity

The move control marks a swim waypoint; while the mob is in water the travel step pushes
along the pitch to the waypoint by the underwater movement attribute and the physics step
skips the water's gravity (`Swimming`). An idle drowned sinks `v = 0.8 v − 0.02`: in water the
gravity now comes after the drag, which also matches BDS's floating slime.

## One navigation config

`navigation.generic` compiles into the same `WalkNavigationConfig` as `navigation.walk`, with
`can_swim` and `can_breach`; `movement.generic` into `BasicMovementConfig`. Decision 0065.

## Transformations

A transformation runs when its delay is a plain number of seconds; it spawns the target with
the named event (`ActorReference`, shared with `spawn_entity`'s actor spawns), copies the
equipment when it preserves it and plays its sound. The zombie converted 902 ticks after its
summon in all six BDS runs.

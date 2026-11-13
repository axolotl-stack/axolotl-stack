---
status: proposed
date: 2026-11-12
decision-makers: [project owner (delegated)]
---

# Swimming mobs follow a pulled swim path with no gravity

## Context and Problem Statement

The drowned (`navigation.generic` with `can_swim` while hunting) swims. BDS 1.26.51's hunting
drowned rose straight at its target's feet from a pool floor, 0.0135 a tick along the line,
with no downward pull, while an idle one sank (`sweep.py drowned`). The land path follower and
move control only walk, and `navigation.generic` had no runtime kind.

## Decision Drivers

- Copy BDS exactly (decision 0013).
- The code-shape rules: one canonical type per concept, data before code.

## Considered Options

1. **Port the prior art's swim search as is** (`Jviguy/stab-ent-m5`, `503bf672`, 709 lines):
   a cell search whose waypoints are node centres.
2. **A cell search pulled straight, with a swim mode in the move control:** the same node
   rules, the route then pulled to the farthest node reachable in a straight line; the move
   control pushes along the pitch to the waypoint and the physics skips gravity while the mob
   swims.
3. **No search:** steer straight at the target in water.

## Decision Outcome

Chosen option: 2. Option 1's unpulled route would zigzag through cell centres where BDS swam a
straight line; option 3 cannot route around walls. `navigation.generic` compiles into the
same `WalkNavigationConfig` as `navigation.walk` (one navigation config), `movement.generic`
into `BasicMovementConfig`.

### Consequences

- Good, because the drowned's swim replays BDS's line and speed.
- Good, because any behaviour pack navigation that can swim gets the same control.
- Bad, because how BDS re-aims near the surface, and its climbing out onto a block beside the
  water, are not modelled yet.

### Confirmation

`crates/unastar/src/entity/mob/tests/drowned.rs`: `a_hunting_drowned_swims_straight_at_a_target_above_it`,
`a_hunting_drowned_walks_the_pool_floor_at_bds_speed`, `an_idle_drowned_sinks_like_bds` against
`mob/ai/testdata/bds-1.26.51-drowned.json`.

## More Information

- Copied quirks: the Entities / drowned rows of `docs/quirks/bds.md`.
- Numbered 0065 to avoid collisions with records on open branches.

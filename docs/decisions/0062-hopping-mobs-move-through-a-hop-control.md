---
status: proposed
date: 2026-11-12
decision-makers: [project owner (delegated)]
---

# Hopping mobs move through a hop control, not the path follower

## Context and Problem Statement

The slime and the magma cube (`minecraft:movement.jump`) move only by hopping: they wait on the
ground, jump along their body yaw and stop when they land. Their four goals
(`slime_float`, `slime_attack`, `slime_random_direction`, `slime_keep_on_jumping`) never path.
Every other mob in unastar moves by goals handing waypoints to the path follower and the land
move control. The slimes need a move model that reproduces BDS 1.26.51's hops, measured with
`sweep.py slime`.

## Decision Drivers

- Copy BDS exactly (decision 0013): BDS's hop matched the ordinary living travel step to the
  thousandth of a block, waited the authored delay cut to whole ground ticks, and turned only on
  the ground.
- The code-shape rules: one canonical type per concept, data before code.

## Considered Options

1. **Path follower with a jump flag:** the goals pick random targets and path to them; the move
   control jumps whenever it moves (the shape of the unmerged PR #71 prior art, `0abaa4d1`).
2. **A hop control:** the goals write the yaw they want and whether to move into a `HopState`;
   a control turns, counts the hop delay down on the ground and jumps; travel stays the
   ordinary living step.

## Decision Outcome

Chosen option: 2, a hop control (`mob::ai::hop`). A mob carrying `minecraft:movement.jump` takes
that branch of the AI tick instead of the path follower. The class facts that are not data (the
magma cube's jump growing with size, splitting on death, the `CLIENT_EVENT` hop reports) are one
native trait, `NativeTraits::slime`. A split child's size group is found in the definition (the
group whose `minecraft:variant` is half the parent's), not in a table of group names.

### Consequences

- Good, because the measured hop physics, delays and turning replay without special cases in
  the path follower.
- Good, because a behaviour pack giving another mob `movement.jump` gets the same control.
- Bad, because a second movement branch in the AI tick has to be kept in step with the first
  (travel, jump power and knockback are shared).

### Confirmation

`crates/unastar/src/entity/mob/tests/slime.rs`: `a_hop_moves_like_bds_tick_by_tick` and
`hop_delays_and_random_turns_match_bds_by_distribution` against
`mob/ai/testdata/bds-1.26.51-slime.json`.

## Pros and Cons of the Options

### Path follower with a jump flag

- Good, because it reuses the path follower.
- Bad, because it moves the mob between hops and turns it in the air, which BDS does not do,
  and pathing cost would be spent on mobs that never follow a path.

## More Information

- Copied quirks: the Entities / slime rows of `docs/quirks/bds.md`.
- Numbered 0062 to avoid collisions with records on open branches.

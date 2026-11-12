---
status: proposed
date: 2026-11-12
decision-makers: [lane BLOCKS]
---

# Move actors with measured piston pushes, not moving-block collision

## Context and Problem Statement

Pistons moved blocks ([0050](0050-move-pistons-from-the-circuit-update.md)) but no actors. BDS
1.26.51, measured tick by tick with the piston sweeps (`pistons_entities.json`, actors read every
tick), pushes an actor in front of the head to the head's half-way face and then its full face,
moves nothing in front of a pushed block in the first tick and the whole way in the second,
carries a rider half a block per carrier under it in the first tick of a sideways move, and holds
nothing up with a moving block. The question was where those moves come from.

## Considered Options

- Moving blocks as colliders in actor physics: give `moving_block` a box that follows the move's
  progress and let collision push actors out (the Java model).
- Explicit pushes: each piston move gives, per tick, the boxes that push and the boxes that
  carry; a system after the circuit step moves the actors by the measured rules.

## Decision Outcome

Chosen option: explicit pushes (`server/game/redstone/piston_actors.rs`), because the measured
moves do not follow one collider: the head and the moved blocks push in different ticks, a rider
over two carriers moves twice, and a moving block both carries a rider and fails to hold it up.
Collision would need each of these as an exception; as pushes they are three rules in one place
and the replay compares them with every measured actor.

### Consequences

- Good, because `redstone/tests.rs::pistons_move_actors_like_bds` replays every measured actor
  per tick, and a new measurement becomes a rule change in one module.
- Good, because actor physics stays unaware of pistons.
- Bad, because players are not moved (their client is authoritative; pushing them needs a motion
  packet, not measured).
- Bad, because cases the sweep did not try (several pistons moving one actor in one tick, riders
  in the second tick) follow the rules as written, unmeasured.

### Confirmation

`cargo test -p unastar --lib pistons_move_actors` replays every piston row with actors.

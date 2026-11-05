---
status: accepted
date: 2026-02-13
decision-makers: [project owner]
---

# Game state lives in ECS resources; the plugin manager stays outside the World

## Context and Problem Statement

The server mixed ECS and non-ECS state, routed packets through a queue nothing drained, and looked
up players by string UUID in O(N).

## Considered Options

- Keep the mixed model
- All game state in bevy_ecs Resources and components, per-domain packet queues, and the plugin
  manager as an ECS system
- The same, but with the plugin manager outside the World, ticked with `&mut World`

## Decision Outcome

Chosen option: ECS resources and per-domain packet queues with the plugin manager outside the World,
because disjoint Resources let bevy schedule systems in parallel, while Wasmtime stores hold a raw
World pointer and are not genuinely `Send`. Plugin actions address players by `Entity`.

### Consequences

- Good, because packet handling parallelises by domain (capped at 64 packets per session per tick).
- Bad, because plugin dispatch is a serial phase the runtime must call explicitly.

## More Information

Code: `crates/unastar/src/server/game/packet_queues.rs`, `src/ecs/schedules.rs`, `src/ecs/events.rs`.
Lesson: `docs/solutions/server/plugin-manager-runs-outside-the-ecs-schedule.md`.

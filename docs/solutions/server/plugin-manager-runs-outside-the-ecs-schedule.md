---
title: The plugin manager runs after the ECS schedule with the whole World, not as a system
area: server
tags: [ecs, bevy, plugins, scheduling, parallelism, packets]
date: 2026-02-13
origin: thoughts/shared/plans/2026-02-13-ecs-architecture-migration.md (removed; see git history)
status: current
---

## Context
The February 2026 ECS migration moved all game state into Resources and split packet
handling into per-domain queues drained by systems in `PacketApplySet`, so bevy's
multi-threaded executor can run movement, inventory, chunk requests and chat in parallel.
The plugin manager was deliberately left out of the World and out of the schedule.

## What we learned
- Plugin stores cannot be ordinary Resources. Each store's host context carries a raw
  `*mut World` that is attached only for the duration of one synchronous Wasm call and
  cleared on return or unwind; it is `Send` only by an `unsafe impl` that relies on the
  World being touched from its owning thread. Making the manager a system would need a
  non-`Send` resource and an exclusive system, which buys nothing over calling it
  directly.
- So the runtime calls `GameServer::tick()` (the whole schedule) and then
  `PluginManager::tick(world_mut)` on the game thread, once per tick. Plugins see the
  World after this tick's systems ran; their mutations go through `PluginAction`
  (keyed by `Entity`, not player UUID strings, which made each action an O(N) scan) and
  apply in the next schedule run.
- Trap from before the migration: packets were cloned into a routing queue resource that
  no system drained, an unbounded leak per packet. Every queue Resource needs a draining
  system in the same tick; the domain queues are emptied by their systems and domain
  events are folded into `EventBuffer` by `consolidate_domain_events`. Inbound packets are
  also capped at 64 per session per tick before they reach the queues.
- Parallelism comes from disjoint Resources: each domain has its own queue and event
  Resource so bevy's access checks let those systems overlap. Block, mob-interaction,
  item-use and command systems are exclusive and ordered after the systems whose
  results they read. The split queues lose a player's packet order, so command requests
  that arrive before their player's inputs run first (see
  [per-domain-queues-lose-a-players-packet-order.md](per-domain-queues-lose-a-players-packet-order.md)).

## Where it lives
- `crates/unastar/src/server/runtime.rs` (tick loop, `MAX_PACKET_EVENTS_PER_SESSION_PER_TICK`)
- `crates/unastar/src/server/game/mod.rs` (`GameServer`, `PacketApplySet` registration)
- `crates/unastar/src/server/game/packet_queues.rs` (domain queues, `consolidate_domain_events`)
- `crates/unastar/src/plugin/manager.rs` (`HostContext`, `with_world`)
- `crates/unastar/src/ecs/events.rs` (`PluginAction`)

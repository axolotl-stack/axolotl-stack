---
status: proposed
date: 2026-11-11
decision-makers: [project owner]
supersedes: "0036"            # once built; until then 0036 stays in force
---

# Apply client packets in BDS's network pass and player pass

## Context and Problem Statement

PvP has to come out exactly as on BDS 1.26.51 whenever the same packets arrive in the same
order. unastar applies a player's packets grouped by domain (movement, inventory, chat and
commands, ...), one system per domain, so the order within a tick is lost.
[0036](0036-run-a-command-before-its-players-later-inputs.md) patched the common
command-then-input case and left the rest to this follow-up.

The PvP race captures (`tools/bds-sweep/pvp_race.py`; fixtures
`crates/unastar/src/server/game/testdata/pvp-race-*.jsonl`) show that BDS does not use arrival
order either. Each update has a **network pass**: commands, chat and swings apply at once,
`PlayerAuthInput` is queued, and an attack or an item use in the air is held until that
player's next input arrives. Then a **player pass** consumes each player's queued inputs, and
a held action resolves with the player's state from before its input. Players take their turn
in reverse join order, the latest joiner first, not in arrival order. Then one flush. Details
and numbers:
[openspec/changes/ordered-packet-apply/design.md](../../openspec/changes/ordered-packet-apply/design.md).

## Decision Drivers

- The owner: "maintain vanilla ordering and ensure there are no multi-threaded issues; if
  Bevy systems can do the ordering, best."
- Every case of the PvP race matrix must replay to BDS's outcome: sprint and crit state at
  hit time, mutual hits, kill races, two attackers, lag bursts.
- Deterministic: the same packets in the same order give the same result, with no data race
  between parallel systems.
- Contained cost: the packet stage must stay well inside the 50 ms tick.

## Considered Options

1. **Two ordered passes in one exclusive system.** Routing appends each packet to an ordered
   per-tick log. One exclusive system runs the network pass (each session's packets in
   arrival order: apply on arrival, queue the input, or hold the action) and then the player
   pass (players latest joiner first; each player's queued inputs, held actions before their
   input). Domain handlers become
   functions over `&mut World`. Everything that does not consume client packets stays
   parallel.
2. **Arrival order.** One ordered log applied packet by packet as it arrived, across all
   players.
3. **Per-player segments with chained system sets.** Split each player's packets into
   segments and run the domain systems once per segment.
4. **Keep 0036** (per-domain queues with early and late command slots).

## Decision Outcome

Proposed: option 1, two ordered passes in one exclusive system, because it is the only option
that reproduces the measured order. It is deterministic by construction: one system, one
thread, a fixed order. Today's packet systems are already exclusive except movement, chunk
requests and chat parsing, so little parallelism is lost (benchmark in the design).

### Consequences

- Good, because every race case replays to BDS's result: attacks resolve with their player's
  next input and see that player's state from before it; commands sent after an attack are in
  place for it; a dead player's held attack is dropped; a lag burst resolves within one tick.
- Good, because the order no longer depends on system scheduling, so adding a domain system
  cannot reorder packets.
- Bad, because the domain systems' packet draining becomes handler functions called from one
  system. That touches the files the movement, items, blocks and entities lanes are changing
  now, so it is scheduled after they merge.
- Bad, because an attacker that stops sending inputs never hits until it sends one. That
  copies BDS, and the quirk is logged.

### Confirmation

`crates/unastar/src/server/game/packet_order_tests.rs` replays the captured client packets
through the model and checks five predictions against BDS: when each attack resolves, sprint
state, crit timing in both orders, facing under strict interactions, and the order across
players in the pair races. When the change is
built, the end-to-end replays of every `pvp-race-*` fixture through `GameServer::route_packet`
(ignored until then) must pass, together with `command_order_tests.rs`.

## Pros and Cons of the Options

### Two ordered passes in one exclusive system

- Good, because it matches every capture, including order inside a batch and across batches.
- Good, because one system has no ordering races.
- Bad, because the packet stage runs on one thread, as most of it already does.

### Arrival order

- Good, because it is simple, and it is what 0036 assumed BDS does.
- Bad, because the captures refute it. An attack sent before a sprint stop must use the
  sprint, and arrival order would apply the stop first. A command sent after an attack must be
  in place for the hit, and arrival order would apply the hit first. Two players' attacks in
  one tick resolve in the measured player order, not in arrival order.

### Per-player segments with chained sets

- Good, because the domain systems stay as systems.
- Bad, because every domain system runs once per segment, so the schedule depends on the
  traffic, and the systems must be re-entrant within a tick.
- Bad, because a held attack still has to be ordered against another player's input. Segments
  per player cannot express that without a global order, which is option 1.

### Keep 0036

- Good, because nothing changes.
- Bad, because the race matrix fails: attacks resolve with today's movement system rather
  than with the attacker's next input, and the order across players depends on the system
  that drains each queue.

## More Information

- Supersedes [0036](0036-run-a-command-before-its-players-later-inputs.md) when built:
  `CommandOrder` and `run_early_commands` are removed, because commands apply in the network
  pass.
- The PvP lane (decision 0046) provides the single-hit function the player pass calls.
- Outbound timing (a hit's sound one tick after its hurt, actor data two ticks after) is
  measured in the design and left to a separate send-side change.

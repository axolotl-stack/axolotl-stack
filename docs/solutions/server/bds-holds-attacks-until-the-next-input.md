---
title: BDS does not apply packets in arrival order; attacks wait for the attacker's next input
area: server
tags: [packets, ordering, pvp, combat, PlayerAuthInput, InventoryTransaction, commands, scheduling]
date: 2026-11-11
origin: docs(order) PvP race captures (decision 0045, proposed)
status: current
---

## Context
Decision 0036 assumed BDS applies each client's packets in arrival order and fixed the
command-then-input case. The follow-up measured PvP races on BDS 1.26.51 with three
scripted players in one process (`tools/bds-sweep/pvp_race.py`), so the order of packets
inside a tick and their batching were under control.

## What we learned
- "Arrival order" is wrong for anything that moves or hits. One server update is: a
  network pass (commands, chat, swings, opening the inventory take effect at once; each
  `PlayerAuthInput` is queued; an attack or an item use in the air is held until that
  player's next input arrives), then a player pass (each player's queued inputs, oldest
  first; held actions resolve before their input's flags, rotation and movement), then
  one flush.
- So an attack uses the attacker's state from after the inputs sent before it and before
  the input sent after it: sprint knockback, critical hits and (under
  `server-authoritative-entity-interactions-strict`) facing all follow that, both orders
  and both batchings, 10/10. An attacker that stops sending inputs never hits until it
  sends one (+7 ticks when silent for 5).
- A command or chat line sent after an attack still runs first: `/effect resistance`
  sent after the attack, by either player, was in place for the hit, and a chat echo
  sent between an attack and its input goes out before the hurt in the same batch.
- Within one pass, players do not resolve in arrival order across connections either: the
  latest joiner goes first (251 of 280 races over four join orders). A victim who steps out
  of reach in the same tick escapes even when the step arrives after the attack, because
  the attack waits for the attacker's next input.
- When something looks like a per-session random order, test every pair under several join
  orders before concluding: two early readings (a two-attacker knockback, a noisy session)
  pointed the wrong way, and only the pair races (decided by who dies) were clean.
- A burst of queued inputs (lag) is consumed in one update, so every attack in it
  resolves in the same server tick and the hurt window (10 server ticks) blocks the rest.
- Measuring order needs observables that are stamped, not read back: BDS stamps
  `SetActorMotion`, `UpdateAttributes`, `SetActorData` and corrections with the target's
  last applied input tick. Reading state back through commands misleads:
  `/querytarget` is refused for operators, `rotated as @s` reads a smoothed yaw, and a
  scripted client that claims a stationary position while walking skews positions.
- BDS stalls for 1-1.5 s now and then (an autosave, likely); a trial that spans a stall
  must be excluded, not read as "no hit" (`pvp_race_summary.py` marks them STALLED).

## Where it lives
- Captures and tools: `tools/bds-sweep/pvp_race.py`, `pvp_race_scripts.py`,
  `pvp_race_summary.py`, `pvp_race_fixture.py`; probe `crates/bds-sweep/src/bin/order_probe.rs`
- Fixtures: `crates/unastar/src/server/game/testdata/pvp-race-*.jsonl`
- Model and tests: `crates/unastar/src/server/game/packet_order.rs` (+ `_tests.rs`, `_bench.rs`)
- Design: `openspec/changes/ordered-packet-apply/`, decision 0045 (proposed)
- Supersedes the "still not exact" analysis in
  [per-domain-queues-lose-a-players-packet-order.md](per-domain-queues-lose-a-players-packet-order.md)
  once 0045 is built.

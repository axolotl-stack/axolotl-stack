---
title: Lockstep clients cannot pick BDS's tick by timing; mark each input and read it back
area: tooling
tags: [bds-sweep, lockstep, pvp, tick, ordering, endstone, capture]
date: 2026-11-11
origin: commit "feat(bds-sweep): lockstep clients and the pvp sweep"
status: current
---

## Context

The PvP race matrix needs several clients whose packets reach BDS in a chosen server tick and
order. `bds-sweep-client --also` runs all clients in one process and the sweep plugin sends a
`bds-sweep:tick` line every server tick naming what each client sends next. The first plan was
to send each line's packets "just after a tick" so they land in the next one.

## What we learned

- BDS handles client packets late in each tick, after the plugin's scheduled task. A tick line
  sent from that task and answered at once was handled in the same tick for 124 of 158 lines and
  one tick later for the rest. Waiting 10 ms first changed nothing useful (116 of 158), and
  waiting 20 ms (which Windows rounds up to about 30) put two lines into one tick whenever BDS
  ran a short tick: under Endstone, ticks were 24 to 90 ms apart.
- Waiting for the next `MovePlayer` relay of another player (BDS relays an input as it consumes
  it) failed too: on 135 of 636 lines no relay came within 45 ms, the coordinator fell behind,
  and lines bunched.
- BDS sometimes consumes two queued inputs of one player in one tick, or none (165 ticks with
  one, 29 with two, 40 with none in one run), so "one line, one tick" never holds.
- What works: send at once, and measure. Every line turns each client 0.01 degrees further, and
  BDS takes rotation from each input it consumes, so the yaw the plugin samples each tick tells
  which line's input BDS consumed last. Endstone's `PacketReceiveEvent`, `PacketSendEvent`,
  `ActorDamageEvent` and `ActorKnockbackEvent` share one sequence counter with the samples, so
  their order within a tick is recorded too. The distiller (`pvp_fixture.py`) then gives every
  attack its arrival tick (the attacker's inventory transactions in order), its application
  tick (its damage event, or its input's consumption mark) and BDS's order within the tick.
- Endstone's sampled state lags: `is_sprinting` changed a tick after the sprint did, and
  `health` is an integer. Take the sprint from the client's own input (`Sprinting` of the input
  before the one the attack joins, found by its yaw mark in the client capture) and health from
  the `UpdateAttributes` packets.
- Endstone's knockback event reports the velocity change, not the push: for a resting player
  (0, 0.4784, 0) is the 0.4 cap minus the -0.0784 it had. The velocity BDS had before a single
  push is the `SetActorMotion` it sent less that change.
- Plugin-set items never reach the client, whose attack then names a stale item and is refused
  (the attack sweep's "warm-up" trial). An inventory-mismatch transaction (`bds-sweep:resync`)
  makes BDS send the whole inventory.
- BDS refuses a `/damage` right after a player respawns, so a setup that damages a just-killed
  player silently fails: record each player's health at the start of the trial and replay from
  that.
- `max-players` must cover every client (the harness default is 2).

## Where it lives

`crates/bds-sweep/src/lockstep.rs`, `tools/bds-sweep/endstone-sweep/src/endstone_bds_sweep/`
(`plugin.py`: `send_lockstep`, the packet events; `jobs.py`: `pvp`), `tools/bds-sweep/pvp_items.py`,
`tools/bds-sweep/pvp_fixture.py`.

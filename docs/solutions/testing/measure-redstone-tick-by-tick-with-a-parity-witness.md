---
title: Measure redstone tick by tick with per-tick traces and a parity witness, and replay the traces exactly
area: testing
tags: [redstone, bds-sweep, scenarios, timeline, witness, replay, circuits, timing]
date: 2026-11-10
origin: lane BLOCKS round 3 (feat(blocks) commit adding server/game/redstone/)
status: current
---

## Context

Redstone timing could not be guessed from Java: BDS works the circuit every other game tick, and
the same circuit gives traces one tick apart depending on when the input changed. Reading the end
state after a wait (round 1's scenarios) hides all of it. The work needed a per-tick record of
every block of a circuit and a replay that lands on the same ticks.

## What we learned

- Add a `timeline` step to the `scenarios` job: apply the events due at each tick, then read every
  watched position, then `yield 1`; never `yield None` inside it (that can let the work slip into
  the next tick). The plugin task runs at the start of a server tick, so read `t` shows the world
  after tick `t - 1`; a set at read `t` is a change at the start of tick `t`.
- Every wave starts at an unknown tick parity. A witness (a redstone block set at timeline tick 0
  next to a dust outside the cells) lights at read 2 or 3; updates show at the reads with that
  parity. Record it in every row and start the replay's tick counter on the same parity.
- Inputs: a redstone block set and removed is the clean switch. `set_data` on an existing lever
  does not change its power (the circuit keeps the old output); real clicks (`use` events, the
  player teleported beside the cell first) reach BDS a tick or two after the order, so the replay
  applies each switch change at the tick the trace shows it, a button's release a tick earlier.
- Water plants set by Endstone (`set_data` fills them with water) flood the cells beside them in
  the same wave; leave them out of per-block families (`circuits.WATERY`).
- Many per-block rows test something else (a block popping, dying or taking a default state on
  its own). Compare only the redstone positions, take BDS's stored state for blocks it normalises
  (a chest set without states faces north), and skip rows with a stated reason.
- Several timings had to be read off the traces rather than derived: dust shows its level an
  update after a lamp behind it lights; a newly placed repeater waits an update; a container
  change reaches a comparator an update late; block updates caused by a circuit change (dust
  popping off a trapdoor it opened) run in the next tick. Each is a quirk row.
- Derived tables (conductors, cuts, climbing) need care: judge "conductor" by the lamp lighting
  in the same update as the repeater (a sculk sensor lights it later on its own) and "cut" by the
  dust staying dark in the update the dust below lights (a door set there pops later and lets
  the power climb).
- A few observer cells pulsed a tick early with nothing to explain it; keep them as named skips
  rather than bending the model.

## Where it lives

`tools/bds-sweep/circuits.py`, `endstone-sweep/.../jobs.py` (`timeline`, `run_event`, witness),
`tools/bds-sweep/summarize.py` (`summarize_circuits`, `circuit_blocks`),
`crates/unastar-data/data/measured/redstone/`, `crates/unastar/src/server/game/redstone/tests.rs`.

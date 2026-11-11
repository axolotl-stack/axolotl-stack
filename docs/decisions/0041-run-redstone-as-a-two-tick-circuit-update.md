---
status: accepted
date: 2026-11-10
decision-makers: [lane BLOCKS]
---

# Run redstone as one circuit update every other tick, in BDS's phases

## Context and Problem Statement

unastar had no redstone. BDS 1.26.51 redstone, measured tick by tick with the circuit sweeps
(`tools/bds-sweep/circuits.py`, `crates/unastar-data/data/measured/redstone/`), does not behave like
Java Edition: the circuit changes only every other game tick, a change made in a tick is first seen
by the update of a later tick, dust shows its level one update after a lamp behind it has lit,
every repeater, comparator, torch or observer in a chain adds an update, and repeaters and lamps
time in updates. The question was how to structure the engine so that it can reproduce those
traces exactly ([0013](0013-copy-bds-behaviour-exactly.md)).

## Considered Options

- Java-style block updates: each component reacts to its neighbours' changes in the same tick
  (dust recomputed through neighbour updates), delays as scheduled block ticks in game ticks.
- One circuit update every other game tick over the tracked components, in phases: dust levels
  from the outputs as they stand (and shown on the dust), then new repeater, comparator, torch and
  observer outputs, then the blocks power acts on, with outside changes of the current tick hidden
  from the update.

## Decision Outcome

Accepted by the owner on 2026-11-11.

Chosen option: the phased circuit update every other tick (`server/game/redstone/`), because the
measured traces are reproducible with it and not with the other option: a lamp behind a repeater
and a dust lights in the update the repeater turns on while the dust shows the level an update
later; a redstone block set at tick t lights dust at t + 2 or t + 3 depending on the tick's parity;
a repeater set to the shortest delay switches in the update its input changes and each further
diode adds one. Java-style updates settle the whole chain in one tick and cannot produce these
offsets without per-case corrections.

### Consequences

- Good, because the replay tests compare every measured circuit with BDS tick by tick
  (`server/game/redstone/tests.rs`), and new BDS timing facts become phase or latency rules in
  one place.
- Good, because outputs only the circuit knows (comparator strength, observer pulses seen by the
  circuit, repeater schedules, burnt-out torches) live in one resource, not in block actors.
- Bad, because each update visits every tracked component (cost grows with circuit size, not with
  what changed), and components in chunks loaded from disk join the circuit only when a block next
  to them changes.
- Bad, because some BDS timings are copied as measured latencies without a model behind them
  (observers pulse at the first tick between updates two or more ticks after the change they see,
  a newly placed repeater waits one update, a container change reaches a comparator an update
  late, a block update caused by the circuit runs in the next tick).

### Confirmation

`cargo test -p unastar --lib redstone` replays every row of the `redstone_*` circuit files; a
change that breaks the timing fails those replays. Copied oddities are rows in
`docs/quirks/bds.md` (area "Redstone").

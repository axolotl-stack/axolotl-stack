---
status: proposed
date: 2026-11-11
decision-makers: [lane BLOCKS]
---

# Move pistons from the circuit update, with measured per-block movement

## Context and Problem Statement

unastar's pistons did nothing. BDS 1.26.51 pistons, measured tick by tick with the piston sweeps
(`tools/bds-sweep/pistons.py`, `crates/unastar-data/data/measured/pistons/`), read their power at
the same point of the every-other-tick circuit update as lamps ([0041](0041-run-redstone-as-a-two-tick-circuit-update.md)),
move a tick later, ignore their power while moving, and treat each block in one of four ways
(moves, moves only away, breaks, holds). The questions were where a piston's move belongs in the
engine and where the per-block movement comes from. Older piston code on the unmerged #71
(`a8732b6c`) ran pistons as block ticks with hand-written block lists taken from public behaviour.

## Considered Options

- Pistons as scheduled block ticks reacting to neighbour updates, with hand-written lists of
  movable, breaking and immovable blocks (the #71 design).
- Pistons as a consumer of the circuit update (power read in the update's consumer phase, the move
  made a tick later by the circuit's own scheduler), with the block movement read from a table
  measured on BDS for every block.

## Decision Outcome

Chosen option: the circuit consumer with a measured block table (`server/game/redstone/pistons.rs`,
`unastar_data::pistons`), because the measured timings line up with the circuit update without
extra latencies (a piston behind a repeater moves a tick after the repeater turns on, dust beside
it a tick after the dust lights), the circuit already hides same-tick changes from the update and
runs the block updates of its own changes a tick later (a torch on a slime block pops a tick after
the slime leaves), and the measured table corrects the hand-written lists (chests move with their
contents, beds and shulker boxes break, grindstones and lodestones hold).

### Consequences

- Good, because every measured piston row replays tick by tick (`redstone/tests.rs` piston
  families) and the client packets are compared with a BDS capture (`redstone/piston_packets.rs`).
- Good, because a block's movement is data: a new BDS version is re-measured with one sweep and
  `summarize.py round4`.
- Bad, because blocks the sweep cannot stand in a cell (water plants, blocks whose default state
  falls off) get no measurement and default to breaking.
- Bad, because pistons now depend on the circuit resource: a piston only moves where the circuit
  has found it (the first change next to it), like every other circuit component.

### Confirmation

`cargo test -p unastar --lib redstone` replays every piston row; a change to the timing, the push
limit or the block table fails those replays.

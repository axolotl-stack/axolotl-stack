# Proposal: pistons measured on BDS

## Why

Pistons and sticky pistons did nothing in unastar (`world.pistons` missing; the piston, sticky
piston, head and moving block rows partial). Round 3's redstone engine
([decision 0041](../../../../docs/decisions/0041-run-redstone-as-a-two-tick-circuit-update.md)) skipped
every circuit that held a piston.

Prior work: the redstone timing notes in
[measure-redstone-tick-by-tick-with-a-parity-witness](../../../../docs/solutions/testing/measure-redstone-tick-by-tick-with-a-parity-witness.md)
and the "Redstone / ..." rows of [docs/quirks/bds.md](../../../../docs/quirks/bds.md). The unmerged
#71 has piston code (`a8732b6c`) with hand-written block lists; it is used as prior art only and
re-checked against new BDS measurements.

## What Changes

- Measure pistons on BDS 1.26.51 with new `scenarios` families (`tools/bds-sweep/pistons.py`):
  push lines, every block's default state in front of a sticky piston, slime and honey, power,
  timing, chains and actors; and a `placements` case per piston kind for the packets a client is
  sent.
- `unastar_data::pistons`: the measured movement of every block.
- `server/game/redstone/pistons.rs`: pistons read their power in the circuit update, move a tick
  later, land the moved blocks two ticks after that, move block actors with the blocks and send
  BDS's `PistonArm` and `MovingBlock` actors and sounds.
- A block change reaches each viewer once (it was sent twice).

Affected crates: `unastar`, `unastar-data`; harness `tools/bds-sweep`.

## Capabilities

### New Capabilities

None. This change is vanilla behaviour, tracked in `docs/parity/`, not our own contracts, so it
has no delta specs (`skip_specs: true`).

### Modified Capabilities

None.

## Impact

### Parity rows

- systems: `world.pistons` missing -> partial (actors are not pushed yet); `world.redstone` gaps.
- blocks: `minecraft:piston`, `minecraft:sticky_piston`, `minecraft:piston_arm_collision`,
  `minecraft:sticky_piston_arm_collision`, `minecraft:moving_block` partial -> verified;
  `minecraft:slime`, `minecraft:honey_block` notes.

These rows are already claimed by lane BLOCKS in `docs/parity/claims.toml`; no new claim.

### Out of scope

Actors pushed by the head or carried by moving blocks (measured in `pistons_entities.json`, not
implemented), batching block changes into `UpdateSubChunkBlocks`, and liquids moved by pistons
(waterlogged blocks keep their stored water layer; the liquids lane owns flow).

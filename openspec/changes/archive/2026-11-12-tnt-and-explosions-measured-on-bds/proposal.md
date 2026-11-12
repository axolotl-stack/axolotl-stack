# Proposal: TNT and explosions measured on BDS

## Why

TNT could not be primed and nothing exploded (`world.explosions` missing, `minecraft:tnt`
partial, the primed TNT actor not spawnable: `minecraft:explode` was not implemented).

Prior work: the circuit model of [decision 0041](../../../../docs/decisions/0041-run-redstone-as-a-two-tick-circuit-update.md)
and the measuring notes in `docs/solutions/testing/`. The unmerged #71 has explosion and fuse code
(`fb1291c3` creeper `explode`, `f022f305` TNT, `c509be4d` batched sends); it is used as prior art,
re-checked against new BDS measurements.

## What Changes

- Measure TNT on BDS 1.26.51 (`tools/bds-sweep/tnt.py`): priming and the primed actor, 64 blasts
  per material, mobs held still at known spots; a `placements` case for the packets.
- `server/game/explosion.rs`: rays and block resistance, drops, damage with exposure, the
  explosion packets; `minecraft:explode` (fuse) for actors; power primes TNT.

Affected crates: `unastar`, `unastar-data`; harness `tools/bds-sweep`.

## Capabilities

### New Capabilities

None. This change is vanilla behaviour, tracked in `docs/parity/`, not our own contracts, so it
has no delta specs (`skip_specs: true`).

### Modified Capabilities

None.

## Impact

### Parity rows

- systems: `world.explosions` missing -> partial.
- blocks: `minecraft:tnt` partial -> verified.
- entities: `minecraft:tnt/explode` missing -> verified; `minecraft:tnt` missing -> partial.

The blocks and systems rows are claimed by lane BLOCKS; the entities TNT rows are in no claim.

### Out of scope

Knockback (not measured), fire from explosions, creepers and other exploding mobs (entities
lane), flint and steel and dispensers priming TNT, sending block changes with the next tick.

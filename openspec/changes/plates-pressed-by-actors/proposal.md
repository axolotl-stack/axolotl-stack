# Proposal: pressure plates pressed by actors

## Why

Pressure plates powered circuits only while their stored state said so: nothing pressed them
(17 pressure plate rows partial, `world.redstone` gap). Prior work: the circuit model of
[decision 0041](../../../docs/decisions/0041-run-redstone-as-a-two-tick-circuit-update.md) and the
round 3 pilot (`circuits_pilot3` plates).

## What Changes

- Measure plates on BDS 1.26.51 (`tools/bds-sweep/plates.py`): every plate kind under armour
  stands, pigs held still, items and stacks; the weighted plates under 1-64 items; tripwires
  (measured, not implemented yet). The actors in the cell are read every tick.
- `server/game/redstone/plates.rs`: what each plate counts, when it presses and releases; a system
  presses plates before the circuit steps. A pressed wooden or stone plate powers 15.

Affected crates: `unastar`; harness `tools/bds-sweep`.

## Capabilities

### New Capabilities

None. This change is vanilla behaviour, tracked in `docs/parity/`, not our own contracts, so it
has no delta specs (`skip_specs: true`).

### Modified Capabilities

None.

## Impact

### Parity rows

- blocks: the 17 `*_pressure_plate` rows partial -> verified.
- systems: `world.redstone` gaps.

Claimed by lane BLOCKS; no new claim.

### Out of scope

Tripwires (measured in `plates_tripwire.json`), arrows and minecarts on plates.

# Proposal: tripwires and actors moved by pistons

## Why

String between tripwire hooks did nothing (`trip_wire` and `tripwire_hook` partial), and pistons
moved blocks but no actors (`world.pistons` gap). Both were measured on BDS 1.26.51 in round 4
(`plates_tripwire.json`, `pistons_entities.json`) and not implemented. Prior work: the plate
engine (`redstone/plates.rs`) and the piston moves of
[decision 0050](../../../docs/decisions/0050-move-pistons-from-the-circuit-update.md).

## What Changes

- Tripwires (`server/game/redstone/tripwire.rs`): a placed hook or string that completes a line
  attaches it; string links to the hooks facing it; string is pressed like a plate (kind
  `Tripwire`: every actor, a check every 10 ticks, `powered_bit`); a line's hooks follow its
  string in the same tick.
- Actors moved by pistons (`server/game/redstone/piston_actors.rs`): per move tick, the head
  pushes, the landed blocks push in the second tick, and sideways moves carry riders once per
  carrier in the first tick. A system after the circuit step moves every actor but players.
- New measurements (`tools/bds-sweep`): `plates_tripwire_more` (string height, hooks placed last,
  no hooks, one hook, stepping on a cut line), `pistons_riding` (riders over the head, one or two
  blocks, pulled back, lifted) and `infested_break` (a survival player breaking infested blocks;
  the scenarios job gains the player's `mine` and `stop` events).

Affected crates: `unastar`; harness `tools/bds-sweep`.

## Capabilities

### New Capabilities

None. This change is vanilla behaviour, tracked in `docs/parity/`, not our own contracts, so it
has no delta specs (`skip_specs: true`).

### Modified Capabilities

None.

## Impact

### Parity rows

- blocks: `trip_wire`, `tripwire_hook` partial -> verified; `piston`, `sticky_piston` gaps.
- systems: `world.pistons`, `world.redstone` gaps.

Claimed by lane BLOCKS; no new claim.

### Out of scope

Players pushed by pistons (the client moves them), shears and players breaking string, the
longest line a hook finds, batching block changes into `UpdateSubChunkBlocks`.

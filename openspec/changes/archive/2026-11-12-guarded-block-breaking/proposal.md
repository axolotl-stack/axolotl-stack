# Proposal: guarded survival breaking by default

## Why

Survival breaking copies BDS 1.26.51 input by input (archived change
`survival-breaking-per-input`). Two of the behaviours copied help a modified client:

- BDS answers every input, so a client that floods inputs breaks faster than real time.
- Progress built while aiming at the air of a break breaks a block put there afterwards. This was
  measured with dirt only; obsidian was extrapolated.

The owner decided on 2026-11-12: "make the anti cheat like default ig but lets copy bds as well"
([decision 0056](../../../../docs/decisions/0056-guarded-block-breaking.md)).

## What changes

- A `[gameplay] block_breaking` setting: `"guarded"` (the default) or `"bds"`.
- Guarded mode:
  - at most one input of progress per server tick since the progress began;
  - the progress begins again when the block at the position is no longer the one being broken.
- New BDS captures (`tools/bds-sweep/scripts/breaking-3.txt`) show that BDS counts the inputs over
  a break's air at the speed of the block put there next. Obsidian put by hand on the air of dirt
  does not break at once; obsidian on its own air does. The `bds` mode now follows that, instead
  of the extrapolated last block's speed.

## Capabilities

### Modified Capabilities

- `server-authority`: survival breaking has a guarded default and a BDS mode.

## Impact

- `crates/unastar/src/server/config.rs`, `src/config/mod.rs`, `unastar.toml`:
  `BlockBreaking`, `GameplayConfig`.
- `crates/unastar/src/entity/components/player.rs`: `BreakingState::idle_inputs`,
  `BreakInputBudget`.
- `crates/unastar/src/server/game/blocks.rs`: `advance_breaking`, `guard_input`.
- Replays run in the `bds` mode; guarded-mode unit tests in `blocks.rs`.
- `docs/quirks/bds.md`, `docs/parity/systems.toml` (`player.block_breaking`), decision 0056.

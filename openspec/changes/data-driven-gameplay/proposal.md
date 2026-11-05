# Proposal: finish the data-driven gameplay architecture

## Why

The 2026-10-24 data-driven architecture is implemented in shape, but parts of decisions
[0014](../../../docs/decisions/0014-measure-built-in-facts-from-bds.md) (measure built-in facts) and
[0015](../../../docs/decisions/0015-vanilla-scope-with-extensible-registries.md) (extensible
registries) are not carried out: block drops still come from Java-authored tables (deviation X1),
mining uses unmeasured constants (X2), loot cannot reproduce seeded containers, addon packs are not
loaded, and some world-ticking and protocol pieces are missing. This change collects that remainder
in one place instead of the removed plan files.

Prior work: [docs/research/2026-10-24-data-driven-architecture.md](../../../docs/research/2026-10-24-data-driven-architecture.md)
(§8 roadmap), [decision 0023](../../../docs/decisions/0023-land-parked-branches-with-recorded-deviations.md)
(X1, X2), [mob runtime constants](../../../docs/research/mob-runtime-constants.md), and the
2026-10-28 mob-runtime follow-ups (removed 2026-11-04; see git history).

## What Changes

Each group can be split into its own change when it starts:

- **A. Measured drops and mining**: block drops read the BDS sweep (`data/measured/block_drops`);
  the Java-authored `overrides/loot_tables/blocks.json` is retired (X1). Mining reads
  `mining_ticks.json`, with the sweep extended past the 10-block slice and to airborne mining (X2).
- **B. Loot completion**: the 12 unsupported functions and 8 conditions; an MT19937 loot random in
  BDS draw order so seeded containers match.
- **C. Addon packs at startup**: actors, items, blocks and recipes through `unastar-packs` before
  registries freeze. WIT gameplay hooks stay a separate approval.
- **D. World ticking and protocol leftovers**: random ticks (formula from a sweep first) with
  crop/leaf/grass behaviour; periodic autosave at a cadence measured from BDS; `SetHealth`; a test
  for `SetCommandsEnabled`; `UnlockedRecipes`; the `CraftRecipeAuto` layout from a captured packet.
- **E. Engineering follow-ups**: parallelise `tick_breeding` / `tick_baby_growth` (exclusive
  systems today); a per-tick mob benchmark; move shared damage state (hurt timer, `LastHurt`,
  `Dead`) out of `entity/mob/`; use or remove the unused `LivingBundle`; split actor-component
  codegen out of `unastar-data/build/`.
- Re-check constants confirmed on the 26.30 build against 1.26.51 and replace provisional rows.

Out of scope: per-feature vanilla gaps already tracked as parity rows (pick those from
`docs/parity/PARITY.md`).

## Capabilities

### New Capabilities

None. This change is vanilla behaviour, tracked in `docs/parity/`, not our own contracts, so it
has no delta specs (`skip_specs: true`).

### Modified Capabilities

None.

## Impact

- Crates: `unastar`, `unastar-data`, `unastar-rules`, `unastar-packs`, `bds-sweep`.

### Parity rows

Target `done` unless noted. Exact item/block sub-rows for groups A and B are listed when each
group starts as its own change and claims them.

| Group | Domain | Rows |
|---|---|---|
| A | items, blocks | block-drop and mining facets touched by X1/X2 (list on start) |
| B | systems | `crafting.loot_tables` |
| C | systems | `platform.addons` |
| D | systems | `world.random_ticks`, `crafting.recipe_book` |
| D | protocol | `SetHealthPacket`, `SetCommandsEnabledPacket` (`verified`) |

Group E (engineering) and F (constants) have no parity rows.

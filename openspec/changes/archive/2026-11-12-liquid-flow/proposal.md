## Why

`world.liquids` is missing: entities move in liquids and the waterlogged layer is stored and sent
(`crates/unastar/src/world/chunk.rs`), but liquids never flow or mix, game logic cannot see water
inside a block (round 3's coral notes hit this), and placing a block clears that water. Water and
lava should behave as BDS 1.26.51 does, measured and replayed tick by tick.

Prior work: none on liquid flow (no solution notes, decisions or specs about it). Related:
`docs/solutions/testing/measure-redstone-tick-by-tick-with-a-parity-witness.md` (the timeline
step reused here), `space-cells-apart-and-mind-pending-ticks-between-cases.md` (it assumed set
water flows; it does not), the entity liquid quirks in `docs/quirks/bds.md` (currents already
modelled), decision 0041 (redstone's engine, a separate exclusive system).

## What Changes

- Measure BDS (black box): liquid sweep families in `tools/bds-sweep/liquids.py` (spread, falling,
  drops, sources, receding, mixing, chunk crossings, same-tick order, the nether, every block
  against water and lava, which blocks keep water set into them), a per-tick chat marker so the
  client capture is cut per tick (liquid-layer updates and batching), and `summarize.py liquids`.
- A liquid engine in `crates/unastar/src/server/game/liquids/`: ticks per chunk, the Beta-era flow
  rules BDS still runs (no random part), mixing, the liquid layer, per-tick batched packets.
- The block write path keeps water in the liquid layer of a block that holds it and leaves it as
  a source when such a block is removed; block changes wake the liquids beside them.
- A world-side bucket API (`place_source`, `take_source`) the existing bucket code now uses.
- Coral's water check sees water in a neighbour's liquid layer.

## Impact

Crates: `unastar` (server/game), `unastar-data` (measured liquid block table), `tools/bds-sweep`.

### Parity rows

- systems: `world.liquids` → partial (gaps listed in the row).
- blocks, family `liquids`: `minecraft:water`, `minecraft:flowing_water`, `minecraft:lava`,
  `minecraft:flowing_lava` → verified or partial per row.
- The stale "Liquids" claims (blocks `liquids` + sponges, systems `world.liquids`, a contributor,
  2026-10-29) are replaced by this lane's claim; sponges stay unclaimed.

## Out of scope

Bucket items (lane ITEMS), mob swimming (lane ENTITIES), bubble columns, sponges, fire spread
from lava, nether water evaporation, the settling BDS does right after generating terrain.

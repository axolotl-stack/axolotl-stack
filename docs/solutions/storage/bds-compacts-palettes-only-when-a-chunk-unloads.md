---
title: BDS compacts a changed section's palette only when the chunk unloads and the width shrinks, so measure its writer through unloads
area: storage
tags: [leveldb, subchunk, palette, compaction, bds, persistence, unload, autosave, waterlogged]
date: 2026-11-10
origin: openspec/changes/archive/2026-11-11-persist-block-states-by-name (task 1.2, the sub-chunk writer)
status: current
---

## Context

unastar's sub-chunk writer had to produce the bytes BDS 1.26.51 writes. The first edit
session (a BDS world edited from the console, then `stop`) looked inconsistent: two
sections in the same state, given the same kind of edit, came out one with its unused
palette entries and one without, and repeating an experiment flipped the result.

## What we learned

- **Two save paths, two palettes.** Read every save from the LevelDB log (sequence
  numbers tell which batch wrote a key: `tools/bds-sweep/subchunk_roundtrip.py`, step
  `compare`). BDS saves loaded chunks periodically (a chunk edited and left loaded was
  written within ~70 s), and saves a chunk when it unloads. The periodic save writes a
  section's palette as it is in memory: never compacted. The unload save of a section
  changed since it was read drops the palette entries no cell uses **only when that lets
  a smaller bit width hold the rest**: `[air, stone, gold*, diamond]` (`*` unused) plus a
  stone stays 2-bit with gold; `[air, stone*, gold*, diamond]` becomes 1-bit
  `[air, diamond]`; `[end_stone, gold*]` becomes 0-bit `[end_stone]`. The rest keep their
  order. The section is compacted as a whole: when only its first layer changed, a
  liquid layer `[air, water]` with no water left still became a 0-bit `[air]`. Measured
  on 2026-11-10 in five extra console sessions with the same edit history in many
  chunks, some left loaded for 70 to 100 s and some unloaded right after the edit by
  removing their ticking area: every changed section of the 33 unloaded chunks followed
  the rule, and none of the chunks the periodic save wrote was compacted. The harness's
  `source` step lets the periodic save write the source world and its `bds-edit` step
  unloads what it edits; its fixture holds both outcomes (sections narrowed by
  compaction, and a 3-bit section that keeps an unused `air`).
- **`stop` races them.** At shutdown the first and last few chunks written came out
  uncompacted and the ones between compacted, for identical edit histories: a periodic
  save had written some of them just before. To get deterministic bytes, unload the
  edited chunks (`tickingarea remove_all` in each dimension, wait ~90 s) before stopping.
  unastar saves only on unload and at shutdown, so it copies the unload save.
- **Palettes otherwise evolve append-only.** A new state is appended; a full palette
  widens to the next width (0→1, 1→2, 2→3, 3→4, 4→5, 5→6, 6→8, 8→16 bits measured) even when an
  entry is unused; an unused entry's slot is never reused. A fresh section starts as
  `[air]`, a `/fill` of a whole section gives 1-bit `[air, stone]`, a generated uniform
  section is 0-bit.
- **Only changed sections are written.** An edited column gets its changed sections, its
  Data3D, version, finalization record and a deletion of the `0x77` key; unchanged
  sections keep their records (sequence numbers unchanged), unused entries and all. A
  chunk unloaded without an edit writes nothing. In a flat world BDS saved no chunk that
  had no block changed, in any dimension, so a source world needs one block set in every
  column a later step loads.
- **The liquid storage outlives its water.** After the only waterlogged slab of a section
  was replaced, BDS kept writing two storages: `[air, water]` with no water left, or,
  compacted on unload, a 0-bit `[air]`. A section with air in both layers has no record
  even with such a storage left: BDS deleted it.
- **Records.** A section cleared to air has its record deleted; air sections below a used
  one have none (a block at y 100 over flat ground gave records -4 and 6 only). Every
  entry of every record carried `version` 18168865 (1,393 in the edited columns,
  `pale_oak_planks`, `resin_bricks`, `copper_grate` and other 1.21.x blocks included), the
  value every state of the native block palette has.
- **Result.** After the same 50 edits (every width step, compaction that narrows and that
  would not, waterlogging, liquid layers left without water, two sections cleared to air;
  overworld, nether and End), 77 of 77 sub-chunk records of the edited columns were
  byte-identical between BDS's unload save and unastar's save. When BDS reopened
  unastar's world, 4,921 of 4,921 `/testforblock` probes matched (237 of them edited
  cells), and the liquid layers BDS saved from unastar's records matched. A world unastar
  generated in all three dimensions: 3,840 of 3,840 probes (1,175 not air).
- **`/setblock air` over a waterlogged slab leaves still water in the cell** (the water
  moves to the first layer). That is command behaviour, not storage; the harness keeps it
  out of the comparison.

## Where it lives

- `crates/unastar/src/world/chunk.rs` (`SubChunk::encode_disk`, `SubChunk::changed`,
  `PalettedStorage::compacted`, `PalettedStorage::encode_persistent`, `SecondaryStorage`),
  `crates/unastar/src/world/chunk/persistent.rs` (`write_palette_entry`,
  `BLOCK_STATE_VERSION`)
- `crates/unastar/tests/bds_subchunks.rs` with fixture
  `crates/unastar/tests/fixtures/bds_1_26_51_subchunk_edits.json`
- `tools/bds-sweep/subchunk_roundtrip.py`, `crates/unastar/examples/subchunk_roundtrip.rs`

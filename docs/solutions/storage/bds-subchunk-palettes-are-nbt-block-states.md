---
title: BDS saves sub-chunk palettes as NBT block states, so reading them as runtime ids crashed the loader
area: storage
tags: [leveldb, subchunk, palette, nbt, bds, persistence, corrupt-data]
date: 2026-11-10
origin: openspec/changes/persist-block-states-by-name (read side, landed ahead of the writer)
status: current
---

## Context

Loading a world that BDS 1.26.51 had saved panicked with an index out of bounds while
counting non-air blocks of a sub-chunk (`SubChunk::recount` in
`crates/unastar/src/world/chunk.rs`). The record was a plain flat-world ground sub-chunk:
`09 01 fc 04 …`, which is version 9, one storage, y -4 and header `0x04`.

## What we learned

- **The header's low bit picks the palette form, and the old decoder ignored it.** A
  storage header is `bits << 1 | form`. BDS saves form 0, the persistent form: index
  words, the palette length as `i32 LE`, then one little-endian NBT compound
  `{name, states, version}` per entry (`version` 18168865 in 1.26.51). unastar's own saves
  and the network use form 1, with a varint length and varint runtime ids. Read as form 1,
  the bytes `04 00 00 00` became a length of 2 (zigzag 4) and ids `0, 0`. The 2-bit
  indices 2 and 3 then pointed past a two-entry palette, and the count indexed out of
  bounds. Indices past the palette only ever came from this misreading. None of the 938
  sub-chunk records BDS saved in seven 2026-11-10 world snapshots had one (shapes built
  with `/fill` on flat worlds, plus generated overworld, nether and End). They also had
  no runtime-form storage, no y byte differing from the key and no trailing bytes.
- **How BDS writes sub-chunks.** Measured by decoding every record independently of
  unastar (`tools/bds-sweep/leveldb_read.py`, `subchunk_fixture.py`):
  - The records of these worlds were written by BDS's periodic save, which never compacts
    a palette. `air` stays entry 0 after a fill, so a solid stone sub-chunk is saved as
    1-bit `[air, stone]`. When a chunk unloads, BDS does compact a changed section if that
    narrows its width, down to 0 bits; see
    `bds-compacts-palettes-only-when-a-chunk-unloads.md` (the writer, task 1.2).
  - The bit width is the smallest of 1, 2, 3, 4, 5, 6, 8 and 16 that holds the palette
    length, unused entries included (66 entries → 8 bits).
  - 0 bits appears for sections generated uniform, such as end stone inside the End
    island, and for a changed section compacted to one entry on unload. It has no length
    field: the NBT entry follows the header byte.
  - A sub-chunk cleared back to air has no record.
  - Blocks set into water keep the water as a second storage `[air, water]`. Natural
    water also shows `[air, water, flowing_water]`.
  - Every entry has exactly `name`, `states` and `version`.
- **How BDS reads damaged or unusual records.** Each record below was written into a copy
  of a saved flat world before BDS started, one per chunk. After the reload, a
  `/testforblock` of an unmodified sub-chunk in the same chunk confirmed the chunk had
  loaded, then the edited cells were asked. BDS rewrote none of these records.

  | Edit | BDS read |
  |---|---|
  | 1-bit `[air, stone]` storage with a third entry (`gold_block`) appended | all cells as before |
  | y byte 5 in the record under key y 0 | the blocks at y 0..15; y 80..95 stayed air |
  | both padding bits of every 3-bit word set | all cells as before |
  | rewritten as version 8 (no y byte) | all cells as before |
  | zero-bit storage, `gold_block` entry right after the header | gold everywhere |
  | last 10 bytes cut (inside the last entry's `version` field) | all cells as before |
  | one log entry's `pillar_axis` set to `"w"` | those cells an oak log |
  | entry 1 renamed `minecraft:not_a_block` | those cells `%tile.not_a_block.name` |
  | 8 bytes `0xAB` after the storage | all cells as before |
  | ground record replaced by `[9, 0, 0xfc]` (no storages) | air |
  | bedrock entry's `infiniburn_bit` as TAG_Int, or missing; dirt with an extra `foo: 1b`; entries without `version` | bedrock, dirt, everything as before |
  | palette of a 4-bit `[air, …]` storage cut from 10 to 8 entries | cells with index 8 or 9 read air (its first entry) |
  | hand-made 2-bit `[gold, diamond]` storage, indices cycling 0..3 | indices 2 and 3 read gold: the first entry |
  | runtime-form storage (header `0x01`, varint id 3317, stone in unastar's numbering) | air |
  | zero-bit storage with an i32 length 1 before the entry | air |
  | header `0xff` as the only storage; as the second storage after a gold one | air; gold |
  | 7-bit storage (header `0x0e`, 1024 words) | the server exited while loading the chunk |

- **Decision.** unastar decodes exactly what BDS writes and copies the harmless
  leniencies: the ignored y byte, longer palettes (entries the width can't index are
  dropped) and trailing bytes (quirk row "World / storage"). Damage BDS papers over
  (indices past the palette, unknown names or states, truncation) fails the column load
  instead, so the record is kept and never replaced by a later save. That is the
  world-storage spec's rule for corrupt records, and the fail-fast rule of change
  `persist-block-states-by-name`. Runtime-form palette ids must be canonical states too;
  otherwise a corrupt unastar save loads and panics later, when the section is sent.
- **Saving was the gap, until the writer landed.** A column is saved again after any edit
  and whenever it holds or held actors. unastar now saves sub-chunks in BDS's form (task
  1.2, `bds-compacts-palettes-only-when-a-chunk-unloads.md`); before that it wrote runtime
  ids, which BDS reads as air.
- `/testforblock` sees only the first layer; for a waterlogged slab it answers "Oak Slab".
  Check the liquid layer against the placement plan and an independent decode.

## Where it lives

- `crates/unastar/src/world/chunk.rs` (`decode_block_storage`, `decode_persistent`,
  `checked`, `check_indices`), `crates/unastar/src/world/chunk/persistent.rs` (entry
  reader), `crates/unastar/src/registry/block.rs` (`state_by_properties`)
- `crates/unastar/tests/bds_subchunks.rs` with fixture
  `crates/unastar/tests/fixtures/bds_1_26_51_subchunks.json`
- `tools/bds-sweep/subchunk_fixture.py`, `tools/bds-sweep/leveldb_read.py`

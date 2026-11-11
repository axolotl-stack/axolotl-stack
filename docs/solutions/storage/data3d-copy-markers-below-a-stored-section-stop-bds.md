---
title: A Data3D "same as below" marker under a stored biome section stops BDS, so write markers only above the last stored one
area: storage
tags: [leveldb, data3d, biomes, bds, crash, persistence]
date: 2026-11-10
origin: openspec/changes/persist-block-states-by-name (the unastar-generated world opened in BDS)
status: current
---

## Context

The sub-chunk round trip opened a world unastar generated (Java-style generator, seed
12345) in BDS 1.26.51. BDS exited with an access violation (exit code 0xC0000005) as soon
as a ticking area reached the first overworld column, with nothing in its log. Nether and
End columns loaded, and so had every column unastar re-saved in a BDS flat world.

## What we learned

- **Bisect records, not chunks.** Worlds holding a subset of the column's records (an
  empty LevelDB from unastar plus one write batch appended to its log by a small script)
  showed that every sub-chunk record loaded and that the Data3D record (key tag `0x2b`)
  alone stopped BDS.
- **The cause is where the `0xFF` markers go.** The value had biome storages for sections
  0 (`[lush_caves]`), `0xFF` for 1 to 5, a 1-bit `[lush_caves, ocean]` storage for 6 and
  `[ocean]` for 7, then `0xFF` again. BDS's own Data3D for the same seed and column stores
  sections 0 to 7 explicitly (sections 1 to 5 repeat section 0 byte for byte) and uses
  `0xFF` only from section 8 up. Expanding unastar's markers for sections 1 to 5 made the
  column load; putting BDS's section 6 into unastar's value did not. So `0xFF` below a
  stored section stops BDS, while any number of trailing markers is fine (flat-world
  chunks are one storage and 23 markers). unastar's writer had used `0xFF` for every
  section equal to the one below, which only went wrong once biomes changed above a
  repeated run: never in a flat world, but in most generated columns, and in any
  multi-biome BDS column unastar re-saved.
- **What BDS writes otherwise.** Heights are the first air above each column relative to
  the dimension's bottom (nether 128, End 59 to 66), and Data3D has one section per
  section of the dimension's height (8 nether, 16 End, 24 overworld). unastar still
  writes heights relative to y -64 and 24 sections in every dimension; BDS loads both.

## Where it lives

- `crates/unastar/src/world/chunk.rs` (`Chunk::encode_biomes_disk`, test
  `data3d_round_trips_mixed_biomes_and_writes_heights`)
- `tools/bds-sweep/subchunk_roundtrip.py` (steps `generate`, `generated-verify`)

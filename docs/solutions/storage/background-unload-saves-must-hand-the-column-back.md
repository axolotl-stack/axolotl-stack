---
title: A background unload save must hand the column back when the write fails
area: storage
tags: [chunk-unload, persistence, background-save, held-chunks, streaming]
date: 2026-11-12
origin: "feat/chunk-streaming-port-71 (port of #71 000f8db7 onto main's held-chunk retry)"
status: current
---

## Context

Main keeps a chunk loaded, with its edits and mobs, when its save before unload fails, and
retries the unload with an exponential backoff. The #71 streaming lane moved unload saves off the
tick: the column is snapshotted, the chunk entity and its mobs are despawned at once, and the
write runs in the background. Ported as it was, a failed background write would have dropped the
only copy of the column, which is exactly what main's held-chunk work had removed.

## What we learned

- Once the write is off the tick, "keep it loaded" is no longer an option: the entity is already
  gone when the error arrives. The write job has to own the column and return it on failure.
- The port carries it back as a `HeldColumn` (column, stored actors, failed-save count) in the
  `Saved` event. The next `apply_loaded_columns` respawns it dirty with
  `ChunkPendingUnload::after_failed_save`, so the backoff and the held-chunk summary work as
  before. Its mobs come back through the column's stored actors, not as the same entities.
- Two other readers must see a held column before storage: `get_or_create_chunk` (game code that
  needs a chunk now) takes it from `ColumnLoads` after `wait_for_save`, and shutdown
  (`save_all_chunks`) restores held columns before the final save, so they are written again and
  counted as failures if storage still refuses.
- Tests of the unload path cannot assume the chunk or mob `Entity` survives a failed save. They
  tick, wait for the writes (`ColumnLoads::wait_for_saves`) and take back held columns, then look
  the chunk up by position.

## Where it lives

`crates/unastar/src/world/ecs/streaming.rs` (`HeldColumn`, `ColumnLoads::start_save`),
`crates/unastar/src/world/ecs/manager.rs` (`held_column_components`, `restore_held_columns`),
`crates/unastar/src/world/ecs/systems.rs` (`process_chunk_unloads`),
`crates/unastar/src/entity/mob/persistence/tests.rs`.

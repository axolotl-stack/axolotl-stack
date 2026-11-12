## MODIFIED Requirements

### Requirement: Chunks load from storage before they are generated
Every path that creates a chunk entity (background streaming loads in `ColumnLoads`, and the synchronous `ChunkManager::load_or_generate_chunk` behind `World::get_or_create_chunk`) SHALL ask the provider first and MUST generate terrain only when the provider answers `Ok(None)`. The generation pool MUST be given only storage misses. A column loaded from storage MUST NOT be replaced by generated terrain.

#### Scenario: Stored column survives the asynchronous path
- **WHEN** a column with a diamond ore block is saved, then loaded in the background (`ColumnLoads`) with a Java generator and its generation pool configured
- **THEN** the resulting chunk entity holds the stored diamond ore, not generated terrain

#### Scenario: Stored column survives the synchronous path
- **WHEN** a stored column is requested through `World::get_or_create_chunk`
- **THEN** the entity holds the stored blocks

Evidence: crates/unastar/src/world/ecs/streaming.rs (`ColumnLoads::start_read`, `ColumnLoads::start_generation`), crates/unastar/src/world/ecs/manager.rs (`load_or_generate_chunk`, `get_or_create_chunk`), crates/unastar/src/world/ecs/generation_worker.rs (`run_worker`), crates/unastar/tests/persistence.rs (`async_stored_chunks_survive_save_cycle_for_both_providers`, `synchronous_world_extension_does_not_replace_stored_column`)

### Requirement: Storage read errors never become generated terrain
A provider read error SHALL stop that chunk from being created; it MUST NOT be treated as a miss and MUST NOT panic. On the streaming path and the synchronous path the server MUST log one error naming the column's coordinates and dimension and saying the stored column is kept on disk untouched, then drop the request without spawning a chunk.

#### Scenario: Unreadable column on the generation pool
- **WHEN** a provider whose `load_column` always fails is attached to a world with a generation pool and a column is read in the background (`ColumnLoads`)
- **THEN** the load is reported failed, no column is ready to spawn, the pool is never asked and `save_column` is never called

#### Scenario: Unreadable column on the synchronous path
- **WHEN** a void world (inline generation, no pool) with an unreadable provider and a spawned player is ticked until its loads finish
- **THEN** nothing panics, no chunk entity is spawned, the column is not loaded for the player and no `LevelChunk` is sent

#### Scenario: Synchronous API
- **WHEN** `World::get_or_create_chunk` is called against an unreadable provider
- **THEN** it returns the storage error and creates no chunk

Evidence: crates/unastar/src/world/ecs/streaming.rs (`LoadFailure`, `ColumnLoads::drain`, test `an_unloadable_column_message_names_it_and_says_it_is_kept`), crates/unastar/src/world/ecs/manager.rs (`load_or_generate_chunk`, `get_or_create_chunk`), crates/unastar/src/server/game/mod.rs (test `a_storage_read_error_drops_the_request`), crates/unastar/tests/persistence.rs (`storage_read_errors_never_generate_replacement_terrain`, `synchronous_storage_read_error_creates_no_chunk_and_does_not_panic`)

### Requirement: Chunks are saved on unload
A chunk whose unload grace period expires SHALL be written in the background (when it has something to write) with the mobs standing in it, and removed from the world with those mobs; its column MUST NOT be read again until the write has finished. If the write fails, the column MUST come back with its edits and mobs, dirty, and its unload MUST be retried after a backoff. Unloading MUST pause while 64 writes are in flight. There is no periodic autosave.

#### Scenario: Failed save before unload
- **WHEN** a chunk's write before unload fails
- **THEN** the chunk is loaded again at the same position with its edits and its mobs, dirty, and the unload is retried after the grace period doubled per failure in a row, capped at 64 grace periods

#### Scenario: Held chunks are summarised
- **WHEN** chunks are held in memory because their saves keep failing
- **THEN** one summary `N chunks held in memory awaiting save` is logged when the first is held and then at most every 30 s (600 ticks), as a warning below 64 held chunks and an error from 64; the per-chunk line is only at debug

#### Scenario: Unload saves mobs with the chunk
- **WHEN** a chunk holding a cow unloads and is later loaded again
- **THEN** the cow is gone from the world after unload and restored on reload

#### Scenario: Restart round trip
- **WHEN** cows are saved through LevelDB and the server is rebuilt on the same directory
- **THEN** the cows load back with their saved state

#### Scenario: Shutdown with writes in flight
- **WHEN** the server shuts down while unload writes are in flight
- **THEN** "Shutdown saves everything, then closes storage" first waits for them, and saves again any column whose write failed

Evidence: crates/unastar/src/world/ecs/systems.rs (`process_chunk_unloads`, `HeldChunkReport`, `MAX_SAVES_IN_FLIGHT`, test `held_chunk_summary_is_rate_limited`), crates/unastar/src/world/ecs/streaming.rs (`ColumnLoads::start_save`, `HeldColumn`), crates/unastar/src/world/ecs/manager.rs (`restore_held_columns`), crates/unastar/src/world/ecs/components.rs (`ChunkPendingUnload::after_failed_save`, test `failed_unload_saves_back_off_exponentially_up_to_the_cap`), crates/unastar/src/entity/mob/persistence/tests.rs (`unloading_saves_and_removes_the_cow_and_reloading_restores_it`, `cows_round_trip_through_leveldb_across_a_restart`, `a_dead_cow_is_deleted_from_storage_on_the_next_save`, `unknown_stored_actors_are_kept_and_written_back`, `a_chunk_whose_unload_save_fails_stays_loaded_with_its_edits`, `a_held_chunk_backs_off_between_save_attempts`)

# world-storage Specification

## Purpose
How `unastar` persists worlds and players today: the `WorldProvider` and `PlayerProvider` traits and their implementations in `crates/unastar/src/storage/` (LevelDB, the default, and BlazeDB), the chunk load-before-generate path in `crates/unastar/src/world/ecs/`, when chunks and players are saved, and the on-disk formats. Generation itself, including the generation pool, is specified in `worldgen-pipeline`; the configuration keys that choose a provider are in `server-config`. Vanilla parity of saving is tracked by the `world.persistence` row in `docs/parity/systems.toml`.

## Requirements

### Requirement: Provider selection and location
At startup the server SHALL open exactly one world provider chosen by `world.storage_provider` (`level_db` by default, or `blaze_db` with `world.blazedb_cache_chunks`) at `worlds/default/db` relative to the working directory, for the single configured `world.dimension`. When the world database, or the player database while `players.leveldb_enabled` is true, cannot be opened, startup MUST fail with an error naming the path; the server MUST NOT run without persistence.

#### Scenario: Default provider
- **WHEN** the server starts with the default configuration
- **THEN** a LevelDB world database is opened at `worlds/default/db` and attached to the `ChunkManager`

#### Scenario: World database open failure
- **WHEN** `worlds/default/db` cannot be opened (a second instance holds the LevelDB lock, or a regular file sits at that path)
- **THEN** `UnastarServer::new` returns `Failed to open world storage at worlds/default/db: ...` and the server does not start

#### Scenario: Player database open failure
- **WHEN** `players.leveldb_enabled` is true and `<players.data_dir>/db` cannot be opened
- **THEN** `UnastarServer::new` returns `Failed to open player storage at <path>: ...` and the server does not start

Evidence: crates/unastar/src/server/runtime.rs (`UnastarServer::new`, `open_world_provider`, tests `a_locked_world_leveldb_fails_to_open`, `an_unopenable_world_blazedb_fails_to_open`), crates/unastar/src/world/mod.rs (`StorageProvider`, `WorldConfig`), crates/unastar/src/config/tests.rs (`storage_settings_reach_runtime_configuration`), crates/unastar/tests/startup_storage.rs (`startup_fails_when_the_world_database_cannot_be_opened`), crates/unastar/tests/startup_player_storage.rs (`startup_fails_when_the_player_database_cannot_be_opened`)

### Requirement: Chunks load from storage before they are generated
Every path that creates a chunk entity (the synchronous `ChunkManager::load_or_generate_chunk`, behind `get_or_create` and `get_or_create_chunk`, and the asynchronous `request_generation` pool) SHALL ask the provider first and MUST generate terrain only when the provider answers `Ok(None)`. A column loaded from storage MUST NOT be replaced by generated terrain.

#### Scenario: Stored column survives the asynchronous path
- **WHEN** a column with a diamond ore block is saved, then requested through `request_generation` with a Java generator configured
- **THEN** the resulting chunk entity holds the stored diamond ore, not generated terrain

#### Scenario: Stored column survives the synchronous path
- **WHEN** a stored column is requested through `World::get_or_create_chunk`
- **THEN** the entity holds the stored blocks

Evidence: crates/unastar/src/world/ecs/manager.rs (`load_or_generate_chunk`, `request_generation`, `get_or_create_chunk`), crates/unastar/src/world/ecs/generation_worker.rs (`run_worker`), crates/unastar/tests/persistence.rs (`async_stored_chunks_survive_save_cycle_for_both_providers`, `synchronous_world_extension_does_not_replace_stored_column`)

### Requirement: Storage read errors never become generated terrain
A provider read error SHALL stop that chunk from being created; it MUST NOT be treated as a miss and MUST NOT panic. On both the asynchronous and the synchronous path the server MUST log one error naming the column's coordinates and dimension and saying the stored column is kept on disk untouched, then drop the request without spawning a chunk.

#### Scenario: Unreadable column on the generation pool
- **WHEN** a provider whose `load_column` always fails is attached and a chunk is requested through the generation pool
- **THEN** the request resolves to `Err(StorageError::Database(_))` and `save_column` is never called

#### Scenario: Unreadable column on the synchronous path
- **WHEN** a void world (synchronous path) with an unreadable provider and a spawned player is ticked
- **THEN** nothing panics, no chunk entity is spawned, the column is not in `loaded` and no `LevelChunk` is sent

#### Scenario: Synchronous API
- **WHEN** `World::get_or_create_chunk` is called against an unreadable provider
- **THEN** it returns the storage error and creates no chunk

Evidence: crates/unastar/src/world/ecs/systems.rs (`process_completed_generations`, `process_chunk_load_queues`, `log_unloadable_column`, test `an_unloadable_column_message_names_it_and_says_it_is_kept`), crates/unastar/src/world/ecs/manager.rs (`load_or_generate_chunk`, `get_or_create`), crates/unastar/src/server/game/mod.rs (test `a_storage_read_error_on_the_sync_load_path_drops_the_request`), crates/unastar/tests/persistence.rs (`storage_read_errors_never_generate_replacement_terrain`, `synchronous_storage_read_error_creates_no_chunk_and_does_not_panic`)

### Requirement: Dirty tracking decides what is saved
A chunk entity created from a loaded column SHALL start clean, and one created from generated terrain MUST start dirty. Block edits MUST mark the chunk dirty. A chunk MUST be written only when it is dirty, holds mobs, or had actors stored with it earlier; a successful write MUST clear the dirty flag.

#### Scenario: Loaded column is not rewritten
- **WHEN** a stored column is loaded and `save_all_chunks` runs with no edits
- **THEN** the chunk is not dirty, `save_all_chunks` reports 0 saved, and the stored column is unchanged

Evidence: crates/unastar/src/world/ecs/manager.rs (`chunk_entity_components`), crates/unastar/src/world/ecs/components.rs (`ChunkStateFlags::new_generated`), crates/unastar/src/entity/mob/persistence/mod.rs (`snapshot_chunks`, `mark_saved`), crates/unastar/tests/persistence.rs (`async_stored_chunks_survive_save_cycle_for_both_providers`, `synchronous_world_extension_does_not_replace_stored_column`)

### Requirement: Chunks are saved on unload
A chunk whose unload grace period expires SHALL be saved (when it has something to write) together with the mobs standing in it, and then removed with those mobs. If that save fails, the chunk and its mobs MUST stay loaded and dirty, and the unload MUST be retried after a backoff. There is no periodic autosave; shutdown is covered by "Shutdown saves everything, then closes storage".

#### Scenario: Failed save before unload
- **WHEN** a chunk's save before unload fails
- **THEN** its terrain is put back on the entity, the chunk and its mobs stay loaded with their edits, and the unload is retried after the grace period doubled per failure in a row, capped at 64 grace periods

#### Scenario: Held chunks are summarised
- **WHEN** chunks are held in memory because their saves keep failing
- **THEN** one summary `N chunks held in memory awaiting save` is logged when the first is held and then at most every 30 s (600 ticks), as a warning below 64 held chunks and an error from 64; the per-chunk line is only at debug

#### Scenario: Unload saves mobs with the chunk
- **WHEN** a chunk holding a cow unloads and is later loaded again
- **THEN** the cow is gone from the world after unload and restored on reload

#### Scenario: Restart round trip
- **WHEN** cows are saved through LevelDB and the server is rebuilt on the same directory
- **THEN** the cows load back with their saved state

Evidence: crates/unastar/src/world/ecs/systems.rs (`process_chunk_unloads`, `HeldChunkReport`, test `held_chunk_summary_is_rate_limited`), crates/unastar/src/world/ecs/components.rs (`ChunkPendingUnload::after_failed_save`, test `failed_unload_saves_back_off_exponentially_up_to_the_cap`), crates/unastar/src/entity/mob/persistence/tests.rs (`unloading_saves_and_removes_the_cow_and_reloading_restores_it`, `cows_round_trip_through_leveldb_across_a_restart`, `a_dead_cow_is_deleted_from_storage_on_the_next_save`, `unknown_stored_actors_are_kept_and_written_back`, `a_chunk_whose_unload_save_fails_stays_loaded_with_its_edits`, `a_held_chunk_backs_off_between_save_attempts`)

### Requirement: Shutdown saves everything, then closes storage
On shutdown or `/stop`, after plugins have seen every online player quit and `on-unload` has run, the server SHALL save all players, then every loaded chunk that has something to write, then close the world provider and then the player provider. Shutdown MUST return an error, so the process exits non-zero, when any player save, chunk save or provider close fails.

#### Scenario: Shutdown order
- **WHEN** a server with recording world and player providers shuts down
- **THEN** the calls are player save, chunk save, world close, player close, in that order, and the result is clean

#### Scenario: Failed saves make shutdown unclean
- **WHEN** both providers refuse every save at shutdown but close cleanly
- **THEN** `StorageShutdown` reports 1 player and 1 chunk failed, `is_clean()` is false, and `shutdown` returns an error naming both counts

Evidence: crates/unastar/src/server/runtime.rs (`shutdown`), crates/unastar/src/server/game/mod.rs (`save_and_close_storage`, `SaveCounts`, `StorageShutdown::is_clean`, `save_all_players`, `save_all_chunks`, tests `shutdown_saves_players_then_chunks_then_closes_storage`, `failed_shutdown_saves_make_the_shutdown_unclean`)

### Requirement: LevelDB key layout
The LevelDB provider SHALL key chunk records by the chunk index: little-endian `x` and `z` as 4 bytes each, followed by a 4-byte little-endian dimension only when the dimension is not 0, then a one-byte record tag. Actor lists MUST use `digp` + chunk index and actor records `actorprefix` + the 8-byte little-endian unique ID.

#### Scenario: Record tags
- **WHEN** keys are built for an overworld chunk
- **THEN** version is `,` (0x2c), subchunk is `/` (0x2f) followed by the signed Y index byte, Data3D is `+` (0x2b), block entities are `1` (0x31) and finalisation is `6` (0x36), each 9 bytes long for subchunk-less records

#### Scenario: Other dimensions
- **WHEN** a version key is built for dimension 1 or 2
- **THEN** it is 13 bytes and bytes 8..12 hold the dimension

#### Scenario: Actor keys
- **WHEN** `entity_ids_key` and `entity_data_key(12345)` are built for an overworld chunk
- **THEN** they are 12 and 19 bytes and start with `digp` and `actorprefix`

Evidence: crates/unastar/src/storage/keys.rs (tests `test_chunk_key_overworld`, `test_chunk_key_nether`, `test_end_dimension`, `test_subchunk_key`, `test_negative_coords`, `test_biome_key_format`, `test_block_entities_key`, `test_finalisation_key`, `test_entity_ids_key`, `test_entity_data_key`)

### Requirement: LevelDB column writes are one atomic batch
`LevelDBWorldProvider::save_column` SHALL write a whole column in one LevelDB write batch: chunk version 41, finalisation 2 (populated) as a 4-byte little-endian integer, the Data3D value, each subchunk's record (a deletion for an air subchunk), the block entity record, the actor digest and actor records. An empty block entity list or actor list MUST delete its record. Loading MUST treat a column as present only when its version record exists.

#### Scenario: Absent column
- **WHEN** `load_column` is called for a chunk with no version record
- **THEN** it returns `Ok(None)`

#### Scenario: Biomes survive a reopen
- **WHEN** a column with mixed biomes is saved, the database is closed and reopened, and the column is loaded
- **THEN** every sampled biome and block matches what was saved

#### Scenario: Subchunk cleared to air
- **WHEN** a column whose stored subchunk was cleared to air is saved
- **THEN** that subchunk's record is deleted in the same batch

Evidence: crates/unastar/src/storage/leveldb_world.rs (`save_column`, `load_column`), crates/unastar/src/storage/keys.rs (`CHUNK_VERSION`), crates/unastar/tests/persistence.rs (`chunk_biomes_survive_save_and_reopen_for_both_providers`), crates/unastar/tests/bds_subchunks.rs (`edited_columns_saved_through_the_leveldb_provider_match_bds`)

### Requirement: Subchunk palettes store block names and states
Both providers SHALL store each subchunk in BDS 1.26.51's record layout: version 9, the storage count, the subchunk's Y index, then the block storage and, once the subchunk has one, its liquid storage. Each storage MUST use the persistent form: header `bits << 1`, index words with unused bits zero, the palette length as `i32 LE` (absent at 0 bits), then one NBT compound `{name, states, version}` per entry, states sorted by name and typed as the registry types them, `version` 18168865.

#### Scenario: Round trip within one build
- **WHEN** a chunk with stone in every section is saved and loaded by the same build
- **THEN** every block holds the same runtime id

#### Scenario: Round trip of every block state
- **WHEN** a chunk containing every block state is saved and loaded again
- **THEN** every position holds the same block state as before saving

#### Scenario: Palette order changes
- **WHEN** a world saved by one build is loaded by a build whose runtime palette order differs
- **THEN** every block loads as the same named block with the same states

Evidence: crates/unastar/src/world/chunk.rs (`SUBCHUNK_VERSION`, `encode_subchunk_disk`, `SubChunk::encode_disk`, `PalettedStorage::encode_persistent`, tests `every_block_state_round_trips_through_the_persistent_form`, `palette_entries_are_written_as_bds_writes_them`, `records_frame_storages_as_bds_does`, `word_padding_is_written_as_zero`), crates/unastar/src/world/chunk/persistent.rs (`write_palette_entry`, `BLOCK_STATE_VERSION`)

### Requirement: Subchunks are saved as BDS saves a chunk it unloads
A subchunk that is air in both layers SHALL have no record. A subchunk unchanged since it was read MUST keep its palettes as read (entries, order and widths; only a state listed twice is merged), so a record BDS saved is written back byte for byte. A changed subchunk MUST drop each storage's unused palette entries, the rest in order at the smallest width that holds them (0 bits for one), only when that narrows the width (quirk "World / storage"); otherwise its palettes are written as they grew.

#### Scenario: Subchunk saved by BDS written back
- **WHEN** a subchunk record saved by BDS 1.26.51 is loaded and its column saved without a change to it
- **THEN** the saved record equals BDS's byte for byte

#### Scenario: Subchunks edited as BDS saves them on unload
- **WHEN** the same block writes are applied to subchunks of a BDS-saved world by BDS, which then unloads the chunks, and by unastar, which then saves the columns: entries added at every width boundary, blocks removed, unused entries that narrow or keep the width, blocks set into water, liquid layers left without water, subchunks cleared to air, in the three dimensions
- **THEN** every record unastar saves equals BDS's byte for byte, and the cleared subchunks have no record in either

#### Scenario: Liquid storage outlives its water
- **WHEN** the only waterlogged block of a subchunk is replaced by stone and the column is saved
- **THEN** the record still holds two storages, the second all air (a 0-bit `[air]` palette)

Evidence: crates/unastar/src/world/chunk.rs (`SubChunk::encode_disk`, `SubChunk::changed`, `PalettedStorage::compacted`, tests `changed_sections_drop_unused_entries_only_to_narrow_the_width`, `the_liquid_storage_outlives_its_last_liquid_cell`, `a_state_listed_twice_is_saved_once`), crates/unastar/tests/bds_subchunks.rs (`every_bds_record_is_written_back_byte_for_byte`, `edited_sections_are_saved_as_bds_saves_them_on_unload`, fixture `bds_1_26_51_subchunk_edits.json`), tools/bds-sweep/subchunk_roundtrip.py

### Requirement: Stored subchunks load in either palette form
Loading SHALL accept subchunk versions 8 and 9, at most two storages, and per storage both the persistent form (header low bit 0) and the runtime-id form unastar wrote before (low bit 1); the record's key, not its Y byte, places it. A subchunk that cannot be read or decoded MUST fail the whole column load.

#### Scenario: LevelDB subchunk that does not decode
- **WHEN** a stored LevelDB subchunk has an unsupported version or storage count
- **THEN** `load_column` returns `Err(StorageError::Deserialization(_))` naming the chunk and subchunk Y, so the column is neither loaded as air nor regenerated, and the stored bytes are kept

#### Scenario: Subchunk saved by BDS
- **WHEN** a subchunk record saved by BDS 1.26.51 is loaded: a palette of 0, 1, 2, 3, 4, 5, 6, 8 or 16 bits, unused palette entries, one or two storages, any dimension
- **THEN** every cell holds the block state BDS reports for it after reloading the same world

#### Scenario: Lenient framing BDS accepts
- **WHEN** a version 9 record's Y byte differs from its key, bytes follow its last storage, or a palette lists more entries than its bit width can index
- **THEN** it loads at the key's Y; the extra bytes are not read, and the extra entries are resolved but no cell can use them (quirk "World / storage" in `docs/quirks/bds.md`)

#### Scenario: Index outside the palette
- **WHEN** a stored storage has an index past the end of its palette
- **THEN** `load_column` returns `Err(StorageError::Deserialization(_))`, nothing panics, and the stored bytes are kept

Evidence: crates/unastar/src/world/chunk.rs (`decode_subchunk`, `decode_block_storage`, tests `indices_outside_the_palette_are_errors_not_panics`, `record_framing_follows_the_key_not_the_y_byte`, `palette_lengths_are_positive_and_may_exceed_the_bit_width`), crates/unastar/src/storage/leveldb_world.rs (`load_column`, test `undecodable_subchunk_fails_the_column_load`), crates/unastar/tests/bds_subchunks.rs (`every_bds_record_decodes_to_the_blocks_bds_reports`, `bds_records_load_through_the_leveldb_provider`)

### Requirement: Runtime-id saves are converted by their next save
A column whose subchunks were saved in the runtime-id form SHALL load unchanged, and loading it MUST NOT mark it changed. Its next save, after an edit or while it holds or held actors, MUST write every subchunk in the persistent form and leave no record for an air subchunk.

#### Scenario: Legacy runtime-id subchunk
- **WHEN** a column whose subchunks were saved with runtime-id palette entries, one record per section with air ones included, is loaded and then saved again
- **THEN** its blocks load unchanged
- **AND** the save writes every subchunk in the name-and-state form and removes the records of the air sections

Evidence: crates/unastar/src/storage/leveldb_world.rs (test `runtime_id_sections_load_and_are_rewritten_by_name_on_the_next_save`)

### Requirement: Columns holding unknown runtime ids are not saved
Saving a column that holds a runtime id which is not a canonical block state SHALL fail with `StorageError::Serialization` naming the id, and MUST write nothing and not panic.

#### Scenario: Unknown runtime id at save
- **WHEN** a column holding an id that is not a canonical block state is saved
- **THEN** `save_column` returns `Err(StorageError::Serialization(_))` naming the id, nothing panics, and no record of the column is written

Evidence: crates/unastar/src/world/chunk.rs (`Chunk::unknown_block_state`), crates/unastar/src/storage/leveldb_world.rs (`save_column`, test `a_column_with_an_unknown_block_state_is_not_saved`), crates/unastar/src/storage/blazedb.rs (`save_column`)

### Requirement: Unknown saved block states fail to load
Loading a subchunk whose palette names a block or state set the registry does not know MUST fail that chunk with a typed error naming the entry, and MUST NOT substitute another block.

#### Scenario: Unknown block name
- **WHEN** a saved palette entry names a block that is not in the registry
- **THEN** loading that chunk returns an error naming the block, and no air or fallback block is placed

#### Scenario: Unknown state
- **WHEN** a saved palette entry names a known block with a state value, a state type or a set of state keys no state of that block has
- **THEN** loading that chunk returns an error naming the block and the states

### Requirement: Data3D biomes use the BDS layout
Biomes SHALL be saved as a Data3D value: a 512-byte height map, then up to 24 biome storages, every section stored up to the last one whose biomes differ from the section below and only the sections above it written as `0xFF` (repeat the previous section), as BDS writes them (quirk "World / storage"). Loading MUST also accept `0xFF` anywhere, and unastar's older network biome layout for values shorter than 512 bytes when it parses exactly. Any other malformed value MUST fail the load.

#### Scenario: Legacy network layout
- **WHEN** a Data3D record holds the LevelChunk network biome encoding
- **THEN** the column loads with those biomes

#### Scenario: Short garbage
- **WHEN** a Data3D record holds 30 bytes that are neither layout
- **THEN** `load_column` returns `Err(StorageError::Deserialization(_))`

#### Scenario: Repeat markers only on top
- **WHEN** a chunk's biomes repeat over some sections and then change above them
- **THEN** every section up to the last change is stored and only the sections above it are `0xFF`

Evidence: crates/unastar/src/world/chunk.rs (`encode_biomes_disk`, `decode_biomes_disk`, `DATA3D_HEIGHTMAP_LEN`, tests `data3d_decode_hand_made_blob_with_copy_marker`, `data3d_decode_rejects_malformed_values`, `data3d_round_trips_mixed_biomes_and_writes_heights`), crates/unastar/src/storage/leveldb_world.rs (tests `legacy_network_data3d_loads_with_its_biomes`, `short_garbage_data3d_is_an_error`), docs/solutions/storage/data3d-copy-markers-below-a-stored-section-stop-bds.md

### Requirement: Block entities and actors are stored with their chunk
Block entities SHALL be saved as concatenated rooted little-endian NBT compounds; an entity outside its chunk MUST fail the save, and a malformed or out-of-chunk entity MUST fail the load. Actors MUST be stored as rooted little-endian NBT per unique ID; saving replaces the chunk's actor list and deletes records of actors that no longer exist. A LevelDB digest entry with no record MUST be skipped with a warning.

#### Scenario: Container contents survive a reopen
- **WHEN** chests (paired and single), a furnace mid-smelt and a hopper on cooldown are saved and the provider is reopened
- **THEN** both providers load the same contents, pairing, progress and cooldown

#### Scenario: Removed actors are deleted
- **WHEN** a column listing actors `-4294967295` and `7` is saved again with only `7` and `removed_actors = [-4294967295]`
- **THEN** the digest lists only `7` and the `actorprefix` record of `-4294967295` is gone

Evidence: crates/unastar/src/world/block_entity.rs (`read_disk_nbt`, `write_disk_nbt`), crates/unastar/src/storage/leveldb_world.rs (test `actors_are_stored_under_a_chunk_digest_and_removed_records_are_deleted`), crates/unastar/src/storage/blazedb.rs (tests `malformed_block_entity_payloads_fail_without_partial_chunk_fallback`, `actors_round_trip_inside_the_chunk_entry`), crates/unastar/tests/container_persistence.rs (`container_actors_survive_both_provider_reopens`)

### Requirement: LevelDB tables are BDS-compatible
The LevelDB world database SHALL write compressible SST blocks as raw deflate (compression id 4) and incompressible ones uncompressed (id 0), as BDS does. Reads MUST accept raw zlib (4), legacy wrapped zlib (2) and Snappy (1) tables.

#### Scenario: Reading each codec
- **WHEN** checksummed SST tables authored independently of the native writer use codec 4, 2 or 1
- **THEN** the provider reads the stored value from each

#### Scenario: Writing raw zlib
- **WHEN** a stone-filled column is saved and the database is compacted to `.ldb` files
- **THEN** every data block uses id 0 or 4, at least one uses 4, each inflates and checksums independently, and the column loads back

Evidence: crates/unastar/src/storage/leveldb_world.rs (`LevelDBWorldProvider::open`), crates/unastar/tests/leveldb_compression.rs (`reads_raw_zlib_sst_not_just_uncompressed_log`, `reads_legacy_zlib_enum_sst`, `reads_snappy_sst`, `provider_writes_independently_decodable_raw_zlib_tables`)
Verified by: `cargo test -p unastar --test leveldb_compression`

### Requirement: BlazeDB entries are checked before use
BlazeDB SHALL store each column as an appended entry with magic `BLAZ`, its size, a CRC32 of the payload, its coordinates and dimension, keyed by a Morton code of `(x, z, dim)`. Version 4 MUST be written and versions 1 to 3 MUST stay readable. A load MUST fail on bad magic, an entry past the end of the file, mismatched coordinates, a CRC mismatch, an unknown compression id or trailing payload bytes. Saves are queued to a background writer and cached immediately.

#### Scenario: Reopen
- **WHEN** an LZ4-compressed column is saved through the public API, flushed, and the provider reopened
- **THEN** the column loads from disk with the same content

#### Scenario: Oversized index entry
- **WHEN** the index names an entry larger than the data file
- **THEN** the entry is dropped before any read buffer is allocated

Evidence: crates/unastar/src/storage/blazedb.rs (`FORMAT_VERSION`, `load_column`, `deserialize_chunk`, tests `entry_size_includes_compression_prefix`, `saved_column_loads_after_reopen`, `public_lz4_save_column_loads_after_reopen`, `version_1_network_biomes_are_skipped_on_load`, `version_2_data3d_entries_remain_readable_without_metadata`, `legacy_short_size_index_is_repaired_on_open`, `legacy_short_size_entry_is_repaired_during_index_rebuild`, `oversized_index_entry_is_dropped_before_read_allocation`)

### Requirement: Player data is restored before spawn
When `players.leveldb_enabled` is true, player records SHALL be stored in a LevelDB database at `<players.data_dir>/db`, keyed by the 16 UUID bytes, as JSON `PlayerData`. A record MUST be loaded and validated during login and applied when the player entity spawns, before the initial inventory is sent: position, rotation, dimension, game mode, main inventory, armour, offhand, cursor, held slot, ender chest, health, hunger and experience. Fields added later MUST default when missing.

#### Scenario: Disconnect and reconnect
- **WHEN** a player's position, rotation, adventure mode, inventory slot 8 (17 diamonds), damaged helmet, offhand torches, held slot, health 13/24, hunger and experience are saved and the player joins again
- **THEN** every value is restored and the first main-inventory `InventoryContent` shows 17 items in slot 8

#### Scenario: First join
- **WHEN** no record exists for the UUID
- **THEN** `load_player_data` returns `Ok(None)` and first-join defaults apply

Evidence: crates/unastar/src/storage/leveldb_player.rs, crates/unastar/src/storage/provider.rs (`PlayerData`, `StoredItemStack::restore`), crates/unastar/src/server/connect.rs (`load_player_data`), crates/unastar/src/server/game/mod.rs (`spawn_player`, `restore_items`), crates/unastar/tests/persistence.rs (`disconnect_reconnect_restores_inventory_and_player_state`)

### Requirement: Unreadable player records refuse the join and are kept
A player record that cannot be read, fails to parse or fails `PlayerData::validate` (wrong UUID, non-finite position or rotation, unknown game mode, more than 36 inventory slots, held slot above 8, health outside `0..=max`, food outside `0..=20`, negative experience) SHALL refuse the join, transient read errors included, and MUST be left untouched. A record whose items do not resolve against the item registry MUST also refuse the join.

#### Scenario: Corrupt JSON
- **WHEN** the stored value for a UUID is `{not valid json` and that player joins
- **THEN** `load_player_data` returns an error, the client is disconnected with "Unable to load saved player data. Contact the server operator.", the join ends with `JoinError::SavedPlayerData`, the log names the UUID, display name and XUID, and the stored bytes are unchanged afterwards

Evidence: crates/unastar/src/storage/provider.rs (`PlayerData::validate`, `StoredItemStack::restore`), crates/unastar/src/server/connect.rs (`load_player_data`, `accept_join_sequence`, `JoinError::SavedPlayerData`, test `the_unreadable_record_disconnect_points_to_the_operator`), crates/unastar/src/server/runtime.rs (`Rejecting player with invalid saved data`, test `join_with_unresolvable_saved_items_is_rejected`), crates/unastar/tests/persistence.rs (`corrupt_player_record_is_an_error_and_is_kept`)

### Requirement: Player saves use one snapshot on disconnect and shutdown
A player SHALL be saved on disconnect when `players.save_on_disconnect` is true, and every connected player MUST be saved on shutdown. Both MUST write the same `PlayerData` snapshot. When `players.save_previous_position` is true the save MUST also write `<players.data_dir>/<uuid>.toml` with the dimension and position used by spawn rules. A save is reported successful only if every configured store succeeds; a failed save at shutdown makes the shutdown unclean.

#### Scenario: Last position file
- **WHEN** a player at (33.25, 75, -8.5) with yaw 123 and pitch -12 is saved
- **THEN** both the LevelDB record and the per-player TOML hold that position and rotation

Evidence: crates/unastar/src/server/game/mod.rs (`save_player`, `save_all_players`, `save_player_record`), crates/unastar/src/server/runtime.rs (`network_tick`, `shutdown`), crates/unastar/src/config/mod.rs (`PlayerDataStore`), crates/unastar/tests/persistence.rs (`disconnect_reconnect_restores_inventory_and_player_state`), crates/unastar/src/entity/player_life/tests/respawn.rs

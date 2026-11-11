# world-storage Specification

## Purpose
How `unastar` persists worlds and players today: the `WorldProvider` and `PlayerProvider` traits and their implementations in `crates/unastar/src/storage/` (LevelDB, the default, and BlazeDB), the chunk load-before-generate path in `crates/unastar/src/world/ecs/`, the level data (`level.dat` and the level records in `crates/unastar/src/storage/level/`, applied by `crates/unastar/src/world/level_data.rs`), when chunks, players and level data are saved, and the on-disk formats. Generation itself, including the generation pool, is specified in `worldgen-pipeline`; the configuration keys that choose a provider are in `server-config`. Vanilla parity of saving is tracked by the `world.persistence` row in `docs/parity/systems.toml`.

## Requirements

### Requirement: Provider selection and location
At startup the server SHALL open exactly one world provider chosen by `world.storage_provider` (`level_db` by default, or `blaze_db` with `world.blazedb_cache_chunks`) at `worlds/default/db` relative to the working directory, for the single configured `world.dimension`. When the provider cannot be opened the server MUST log a warning and run without chunk persistence rather than exit.

#### Scenario: Default provider
- **WHEN** the server starts with the default configuration
- **THEN** a LevelDB world database is opened at `worlds/default/db` and attached to the `ChunkManager`

#### Scenario: Open failure
- **WHEN** opening the configured world database fails
- **THEN** a warning `chunk persistence disabled` is logged, no provider is attached, and chunks are generated and never saved

Evidence: crates/unastar/src/server/runtime.rs (`UnastarServer::new`), crates/unastar/src/world/mod.rs (`StorageProvider`, `WorldConfig`), crates/unastar/src/config/tests.rs (`storage_settings_reach_runtime_configuration`)

### Requirement: Chunks load from storage before they are generated
Every path that creates a chunk entity (the synchronous `ChunkManager::load_or_generate_chunk` and the asynchronous `request_generation` pool) SHALL ask the provider first and MUST generate terrain only when the provider answers `Ok(None)`. A column loaded from storage MUST NOT be replaced by generated terrain.

#### Scenario: Stored column survives the asynchronous path
- **WHEN** a column with a diamond ore block is saved, then requested through `request_generation` with a Java generator configured
- **THEN** the resulting chunk entity holds the stored diamond ore, not generated terrain

#### Scenario: Stored column survives the synchronous path
- **WHEN** a stored column is requested through `World::get_or_create_chunk`
- **THEN** the entity holds the stored blocks

Evidence: crates/unastar/src/world/ecs/manager.rs (`load_or_generate_chunk`, `request_generation`, `get_or_create_chunk`), crates/unastar/src/world/ecs/generation_worker.rs (`run_worker`), crates/unastar/tests/persistence.rs (`async_stored_chunks_survive_save_cycle_for_both_providers`, `synchronous_world_extension_does_not_replace_stored_column`)

### Requirement: Storage read errors never become generated terrain
A provider read error SHALL stop that chunk from being created; it MUST NOT be treated as a miss. The asynchronous path MUST log the error and drop the request without spawning a chunk; the synchronous path MUST panic rather than generate.

#### Scenario: Unreadable column
- **WHEN** a provider whose `load_column` always fails is attached and a chunk is requested through the generation pool
- **THEN** the request resolves to `Err(StorageError::Database(_))` and `save_column` is never called

Evidence: crates/unastar/src/world/ecs/systems.rs (`process_completed_generations`), crates/unastar/src/world/ecs/manager.rs (`load_or_generate_chunk`), crates/unastar/tests/persistence.rs (`storage_read_errors_never_generate_replacement_terrain`)

### Requirement: Dirty tracking decides what is saved
A chunk entity created from a loaded column SHALL start clean, and one created from generated terrain MUST start dirty. Block edits MUST mark the chunk dirty. A chunk MUST be written only when it is dirty, holds mobs, or had actors stored with it earlier; a successful write MUST clear the dirty flag.

#### Scenario: Loaded column is not rewritten
- **WHEN** a stored column is loaded and `save_all_chunks` runs with no edits
- **THEN** the chunk is not dirty, `save_all_chunks` returns 0, and the stored column is unchanged

Evidence: crates/unastar/src/world/ecs/manager.rs (`chunk_entity_components`), crates/unastar/src/world/ecs/components.rs (`ChunkStateFlags::new_generated`), crates/unastar/src/entity/mob/persistence/mod.rs (`snapshot_chunks`, `mark_saved`), crates/unastar/tests/persistence.rs (`async_stored_chunks_survive_save_cycle_for_both_providers`, `synchronous_world_extension_does_not_replace_stored_column`)

### Requirement: Chunks are saved on unload and on shutdown
A chunk whose unload grace period expires SHALL be saved (when it has something to write) together with the mobs standing in it, and then removed with those mobs. On shutdown or `/stop` the server MUST save all players first, then every loaded chunk that has something to write, then the level data. A failed chunk save MUST be logged; the unload still removes the chunk. There is no periodic autosave.

#### Scenario: Unload saves mobs with the chunk
- **WHEN** a chunk holding a cow unloads and is later loaded again
- **THEN** the cow is gone from the world after unload and restored on reload

#### Scenario: Restart round trip
- **WHEN** cows are saved through LevelDB and the server is rebuilt on the same directory
- **THEN** the cows load back with their saved state

#### Scenario: Level data after the chunks
- **WHEN** the server shuts down
- **THEN** the level data is written after the chunks, so every saved mob has the unique id its scores are stored under

Evidence: crates/unastar/src/world/ecs/systems.rs (`process_chunk_unloads`), crates/unastar/src/server/runtime.rs (`shutdown`), crates/unastar/src/server/game/mod.rs (`save_all_chunks`), crates/unastar/src/entity/mob/persistence/tests.rs (`unloading_saves_and_removes_the_cow_and_reloading_restores_it`, `cows_round_trip_through_leveldb_across_a_restart`, `a_dead_cow_is_deleted_from_storage_on_the_next_save`, `unknown_stored_actors_are_kept_and_written_back`)

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
`LevelDBWorldProvider::save_column` SHALL write a whole column in one LevelDB write batch: chunk version 41, finalisation 2 (populated) as a 4-byte little-endian integer, the Data3D value, every subchunk in Y index -4..19, the block entity record, the actor digest and actor records. An empty block entity list or actor list MUST delete its record so an overwrite leaves no stale data. Loading MUST treat a column as present only when its version record exists.

#### Scenario: Absent column
- **WHEN** `load_column` is called for a chunk with no version record
- **THEN** it returns `Ok(None)`

#### Scenario: Biomes survive a reopen
- **WHEN** a column with mixed biomes is saved, the database is closed and reopened, and the column is loaded
- **THEN** every sampled biome and block matches what was saved

Evidence: crates/unastar/src/storage/leveldb_world.rs (`save_column`, `load_column`), crates/unastar/src/storage/keys.rs (`CHUNK_VERSION`), crates/unastar/tests/persistence.rs (`chunk_biomes_survive_save_and_reopen_for_both_providers`)

### Requirement: Subchunk palettes store runtime block-state ids
Both providers SHALL store each subchunk in the version 9 layout (version byte, storage count, Y index, then one or two paletted storages) whose palette entries are unastar's runtime block-state ids written as signed varints, with the network header bit set. Loading MUST accept versions 8 and 9 and at most two storages. Saved worlds therefore depend on the runtime palette order of the build that wrote them.

#### Scenario: Round trip within one build
- **WHEN** a chunk with stone in every section is saved and loaded by the same build
- **THEN** every block holds the same runtime id

#### Scenario: LevelDB subchunk that does not decode
- **WHEN** a stored LevelDB subchunk has an unsupported version or storage count
- **THEN** a warning is logged and that section loads as air while the rest of the column loads

Evidence: crates/unastar/src/world/chunk.rs (`SUBCHUNK_VERSION`, `encode_subchunk_disk`, `decode_subchunk`, `PalettedStorage::encode`, test `test_subchunk_encoding`), crates/unastar/src/storage/leveldb_world.rs (`load_column`), crates/unastar/tests/leveldb_compression.rs (`provider_writes_independently_decodable_raw_zlib_tables`)

### Requirement: Data3D biomes use the BDS layout
Biomes SHALL be saved as a Data3D value: a 512-byte height map followed by up to 24 biome storages, with header `0xFF` repeating the previous section. Loading MUST also accept unastar's older network biome layout for values shorter than 512 bytes, but only when it parses exactly. Any other malformed value MUST fail the load with a deserialization error instead of losing the biomes.

#### Scenario: Legacy network layout
- **WHEN** a Data3D record holds the LevelChunk network biome encoding
- **THEN** the column loads with those biomes

#### Scenario: Short garbage
- **WHEN** a Data3D record holds 30 bytes that are neither layout
- **THEN** `load_column` returns `Err(StorageError::Deserialization(_))`

Evidence: crates/unastar/src/world/chunk.rs (`encode_biomes_disk`, `decode_biomes_disk`, `DATA3D_HEIGHTMAP_LEN`, tests `data3d_decode_hand_made_blob_with_copy_marker`, `data3d_decode_rejects_malformed_values`, `data3d_round_trips_mixed_biomes_and_writes_heights`), crates/unastar/src/storage/leveldb_world.rs (tests `legacy_network_data3d_loads_with_its_biomes`, `short_garbage_data3d_is_an_error`)

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
- **THEN** `load_player_data` returns `None` and first-join defaults apply

Evidence: crates/unastar/src/storage/leveldb_player.rs, crates/unastar/src/storage/provider.rs (`PlayerData`, `StoredItemStack::restore`), crates/unastar/src/server/connect.rs (`load_player_data`), crates/unastar/src/server/game/mod.rs (`spawn_player`, `restore_items`), crates/unastar/tests/persistence.rs (`disconnect_reconnect_restores_inventory_and_player_state`)

### Requirement: Invalid player records are kept, not deleted
A player record that fails to parse or fails `PlayerData::validate` (wrong UUID, non-finite position or rotation, unknown game mode, more than 36 inventory slots, held slot above 8, health outside `0..=max`, food outside `0..=20`, negative experience) SHALL be logged and ignored for that login, and MUST NOT be deleted by the load. A record whose items do not resolve against the item registry MUST make the join fail with a disconnect message instead of spawning the player with lost items.

#### Scenario: Corrupt JSON
- **WHEN** the stored value for a UUID is `{not valid json`
- **THEN** `load_player_data` returns `None` and the stored bytes are unchanged afterwards

Evidence: crates/unastar/src/storage/provider.rs (`PlayerData::validate`, `StoredItemStack::restore`), crates/unastar/src/server/connect.rs (`load_player_data`), crates/unastar/src/server/runtime.rs (`Rejecting player with invalid saved inventory`), crates/unastar/tests/persistence.rs (`corrupt_player_record_falls_back_without_deleting_data`)

### Requirement: Player saves use one snapshot on disconnect and shutdown
A player SHALL be saved on disconnect when `players.save_on_disconnect` is true, and every connected player MUST be saved on shutdown. Both MUST write the same `PlayerData` snapshot. When `players.save_previous_position` is true the save MUST also write `<players.data_dir>/<uuid>.toml` with the dimension and position used by spawn rules. A save is reported successful only if every configured store succeeds.

#### Scenario: Last position file
- **WHEN** a player at (33.25, 75, -8.5) with yaw 123 and pitch -12 is saved
- **THEN** both the LevelDB record and the per-player TOML hold that position and rotation

Evidence: crates/unastar/src/server/game/mod.rs (`save_player`, `save_all_players`, `save_player_record`), crates/unastar/src/server/runtime.rs (`NetworkEvent::Disconnected` handling, `shutdown`), crates/unastar/src/config/mod.rs (`PlayerDataStore`), crates/unastar/tests/persistence.rs (`disconnect_reconnect_restores_inventory_and_player_state`), crates/unastar/src/entity/player_life/tests/respawn.rs

### Requirement: Level data uses BDS's layout
The world's level data SHALL be stored as BDS 1.26.51 stores it: `level.dat` in the world directory (an `i32` storage version 10, an `i32` length of the rest, one rooted little-endian NBT compound with keys in byte order) and the records `scoreboard`, `mobevents`, `WorldClocks`, `tickingarea_<uuid>` and `structuretemplate_<name>` in the world database. Fields and records the server does not use MUST be written back unchanged.

#### Scenario: A world BDS saved is saved back as BDS wrote it
- **WHEN** a world BDS 1.26.51 saved is loaded and saved again with nothing changed
- **THEN** `level.dat` equals BDS's except `LastPlayed`, `worldStartCount` (one less) and `Difficulty` (the configuration's), the old file is kept as `level.dat_old`, and every level record equals BDS's bytes but the scoreboard's display slot order

#### Scenario: BDS reads what unastar writes
- **WHEN** BDS 1.26.51 opens a world whose level data unastar wrote and is asked for its game rules, clock, weather, scoreboard, ticking areas, mob events and structures
- **THEN** it answers what unastar answers for that world

Evidence: crates/unastar/src/storage/level/ (`dat.rs`, `records.rs`, `nbt.rs`), crates/unastar/src/world/level_data/tests.rs (`a_world_bds_saved_is_saved_back_as_bds_wrote_it`, `bds_reads_the_level_unastar_writes`), crates/unastar/tests/fixtures/bds_1_26_51_level_data.json

### Requirement: Level data is restored before players join
At startup, when a world provider is open, the server SHALL read `level.dat` (a new world gets BDS's fresh-world fields with the configured generator, seed, game mode and difficulty) and the level records, and restore the clock, the tick count, the game rules, the world spawn, the weather levels, the scoreboard, the mob event switches, the ticking areas and the `disk` structures before players join. The difficulty MUST come from the configuration.

#### Scenario: Restart round trip
- **WHEN** level data is changed with commands, the server saves and a new server opens the same directory
- **THEN** every query answers as before, except ticking areas listed in record-key order and `memory` structures, which are gone

#### Scenario: A world BDS saved
- **WHEN** a world BDS 1.26.51 saved is opened
- **THEN** the console answers the game rules, clock, weather, scoreboard, ticking area, mob event and structure queries as BDS answered after reloading it

Evidence: crates/unastar/src/world/level_data.rs (`open`, `LoadedLevel::apply`), crates/unastar/src/server/runtime.rs (`UnastarServer::new`), crates/unastar/src/world/level_data/tests.rs (`level_data_survives_a_restart`, `a_world_bds_saved_answers_like_bds_after_a_reload`)

### Requirement: Unreadable level data is never replaced
An unreadable `level.dat`, `scoreboard`, `mobevents` or `WorldClocks` record SHALL stop the server before it accepts players, and MUST be left unchanged. An unreadable ticking area or structure record MUST be skipped with a warning and left unchanged by later saves.

#### Scenario: Corrupt level.dat
- **WHEN** `level.dat` is truncated by one byte
- **THEN** opening the level data fails and the file keeps its bytes

#### Scenario: Corrupt ticking area
- **WHEN** a `tickingarea_` record does not decode
- **THEN** the other areas load, and the record is still there with its bytes after a save

Evidence: crates/unastar/src/world/level_data.rs (`decode`), crates/unastar/src/world/level_data/tests.rs (`unreadable_level_data_stops_the_start_and_stays_on_disk`)

### Requirement: Removed areas and structures are deleted
A save SHALL write one record per ticking area and per `disk` structure and MUST delete the records of areas and structures it loaded or wrote that no longer exist. `memory` structures MUST NOT be saved.

#### Scenario: Remove and delete
- **WHEN** two areas and two `disk` structures are saved, then one area is removed and one structure deleted, and the level data is saved again
- **THEN** one area record and one structure record remain

Evidence: crates/unastar/src/world/level_data.rs (`capture`), crates/unastar/src/world/level_data/tests.rs (`removed_areas_and_structures_are_deleted_and_memory_saves_are_not_kept`)

### Requirement: Scores are kept while their holders are away
A player's scores SHALL be stored under the id its player record keeps (`unique_id`, created at its first join) and a saved actor's under its unique id. A player who leaves and an actor whose chunk unloads MUST keep their scores, and get them back with the same scoreboard id when they return.

#### Scenario: A player leaves and returns
- **WHEN** a player with saved id 77 has score 5, leaves, and joins again with id 77
- **THEN** its score is 5 under the same scoreboard id

Evidence: crates/unastar/src/world/scoreboard.rs (`PlayerUniqueId`, `Scoreboard::bind`, `Scoreboard::unbind`), crates/unastar/src/world/level_data/tests.rs (`players_and_saved_actors_keep_their_scores_while_away`)

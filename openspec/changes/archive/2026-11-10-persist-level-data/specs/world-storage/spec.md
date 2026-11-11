# Spec Delta

## ADDED Requirements

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

## MODIFIED Requirements

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

## RENAMED Requirements

- FROM: `### Requirement: Subchunk palettes store runtime block-state ids`
- TO: `### Requirement: Subchunk palettes store block names and states`

## MODIFIED Requirements

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

## ADDED Requirements

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

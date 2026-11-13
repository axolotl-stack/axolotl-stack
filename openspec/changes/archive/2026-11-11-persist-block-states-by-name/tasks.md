# Tasks

## 0. Decide

- [x] 0.1 Decision proposed as record 0033 (option A)
- [x] 0.2 Claim `world.persistence` in `docs/parity/claims.toml` with `change = "persist-block-states-by-name"`
- [x] 0.3 Owner approves 0033 (set `status: accepted`) in the implementing pull request
  (approved 2026-11-10)

## 1. Format (option A)

- [x] 1.1 Reverse lookup from (name, states) to runtime id in the block registry, with typed errors
  (`BlockRegistry::state_by_properties`, `UnknownBlockState`; landed with the BDS-world load fix)
- [x] 1.2 Encode subchunk palettes as `{name, states, version}` NBT
  (`PalettedStorage::encode_persistent`, `chunk/persistent.rs` `write_palette_entry`; records as
  BDS writes them on unload: `SubChunk::encode_disk`, `PalettedStorage::compacted`; no record for
  a section of air, the LevelDB provider deletes an older one)
- [x] 1.3 Decode both the NBT palette and the legacy runtime-id palette (per storage, from the
  header's low bit; `PalettedStorage::decode_block_storage`)
- [x] 1.4 Round-trip test across every block state, for the lookup
  (`registry::block::tests::every_state_resolves_from_its_name_and_properties`) and for the
  save/load round trip (`world::chunk::persistent_form_tests::every_block_state_round_trips_through_the_persistent_form`)
- [x] 1.5 Fixture from a BDS 1.26.51 world; load test compares block-for-block
  (`crates/unastar/tests/bds_subchunks.rs`, `tests/fixtures/bds_1_26_51_subchunks.json`,
  written by `tools/bds-sweep/subchunk_fixture.py`)
- [x] 1.6 Measure what BDS writes after edits and copy it byte for byte: the same edits applied
  by BDS and by unastar to one BDS world (`tools/bds-sweep/subchunk_roundtrip.py`); 77 of 77
  records of the edited columns equal; fixture `tests/fixtures/bds_1_26_51_subchunk_edits.json`
  with tests `edited_sections_are_saved_as_bds_saves_them_on_unload`,
  `edited_columns_saved_through_the_leveldb_provider_match_bds` and
  `every_bds_record_is_written_back_byte_for_byte`
- [x] 1.7 Round trip through BDS: BDS reopens the world unastar edited (4,921 of 4,921
  `/testforblock` probes, liquid layers saved back as planned) and a world unastar generated in
  all three dimensions (3,840 of 3,840 probes); the Data3D repeat markers fixed on the way
- [x] 1.8 Legacy saves: runtime-id sections load and are rewritten in BDS's form on the next
  save of their column (`storage::leveldb_world::tests::runtime_id_sections_load_and_are_rewritten_by_name_on_the_next_save`)
- [x] 1.9 A column holding an id that is no canonical block state is refused at save with a
  serialization error, not a panic (`storage::leveldb_world::tests::a_column_with_an_unknown_block_state_is_not_saved`)
- [x] 1.10 Re-capture the worldgen output goldens for the new record format
  (`tests/worldgen_output_hash.rs`, both generator features: the base goldens passed with the
  base encoders; every chunk hash moves, no actor hash does)

## 2. Records

- [x] 2.1 Update `world.persistence` in `docs/parity/systems.toml`; `cargo run -p parity -- generate` and `-- check`
  (stays `partial`: no level data is saved)
- [x] 2.2 Fix the storage docs in `crates/unastar/src/storage/mod.rs` and `crates/unastar/README.md` ("LevelDB / BDS SST interoperability")
- [x] 2.3 Quirks (`docs/quirks/bds.md`, "World / storage") and solution notes
  (`docs/solutions/storage/`)

## 3. Validate

- [x] 3.1 `cargo fmt --all -- --check`
- [x] 3.2 `cargo clippy -p unastar --all-targets -- -D warnings`
- [x] 3.3 `cargo test -p unastar --lib --tests`
- [x] 3.4 `openspec validate persist-block-states-by-name --strict`; run `review-change`

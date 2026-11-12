# Tasks

## 0. Decide

- [x] 0.1 Decision proposed as record 0033 (option A)
- [ ] 0.2 Claim `world.persistence` in `docs/parity/claims.toml` with `change = "persist-block-states-by-name"`
- [ ] 0.3 Owner approves 0033 (set `status: accepted`) in the implementing pull request

## 1. Format (option A)

- [x] 1.1 Reverse lookup from (name, states) to runtime id in the block registry, with typed errors
  (`BlockRegistry::state_by_properties`, `UnknownBlockState`; landed with the BDS-world load fix)
- [ ] 1.2 Encode subchunk palettes as `{name, states, version}` NBT
- [x] 1.3 Decode both the NBT palette and the legacy runtime-id palette (per storage, from the
  header's low bit; `PalettedStorage::decode_block_storage`)
- [x] 1.4 Round-trip test across every block state, for the lookup
  (`registry::block::tests::every_state_resolves_from_its_name_and_properties`); the
  save/load round trip waits for 1.2
- [x] 1.5 Fixture from a BDS 1.26.51 world; load test compares block-for-block
  (`crates/unastar/tests/bds_subchunks.rs`, `tests/fixtures/bds_1_26_51_subchunks.json`,
  written by `tools/bds-sweep/subchunk_fixture.py`)

## 2. Records

- [ ] 2.1 Update `world.persistence` in `docs/parity/systems.toml`; `cargo run -p parity -- generate` and `-- check`
  (reading BDS saves is recorded; the row stays `partial` until 1.2 lands)
- [ ] 2.2 Fix the storage docs in `crates/unastar/src/storage/mod.rs` and `crates/unastar/README.md` ("LevelDB / BDS SST interoperability")
  (both now say BDS saves load and unastar saves don't; finish when 1.2 lands)

## 3. Validate

- [ ] 3.1 `cargo fmt --all -- --check`
- [ ] 3.2 `cargo clippy -p unastar --all-targets -- -D warnings`
- [ ] 3.3 `cargo test -p unastar --lib --tests`
- [ ] 3.4 `openspec validate persist-block-states-by-name --strict`; run `review-change`

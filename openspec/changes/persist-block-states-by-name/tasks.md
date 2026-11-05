# Tasks

## 0. Decide

- [x] 0.1 Decision proposed as record 0033 (option A)
- [ ] 0.2 Claim `world.persistence` in `docs/parity/claims.toml` with `change = "persist-block-states-by-name"`
- [ ] 0.3 Owner approves 0033 (set `status: accepted`) in the implementing pull request

## 1. Format (option A)

- [ ] 1.1 Reverse lookup from (name, states) to runtime id in the block registry, with typed errors
- [ ] 1.2 Encode subchunk palettes as `{name, states, version}` NBT
- [ ] 1.3 Decode both the NBT palette and the legacy runtime-id palette
- [ ] 1.4 Round-trip test across every block state
- [ ] 1.5 Fixture from a BDS 1.26.51 world; load test compares block-for-block

## 2. Records

- [ ] 2.1 Update `world.persistence` in `docs/parity/systems.toml`; `cargo run -p parity -- generate` and `-- check`
- [ ] 2.2 Fix the storage docs in `crates/unastar/src/storage/mod.rs` and `crates/unastar/README.md` ("LevelDB / BDS SST interoperability")

## 3. Validate

- [ ] 3.1 `cargo fmt --all -- --check`
- [ ] 3.2 `cargo clippy -p unastar --all-targets -- -D warnings`
- [ ] 3.3 `cargo test -p unastar --lib --tests`
- [ ] 3.4 `openspec validate persist-block-states-by-name --strict`; run `review-change`

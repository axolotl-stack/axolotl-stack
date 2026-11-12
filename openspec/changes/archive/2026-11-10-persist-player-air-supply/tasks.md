# Tasks

## 1. Implement

- [x] 1.1 `PlayerData::air` with a serde default of 300 and validation of -19..=300
- [x] 1.2 Save the breath's air (full air for a player saved dead); restore it at spawn after the join packet
- [x] 1.3 Tests: `player_life/tests/actor_data.rs::a_rejoining_player_keeps_its_air` (both captured rejoins) and `::the_saved_record_keeps_the_air`

## 2. Records

- [x] 2.1 Update `player.damage_sources` in `docs/parity/systems.toml`; `cargo run -p parity -- generate` and `-- check`
- [x] 2.2 Quirk row "Entities / player air" (saved air) in `docs/quirks/bds.md`; `crates/unastar/README.md` Persistence section

## 3. Validate

- [x] 3.1 `cargo fmt --all -- --check`
- [x] 3.2 `cargo clippy -p unastar --all-targets -- -D warnings`
- [x] 3.3 `cargo test -p unastar --lib --tests`
- [x] 3.4 `openspec validate persist-player-air-supply --strict`; run `review-change`

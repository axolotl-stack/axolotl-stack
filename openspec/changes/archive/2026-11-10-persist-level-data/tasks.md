# Tasks

## 1. Claim and measure

- [x] 1.1 Add a `[[claim]]` for `world.game_rules` (systems) with `change = "persist-level-data"` to `docs/parity/claims.toml` (the commands rows are under the `lane COMMANDS` claim); verify `cargo run -p parity -- check` passes
- [x] 1.2 Measure BDS 1.26.51's level data with `tools/bds-sweep/level_data.py measure`, `clocks` and (after 3.x) `import` on a BDS slot, write `crates/unastar/tests/fixtures/bds_1_26_51_level_data.json` with `fixture`; verify it decodes with `level_data.py dump` and holds `saved`, `query`, `viewer`, `player_set`, `player_join`, `clocks`, `unastar_world` and `import`
- [x] 1.3 Add `--uuid` to `bds-extractor` so an offline player keeps its BDS record across sessions; verify the player-query capture lists the player's scores and tags after a restart

## 2. Storage layer

- [x] 2.1 Ordered, bounded little-endian NBT (`storage/level/nbt.rs`); verify its unit tests (byte-order keys, empty lists, truncation, nesting bound) pass
- [x] 2.2 `level.dat` read/write with header checks and BDS's fresh-world fields (`storage/level/dat.rs`); verify its unit tests pass
- [x] 2.3 Record codecs for `scoreboard`, `mobevents`, `tickingarea_*`, `structuretemplate_*`, `WorldClocks` (`storage/level/records.rs`); verify round-trip and rejection tests pass
- [x] 2.4 Level record store for LevelDB (prefix scan, one batch) and BlazeDB (checked side file), and the world directory's files (`storage/level/mod.rs`); verify `leveldb_keeps_only_level_records` and `blazedb_keeps_level_records_in_a_side_file` pass

## 3. Game state

- [x] 3.1 Scoreboard: stored identities for players away and actors unloaded, `LastUniqueID`, conversions to and from the record, join packets for displayed objectives; player records gain `unique_id`; verify `players_and_saved_actors_keep_their_scores_while_away` and `a_rejoining_player_gets_the_displays_and_scores_like_bds` pass
- [x] 3.2 Ticking areas: record uuid and dimension, circles centred on their chunk at y 0, lists per dimension; verify the `tickingarea` replay test and the set-session comparison pass
- [x] 3.3 Structures: origin and palette conversions (`disk` saves only); verify the BDS-saved structure loads the captured blocks
- [x] 3.4 `world/level_data.rs`: open, apply, capture and write; runtime loads at startup (stopping on unreadable data) and saves after the chunks; new players spawn at the level's world spawn; StartGame's level time and clock; verify `level_data_survives_a_restart`, `a_world_bds_saved_answers_like_bds_after_a_reload`, `a_world_bds_saved_is_saved_back_as_bds_wrote_it`, `bds_reads_the_level_unastar_writes`, `unreadable_level_data_stops_the_start_and_stays_on_disk` pass
- [x] 3.5 `sendCommandFeedback`: no answer for players' successful commands, `silent` failures, no console line for `/gamerule` changes while off; verify `players_get_no_feedback_while_send_command_feedback_is_off_like_bds` and `the_set_commands_answer_like_bds` pass

## 4. Records and docs

- [x] 4.1 Update `docs/parity/commands.toml` (setworldspawn, scoreboard, tickingarea, structure, mobevent, gamerule, time, daylock, difficulty, weather) and `docs/parity/systems.toml` `world.game_rules`; run `cargo run -p parity -- generate` and `-- check`
- [x] 4.2 Log the copied BDS level-data behaviour in `docs/quirks/bds.md` (log-quirk), including the one not copied
- [x] 4.3 Update `crates/unastar/README.md` (Persistence, command differences), `tools/bds-sweep/README.md` and module docs
- [x] 4.4 Python tests for the BDS-free parts of `le_nbt.py` and `level_data.py`; verify `python -m unittest discover -s tools/bds-sweep -p "test_*.py"` passes

## 5. Validation

- [x] 5.1 `cargo fmt --all -- --check`
- [x] 5.2 `cargo clippy -p unastar -p unastar-data -p bds-extractor --all-targets -- -D warnings`
- [x] 5.3 `cargo test -p unastar --lib --tests` and `cargo test -p unastar-data -p parity`
- [x] 5.4 `openspec validate --all --strict`

## Workflow follow-up

- Archive the change, remove its claim, write the decision record for design.md's decisions
  (status proposed), and capture the measurement lessons in `docs/solutions`.

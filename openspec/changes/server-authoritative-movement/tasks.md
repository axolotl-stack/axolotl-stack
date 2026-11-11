# Tasks

## 1. Measure

- [x] 1.1 Sweep client probe orders `teleports follow|late N|ignore|fake`, `corrections follow|ignore`, `offset DX DY DZ`, `drift DX DY DZ` (unit tests in `crates/bds-sweep/src/main.rs`)
- [x] 1.2 BDS runs: teleports, constant offsets, cheats (speed, wall, step, fly, no-fall), acceptance (0.09 to 0.12 offsets, drifts) and short teleports, each with a viewer recording BDS's copy; raw captures kept off the repo
- [x] 1.3 `tools/bds-sweep/authority_fixture.py` distils them into `player_life/testdata/bds-1.26.51-authority.json`; `player_fixture.py` records each phase's teleports and corrections in the movement fixture

## 2. Shared simulation

- [x] 2.1 Move the sweep client's physics to `unastar-rules` (`movement`, with a `Terrain` trait), and re-export it from `bds-sweep` (`cargo test -p unastar-rules`, `cargo test -p bds-sweep`)
- [x] 2.2 BDS's acceptance rules in `movement::authority` (unit tests for pull, take, correction cadence and teleport rest)

## 3. Server

- [x] 3.1 `player_life::authority`: the per-player copy, motion table, trust modes, correction packet; `server/game/movement.rs` judges every input through it and sends the corrections (`player_life/tests/authority.rs`)
- [x] 3.2 Hooks: `/tp`, plugin teleports, ender pearls and chorus fruit (teleport), respawn, knockback (`server/game/command_tests.rs::a_teleport_cannot_be_undone_by_the_input_already_on_its_way`, `player_life/tests/respawn.rs::the_copy_of_the_player_respawns_with_it`, `player_life/tests/movement.rs::a_pushed_player_is_moving`)
- [x] 3.3 Movement replays require BDS's corrections and never one BDS did not send (`player_life/tests/movement.rs`); harness moves update the copy

## 4. Records

- [x] 4.1 Quirk rows: Entities / player movement authority, pull, teleport, no-fall, respawn; Survival: movement exhaustion updated
- [x] 4.2 Decision 0040 (`proposed`) and the README index
- [x] 4.3 Update `player.movement` and `player.damage_sources` in `docs/parity/systems.toml`, and `CorrectPlayerMovePredictionPacket`, `PlayerAuthInputPacket` and `MovePlayerPacket` in `docs/parity/protocol.toml`; `cargo run -p parity -- generate` and `-- check`
- [x] 4.4 Solution note on watching BDS's copy; `crates/unastar/README.md` and `tools/bds-sweep/README.md`

## 5. Validate

- [x] 5.1 `cargo fmt --all -- --check`
- [x] 5.2 `cargo clippy -p unastar -p unastar-rules -p bds-sweep --all-targets -- -D warnings`
- [x] 5.3 `cargo test -p unastar --lib --tests`, `cargo test -p unastar-rules -p bds-sweep -p parity`
- [x] 5.4 `python -m unittest discover -s tools/bds-sweep -p "test_*.py"`
- [x] 5.5 `openspec validate --all --strict`, `bun tools/agents/check-links.ts`; run `review-change`

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

## 5. Review fixes

- [x] 5.1 A trusted input more than 64 blocks from the copy is rejected before it changes the copy (`player_life/tests/authority.rs::flight_is_trusted_within_64_blocks`)
- [x] 5.2 Flight is trusted only in a game mode that can fly, and a client's flight toggles count only there (`::a_survival_client_cannot_fly`)
- [x] 5.3 A trusted input leaves the copy's carried state (sprint speed, sneak, swim, glide, jump delay, collisions) as the client reports it, and clears the pull and count (`::a_sprint_goes_on_after_trusted_inputs`, `unastar-rules` `movement::tests::a_synced_input_clears_the_pull_and_the_count`)
- [x] 5.4 Both block layers are read (waterlogged water); flowing water, ice and unnamed ids are trusted; unloaded blocks add no collision (`::waterlogged_blocks_are_water_and_flowing_water_is_trusted`)
- [x] 5.5 Trust items in use, a sprint at 6 food or less, unconfirmed effects, movement enchantments, riptide spins and crawl spaces (`::situations_the_simulation_does_not_cover_are_trusted`)
- [x] 5.6 One crawl test and one set of box sizes for the actor data and the copy; one block-family mapping for the server and the sweep client; one elytra check

## 6. Second review fixes

- [x] 6.1 A non-finite reported velocity rejects the input; a trusted velocity is capped at 4 a tick per axis (`player_life/tests/authority.rs::a_trusted_velocity_is_bounded`, `server/game/movement.rs::non_finite_positions_and_rotations_are_rejected`)
- [x] 6.2 Trusts a client can seek out (blocks, gear, crawl spaces, riptide) reach about one tick of movement and never through a block the simulation reproduces, and are judged beyond that; crawl spaces count only at the copy (`::near_trusts_do_not_reach_far`)
- [x] 6.3 A riptide spin is trusted only from water and for at most 20 inputs; soul speed only near soul blocks (`::a_riptide_spin_is_trusted_for_twenty_inputs`, `::situations_the_simulation_does_not_cover_are_trusted`)
- [x] 6.4 A client only slower than the copy's simulation of its keys is taken, except after a knockback; item use and low food need no trust reason (`::a_slower_client_is_taken`, `::a_knockback_must_be_followed`)
- [x] 6.5 One `Judgement` (judged, trusted, refused) instead of a verdict and a trust; one layer reader for blocks; `Condition` as a query struct; the swim start reads both block layers; a sprint started without the key counts

## 7. Validate

- [x] 7.1 `cargo fmt --all -- --check`
- [x] 7.2 `cargo clippy -p unastar -p unastar-rules -p bds-sweep --all-targets -- -D warnings`
- [x] 7.3 `cargo test -p unastar --lib --tests`, `cargo test -p unastar-rules -p bds-sweep -p parity`
- [x] 7.4 `python -m unittest discover -s tools/bds-sweep -p "test_*.py"`
- [x] 7.5 `openspec validate --all --strict`, `bun tools/agents/check-links.ts`; run `review-change`

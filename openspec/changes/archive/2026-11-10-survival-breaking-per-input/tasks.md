# Tasks

## 1. Claim and measure

- [x] 1.1 Claim `player.block_breaking` for lane ITEMS in `docs/parity/claims.toml`. The claim names no change because it stays with the lane for the remaining gaps after this change is archived. Verify with `cargo run -p parity -- check`.
- [x] 1.2 Add `--script FILE` to `bds-sweep-client`, with timed orders, slash commands and the orders `pause N`, `burst N` and `release`. Verify with `cargo test -p bds-sweep`.
- [x] 1.3 Add the BDS script runner and breaking scenarios: `tools/bds-sweep/script.py` and `scripts/breaking.txt`. The twelve scenarios cover:
  - dirt by hand, a diamond pickaxe, an instant break and an abort;
  - a frozen client, Haste II and a block above;
  - on-time and early predictions;
  - a release, and blocks put back where a break goes on;
  - a burst.
- [x] 1.4 Capture the scenarios against BDS 1.26.51 and cut the fixture with `tools/bds-sweep/breaking_fixture.py` into `crates/unastar/src/server/game/blocks/testdata/bds-breaking.jsonl`.
- [x] 1.5 Add the `sweep.py mining --variants capture` cells. Verify with `python -m unittest discover -s tools/bds-sweep -p "test_*.py"`.

## 2. Breaking per input

- [x] 2.1 Forward every accepted `PlayerAuthInput` to the block system. Keep the position, block, face, progress, speed and completing break in `BreakingState`. Verify with `cargo test -p unastar --lib server::game::blocks`.
- [x] 2.2 Implement the BDS flow and remove the per-tick crack system:
  - start, continue, abort and stop (a stop does not end the break);
  - a crack event per input;
  - server-completed and predicted breaks, which go on from zero at that position;
  - refused early predictions with the four corrections;
  - no sounds;
  - ignored actions with an invalid face.

  Verify with `survival_breaks_advance_per_input_and_refuse_early_predictions`, `survival_creative_destroy_is_refused`, `breaking_actions_without_a_valid_face_are_ignored` and the loopback test `eat_mine_and_shoot_over_loopback`.
- [x] 2.3 Apply inputs that arrive together in one tick. A player's actions after a completed break wait for the next tick, at most 64. Verify with the burst scenario of the replay test.
- [x] 2.4 Replay the committed capture input by input (`blocks/breaking_tests.rs::survival_breaking_answers_every_input_like_bds`). Check that it fails without each rule: the stop, the start on another block, the speed over air and the wait after a completed break.
- [x] 2.5 Record the copied behaviour in `docs/quirks/bds.md` (Blocks / mining) and the lesson in `docs/solutions/server/block-breaking-advances-per-client-input.md`. Describe the behaviour in `crates/unastar/README.md`, `tools/bds-sweep/README.md` and `docs/guides/tools.md`.

## 3. Parity and validation

- [x] 3.1 Update `player.block_breaking` in `docs/parity/systems.toml` (tests, notes, gaps) and the `PlayerActionPacket` test reference in `docs/parity/protocol.toml`. Then run `cargo run -p parity -- generate` and `cargo run -p parity -- check`.
- [x] 3.2 Run the validation: `cargo fmt --all -- --check`, `cargo clippy -p unastar -p bds-sweep --all-targets -- -D warnings`, `cargo test -p unastar --lib --tests`, `cargo test -p bds-sweep`, `cargo test -p parity` and `openspec validate --all --strict`. Then run the `review-change` skill on the diff.

## Workflow follow-up

- Archived in the same commit as the implementation (`openspec archive survival-breaking-per-input --yes`). The delta renames and replaces the server-authority requirement "Survival breaking is timed by the server" and adds two requirements.

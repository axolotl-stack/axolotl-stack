# Tasks

## 1. Measure

- [x] 1.1 Add `tools/bds-sweep/scripts/breaking-3.txt` (blocks put where a break goes on), capture
  it against BDS 1.26.51 and cut `crates/unastar/src/server/game/blocks/testdata/bds-breaking-3.jsonl`
  with `breaking_fixture.py --windows`.

## 2. BDS mode

- [x] 2.1 Count inputs over the air at the speed of the block found there next
  (`BreakingState::idle_inputs`), and use the speed of the block there now for each input. Verify
  with `breaking_tests.rs::blocks_put_on_a_break_answer_like_bds` and the existing replays.

## 3. Guarded mode

- [x] 3.1 Add `[gameplay] block_breaking` (`BlockBreaking`, default `guarded`) to the config and
  the sample `unastar.toml`.
- [x] 3.2 Add the per-tick input budget (`BreakInputBudget`) and the block-change restart
  (`BreakingState::retarget`). Verify with the `guarded_breaking_*` tests in `blocks.rs`.

## 4. Docs

- [x] 4.1 Decision 0056, the Blocks / mining quirk rows, the `player.block_breaking` parity row,
  and the unastar and bds-sweep READMEs.

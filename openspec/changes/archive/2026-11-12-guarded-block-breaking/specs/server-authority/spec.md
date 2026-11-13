## MODIFIED Requirements

### Requirement: Inputs that arrive together are applied together
In the `bds` breaking mode, a player's `PlayerAuthInput`s that arrive in the same tick SHALL each advance the break, in arrival order, within that tick; the server does not hold a client to server time. A player's block actions after an input that completed a break MUST wait for the next tick, when that break has been committed. At most 64 actions MAY wait per player; more MUST be dropped.

#### Scenario: A burst of inputs
- **WHEN** a survival player breaking dirt by hand on a `bds` server sends its 2nd to 16th inputs at once
- **THEN** the block breaks on the 15th input without waiting for further ticks, and the 16th input finds air

Evidence: crates/unastar/src/server/game/blocks.rs (`apply_block_actions`, `MAX_WAITING_BLOCK_ACTIONS`), crates/unastar/src/server/game/blocks/breaking_tests.rs (`survival_breaking_answers_every_input_like_bds`: the burst scenario), [docs/quirks/bds.md](../../../../../../docs/quirks/bds.md) (Blocks / mining: inputs are not held to server time)

## ADDED Requirements

### Requirement: Guarded breaking is the default
Survival breaking SHALL follow `[gameplay] block_breaking`: `"guarded"` unless configured otherwise, or `"bds"`. In the guarded mode, since a break's progress last began from zero at most one input of progress SHALL count per server tick, counted from the tick of its first input; a new progress MUST NOT count an input in a tick where the previous one counted one. Every input is still answered as in the `bds` mode.

#### Scenario: A flood of inputs
- **WHEN** a survival player breaking dirt by hand on a guarded server sends 21 inputs in one tick and then 3 inputs in each later tick
- **THEN** the dirt breaks in the 15th tick, not on the 15th input

#### Scenario: A lagging client
- **WHEN** a breaking player's inputs for ten ticks arrive together
- **THEN** they count up to the ticks elapsed since the progress began, and no further

Evidence: crates/unastar/src/server/config.rs (`BlockBreaking`), crates/unastar/src/config/mod.rs (`GameplayConfig`), crates/unastar/src/entity/components/player.rs (`BreakInputBudget`, `BreakingState::retarget`), crates/unastar/src/server/game/blocks.rs (`guard_input`, tests `guarded_breaking_counts_one_input_per_tick`, `guarded_breaking_lets_a_lagging_client_catch_up`, `guarded_breaking_begins_again_on_a_block_placed_over_air`, `guarded_breaking_begins_again_on_another_block`), [docs/decisions/0056-guarded-block-breaking.md](../../../../../../docs/decisions/0056-guarded-block-breaking.md)

### Requirement: Guarded breaking begins again on another block
In the guarded mode the progress of a break MUST begin again when the block at its position is no longer the one being broken: another block, or one put there after the progress began over the air.

#### Scenario: A block put on a break's air
- **WHEN** dirt is broken by hand on a guarded server, the player keeps breaking the air for 30 inputs, and dirt is put back
- **THEN** the new dirt's progress begins from zero, where a `bds` server breaks it on the next input

Evidence: crates/unastar/src/server/config.rs (`BlockBreaking`), crates/unastar/src/config/mod.rs (`GameplayConfig`), crates/unastar/src/entity/components/player.rs (`BreakInputBudget`, `BreakingState::retarget`), crates/unastar/src/server/game/blocks.rs (`guard_input`, tests `guarded_breaking_counts_one_input_per_tick`, `guarded_breaking_lets_a_lagging_client_catch_up`, `guarded_breaking_begins_again_on_a_block_placed_over_air`, `guarded_breaking_begins_again_on_another_block`), [docs/decisions/0056-guarded-block-breaking.md](../../../../../../docs/decisions/0056-guarded-block-breaking.md)

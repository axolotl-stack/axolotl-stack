## RENAMED Requirements

- FROM: `### Requirement: Survival breaking is timed by the server`
- TO: `### Requirement: Survival breaking advances once per input on the server`

## MODIFIED Requirements

### Requirement: Survival breaking advances once per input on the server
The server SHALL advance each survival break's progress by its own per-input break fraction for the held item, block and player state, once per accepted `PlayerAuthInput` of the breaking player (idle inputs too), never on a tick without one. The block MUST break only when that progress completes, by the server itself; an earlier `PredictDestroyBlock` MUST get `UpdateBlock` corrections and not end the break. Creative players break instantly. At most 64 block actions per input are processed.

#### Scenario: Skipping the mining time
- **WHEN** a survival player sends `PredictDestroyBlock` on the 8th input of a 15-input break
- **THEN** the block is unchanged, the client receives `UpdateBlock` corrections, and the block still breaks on the 15th input

#### Scenario: The server completes the break
- **WHEN** a survival player breaking dirt by hand keeps sending inputs without predicting the break
- **THEN** the block breaks on the 15th input

#### Scenario: A frozen client
- **WHEN** a breaking player sends no `PlayerAuthInput` for ten server ticks
- **THEN** the break does not advance during those ticks and still completes on its 15th input

Evidence: crates/unastar/src/server/game/blocks.rs (`MAX_BLOCK_ACTIONS`, `process_auth_input_block_actions`, `advance_breaking`, `refuse_prediction`, `break_block`, tests `survival_breaks_advance_per_input_and_refuse_early_predictions`, `break_time_uses_tool_and_ground_state`), crates/unastar/src/server/game/blocks/breaking_tests.rs (`survival_breaking_answers_every_input_like_bds`, replaying `testdata/bds-breaking.jsonl`), crates/unastar/src/entity/components/player.rs (`BreakingState`), crates/unastar/src/server/game/movement.rs (`apply_movement`), [docs/parity/systems.toml](../../../docs/parity/systems.toml) (`player.block_breaking`), [docs/quirks/bds.md](../../../docs/quirks/bds.md) (Blocks / mining: the packets that answer each input, and when a break ends)

## ADDED Requirements

### Requirement: Survival client actions cannot break a block
In survival only the server's completed progress SHALL break a block, and never air or an unbreakable block. A survival player's `CreativeDestroyBlock` MUST NOT break a block. `StopDestroyBlock` and `AbortDestroyBlock` MUST NOT break a block. A breaking action whose face is outside 0 to 5 MUST be ignored.

#### Scenario: A creative destroy from survival
- **WHEN** a survival player sends `CreativeDestroyBlock` for a block within reach
- **THEN** the block is unchanged

#### Scenario: A released break
- **WHEN** a survival player sends `StopDestroyBlock` on the 6th input of a 15-input break
- **THEN** the block is unchanged on that input

#### Scenario: An aborted break
- **WHEN** a survival player aborts a break after five inputs and keeps sending inputs
- **THEN** the block is unchanged

#### Scenario: No valid face
- **WHEN** a survival player sends `StartDestroyBlock` with face -602
- **THEN** no break starts and no event is sent for it

Evidence: crates/unastar/src/server/game/blocks.rs (`process_auth_input_block_actions`, `break_block`, tests `survival_creative_destroy_is_refused`, `breaking_actions_without_a_valid_face_are_ignored`), crates/unastar/src/server/game/blocks/breaking_tests.rs (`survival_breaking_answers_every_input_like_bds`: the abort and release scenarios)

### Requirement: Inputs that arrive together are applied together
A player's `PlayerAuthInput`s that arrive in the same tick SHALL each advance the break, in arrival order, within that tick; the server does not hold a client to server time. A player's block actions after an input that completed a break MUST wait for the next tick, when that break has been committed. At most 64 actions MAY wait per player; more MUST be dropped.

#### Scenario: A burst of inputs
- **WHEN** a survival player breaking dirt by hand sends its 2nd to 16th inputs at once
- **THEN** the block breaks on the 15th input without waiting for further ticks, and the 16th input finds air

Evidence: crates/unastar/src/server/game/blocks.rs (`apply_block_actions`, `MAX_WAITING_BLOCK_ACTIONS`), crates/unastar/src/server/game/blocks/breaking_tests.rs (`survival_breaking_answers_every_input_like_bds`: the burst scenario), [docs/quirks/bds.md](../../../docs/quirks/bds.md) (Blocks / mining: inputs are not held to server time)

## ADDED Requirements

### Requirement: Game packets before StartGame
The server SHALL send the application's `WorldJoinParams::before_start_game` packets first in the `StartGame` batch, in their given order, before `DimensionData` and `JigsawStructureData`; jolyne MUST NOT inspect or reorder them.

#### Scenario: Pre-StartGame packets open the StartGame batch
- **WHEN** the application passes `LevelEventGeneric`, `PlayerList` and `SyncWorldClocks` in `before_start_game`
- **THEN** the server sends them in that order directly before `JigsawStructureData`, in the same single batch as `StartGame`, and the packets still follow the BDS trace

Evidence: crates/jolyne/src/proto/server.rs (`send_start_game`), crates/jolyne/src/world.rs (`WorldJoinParams`), crates/jolyne/src/proto/tests.rs (`game_packets_can_precede_start_game_in_its_batch`), docs/decisions/0044-send-pre-start-game-roster-packets-before-start-game.md

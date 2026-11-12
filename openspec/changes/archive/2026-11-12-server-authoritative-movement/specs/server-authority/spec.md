## RENAMED Requirements

- FROM: `### Requirement: Client-reported movement is accepted within a per-tick delta`
- TO: `### Requirement: Player movement is simulated and judged by the server`

## MODIFIED Requirements

### Requirement: Player movement is simulated and judged by the server
The server SHALL keep its own copy of each player's movement, simulate every `PlayerAuthInput` on it and judge the reported position with the acceptance rules in `docs/quirks/bds.md` (Entities / player movement authority, pull), answering a refused report with `CorrectPlayerMovePrediction` carrying the copy's state after that input. The copy MUST be the player's position for viewers, falls, `MOVING` and exhaustion. An input with a non-finite value MUST be rejected whole.

#### Scenario: Teleport-sized jump
- **WHEN** a `PlayerAuthInput` reports a position 100 blocks from the server's copy of a standing player
- **THEN** the player stays where the copy is and the client is corrected to it

#### Scenario: Too fast
- **WHEN** a client walking forward reports positions 0.4 blocks a tick ahead (authority capture phase `speed`)
- **THEN** the copy walks at walking speed and the client is corrected on the inputs BDS corrected, to BDS's position and velocity

#### Scenario: Through a wall
- **WHEN** a client reports moving 0.2 blocks a tick through a stone wall (phase `phase_wall`)
- **THEN** the copy stops at the wall and the client is corrected as BDS corrected it

#### Scenario: Claiming the ground while falling
- **WHEN** a client falling 10 blocks reports vertical collision on every input (phase `nofall`)
- **THEN** the copy falls, lands and deals the 7 damage BDS dealt

#### Scenario: A close position is taken
- **WHEN** a standing client reports a position 0.09 blocks from the copy (phase `offset_z_0_09`)
- **THEN** the copy takes that position and no correction is sent

#### Scenario: Non-finite input
- **WHEN** a `PlayerAuthInput` carries a NaN position or rotation
- **THEN** it is rejected whole with the warning `Rejected non-finite PlayerAuthInput` and the player does not move

Evidence: crates/unastar-rules/src/movement/mod.rs (`step`), crates/unastar-rules/src/movement/authority.rs (`Authority::judge`), crates/unastar/src/entity/player_life/authority.rs (`PlayerMotion::input`), crates/unastar/src/server/game/movement.rs (`process_auth_input`, test `non_finite_positions_and_rotations_are_rejected`), crates/unastar/src/entity/player_life/tests/authority.rs (`the_copy_moves_and_corrects_like_bds`, `close_positions_bds_keeps_are_taken`, `cheated_falls_hurt_like_bds`, `a_teleport_sized_jump_is_corrected`), crates/unastar/src/entity/player_life/testdata/bds-1.26.51-authority.json

## ADDED Requirements

### Requirement: A server teleport is enforced on the next input
When the server moves a player (a command, a plugin, an ender pearl or chorus fruit), it SHALL NOT simulate the player's next input: the copy MUST stay at rest at the destination, and a reported position beyond the acceptance threshold MUST be corrected to it, whether or not the input says `HandledTeleport` and even when that input would otherwise be trusted. A respawn MUST NOT count as a teleport: the next input is simulated from the spawn point.

#### Scenario: An input sent before the teleport
- **WHEN** `/tp` moves a player 8 blocks and its next input still reports the old position
- **THEN** that input is corrected to the destination with zero velocity and on the ground, and the player stays at the destination

#### Scenario: A client that never follows
- **WHEN** a client ignores a teleport and every correction (phase `tp_ignore_uncorrected`)
- **THEN** the copy stays at the destination and the client is corrected on the inputs BDS corrected

#### Scenario: A respawn the client ignores
- **WHEN** a respawned client's next input still reports where it died
- **THEN** that input is corrected to the copy simulated from the spawn point through that input

Evidence: crates/unastar/src/entity/player_life/authority.rs (`PlayerMotion::teleported`, `PlayerMotion::respawned`), crates/unastar/src/command/exec/teleport.rs, crates/unastar/src/server/game/item_use/mod.rs (`teleport`), crates/unastar/src/server/game/plugins.rs, crates/unastar/src/entity/player_life/respawn.rs, crates/unastar/src/server/game/command_tests.rs (`a_teleport_cannot_be_undone_by_the_input_already_on_its_way`), crates/unastar/src/entity/player_life/tests/authority.rs (`the_copy_moves_and_corrects_like_bds`), crates/unastar/src/entity/player_life/tests/respawn.rs (`the_copy_of_the_player_respawns_with_it`)

### Requirement: Movement the simulation does not cover is trusted
The server SHALL take a client's state without judging it where the simulation does not reproduce vanilla: flight in a game mode that can fly, movement effects, nearby blocks with rules of their own, movement enchantments where they act, a riptide spin started in water, a crawl space, a client only slower than the copy's simulation of its keys (not after a knockback), or the first input after a join or respawn near the spawn point. Each trust MUST be bounded by the move it allows.

#### Scenario: Replayed vanilla movement
- **WHEN** the BDS movement capture's phases (walking, sprinting, sneaking, jumps, swims, glides, falls onto slime, beds, hay, water and powder snow, fire, lava, knockback) are replayed
- **THEN** the server sends no correction BDS did not send, and every correction it sends carries BDS's position and velocity

#### Scenario: Flying
- **WHEN** a flying creative player reports a position 10 blocks above the copy
- **THEN** the copy takes it and no correction is sent

#### Scenario: Teleport-sized jump while flying
- **WHEN** a flying creative player reports a position 100 blocks from the copy, twice
- **THEN** both inputs are rejected with the warning `Rejected implausible PlayerAuthInput movement delta`, the copy stays where it was and no correction is sent

#### Scenario: Claimed flight
- **WHEN** a survival client sends `StartFlying` with a position 5 blocks above the copy
- **THEN** the flag is ignored and the input is judged and corrected

#### Scenario: Situations the simulation does not cover
- **WHEN** a standing player sends an input under jump boost, sneaking in swift sneak leggings, next to water in depth strider boots, on soul soil in soul speed boots, starting a riptide spin next to water, under a block at eye height or on ice
- **THEN** the input is trusted and no correction is sent

#### Scenario: A slower client
- **WHEN** a client eats while it walks, or holds the sprint key at 6 food, and so moves slower than the copy's simulation of its keys
- **THEN** each such input is taken and no correction is sent

#### Scenario: A knockback refused
- **WHEN** a client stands still after the server knocks it back
- **THEN** its inputs are judged, not taken as slower, and it is corrected

#### Scenario: Out of a near trust's reach
- **WHEN** a client next to a ladder reports a position 10 blocks away or inside the wall beside it, or claims a crawl space 5 blocks away
- **THEN** the input is judged and corrected

#### Scenario: A bounded velocity
- **WHEN** a trusted input reports a velocity of 1e30 blocks a tick
- **THEN** the copy's velocity is 4 blocks a tick on that axis

#### Scenario: Judging resumes where the client is
- **WHEN** a client starts sprinting during trusted inputs and keeps sprinting after they end
- **THEN** the copy follows it exactly, at sprint speed, without a correction

Evidence: crates/unastar/src/entity/player_life/authority.rs (`MotionTable`, `Trust`, `Situation`, `PlayerMotion::input`), crates/unastar/src/server/game/movement.rs (`MAX_AUTH_INPUT_MOVE_DELTA_PER_TICK`, `situation_of`, test `moves_within_the_delta_are_accepted_and_larger_ones_rejected`), crates/unastar/src/entity/player_life/tests/authority.rs (`flight_is_trusted_within_64_blocks`, `a_trusted_velocity_is_bounded`, `a_survival_client_cannot_fly`, `situations_the_simulation_does_not_cover_are_trusted`, `a_slower_client_is_taken`, `a_knockback_must_be_followed`, `near_trusts_do_not_reach_far`, `a_riptide_spin_is_trusted_for_twenty_inputs`, `a_sprint_goes_on_after_trusted_inputs`, `waterlogged_blocks_are_water_and_flowing_water_is_trusted`), crates/unastar/src/entity/player_life/tests/movement.rs (`assert_corrections_like_bds`), docs/parity/systems.toml (`player.movement`)

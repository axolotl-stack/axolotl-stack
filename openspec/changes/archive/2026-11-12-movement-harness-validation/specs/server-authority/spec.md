# Delta for server-authority

## MODIFIED Requirements

### Requirement: Movement the simulation does not cover is trusted
The server SHALL take a client's state without judging it where the simulation does not reproduce vanilla: flight in a game mode that can fly, nearby blocks with rules of their own, movement enchantments where they act, a riptide spin started in water, a crawl space, a client only slower than the copy's simulation of its keys (not after a knockback), or the first input after a join or respawn near the spawn point. Each trust MUST be bounded by the move it allows.

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
- **WHEN** a standing player sends an input sneaking in swift sneak leggings, next to water in depth strider boots, on soul soil in soul speed boots, starting a riptide spin next to water, under a block at eye height or next to a bubble column
- **THEN** the input is trusted and no correction is sent

#### Scenario: Situations the movement differential confirmed
- **WHEN** a standing player sends an input under slowness or levitation, next to scaffolding or next to a lava source
- **THEN** the input is judged, not trusted

#### Scenario: The movement attribute under effects
- **WHEN** a player sprints under slowness II
- **THEN** its movement attribute is 0.091 with the `MovementSlowdown` modifier (-0.3) listed before the sprint's

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

Evidence: crates/unastar/src/entity/player_life/authority.rs (`MotionTable`, `Trust`, `Situation`, `PlayerMotion::input`), crates/unastar/src/server/game/movement.rs (`MAX_AUTH_INPUT_MOVE_DELTA_PER_TICK`, `situation_of`, test `moves_within_the_delta_are_accepted_and_larger_ones_rejected`), crates/unastar/src/entity/player_life/tests/authority.rs (`flight_is_trusted_within_64_blocks`, `a_trusted_velocity_is_bounded`, `a_survival_client_cannot_fly`, `situations_the_simulation_does_not_cover_are_trusted`, `a_slower_client_is_taken`, `a_knockback_must_be_followed`, `near_trusts_do_not_reach_far`, `a_riptide_spin_is_trusted_for_twenty_inputs`, `a_sprint_goes_on_after_trusted_inputs`, `waterlogged_blocks_are_water_and_flowing_water_is_trusted`), crates/unastar/src/server/game/attributes.rs (`effect_modifiers_are_listed_as_bds_sends_them`), crates/bds-sweep/tests/movement_diff.rs (`unastar_moves_like_bds_in_every_scenario`), crates/unastar/src/entity/player_life/tests/movement.rs (`assert_corrections_like_bds`), [docs/parity/systems.toml](../../../../../../docs/parity/systems.toml) (`player.movement`)

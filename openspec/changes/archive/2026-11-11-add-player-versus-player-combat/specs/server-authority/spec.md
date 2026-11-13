## ADDED Requirements

### Requirement: Attacks on players are checked against server state
An attack aimed at another player SHALL be applied only when the attacker is alive and not a spectator, the victim is a live player that can take damage, the `pvp` game rule is on, the transaction names the held hotbar slot and its item, its claimed position is within 6 blocks of the attacker's tracked position, and the two tracked positions are within 6.7 blocks (12 in creative). A refused attack MUST change nothing and send nothing.

#### Scenario: Out of reach
- **WHEN** a survival player attacks a player whose position is 6.8 blocks from its own
- **THEN** the victim's health and velocity are unchanged and no packet is sent

#### Scenario: Claimed position off by more than 6 blocks
- **WHEN** the transaction's claimed position is 6.1 blocks from the attacker's tracked position
- **THEN** the attack is refused

#### Scenario: Spectator attacker
- **WHEN** a spectator sends an attack on a survival player within reach
- **THEN** the attack is refused

Evidence: crates/unastar/src/entity/player_life/pvp.rs (`check`, `Refusal`), crates/unastar/src/entity/player_life/tests/pvp.rs (`reach_and_the_claimed_position_limit_hits_like_bds`, `game_modes_and_the_pvp_rule_decide_who_can_be_hit`, `a_spectator_cannot_hit`)

### Requirement: A reported velocity is finite and bounded
The velocity a `PlayerAuthInput` reports (`pos_delta`) MUST be finite, or the input is rejected whole. The velocity a knockback keeps from it MUST be clamped on each axis to the per-input movement limit.

#### Scenario: Non-finite reported velocity
- **WHEN** a `PlayerAuthInput` reports a NaN or infinite `pos_delta`
- **THEN** the input is rejected whole, like a non-finite position

Evidence: crates/unastar/src/server/game/movement.rs (`auth_input_is_finite`, test `non_finite_positions_and_rotations_are_rejected`)

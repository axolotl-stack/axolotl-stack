## ADDED Requirements

### Requirement: A hopping mob moves only through its hop control
A mob with `minecraft:movement.jump` SHALL never follow a path: its goals only set the yaw it
wants and whether it moves, and the hop control turns it, counts its hop delay down on the
ground and jumps. Goals that path (strolling, panicking) MUST NOT move such a mob.

#### Scenario: A calm slime
- **WHEN** a slime with no target stands on the ground
- **THEN** it waits a delay drawn from its `jump_delay`, cut to whole ticks, then hops along its body yaw

Evidence: crates/unastar/src/entity/mob/ai/hop.rs, crates/unastar/src/entity/mob/tests/slime.rs (test `hop_delays_and_random_turns_match_bds_by_distribution`)

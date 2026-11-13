## ADDED Requirements

### Requirement: A stomp reach is judged after the mob moved
A melee goal with the stomp reach SHALL judge its hit, its rear-up and its cooldown restart
once a tick, after the physics step, from the mob's feet after that tick's move and its
target's feet at that point (a player's are its last reported position). A box-reach melee
goal MUST keep deciding inside the AI tick.

#### Scenario: A hostile polar bear next to its target
- **WHEN** a hostile polar bear's target stands within its stomp reach
- **THEN** the bear raises `STANDING` before its cooldown runs out, then hits and lowers it, once per cooldown

Evidence: crates/unastar/src/entity/mob/ai/melee.rs (`StompClock`, `tick_stomp_attacks`), crates/unastar/src/entity/mob/tests/polar_bear.rs (test `a_hostile_polar_bear_rears_up_then_stomps_its_target`)

## ADDED Requirements

### Requirement: A swimming mob moves through its swim path
A mob whose navigation can swim SHALL, while in water, path over water cells (and land cells
when it can walk) and push along the line to its waypoint, pitch included, by its underwater
movement attribute, with no gravity while it swims. Out of water, or with a navigation that
cannot swim, it MUST use its land path.

#### Scenario: A drowned hunting a player above it
- **WHEN** a hunting drowned on a pool floor targets a player standing at the surface
- **THEN** it rises along the straight line to the player's feet at its underwater speed, with no gravity

Evidence: crates/unastar/src/entity/navigation/swim.rs, crates/unastar/src/entity/mob/tests/drowned.rs (test `a_hunting_drowned_swims_straight_at_a_target_above_it`)

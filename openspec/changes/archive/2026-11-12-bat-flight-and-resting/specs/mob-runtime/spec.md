## ADDED Requirements

### Requirement: A float-navigating mob moves only through its float control
A mob with `minecraft:navigation.float` SHALL never follow a path. Its goals only set the
point it flies at, and its float control adds its movement speed times the unit vector from
its feet to that point to its velocity. Its body MUST have no gravity, whatever its
`minecraft:physics` authors.

#### Scenario: A wandering bat
- **WHEN** a bat's wander point lies beyond a wall
- **THEN** it pushes straight at the point every tick of the wander and bumps into the wall, and it never falls

Evidence: crates/unastar/src/entity/mob/ai/fly.rs, crates/unastar/src/entity/mob/tests/bat.rs (test `a_flying_bat_pushes_at_its_point_like_bds`)

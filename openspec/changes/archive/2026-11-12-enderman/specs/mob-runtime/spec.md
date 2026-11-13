## ADDED Requirements

### Requirement: A teleport lands only where the mob fits
A teleport attempt SHALL drop its target onto the first block below it that has a collision box
and SHALL move the mob there only when its box then collides with no block and touches no liquid.
A failed attempt MUST leave the mob where it was and send nothing.

#### Scenario: A target inside the ground
- **WHEN** an enderman on a superflat draws a target below the surface
- **THEN** the attempt fails and the enderman stays

#### Scenario: A target over water
- **WHEN** the target drops onto water
- **THEN** the attempt fails

Evidence: crates/unastar/src/entity/mob/life/teleport.rs (`landing`), crates/unastar/src/entity/mob/tests/enderman.rs (tests `an_enderman_tries_one_teleport_in_its_spawn_tick_as_on_bds`, `an_enderman_never_lands_in_water_as_on_bds`)

### Requirement: A dodged projectile flies on
A projectile about to hit a mob with `teleports_on_projectile_hit` SHALL let the mob make
teleport attempts first; when one lands, the projectile MUST NOT hit the mob and keeps flying.
When none lands, the hit goes through.

#### Scenario: An arrow at an enderman in the open
- **WHEN** an arrow reaches an enderman that can teleport
- **THEN** the enderman teleports, takes no damage, and the arrow flies on

#### Scenario: An arrow at a sealed enderman
- **WHEN** no teleport can land
- **THEN** the arrow hits it

Evidence: crates/unastar/src/entity/projectile/flight.rs, crates/unastar/src/entity/mob/life/teleport.rs (`dodge_projectile`), crates/unastar/src/entity/mob/tests/enderman.rs (test `arrows_make_the_enderman_teleport_as_on_bds`)

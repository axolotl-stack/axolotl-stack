## ADDED Requirements

### Requirement: A wall climber climbs its blocked moves
A body with `navigation.climb` SHALL keep whether its last move was blocked horizontally and show it as the `WALLCLIMBING` actor flag. A blocked move made while that state was already set MUST leave the body rising at the climb speed, with no gravity or drag that tick. A climber chasing its target whose walk path search fails SHALL head straight for the target.

#### Scenario: A spider walks into a 3-tall block
- **WHEN** a spider walking west is first blocked by the block
- **THEN** it shows `WALLCLIMBING` that tick, rises 0.2 a tick from the next blocked tick, and lands on the block

Evidence: crates/unastar/src/entity/systems/physics.rs (`step_body_with_effects`), crates/unastar/src/entity/mob/ai/climb.rs, crates/unastar/src/entity/mob/tests/spider.rs (tests `a_spider_climbs_a_wall_as_on_bds`, `a_spider_chasing_the_player_climbs_onto_its_block`)

### Requirement: Boxes within a rounding error of a block touch it
Collision clipping SHALL treat a moving box that overlaps a block's box by less than 1e-7 on an axis as touching that face, so a box pressed against a wall is stopped at the face instead of moving through it.

#### Scenario: A spider pressed against a wall
- **WHEN** a spider's box rests a rounding error inside a wall and it walks into it again
- **THEN** its horizontal move is blocked

Evidence: crates/unastar/src/registry/collision.rs (`CLIP_EPSILON`), crates/unastar/src/entity/mob/tests/spider.rs (test `a_spider_climbs_a_wall_as_on_bds`)

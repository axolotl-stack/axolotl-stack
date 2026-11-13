## ADDED Requirements

### Requirement: The vex flies through one held course
A vex SHALL never follow a path. Its move control MUST hold one point until its feet come
closer to it than its box size, and on that tick keep half its velocity and drop the point.
On every other tick it MUST add `0.05 × speed × movement` times the unit vector from its feet
to the point. The control steers before the goals think, so a course a goal sets is first
flown on the next tick.

#### Scenario: A charging vex
- **WHEN** a vex's charge starts on a survival player
- **THEN** it pushes at the player from the next tick on and hits on the tick of the move that makes their boxes touch

Evidence: crates/unastar/src/entity/mob/ai/vex.rs, crates/unastar/src/entity/mob/tests/vex.rs (tests `a_wandering_vex_flies_its_course_like_bds`, `a_vex_charges_and_hits_a_survival_player`)

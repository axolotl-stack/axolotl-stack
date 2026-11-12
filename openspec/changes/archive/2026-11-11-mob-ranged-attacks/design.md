# Design: mob ranged attacks

## Measurements

All values come from BDS 1.26.51 captures (`sweep.py ranged --client-capture`, fixture
`crates/unastar/src/entity/mob/combat/testdata/bds-1.26.51-ranged.json`). Each choice below
names the run that settled it; the replays live in `mob/combat/bds_ranged.rs`.

- Timing: the shot interval is the authored interval plus one tick; the first shot comes one
  tick later still after the target (every item on easy, normal and hard).
- Start: one block from the shooter's feet toward the target's feet, horizontally, 0.9 of the
  shooter's height up plus the arrow's −0.1 offset (`geometry`: off-axis, diagonal and raised
  shooters, to 1e-6).
- Aim: a straight line toward a point above the feet (the first guess, with Java's `0.2 ×
  distance` arc) did not fit the `aim` set (residuals grow with height difference). A
  drag-aware ballistic solve fit with one free parameter, and the drag-free parabola of the
  arrow's power and gravity through the centre of the target's box, measured from the
  shooter's horizontal position, fit best (chi²/dof 0.44 over 8 geometries, no free
  parameter). That is what is implemented. The older integration branch reached the same arc
  independently.
- Spread: gaussian per axis, `0.0075 × (uncertainty_base − difficulty ×
  uncertainty_multiplier)`; a triangle draw is excluded (one deviation past its bound,
  kurtosis 3.2 over 91 shots).
- Damage: `ceil(speed × 2 × f)`. The factor's mean rises 0.06 per difficulty level
  (1.035 / 1.095 / 1.155). A uniform spread of ±0.225 fits 1.5 log units better than a
  gaussian of sd 0.13, and 3.3 better than the older branch's `2 + 0.11d ± 0.25 gaussian`, over
  291 hits; the shape stays provisional.
- `has_ranged_weapon`: only a bow (`weapon` set).
- `is_underground`: any light-damping block above the feet (`filters`: stone and leaves yes,
  glass no). The server's height map counts glass, so it is not used here.
- `in_block`: any block overlapping the actor's box (powder snow at the feet or the head).
- The skeleton's `other_with_families` sensor: a spring-trapped skeleton still takes hits in
  full. Reading the key as never-matching and reading it as a legacy `is_family` test both fit
  that, because nothing here strikes with lightning; never-matching follows the
  rules-engine contract (unknown names evaluate false).
- Avoidance: the flight runs until its path ends, past `max_flee` (`avoid`, `avoid-many`).

## Decisions

- Fields that only a few vanilla mobs author and that were not measured are rejected at
  compile time with a typed error (charged and burst attacks, `follow_target`, magic and
  multi-projectile shooters, `avoid_mob_type` sounds, `remove_target`, `on_escape_event`,
  `probability_per_strength`, `check_if_outnumbered`, goal-level distances). Guessing them
  would spawn mobs that behave wrongly.
- The tipped-arrow effects are a table of the three measured aux values; another aux value
  rejects the shooter.
- The movement attribute drives the AI's walking speed. Before, the authored base did, so
  slowness and speed only changed the attribute the client saw.

## Known differences kept

- unastar sends the shot's `undefined` and `bow` sounds before the arrow's AddActor and its
  `disappeared` a tick later; BDS sends AddActor, `disappeared`, `undefined`, `bow` in one
  batch. The sound positions, actors and ticks match. Logged in the quirks.
- Our mobs take a target on their first tick; BDS's scan finds it after 1 to 9 ticks. The
  shots are compared relative to the target.

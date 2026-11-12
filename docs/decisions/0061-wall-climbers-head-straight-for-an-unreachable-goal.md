---
status: proposed
date: 2026-11-12
decision-makers: [project owner (delegated)]
---

# Wall climbers head straight for an unreachable goal

## Context and Problem Statement

`minecraft:navigation.climb` (the spider and the cave spider) lets a mob climb walls. unastar's
walk path search (`entity::navigation`) has no wall-face nodes: a goal on top of a 3-tall block
has no path, so the melee goal never starts and the spider stands still. BDS 1.26.51 spiders
walk straight at such a target and climb the block (`sweep.py spider --items wall,climb`).

## Decision Drivers

- Copy what BDS does (decision 0013), measured, not guessed.
- Keep the path search one canonical search for every walking mob; do not fork it per mob.
- The climb itself is physics (a blocked move while climbing rises 0.2 a tick), already
  replayed against BDS; the navigation only needs to keep the spider pressing into the wall.

## Considered Options

1. **Wall-face nodes in the path search:** climbable faces become path nodes for a climber.
2. **Direct approach when no walk path exists:** a wall climber whose walk search fails gets a
   two-point path (its feet, the goal) and walks straight; walls it meets are climbed by the
   physics step.
3. **A separate climb navigation** with its own follower (PR #71's prior art kept a
   destination block and a direct sweep).

## Decision Outcome

Chosen option: 2. `GoalContext::chase_path_toward` (the melee chase's path request) stages the
direct path only for a body with `PhysicsBody::wall_climber` and only when the walk search found
nothing; paths the search finds are followed as for any walker, and every other goal (stroll,
panic, pickup, avoidance) still needs a walk path, as BDS's climbing is only measured in chases.

### Consequences

- Good, because one search and one follower serve every mob, and the measured climbs (wall-1 in
  `bds-1.26.51-spider.json`) replay exactly.
- Bad, because a climber facing a wall it cannot top (a 5-tall wall in the `climb` sweep) does
  not reproduce BDS's drift away from the wall near the top; that sweep is not replayed.
- Neutral: the direct path ignores dangers (lava, cactus) between the spider and its goal; BDS
  behaviour there is not measured.
- Neutral: a climber's melee chase never fails to find a path, so the melee goal's path-failure
  delay never applies to it.

## More Information

Evidence: `crates/unastar/src/entity/mob/tests/spider.rs`
(`a_spider_climbs_a_wall_as_on_bds`, `a_spider_chasing_the_player_climbs_onto_its_block`).
Prior art: PR #71 (`fb1291c3`) used option 3; its constants were not black-box measured and
were not taken.

# Design: the polar bear's stomp and water drag

## One melee goal, two reaches

`behavior.melee_box_attack` and `behavior.stomp_attack` share the chase: path timers,
boundaries, sight and look. Both compile into `MeleeAttackGoal`, and `MeleeReach` says how a
target is reached. The box reach decides inside the AI tick, as before. The stomp reach
decides in `tick_stomp_attacks`, after the physics step (decision 0068).

## The stomp clock

`StompClock` counts the cooldown down once a tick. It hits when the target is within
`(width × reach_multiplier)² + target width`. Within that distance times
`no_damage_range_multiplier` it restarts a run-out cooldown, and with ten ticks or fewer left
it raises `STANDING`. These are the rules that fit every rear-up, stand-down and hit of five
measured runs. The sixth run (`baby-0`) restarted mid-run for a reason that was not measured.

## Water drag

`PhysicsBody::water_drag` replaces the 0.8 horizontal drag in water. The vertical step keeps
`0.8 v − 0.02`, as BDS's knocked bear showed (0.98 a tick horizontally, 0.8 vertically).

## Risks

- The first cooldown start after the bear turns hostile is not modelled. The live test
  provokes the bear directly.
- A frozen bear (Slowness 255) never stomps in BDS. This is not modelled.

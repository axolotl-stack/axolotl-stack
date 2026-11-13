# Design: the bat's flight and resting

## A float control, not a path

Every push of BDS's bats was `0.1 × û`. Here û is the unit vector from the feet before the
move to one fixed point per wander, with residuals of a thousandth of a degree. Points
outside the glass room and below the floor were flown at. So `navigation.float` is a branch
of the AI tick: the goals write the point, and `FlyState::control` adds the push.
Decision 0069 compares this with a 3D path search and with a bat-only class.

## Drag and gravity

With no push, the bat's motion kept 0.91 a tick on x and z and 0.546 on y. The ghast's kept
0.91 on every axis. `mob::derive` therefore gives a float-navigating body no gravity, and a
vertical drag of 0.91 times `NativeTraits::flight_vertical_factor` (the bat's 0.6).
`HAS_GRAVITY` is still replicated from the empty `minecraft:physics`, as in the BDS summon.

## Wanders

Wanders after a wander lasted 3 to 7 ticks, about equally often. After an idle tick they
pushed one tick fewer. So the duration counts the ticks spent drawing, and is
`floor(float_duration × 20) + 1`. A drawn point in a block with a collision box is not kept.
Points under the world read as air and are kept, as BDS kept them.

## Resting

The bat class rests: it is born resting, and holds its feet at its block top less its
height. It settles with a chance per tick under a solid block (21 settles fit about 1 in 90;
we use 1 in 100). It wakes when the block above is gone, when a player's eyes are within 4
blocks of its feet, or on the tick it is hurt. The resting check runs before the goals. A
resting bat neither thinks nor moves.

## Risks

- One of 24 hurts left a resting BDS bat resting. This is not explained.
- The 1 in 100 rate is fitted, not exact: the test compares rates within 35 %.

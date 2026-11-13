# Design: the vex's flight, charge and decay

## One held course

Every push of BDS's vexes was 0.0125 or 0.05 a tick along the unit vector from the feet
before the move to one point: a block centre for a wander, a point near the player for a
charge. On the tick the feet came within 0.533 (the box size), the velocity kept 0.5 and the
point was gone. So the vex's control holds a `Course` in `FlyState` until it arrives, unlike
the bat's per-tick wander point. Decision 0070 compares this with reusing `navigation.float`
and with a vex-only goal.

## Order within a tick

A charge's first push showed two samples after `CHARGING` went up. A wander could begin on
the very sample after an arrival. Both fit one order: the control steers first, then the
goals think, and a course set by a goal is flown from the next tick. A pending course (the
goals first, the control a tick later) cannot give a one-sample wait, and BDS showed three
in 44.

## The hit after the move

Each BDS hit came on the tick whose move first made the boxes touch. The feet before that
move did not touch. As with the stomp (decision 0068), the hit is judged in a system after
the physics step.

## Aim

33 charges aimed at 1.52 above the player's feet, 0.1 below the eyes. A mob target's aim is
unmeasured, and the same drop is used.

## Risks

- `look_at_entity` targets are only measured for a mob (a cow). Whether players count is
  not known.
- The vex's head, its pitch and `MOVING` are not replicated.

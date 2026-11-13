# Tasks: the polar bear's stomp and water drag

## 1. Measure

- [x] 1.1 `sweep.py polar-bear` sets `stroll`, `attack`, `stomp`, `baby`, `swim`, `swim-push`
- [x] 1.2 `sweep.py bat` and `tadpole` (measured for a later round)
- [x] 1.3 `mob_fixture.py` and the committed polar bear fixture

## 2. Implement

- [x] 2.1 `water_movement`, `movement.sway` and `navigation.float` bodies (`unastar-data`)
- [x] 2.2 `PhysicsBody::water_drag` from `water_movement`
- [x] 2.3 `MeleeAttackGoal` with `MeleeReach`, the stomp clock and `tick_stomp_attacks`

## 3. Verify and record

- [x] 3.1 Replays (`mob/tests/polar_bear.rs`)
- [x] 3.2 Parity rows, quirks, decision 0068, sweep README

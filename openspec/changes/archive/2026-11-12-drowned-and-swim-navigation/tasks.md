# Tasks: the drowned and swim navigation

## 1. Measure

- [x] 1.1 `sweep.py drowned` sets `sink`, `hunt`, `trident`, `day`, `equipment`, `convert`
- [x] 1.2 `sweep.py witch` sets `throw`, `drink`, `resist`
- [x] 1.3 `drowned_fixture.py` and the committed fixture; the witch research note

## 2. Implement

- [x] 2.1 `navigation.generic`, `movement.generic`, `underwater_movement` bodies (`unastar-data`)
- [x] 2.2 Swim navigation, the swim move control and `Swimming` physics
- [x] 2.3 The immersed-target knockback preset, `is_daytime`, `is_riding`
- [x] 2.4 The retained thrown trident AddActor (`spawns` capture role)
- [x] 2.5 Actor `spawn_entity` and transformations with a delay, a sound, an event and kept equipment

## 3. Verify and record

- [x] 3.1 Replays (`mob/tests/drowned.rs`)
- [x] 3.2 Parity rows, quirks, decision 0065, sweep README, solution note

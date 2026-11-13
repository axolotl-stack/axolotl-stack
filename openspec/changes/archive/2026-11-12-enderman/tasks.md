# Tasks: the enderman

## 1. Measure

- [x] 1.1 `sweep.py enderman` round two (spawn, hurt, idle, look, step, dodge, carry)
- [x] 1.2 Round three (hurts near the player, pools, more and sealed dodges, anger, a far target)
- [x] 1.3 `mob_fixture.py` keeps teleports, carried blocks, arrows and turns; the fixture

## 2. Implement

- [x] 2.1 `looked_at`, `teleport` and `variable_max_auto_step` bodies and component kinds
- [x] 2.2 The stare (`mob::life::looked_at`) and players' armour in `has_equipment`
- [x] 2.3 Teleports (`mob::life::teleport`): timer, rolls, hurts, dodges, far targets, the wire
- [x] 2.4 `behavior.take_block` / `behavior.place_block` (`mob::ai::carry`)
- [x] 2.5 Step height; filters `in_contact_with_water` and `is_riding`; the summon's actor data 23

## 3. Verify and record

- [x] 3.1 Enderman replays (`mob/tests/enderman.rs`) and facet lists
- [x] 3.2 Parity rows, quirks, research note, constants note, sweep README, decision 0066,
  solution note

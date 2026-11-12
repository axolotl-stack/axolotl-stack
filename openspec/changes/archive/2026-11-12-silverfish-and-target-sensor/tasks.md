# Tasks: the silverfish, and the creeper's fuse sensor

## 1. Measure

- [x] 1.1 `sweep.py creeper` (approach, cancel, sight, flint, charged, no-griefing, flint-alone, gap, gap-far)
- [x] 1.2 `sweep.py silverfish` sets `pit`, `blocks`, `griefing`, `wake-nogrief`, `ab`
- [x] 1.3 `creeper_fixture.py`, `silverfish_fixture.py` and the committed fixtures

## 2. Implement

- [x] 2.1 Measured infested blocks (`unastar_data::infested`, `InfestedBlocks`)
- [x] 2.2 `behavior.silverfish_merge_with_stone`, `behavior.silverfish_wake_up_friends`, `mob::life::infested`
- [x] 2.3 `minecraft:target_nearby_sensor` body, component and system
- [x] 2.4 `behavior.swell` goal and its `fuse` sound; `minecraft:is_charged` sets `POWERED`

## 3. Verify and record

- [x] 3.1 Silverfish replays (`mob/tests/silverfish.rs`) and facet lists; creeper sensor replay (`mob/tests/creeper.rs`)
- [x] 3.2 Parity rows, quirks, constants research note, sweep README, decision 0059, solution note

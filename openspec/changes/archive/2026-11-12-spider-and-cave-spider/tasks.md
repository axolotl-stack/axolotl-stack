# Tasks: the spider and the cave spider

## 1. Measure

- [x] 1.1 `sweep.py spider` (approach, wall, climb, day, dusk, cave per difficulty, leap)
- [x] 1.2 `sweep.py enderman` (gaze, hurt, arrow, water, rain, random, light, take), kept for the enderman change
- [x] 1.3 `mob_fixture.py` and the committed spider fixture

## 2. Implement

- [x] 2.1 `navigation.climb` body, `WallClimbing` physics and the `WALLCLIMBING` flag
- [x] 2.2 Direct approach for a climber without a walk path; touching clip tolerance
- [x] 2.3 `behavior.leap_at_target`
- [x] 2.4 `renders_when_invisible` flag; `addrider` and `block_movement_slowdown_immunity` accepted
- [x] 2.5 Filters `is_brightness`, `is_daytime`, `is_snow_covered`

## 3. Verify and record

- [x] 3.1 Spider replays (`mob/tests/spider.rs`) and facet lists
- [x] 3.2 Parity rows, quirks, constants research note, sweep README, decision 0061, solution note

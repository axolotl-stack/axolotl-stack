# Tasks: mob ranged attacks

## 1. Measure

- [x] 1.1 Distil the round-3 ranged captures; record the difficulty of each run
- [x] 1.2 Sweep sets `geometry`, `aim`, `spread`, `damage`, `damage-long`, `weapon`, `filters`, `avoid`, `avoid-many`, `pickup` (`sweep.py ranged`)
- [x] 1.3 `ranged_fixture.py` and the committed fixture

## 2. Implement

- [x] 2.1 `minecraft:shooter` body and component; `behavior.ranged_attack` goal and the shot
- [x] 2.2 Arrow difficulty damage, tipped-arrow effects, owner and aux actor data
- [x] 2.3 `behavior.avoid_mob_type`
- [x] 2.4 Filters `has_ranged_weapon`, `in_block`, `is_underground`; tag item descriptors; never-matching unknown-key sensor filters
- [x] 2.5 `freezing_immune` inert; AI speed from the movement attribute

## 3. Verify and record

- [x] 3.1 Replays in `mob/combat/bds_ranged.rs`; AddActor equality for the four mobs
- [x] 3.2 Parity rows, quirks, constants research note, sweep README, solution note

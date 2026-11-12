---
title: Move sweep actors with execute at, and leave peaceful for anything a mob comes out of
area: testing
tags: [bds-sweep, actors, timeline, pistons, tripwires, infested, peaceful, seeds]
date: 2026-11-12
origin: lane BLOCKS round 5 (feat(bds-sweep) round 5 measurements, feat(blocks) tripwires and piston riders)
status: current
---

## Context

Round 5 measured tripwires, actors riding piston moves and infested blocks with the scenarios
job's per-tick actor reads. Two sweeps came back empty or wrong on the first run, and a flaky
placement replay turned out to be an unseeded random.

## What we learned

- A console command's `~` is relative to the world origin, not to the actor a selector names:
  `tp @e[type=armor_stand,...] ~0.4 ~ ~` sent the stands to (0.4, 0, 0) and they vanished from
  the cell. Use `execute as @e[...] at @s run tp @s ~0.4 ~ ~`. Check the first actor read after a
  nudge before trusting a row.
- The sweep world is peaceful (`sweep.py`): BDS releases no silverfish from a broken infested
  block there. Anything that spawns a hostile mob needs `difficulty normal` as a step (and
  `difficulty peaceful` after it, since steps change the world for later items). Measure both:
  "nothing on peaceful" is a fact worth keeping.
- Replaying actor moves against recorded positions works when the comparison is along the move's
  axis only and allows the actor's own motion (the velocity read before or after the tick; a
  landing item moves with the one before). Skip transitions a command moved the actor in.
- One row per case is not enough to fit a rule: the first piston rider rows fit "half a block per
  carrier" until two more spots and a pull broke it. Sweep several positions across each box edge
  (over the head or not, one or two blocks, both directions) before writing the rule down.
- A replay that seeds nothing can fail on a different case each run. The placement arena's
  `EntityRandom` came from entropy; a dropped candle sometimes landed within pickup range. Seed
  every random a test arena owns, then check a few seeds to see whether a case depends on luck.

## Where it lives

- `tools/bds-sweep/plates.py` (`plates_tripwire_more`), `pistons.py` (`pistons_riding`),
  `infested.py` (`infested_break`); the `mine` and `stop` timeline events in the sweep plugin.
- `crates/unastar/src/server/game/redstone/tests.rs` (`pistons_move_actors_like_bds`,
  `tripwires_follow_bds_under_the_measured_actors`).
- `crates/unastar/src/server/game/placement_capture_tests.rs` (`ARENA_SEED`).

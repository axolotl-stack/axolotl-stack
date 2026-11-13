---
title: Survival block breaking must advance per client input, not per server tick
area: server
tags: [mining, block breaking, PlayerAuthInput, prediction, jank, burst, bds capture]
date: 2026-11-10
origin: openspec/changes/archive/2026-11-10-survival-breaking-per-input (items lane; breaking flow replayed against the BDS capture `blocks/testdata/bds-breaking.jsonl`)
status: current
---

## Context

Players reported block breaking as "very jank". The server used to:
- time a break in server ticks from the `StartDestroyBlock` it received;
- accept the client's `PredictDestroyBlock` once 90 % of the expected ticks had passed;
- never complete a break on its own;
- send crack updates every 5 ticks with a "hit" sound, and a "break.block" sound on top of the destroy particles.

## What we learned

With server-authoritative breaking (which unastar advertises in `StartGame`), BDS 1.26.51 advances a
break once per `PlayerAuthInput` of the breaking player, idle inputs included. It completes the break
itself on the input whose progress reaches 1.

- **A frozen client.** A scripted client that sent no input for 10 ticks after 6 inputs still broke
  its dirt on the 15th input, and BDS sent nothing during the gap.
- **No snap-backs.** The client counts its progress per input too, so the client and server agree by
  construction, whatever the latency. A server-tick timer disagrees whenever inputs arrive bunched,
  which turned valid predictions into snap-backs.
- **Early predictions.** An early prediction is refused with block corrections, but the break goes
  on, so the block still breaks on time.
- **No break sounds.** BDS sends no sound for a player's break. The client plays the hit and break
  sounds from the crack and destroy `LevelEvent`s, so server-sent sounds were doubled.

Per-input progress has two consequences that are easy to miss:

- **Bursts are not throttled.** BDS does not hold a client to server time. Fifteen inputs that
  arrived together were answered within about 60 ms and broke dirt on the 15th.
  - A server that applies inputs as they arrive must commit a break before the burst's later inputs
    run, because BDS breaks the block at once and the 16th input found air.
  - unastar's plugins commit breaks after the tick. So a player's actions after an input that
    completed a break wait for the next tick, capped so that a spamming client cannot grow the queue.
- **Breaks don't simply end.**
  - `StopDestroyBlock` does not end a break.
  - A completed break goes on at that position from zero, at the last block's speed, even over air.
  - A test that puts a block back where a player broke one must expect it to break by itself.

  The rules are in `docs/quirks/bds.md` (Blocks / mining).

How to see it:
1. Run `bds-sweep-client --script tools/bds-sweep/scripts/breaking.txt --capture-dir` against BDS
   (`tools/bds-sweep/script.py`) and against a local unastar, using the same script and an operator
   player.
2. Compare the crack events per scenario.

The `pause` and `burst` orders separate per-input progress from per-tick progress, because a
well-behaved client sends exactly one input per tick.

Write scenarios so a block put back by a command is not broken in the same tick it is placed. BDS
merges the two changes into one tick's update, while unastar sends the command's update and the
next tick's repeat. A headless replay test also has to commit the breaks itself, because the plugin
manager does that outside the schedule (`plugin-manager-runs-outside-the-ecs-schedule.md`).

## Where it lives

- `crates/unastar/src/server/game/blocks.rs`: `apply_block_actions`,
  `process_auth_input_block_actions` and `advance_breaking`.
- `crates/unastar/src/entity/components/player.rs`: `BreakingState`.
- `crates/unastar/src/server/game/movement.rs`: every input is forwarded.
- `crates/unastar/src/server/game/blocks/breaking_tests.rs`: the replay test.

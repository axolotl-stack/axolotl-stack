---
title: Replay a moving client's inputs per phase, from the subject's own stamps and starting attributes
area: testing
tags: [bds-sweep, replay, fixture, movement, exhaustion, moving, capture, player-fixture]
date: 2026-11-10
origin: commit "fix(unastar): copy BDS's movement flags, toggles, falls and burning"
status: current
---

## Context

The movement tests replay what the moving `bds-sweep-client` sent in each phase of a capture
(`tools/bds-sweep/player_fixture.py` keeps the subject's inputs and the packets about it) and
compare what unastar sends with what BDS sent, tick by tick. Three things made the first
version compare the wrong packets or the wrong numbers.

## What we learned

- Choose a phase's packets by the stamps of packets about the subject only. Every client's
  `MovePlayer` carries that client's own input tick, and three clients count from different
  starts; a window built from all stamped packets spanned several phases (the walk "had" four
  `MOVING` updates instead of two).
- Absolute values need the phase's starting point. A phase with one exhaustion update (a single
  jump) has no step to compare, and steps lose the first one. The fixture records each
  attribute's last value before the window (`attributes_before`) and the test starts the
  harness player there (health too: BDS healed between falls).
- BDS takes `MOVING`, exhaustion and knockback from its own simulation of the input, not from
  the position the client reports. Where the recording client's physics was off inside the
  acceptance threshold (never corrected), or a correction made it jump back, a replay of its
  positions differs from BDS by a tick or a few ten-thousandths. Name those ticks in the test
  with the measured cause (`BDS_OWN_MOVES`, the swim's last `MOVING` at 0.001022) instead of
  loosening the tolerance for every tick; the rest then matches to 1e-6.
- BDS stamps a packet with the last input it handled, which drifts up to three ticks from its
  own clock over 250 ticks (the burn hurts after a fire are 20 server ticks apart but stamped
  19 to 21 apart). Timing that BDS keeps by its clock needs that slack; timing it keeps per
  input does not.
- A command run between two inputs lands in the replay as a closure after that input's tick
  (`replay_with`): the fire block placed in the tick of input 711, the `/damage` knockbacks
  after inputs 1290 and 1330.

## Where it lives

`tools/bds-sweep/player_fixture.py`, `crates/unastar/src/entity/player_life/tests/movement.rs`,
`crates/unastar/src/entity/player_life/testdata/bds-1.26.51-movement.json`.

---
title: Replay the committed BDS command capture; feedback inferred from the lang was wrong fifteen times
area: testing
tags: [commands, bds-capture, feedback, replay, bds-sweep, verified]
date: 2026-11-10
origin: commit 65f5f79c (harness) and the commands lane commit after it on branch t3/fef46ab8
status: current
---

## Context

Command executors were written against a few transcribed BDS answers plus `[INFERENCE]` guesses
from `en_US.lang` (which keys exist, what their parameters are). Unit tests asserted those
guesses, so they passed whether or not BDS agreed, and no row could be `verified`: the
transcriptions lived in an untracked cache.

## What we learned

- Guessing from the lang is not good enough. One 587-command capture contradicted fifteen
  answers of commands already marked done, ten of them asserted by their own unit tests: a
  no-op `/fill` answers `commands.fill.success [0]` as a failure (not
  `commands.fill.failed`), `/difficulty 7` answers
  `commands.generic.parameter.invalid [7]`, `/setblock` below the world answers `noChange`,
  block-state errors carry the full identifier, `/xp 0L` answers like points, teleporting to an
  entity counts 2, `/effect … 0` succeeds even when nothing was active, and every syntax error
  shows only the last ten characters before the token. Capture first, then implement.
- Make the script replayable: start every line from one known state (a stone platform at
  8 199 8 in chunk 0,0 that the unit-test harness can rebuild, absolute coordinates, `r=16` on
  entity selectors so the surrounding chunks' animals never match), group lines into
  `# segment:` blocks that leave the world as they found it, and commit only the answer packets
  (`commit_fixtures.py --commands`, ~450 KB for 587 commands). A replay test then runs a segment
  on the harness and compares `CommandOutput` field by field and chosen packets byte for byte —
  real `verified` evidence for 33 rows.
- Advance the harness clock between commands (12 ticks for a 600 ms command wait): hurt timers
  and BDS's fading weather levels depend on it, and the capture's answers only reproduce with it.
- Where ids differ between servers (unique ids in scoreboard names and `SetScore`), map them in
  the test instead of skipping the comparison.
- Equal answers do not prove equal effects: `/execute anchored eyes positioned ^ ^ ^1 run summon
  pig ~ ~ ~` answered `commands.summon.success` on both servers, but only comparing the
  `AddActor` position showed BDS anchors a player's eyes 1.52 above the feet, not 1.62.
  Comparing `MovePlayer` byte for byte then showed teleports report the player 1.62001 above the
  feet (201.62001, one f32 step above 201.62) and that `tp … facing` turns from the feet. Compare
  the effect too (positions, health from `UpdateAttributes`) wherever the capture carries it.
- Anything BDS's mobs did between two scripted lines is not replayable: its cows wandered and
  turned, so teleports that face or copy a cow differ in rotation. Compare those by position
  and make the replay player turn as BDS's did before checking what depends on it.
- Facts BDS keeps out of its data files can still be swept with commands: one `/testforblock`
  per palette block (1,477 commands, 150 ms apart, answers paired by order) measured the
  translation keys command feedback names blocks by.

## Where it lives

- `tools/bds-sweep/command-script.txt`, `tools/bds-sweep/commit_fixtures.py`,
  `tools/bds-sweep/block_names.py`
- `crates/bds-extractor/captures/1.26.51/operator-commands/feedback.json`
- `crates/unastar/src/command/exec/evidence.rs` (`Replay`, `assert_segment`,
  `assert_same_packets`) and every executor's `bds_capture` tests
- The ignored `survey_the_bds_capture` test in `crates/unastar/src/command/exec/mod.rs` lists
  every differing answer across the whole capture (`--ignored --nocapture`)

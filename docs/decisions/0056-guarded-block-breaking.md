---
status: accepted
date: 2026-11-12
decision-makers: [project owner]
---

# Guard survival block breaking by default, keep BDS's breaking as an option

## Context and Problem Statement

Survival breaking copies BDS 1.26.51 input by input (the breaking captures in
`crates/unastar/src/server/game/blocks/testdata/`). Two of the behaviours copied help a modified
client:

1. **Inputs are not held to server time.** BDS adds one input's worth of progress for every
   `PlayerAuthInput`, however many arrive in a tick. A client that sends inputs faster than real
   time breaks blocks faster (a burst of 15 inputs broke dirt in about 100 ms instead of 750 ms).
2. **Progress built over the air is kept.** A completed break goes on at that position, also over
   the air, and BDS counts the inputs over the air at the speed of the block put there next. A
   block put on the air of a break can break at once. Dirt put where dirt was broken, obsidian put
   where obsidian was broken with a pickaxe, and dirt put where obsidian was broken all broke on
   the next input. Obsidian put by hand where dirt was broken did not, because by hand it is much
   slower than dirt (`tools/bds-sweep/scripts/breaking-3.txt`, 2026-11-12).

Rule 3 of `AGENTS.md` says copy BDS exactly. The owner was asked whether these two should be
copied too.

## Decision Drivers

- Copy BDS exactly, quirks included ([0013](0013-copy-bds-behaviour-exactly.md)).
- A public server needs breaking that a modified client can't speed up.
- The BDS behaviour must stay available and tested, so parity is still provable.

## Considered Options

1. **Copy BDS only** (what #77 shipped).
2. **Guard only**: drop the BDS behaviour.
3. **Guard by default and keep BDS as a configuration switch.**

## Decision Outcome

Chosen option: 3. In the owner's words (2026-11-12): "make the anti cheat like default ig but lets
copy bds as well".

`[gameplay] block_breaking` in `unastar.toml`:

- `"guarded"` (the default) keeps everything else BDS does (per-input answers, crack events, the
  server completing the break, refused predictions, a break going on after it completes). It adds
  two rules:
  - Since the progress last began from zero, at most one input of progress counts per server
    tick, counted from the tick of its first input. A client that skipped ticks may catch up, but
    it can never get ahead of real time. A new progress may not count an input in a tick where the
    previous one already counted one.
  - When the block at the position is no longer the one being broken, the progress begins again
    on it. That covers another block, or one put there after the progress began over the air.
- `"bds"` restores exactly what BDS 1.26.51 does. The BDS replays run in this mode.

### Consequences

- Good, because a default server can't be mined faster by flooding inputs or by aiming at air.
- Good, because the BDS behaviour stays measured, tested and one setting away.
- Bad, because a guarded server answers a modified client differently from BDS. A flooded input
  past the budget still gets its crack events but adds no progress, and the client predicts a
  break the server then refuses.
- Neutral: a block replaced by the same block between two of the breaking player's inputs is not
  seen as a change. The guard compares the block at each input, not every change to it.

### Confirmation

- `crates/unastar/src/server/game/blocks.rs` tests: `guarded_breaking_counts_one_input_per_tick`,
  `guarded_breaking_lets_a_lagging_client_catch_up`,
  `guarded_breaking_begins_again_on_a_block_placed_over_air`, and
  `guarded_breaking_begins_again_on_another_block`.
- The BDS replays in `blocks/breaking_tests.rs` run in the `bds` mode, including the new
  `blocks_put_on_a_break_answer_like_bds` (capture of `scripts/breaking-3.txt`).
- `docs/quirks/bds.md`, Blocks / mining, records the deviation.

---
title: BDS counts breaking inputs over air at the speed of the block put there next
area: server
tags: [breaking, mining, anticheat, obsidian, bds-mode]
date: 2026-11-12
origin: items round 4 (guarded block breaking, decision 0056)
status: current
---

## Context

Round 1 modelled a break that goes on over the air as adding the last block's per-input speed,
from dirt-only captures. Under that model, obsidian put where dirt was broken 21 inputs before
should break at once. The owner wanted the anticheat case measured with a hard block before
deciding.

## What we learned

- Obsidian by hand did **not** break at once, with or without the client's
  `ContinueDestroyBlock`. Obsidian put on the air of obsidian broken with a diamond pickaxe did,
  and so did dirt put on that air after 89 inputs. Coarse dirt (dirt's speed) put on dirt's air
  also broke at once.
- The model that fits every capture (`blocks/testdata/bds-breaking*.jsonl`): inputs that find no
  breakable block are counted, and when a block is there again they are added at *that* block's
  speed, with the held item as it is now. With a block there, each input adds the current speed:
  a tool selected mid-break, or a block swapped for another, keeps the progress.
- The crack speed BDS reports (3602) is the live speed of the block there, even with no
  `ContinueDestroyBlock` (18 for obsidian by hand after a released dirt break).
- Not everything is deterministic. An obsidian break with a diamond pickaxe took 130 inputs in
  one run and 133 in another, at the same reported speed (1/131.25 per input). Dirt (an exact 15)
  never varied. `blocks_put_on_a_break_answer_like_bds` therefore compares only the sequence of
  speeds and breaks for the obsidian scenarios.

## How to apply

Before extrapolating a breaking rule from one block, capture a block with a different speed. Fast
and slow blocks separate the "last speed" and "next speed" models. When a long break's input count
wobbles between runs, compare the shape of the events (speeds, breaks, nothing in between), not
the exact input.

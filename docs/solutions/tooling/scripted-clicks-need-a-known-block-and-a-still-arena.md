---
title: Scripted clicks need a block the client knows and an arena nothing moves the player in
area: tooling
tags: [bds-sweep, scripted-client, use-on, buckets, spawn-eggs, reach, liquids]
date: 2026-11-12
origin: items round 3 (tools/bds-sweep/scripts/buckets-eggs.txt)
status: current
---

## Context

The first capture of `scripts/buckets-eggs.txt` against BDS 1.26.51 looked like BDS refused
almost everything: a water bucket on dirt did nothing, and survival spawn eggs spawned no mob
while the creative one did. Both were artefacts of the script, not BDS behaviour.

## What we learned

- `use-on X Y Z FACE` sends the clicked block's runtime id as the scripted client last saw it,
  and the client only learns blocks from `UpdateBlock`. A `/setblock` (or `/fill`) is sometimes
  answered with `UpdateSubChunkBlocks`, which the client does not track, so the click carries
  runtime id 0 and BDS answers with corrections only. Teach the client first: an empty-handed
  `use-on` on the block makes BDS send its six corrections, `UpdateBlock`s that the client keeps.
- Liquids flow. Water placed on the stone floor (or out of a waterlogged slab) spread over the
  arena and pushed the player several blocks west. Its later survival clicks were then out of
  reach and refused, while creative's longer reach still worked. Put liquids in a one-block pit
  dug into the floor over dirt, and `/tp` the player back before every step.
- To read a capture, look at the player's position (`CorrectPlayerMovePrediction`) as well as
  the answer to a click. A refused click and an out-of-reach click look the same.

## Where it lives

- `tools/bds-sweep/scripts/buckets-eggs.txt` (pit at (2,-61,0), warm-up clicks, `/tp` per step).
- `crates/bds-sweep/src/main.rs`: `Order::UseOn` and the block map fed by `UpdateBlock`.

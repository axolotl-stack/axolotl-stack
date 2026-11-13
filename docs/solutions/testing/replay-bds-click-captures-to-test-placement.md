---
title: Test block placement by replaying captured BDS clicks into a headless server, after making stacks describe BDS's blocks
area: testing
tags: [placement, bds-sweep, capture, survival, corrections, sounds, item descriptors, fixture]
date: 2026-11-10
origin: lane BLOCKS placement work (feat(blocks) commit adding server/game/placement_capture_tests.rs)
status: current
---

## Context

"Block placing is very jank" had no single cause: wrong orientations, refused placements,
missing corrections, wrong sound positions. Unit tests written from the public reference kept
passing while the game disagreed with BDS. The fix that held was to record what BDS does for
each survival click, both directions, and replay the same clicks into unastar.

## What we learned

- The `placements` job (`tools/bds-sweep/placements.py`) sets up each case, puts the stack in
  hotbar slot 0 and has the scripted client click (`bds-sweep:use`). Give the stack with
  `/replaceitem`: the Endstone inventory API changes the server stack without telling the
  client, so the client clicks with a stack BDS no longer has and BDS answers differently.
- Cut the client capture per case with chat markers (`bds-sweep:mark <key>` ...
  `bds-sweep:mark end`). Keep `-` in keys: `yaw_-30` was folded into `yaw_30` and one case lost
  its packets.
- Keep bytes only for the packets the test decodes. BDS answers every click with attributes,
  actor data and four inventory windows; listing those by name took the fixture from 1.25 MB
  to 0.6 MB for 152 cases.
- Replay into a headless `GameServer`: decode the captured client packet and route it as the
  network task does; run `PluginManager::tick` after `server.tick`, or placements are never
  committed. The plugin host needs a tokio runtime and the codecs a 64 MB stack. Running the
  cases on six worker threads (one server each) took 205 cases from 127 s to 26 s.
- Compare outcomes, not packet order: blocks stored, held count, the six corrections as a
  consecutive run, the last block the client was told per position, sounds, the main-inventory
  slot 0 of the last resync. unastar sends the placed block just before the corrections, BDS
  just after; the client ends up with the same blocks.
- The captured click carries BDS's descriptor of the held stack. A chest stack describes
  `cardinal_direction=north`, an amethyst bud `block_face=up`, and doors, beds, signs, hoppers
  and the newer doors and hanging signs (whose item ids look like block ids) describe no block.
  Until unastar's stacks described the same blocks (`item-blocks` sweep, `item_blocks.json`),
  those replays were refused before any placement rule ran; real clients hit the same refusal
  whenever unastar's descriptor and the client's disagreed.
- What the first replay found (45 of 152 cases): door, anvil, campfire, piston, observer, end
  rod, lever, tripwire hook and hanging-sign orientations, signs not placeable, sound positions
  of items that are not block items, no inventory after a click out of reach, beds placing one
  half and placing over air. Each is now a row in `docs/quirks/bds.md`.
- BDS also resends every attribute and the player's actor data after every click; unastar does
  not copy that.
- `sweep.py` puts this checkout's plugin source first on `PYTHONPATH`, so a worktree's new job
  kinds run on a shared slot without reinstalling the shared venv.

Round 2 added a second, watching client, multi-step cases and per-group fixtures:
`docs/solutions/testing/watch-a-second-player-and-replay-click-timelines.md`.

## Where it lives

- `tools/bds-sweep/placements.py`, `tools/bds-sweep/endstone-sweep/src/endstone_bds_sweep/jobs.py`
  (`placements`, `item_blocks`), `tools/bds-sweep/summarize.py` (`placements`, `item-blocks`)
- `crates/unastar/src/server/game/placement_capture_tests.rs`,
  `crates/unastar/src/server/game/testdata/bds-placements.json`
- `crates/unastar-data/data/measured/item_blocks.json`, `crates/unastar/src/registry/item.rs`
  (`item_block_state`)

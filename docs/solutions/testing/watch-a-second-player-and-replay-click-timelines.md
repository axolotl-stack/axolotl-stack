---
title: Record what other players see with a second scripted client, and replay multi-step clicks at their ticks
area: testing
tags: [placement, bds-sweep, capture, watcher, sign, timeline, block actor, replay]
date: 2026-11-10
origin: lane BLOCKS round 2 (feat(blocks) commits adding server/game/signs.rs and block_use.rs)
status: current
---

## Context

Round 1 replayed single clicks captured by one client. Round 2 needed what other
players receive, sign editing (a click, then a client edit), dye on a sign (a
click, an edit, a hand swap, another click) and timed effects (buttons popping
back). Comparing only the clicking client's packets hid that BDS sends the
clicking player and the others different things.

## What we learned

- Run a second `bds-sweep-client` (`BDSWatch`) in the `placements` job. BDS
  refuses a second login from the same address right after the first ("ip
  recently connected"): start it once the first client logged "spawned as" and
  retry every 15 s. Send the chat markers to both players so both captures cut
  per case.
- The two views differ on purpose: a player who opens a door is not sent the door
  (its client predicted it), the others are; after a sign edit the editor gets one
  actor update and the others two (text, then the released lock). Compare the
  final state each client was told, per position, not packet order.
- Rebuild the client binary (`cargo build -p bds-sweep`) after changing it:
  `cargo test -p bds-sweep` builds only the test binary, and `sweep.py` runs
  `target/debug/bds-sweep-client.exe`. A stale client silently ignores new orders.
- Record the tick of every step (`timeline`) in the plugin and replay each
  captured client packet at that tick. Tick once right after routing a packet:
  the server handles a queued click in the next tick, so a `/replaceitem` hand
  swap applied before that tick changes what the click held.
- BDS sends a tick's block changes after the chat marker that ends the capture,
  so changes in the last 2-3 ticks before a late read are missing from the
  capture although the read sees them. Stop collecting unastar's packets 3 ticks
  before the late read.
- Some outcomes are random (a placed coral block's death tick, a cave vine's
  age, a campfire's crackle): skip those comparisons with a reason
  (`skipped()` in the test) and check the measured distribution separately.
- `set_data` with physics and a player placement differ beyond block updates:
  coral blocks set by the scenario job never die, player-placed ones die within
  60 ticks. Measure player behaviour with the `placements` job.
- Endstone `set_data` fills water plants (coral fans, kelp, seagrass, sea
  pickles) with water: a lily pad stays on them in the `soils_more` scenarios.
- Placement sounds are per item (seeds at the clicked block, candles
  `item.use.on` in the cell on the face, mushroom blocks with their capped
  state): measure the sound of every new item family rather than generalising.
- Windows: Python `write_text` writes CRLF; write repository files with
  `write_bytes` (or `newline=''`) so tracked files keep LF.

## Where it lives

`tools/bds-sweep/sweep.py` (watcher), `endstone-sweep/.../jobs.py` (`placements`
steps and timeline), `crates/bds-sweep/src/main.rs` (`sign` order) and `nbt.rs`,
`tools/bds-sweep/summarize.py` (`capture_by_marker`, per-group fixtures),
`crates/unastar/src/server/game/placement_capture_tests.rs` (timeline replay,
watcher, `skipped`), `testdata/bds-placements*.json`.

---
title: Driving a live client in BDS captures — input, corrections, opening items
area: tooling
tags: [bds-extractor, captures, books, input, player-auth-input]
date: 2026-11-12
origin: OpenSpec change protocol-client-input
status: current
---

## Context

BDS ignored every `BookEdit` from the round 2 extractor, which sent no `PlayerAuthInput`. Input
looked like the missing piece; it was not the only one.

## What we learned

- `bds-extractor --input` sends `PlayerAuthInput` every 50 ms at the player's position. BDS's
  `StartGame` puts a new player at a placeholder height (y 32769.62 in a seed-12345 world) and then
  corrects it with `CorrectPlayerMovePrediction`, never `MovePlayer`. A stream that ignores
  corrections keeps claiming the placeholder and is corrected every few ticks; follow the corrected
  position (`InputStream::follow`).
- With input on, BDS still ignored book edits. A real client opens a book first (`MobEquipment`
  selecting it, then an `ItemUse` `Use` on air with the book); BDS edits only the book opened last.
  Found by decoding the PR #71 `item-uses-misc` recording of a real client
  (`origin/integration/2026-11-04-lanes`, `crates/client-diff/scenarios/item-uses-misc/`). The
  `!equip SLOT` and `!use-item SLOT` actions now send them, built from the inventory the server
  last sent (`Actors::follow`).
- So when BDS ignores a client packet, check what a real client sends *before* it: a recording of
  a real client is the quickest source. `bds-extractor --dump packets.jsonl --packets A,B` prints
  any recording (extractor captures or client-diff recordings) decoded as JSON lines.
- `protocol_probe.single` runs one session per script on one server; a run without
  `BDS_RUNTIME`/`BDS_PORT` falls back to the shared `orca/bds-runtime` on port 19132. Always set
  both, even when calling the probe functions from Python directly.
- A player sent to an open-air platform with `/tp` may fall through it while its input stream is
  still claiming the old position: BDS's corrections dropped the player about 48 blocks, out of block
  reach. Place test blocks beside wherever the player stands instead (`/setblock ~2 ~ ~ ...` and
  `!pick-block ~2 ~ ~`, which the extractor resolves from its last known position).
- `protocol_probe.single` sessions have a timeout (600 s by default). A run over every summonable
  actor (about 8.5 s each) needs about 16 minutes; at the timeout the harness stops BDS and kills
  the extractor mid-script, which looks like a crash. Pass a longer `timeout`.
- zuri_nbt writes compound keys in `HashMap` order. BDS writes them sorted and types an empty list
  `End` (0); `crate::item::nbt::write_sorted` does the same, so item NBT compares byte for byte.

## Where it lives

`crates/bds-extractor/src/input.rs`, `crates/bds-extractor/src/actions.rs` (`!equip`,
`!use-item`, `!pick-block`), `crates/bds-extractor/src/capture.rs` (`dump`),
`tools/bds-sweep/protocol_probe.py` (`book`, `pick`, `pick-actors`); fixtures in
`crates/bds-extractor/captures/1.26.51/protocol/book/`, `pick/` and `pick-actors/`; related:
[scripted-client-packets-in-bds-captures.md](scripted-client-packets-in-bds-captures.md).

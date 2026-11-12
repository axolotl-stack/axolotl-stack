---
title: Scripted BDS command captures fail in four ways; protect the player in survival, keep crowds out of view, check summon syntax, decode packets.jsonl
area: tooling
tags: [bds, capture, bds-extractor, command-capture, summon, packets.jsonl, mobs]
date: 2026-11-10
origin: entities lane, fire immunity / attack effect / zombie pigman anger measurements (2026-11-10)
status: current
---

## Context

`tools/bds-sweep/capture.py --commands FILE` runs a script of operator commands on a fresh BDS
1.26.51 world and writes every server packet per command to `command-outputs.json`. Measuring mob
behaviour this way (fire immunity per `/damage` cause, attack effects, zombie pigman anger) took
six sessions where three would have done, because of four traps.

## What we learned

- **Effects given in creative do not protect the survival player.** `effect @s instant_health`
  and `effect @s absorption` sent while the player was creative did nothing once it switched to
  survival (absorption stayed 0), and hostile mobs killed it, so every later segment of the
  script recorded nothing. Switch to survival first, then apply `resistance 120 4` (full damage
  immunity; mob swings still happen and still count) or absorption. A dead player never
  respawns in the capture, so put the most important segment first.
- **Hundreds of actors in view abort the client.** `execute as @e[type=…] run summon …`
  doubling to 512 actors near the player made BDS send one batch with more than 1600 packets,
  jolyne's batch cap, and `bds-extractor` exited with a protocol error. Run crowd experiments in
  a ticking area about 400 blocks away (`tickingarea add`, a sealed `fill … barrier hollow` box)
  and count with `testfor`, whose feedback lists every match and gives `success_count`.
- **`summon <type> <pos> <event> <name>` is a syntax error** (`commands.generic.syntax`) in
  1.26.51; the named form is `summon <type> <name> <pos>`. Check the first `CommandOutput` of
  every new command shape before trusting the rest of the run.
- **A run that aborts still has its packets.** `command-outputs.json` is only written at the
  end, but `packets.jsonl` is flushed per record. Each line's `hex` is header plus payload:
  prefix the varint length and decode with `McpePacketData::decode_inner_bytes(…,
  McpePacketArgs)` (what `bds_extractor::capture::decode_record` does); `CommandRequest` records
  mark where each command starts.

Movement of other actors is left out of `command-outputs.json`, but `SetActorData` is kept, so
actor flags (`ANGRY`) and the `TARGET` actor data are the cheapest observables for AI state.

## Where it lives

`tools/bds-sweep/capture.py`, `crates/bds-extractor/src/capture.rs` (`decode_record`,
`command_outputs`), the fixtures in `crates/unastar/src/entity/mob/testdata/`.

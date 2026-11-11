---
title: Measure what BDS saves by changing it with commands, stopping, and decoding the world; four traps make the first runs lie
area: tooling
tags: [bds, level.dat, leveldb, persistence, scoreboard, bds-extractor, sendcommandfeedback, measurement]
date: 2026-11-10
origin: openspec/changes/archive/2026-11-10-persist-level-data
status: current
---

## Context

Where BDS 1.26.51 keeps the world spawn, clock, game rules, scoreboard, ticking areas, structures
and mob event switches had to be measured before unastar could save them in BDS's layout. The
technique: give every value a distinct non-default setting with console commands (and an operator
player), stop BDS gracefully, decode `level.dat` and every non-chunk LevelDB key of a copy of the
world, then reload and ask BDS for the values with commands. Four runs were needed because the
first ones gave misleading results.

## What we learned

- **`default-player-permission-level` is stored at world creation.** `level.dat`
  `playerPermissionsLevel` is written on the world's first start; setting the property later does
  not make a new player an operator, so every player command failed with
  `commands.generic.permission.selector`. Set the property on the very first start.
- **An offline client needs a fixed identity.** BDS keys an offline player's record
  (`player_<self-signed id>` -> `player_server_<uuid>`) by the client's self-signed id, which
  `bds-extractor` made up per run: the rejoining "same" player was a stranger with no scores. Use
  `bds-extractor --uuid`.
- **`sendCommandFeedback false` silences players.** With it off, a player gets no `CommandOutput`
  for a successful command (failures come as output type `silent`), and the console prints no
  `/gamerule` change, while every other console command still answers. Turn it back on before a
  player's queries, or the capture looks like the commands did nothing.
- **The first console answer after a start carries BDS's start notices** (a transport and a
  telemetry banner, ERROR and INFO records). Drop them before comparing (`strip_banners`).
- **`server.properties` wins over the world for the difficulty.** A world saved on hard and
  reopened with `difficulty=easy` saves 1; reopen with a different property to tell the two apart.
- Compounds are written with their keys in byte order, so decoded records can be compared with
  what unastar writes byte for byte; only the scoreboard's display-slot list changes order between
  saves. Ticking areas come back in the order of their random `tickingarea_<uuid>` keys, so a list
  after a reload differs from the one before it.
- To learn which of two stores BDS loads, edit one in a copy and ask (`level_data.py clocks`:
  `level.dat` `Time` wins over `WorldClocks`). The pure-Python LevelDB reader reads copies only;
  editing `level.dat` (a plain file) needs no LevelDB writer.

## Where it lives

- `tools/bds-sweep/level_data.py` (`measure`, `clocks`, `import`, `daylock`, `dump`, `fixture`),
  `tools/bds-sweep/le_nbt.py`, `tools/bds-sweep/leveldb_read.py`
- `crates/bds-extractor` (`--uuid`)
- `crates/unastar/tests/fixtures/bds_1_26_51_level_data.json` and the tests in
  `crates/unastar/src/world/level_data/tests.rs`

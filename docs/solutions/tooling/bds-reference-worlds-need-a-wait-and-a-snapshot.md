---
title: Generating BDS reference worlds needs a long wait, a missing-chunk report and a snapshot
area: tooling
tags: [bds, fixtures, tickingarea, leveldb]
date: 2026-10-24
origin: docs/research/2026-10-24-data-driven-world.md §6
status: current
---

## Context

Bedrock worldgen fixtures come from worlds BDS generates with forced ticking areas, read back with
LevelDB.

## What we learned

Use `tickingarea add … true`, wait at least 150 s, then `stop`. Even then, on a first pass about
half of the requested areas were missing on disk (quirk row "World / generation" in
`docs/quirks/bds.md`), so the dumper must skip and report missing chunks rather than fail or
generate them. Copy the world before reading it: opening it with LevelDB compacts the log even for
reads, which changes the files under BDS.

## Where it lives

`crates/unastar/examples/bds_worldgen_reference.rs` (snapshot step), `tools/bds-worldgen-controls/`.

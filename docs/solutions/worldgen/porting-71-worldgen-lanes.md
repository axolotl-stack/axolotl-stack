---
title: Porting #71 worldgen work - perf commits lean on the streaming lane; hash fixtures need one re-baseline
area: worldgen
tags: [porting, golden-hash, native-session, cherry-pick]
date: 2026-11-12
origin: port of PR #71 worldgen lanes (bedrock 77a397b0/4aef57f9, java bd70083f/b9e08661, perf 5ea7c07d)
status: current
---

## Context

PR #71 (`origin/integration/2026-11-04-lanes`) is mined lane by lane. The Bedrock and Java worldgen
lanes and the worldgen perf lane were cherry-picked onto main.

## What we learned

- **The perf lane needs commits from the chunk-streaming lane.** Two commits depend on
  `80f8b638` (per-chunk native locks), which entered #71 through the chunk-streaming merge
  `2464da45`: `86e5dc91` (decorate uncontended origins first) and `b389291b` (contended origins
  without heap allocation). Main's native session is still one mutex. `3d89b456` (in-flight
  requests kept apart) builds on that lane's worker-pool rewrite (`d28d9588`: `threads()`, the
  16-worker cap). Port all of these with the streaming lane. Only `51b0a3df` (biome fill replay)
  and the bench/tool commits stand alone.
- **Don't cherry-pick golden-hash fixtures one commit at a time.** Git auto-merges
  `worldgen_output_hash_*.json` line by line without a conflict. The result mixes old and new
  hashes, and no single commit ever produced that mix. Strip the fixture from every picked commit
  (`git filter-branch --index-filter` with main's blob), then re-baseline once at the end. Compare
  the result with the lane tip's fixture: here Bedrock matched `e743e494` exactly, and Java
  matched `3dd2aa8d`.
- **Ratchet files measured on #71 don't transfer.** `51b0a3df` re-baselined
  `tools/worldgen-perf/ratchet-*.json` on a tree that already had `80f8b638`. Keep main's
  ceilings and re-measure.
- **Lane parity rows sit in integration commits.** The Java and Bedrock row updates for these
  lanes are in `e777f4c4` and `ec487d44`, not in the lanes themselves. Diff those rows key by key
  against main's `worldgen.toml` and check that every cited test exists.

## Where it lives

`crates/unastar/tests/fixtures/worldgen_output_hash_{bedrock,java}.json`,
`crates/unastar/src/world/generator/pipeline/native.rs`, `docs/parity/worldgen.toml`.

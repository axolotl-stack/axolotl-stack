---
title: BDS overworld terrain is Java's noise router; seed each noise the Java RandomState way
area: worldgen
tags: [bedrock-worldgen, seeding, noise, terrain-height]
date: 2026-10-24
origin: docs/research/2026-10-24-data-driven-world.md §2.4
status: current
---

## Context

BDS ships no density-function JSON, so the Bedrock-style generator reuses the Java noise router.
The first attempt seeded every router noise as `seed * 31 + ref`.

## What we learned

Against a BDS 1.26.51 fixture (seed 12345, 68 chunks, 17,408 columns) that seeding matched 0.3% of
terrain heights. Seeding each router noise like Java's RandomState lifted it to 84.4% exact and
92.3% within ±2 blocks; mean error fell from 37.1 to 1.33 blocks and 14,150 of 14,150 BDS water
columns matched. The scheme: Xoroshiro from the world seed, XORed with `md5("minecraft:<name>")`
per noise, and `minecraft:terrain` for BlendedNoise. The remaining surface mismatches were biome
selection, not terrain — look there first when heights look right but blocks don't.

## Where it lives

`crates/unastar_noise/codegen/data/random_hash.rs`; fixtures `crates/unastar/tests/fixtures/bds_1_26_51_seed_12345*.json`.

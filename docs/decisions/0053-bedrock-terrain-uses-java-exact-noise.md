---
status: proposed
date: 2026-11-12
decision-makers: [project owner]
supersedes: 0032
---

# Bedrock-style terrain uses the Java-exact noise backend

## Context and Problem Statement

[Decision 0032](0032-two-worldgen-numeric-backends.md) kept two numeric backends. The fast,
calibrated f64 backend (`NoiseRegistry::new`) drove the Bedrock-style generator's shared terrain,
and the native Java-exact backend drove the Java-style generator. The reason was that the
Bedrock-style output had been calibrated against the fast backend.

Measured against the seed-12345 BDS 1.26.51 fixture, the Java-exact arithmetic is closer to BDS
than the calibrated arithmetic. That holds for noise-stage heights, full-pipeline terrain and
underground open cells. So the question is which backend the Bedrock-style terrain core should
use. Work: PR #71 commit `848eb542`, ported in the change
[bedrock-terrain-java-exact-noise](../../openspec/changes/bedrock-terrain-java-exact-noise/proposal.md).

## Considered Options

- Keep the calibrated backend for Bedrock-style terrain (decision 0032).
- Run Bedrock-style terrain through the native Java-exact backend, without Java's structure starts
  or beardifier, and keep the Bedrock nearest-biome lookup.

## Decision Outcome

Chosen option: the Java-exact backend. On the BDS seed-12345 fixture (68 chunks) it moves the
measured gates toward BDS:

| Gate | Calibrated | Java-exact |
|---|---:|---:|
| Noise-stage heights exact | 0.894 | 0.937 |
| Full-pipeline terrain exact | 0.8847 | 0.9266 |
| Terrain within 2 blocks | 0.9241 | 0.9347 |
| Mean height error | 1.206 | 1.065 |
| Height histogram L1 | 0.0370 | 0.0268 |
| BDS underground open cells matched (of 10221) | 4325 | 5605 |

Surface block dips slightly (0.8473 to 0.8466). Its floors were re-baselined under the standing
native-correctness rule (`crates/unastar_bedrock_codegen/README.md`, "Java-exact Bedrock terrain").

### Consequences

- Good, because both generators share one overworld cell loop and biome fill
  (`overworld_native_cells`, `fill_overworld_native_with`), so a Java exactness fix also reaches
  Bedrock-style terrain.
- Good, because Java-style output is unchanged: its golden hashes match the fixture except for an
  unrelated data-marker fix.
- Bad, because every Bedrock-style golden hash moved and the calibrated tuning is lost.
- Neutral: the calibrated backend (`NoiseRegistry::new`, `CachingNoiseChunk`) no longer drives any
  generator. Only examples and unit tests use it. Removing it is follow-up work.

### Confirmation

`crates/unastar/tests/bedrock_worldgen.rs` ratchets (`bedrock_style_vs_bds_reference_ratchets`,
`underground_open_cells_ratchet_against_bds`) and the exact Bedrock fixture hashes in
`crates/unastar/tests/worldgen_output_hash.rs`.

## More Information

When accepted, decision 0032 becomes `superseded by 0053`, and the `worldgen-pipeline`
requirement "Two numeric backends selected per generator" changes as the change's delta spec
describes.

# Proposal: Bedrock-style terrain uses Java-exact noise

## Why

The `worldgen-pipeline` spec ties the Bedrock-style generator's terrain to the calibrated noise
backend ([decision 0032](../../../docs/decisions/0032-two-worldgen-numeric-backends.md)).
Measured against the seed-12345 BDS 1.26.51 fixture, the native Java-exact backend is closer to
BDS, both in terrain heights and in underground open cells. The work was done in PR #71 (commit
`848eb542`, never merged) and is ported to current main with the rest of #71's worldgen fixes.

Prior work: decision 0032; `docs/solutions/worldgen/bds-terrain-needs-java-randomstate-seeding.md`
and `spline-polynomial-lowering-is-not-java-bit-exact.md` (Java seeding and arithmetic already
mattered for BDS terrain). The measurements are in `crates/unastar_bedrock_codegen/README.md`
("Java-exact Bedrock terrain").

## What Changes

- The Bedrock-style terrain core runs the overworld noise router, aquifers and ore veins through
  the native backend's registry and cell loop, without Java's structure starts or beardifier.
  Climate sampling and biome fill keep the Bedrock nearest-biome lookup.
- The native overworld cell loop and biome fill are shared by both generators.
- Bedrock-style golden hashes are re-baselined. Some aggregate floors dip slightly and are
  re-baselined under the native-correctness rule recorded in that README.

Out of scope: removing the now-unused calibrated backend (`NoiseRegistry::new`,
`CachingNoiseChunk`), which only examples and unit tests still use.

**Decision:** proposed as [record 0053](../../../docs/decisions/0053-bedrock-terrain-uses-java-exact-noise.md),
which supersedes 0032 once the owner accepts it.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `worldgen-pipeline`: the Bedrock-style generator's terrain uses the native Java-exact backend.

## Impact

- Crates: `unastar` (`world/generator/terrain.rs`, `terrain/native.rs`, `biome_source.rs`,
  `biome_source/native.rs`, `bedrock/generator.rs`).

### Parity rows

| Domain | Row | Target |
|---|---|---|
| worldgen | `bedrock/overworld/carver/*`, `bedrock/feature/*roofed*` | rows cite the measured BDS regressions (already updated in this pull request) |

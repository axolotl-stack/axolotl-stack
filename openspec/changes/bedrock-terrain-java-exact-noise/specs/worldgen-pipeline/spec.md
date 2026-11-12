## MODIFIED Requirements

### Requirement: Two numeric backends selected per generator
The noise layer SHALL keep two numeric backends: the calibrated backend (`NoiseRegistry::new`) and the native Java-exact backend (`NoiseRegistry::with_router_settings`, returning `NativeNoiseRegistry`). Both the Java-style generator and the Bedrock-style generator's shared terrain MUST use the native backend; the Bedrock-style terrain core runs it without Java's structure starts or beardifier and keeps its own biome lookup. Selecting one backend MUST NOT change the arithmetic of the other (decision 0053, proposed; supersedes 0032).

#### Scenario: Native backend matches captured Java bits
- **WHEN** the native registry samples legacy climate, blended noise or modern octave sums at the pinned seeds and positions
- **THEN** scalar and SIMD results equal the captured Java 1.21.11 bit patterns

#### Scenario: Backend per generator
- **WHEN** a `JavaGenerator` is built with `JavaGenerator::new(seed)` or `JavaGenerator::for_bedrock_shared_terrain(seed)`
- **THEN** both use the native backend from `NoiseRegistry::with_router_settings`; the first decorates in order, the second generates no Java structure starts

Evidence: crates/unastar/src/world/generator/density/types.rs (`NoiseRegistry::new`, `with_router_settings`, `NativeNoiseRegistry`, `legacy_java_climate_matches_native_scalar_and_simd_bits`, `java_blended_simd_preserves_scalar_bits_across_smear_and_height_boundaries`, `native_modern_router_uses_exact_octave_persistence`), crates/unastar/src/world/generator/terrain.rs (`TerrainBackend`, `for_bedrock_shared_terrain`), crates/unastar/src/world/generator/terrain/native.rs (`overworld_native_cells`), crates/unastar_noise/src/native_density.rs (`native_float_spline_segments_preserve_measured_bits`), docs/decisions/0053-bedrock-terrain-uses-java-exact-noise.md

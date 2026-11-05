# worldgen-pipeline Specification

## Purpose
World generation for `unastar`: the Java-style and Bedrock-style generators behind one `Generator` trait and `ChunkPipeline` (`crates/unastar/src/world/generator/`), the Java-data AOT toolchain (`crates/unastar_noise`) and the BDS-data AOT toolchain (`crates/unastar_bedrock_codegen`). It covers build inputs, compilation, generator selection, determinism, caching and the fixture, hash and performance gates. What vanilla worldgen does is catalogued in `docs/parity/worldgen.toml`, `docs/quirks/java.md` and `docs/quirks/bds.md`; the reasons are decisions 0001-0005, 0017-0019 and 0026-0032 in `docs/decisions/`. The parity catalog itself is specified in `parity-catalog`.

## Requirements

### Requirement: Generator selected by world config
The world config SHALL select exactly one generator through `world.generator.kind`: `super_flat`, `void_spawn_platform` (with `platform_radius_chunks`), `java` (with `seed`) or `bedrock` (with `seed`). Unknown kinds and unknown fields MUST fail to parse. `WorldGenerator::build` MUST return the chosen generator as `Arc<dyn Generator>`, and every kind MUST run through the same `ChunkPipeline` (decision 0018).

#### Scenario: Every kind round-trips through TOML
- **WHEN** a `WorldConfig` with each of `SuperFlat`, `VoidSpawnPlatform { platform_radius_chunks: 2 }`, `Java { seed: 42 }` and `Bedrock { seed: 42 }` is serialised to TOML and parsed back
- **THEN** the parsed config equals the original

#### Scenario: Expensive generators use the generation pool
- **WHEN** `uses_generation_pool` is asked for each kind
- **THEN** it is `true` for `java` and `bedrock` and `false` for `super_flat` and `void_spawn_platform`

Evidence: crates/unastar/src/world/mod.rs (`WorldGenerator`, `build`, `uses_generation_pool`), crates/unastar/src/config/tests.rs (`strict_world_variants_still_load_valid_options`), docs/decisions/0018-ship-java-style-and-bedrock-style-generators.md

### Requirement: Config validation refuses generators the build cannot run
`UnastarConfig::validate` SHALL reject a `bedrock` generator when the binary was built without the `bedrock-worldgen` feature, and a `java` generator when no Java structure templates were embedded (`TEMPLATES_EMBEDDED` is false). The error text MUST name the missing build feature and its environment variable, so a server never starts a world its build cannot generate.

#### Scenario: Bedrock world without the feature
- **WHEN** a config selecting `bedrock` is validated in a build without `bedrock-worldgen`
- **THEN** validation fails with a message naming `--features bedrock-worldgen` and `UNASTAR_BDS_DIR`
- **AND** with the feature, the same config validates

#### Scenario: Default Java world without templates
- **WHEN** the default config (a `java` world) is loaded in a build without `java-worldgen`
- **THEN** loading fails with `ConfigError::Validation` whose message contains `java-worldgen`

Evidence: crates/unastar/src/config/mod.rs (`validate`), crates/unastar/src/world/mod.rs (`BEDROCK_BUILD_REQUIREMENT`), crates/unastar/src/config/tests.rs (`bedrock_config_requires_the_build_time_pack_feature`, `default_java_world_requires_the_java_worldgen_feature`, `generated_and_shipped_configs_load_with_the_same_safe_defaults`)

### Requirement: Worldgen data compiles ahead of time into OUT_DIR
Worldgen derived from data SHALL be compiled by build scripts into Rust under `OUT_DIR` and included from there; generated worldgen code MUST NOT be committed and there MUST be no runtime interpreter for vanilla worldgen data. The build scripts MUST declare `rerun-if-changed` for every input they read and `rerun-if-env-changed` for the install variables (decisions 0001, 0002, 0004, 0017).

#### Scenario: Java toolchain output
- **WHEN** `unastar_noise` builds
- **THEN** its build script parses `worldgen_data/` and `java_data/`, writes the generated modules and `OUT_DIR/mod.rs`, and the crate includes `OUT_DIR/mod.rs`

#### Scenario: Bedrock toolchain output
- **WHEN** `unastar` builds with `bedrock-worldgen`
- **THEN** its build script writes `OUT_DIR/bedrock_worldgen.rs` from `unastar_bedrock_codegen::emit::generate` and `world::generator::bedrock` includes it
- **AND** without the feature the build script does nothing

Evidence: crates/unastar_noise/build.rs, crates/unastar_noise/codegen/emitter/mod.rs (`emit_all`), crates/unastar_noise/src/lib.rs, crates/unastar/build.rs, crates/unastar/src/world/generator/bedrock/mod.rs, docs/decisions/0001-compile-density-functions-to-rust-at-build-time.md, docs/decisions/0002-generate-worldgen-code-in-build-script.md, docs/decisions/0017-compile-worldgen-ahead-of-time-end-to-end.md

### Requirement: Unknown Java data constructs fail the build
The Java toolchain SHALL parse vanilla data into closed, typed IR and MUST fail generation on an unknown feature type, placement modifier, config field, structure field, density-function reference or block actor id, never skipping it or substituting a default. A density input that would lose its vertical coordinate in a 2D cache MUST fail rather than sample at Y=0 (decision 0003).

#### Scenario: Unknown feature data
- **WHEN** the feature parser reads an unknown feature type, an unknown placement modifier or an unknown config field
- **THEN** parsing returns an error naming the offending item

#### Scenario: Missing density reference
- **WHEN** a density function references a name that does not exist
- **THEN** emission fails instead of emitting a zero-valued node

#### Scenario: Blended noise in a 2D cache
- **WHEN** `OldBlendedNoise` would be lowered into a flat or column cache, directly or inline
- **THEN** emission fails

Evidence: crates/unastar_noise/codegen/parser/feature.rs, crates/unastar_noise/codegen/parser/structure.rs, crates/unastar_noise/tests/feature_ir.rs (`unknown_feature_type_is_rejected`, `unknown_placement_modifier_is_rejected`, `unknown_config_field_is_rejected`, `inline_object_errors_keep_their_cause`), crates/unastar_noise/tests/density_ir.rs (`missing_density_reference_is_not_fabricated_as_zero`, `blended_flat_cache_rejects_lost_vertical_coordinates`, `blended_column_cache_rejects_lost_vertical_coordinates`), crates/unastar_noise/tests/template_block_entities.rs (`unknown_actor_and_malformed_lifted_fields_fail_clearly`), docs/decisions/0003-drive-java-worldgen-from-pinned-data.md

### Requirement: Bedrock data comes from a local BDS install and fails closed
With `bedrock-worldgen`, the build SHALL read feature rules, features and biomes from the BDS 1.26.51 install named by `UNASTAR_BDS_DIR` and block identities from the protocolgen native data. A missing variable, unreadable layout, missing native data, unknown key or pass, unknown legacy block value, or reachable built-in feature with no Rust implementation MUST fail the build with a message (decision 0026).

#### Scenario: Variable not set
- **WHEN** `unastar` is built with `bedrock-worldgen` and `UNASTAR_BDS_DIR` unset
- **THEN** the build panics with a message naming `UNASTAR_BDS_DIR` and the expected server directory layout

#### Scenario: Unknown feature-rule key
- **WHEN** a feature rule has an unknown key under `conditions` or an unknown `placement_pass`
- **THEN** `FeatureRule::parse` returns an error naming the key

#### Scenario: Install tests without an install
- **WHEN** `cargo test -p unastar_bedrock_codegen` runs without `UNASTAR_BDS_DIR`
- **THEN** the install tests print a skip message and pass; with it, every document parses, every reachable feature resolves and the whole stack compiles to parseable Rust

Evidence: crates/unastar/build.rs, crates/unastar_bedrock_codegen/src/model.rs (`reachable_features`), crates/unastar_bedrock_codegen/src/builtin.rs, crates/unastar_bedrock_codegen/src/ir.rs (`unknown_keys_and_passes_are_rejected`), crates/unastar_bedrock_codegen/src/blocks.rs (`unknown_legacy_values_fail`), crates/unastar_bedrock_codegen/tests/bds_install.rs (`resolves_the_1_26_51_stack`, `every_document_parses_and_every_reachable_feature_resolves`, `the_whole_stack_compiles_to_parseable_rust`), docs/decisions/0026-bedrock-features-from-the-bds-install.md

### Requirement: Mojang assets are read at build time and never committed
Java structure templates SHALL be read only at build time from the server jar named by `UNASTAR_JAVA_SERVER_JAR`, and only with the `unastar_noise` feature `java-templates` (forwarded by `unastar`'s `java-worldgen`); BDS packs SHALL be read only through `UNASTAR_BDS_DIR`. Neither MUST be copied into the repository (decision 0024).

#### Scenario: Templates feature without a jar
- **WHEN** `unastar_noise` builds with `java-templates` and `UNASTAR_JAVA_SERVER_JAR` unset
- **THEN** the build panics with a message naming the variable and the Java 1.21.11 server jar

#### Scenario: Build without templates
- **WHEN** `unastar_noise` builds without `java-templates`
- **THEN** the embedded template table is empty and `TEMPLATES_EMBEDDED` is `false`

Evidence: crates/unastar_noise/build.rs, crates/unastar_noise/codegen/templates.rs, crates/unastar_noise/Cargo.toml, crates/unastar/Cargo.toml, docs/decisions/0024-do-not-commit-eula-derived-artefacts.md

### Requirement: Chunk pipeline stages and caches
`ChunkPipeline` SHALL produce each finished chunk from per-chunk proto stages plus decoration of the 3x3 origins around it, and MUST generate each proto at most once per position while it is cached. The proto and decoration caches MUST be bounded single-flight FIFO caches (default capacity 1024). `generate` MUST convert source-native blocks to Bedrock state ids exactly once, at the end, returning an `io::Error` when an actor is unrepresentable.

#### Scenario: Protos generated once
- **WHEN** a pipeline generates chunk (0,0) and then chunk (1,0)
- **THEN** the generator built 25 protos for the first chunk and only 5 more for the second

#### Scenario: Decoration spills into neighbours
- **WHEN** decoration of one origin writes across the +x/+z chunk border
- **THEN** the write appears in the neighbouring finished chunk

#### Scenario: Generators without decoration
- **WHEN** `decorates()` is false (flat, void)
- **THEN** the 3x3 decoration stage is skipped and the chunk is the proto with its biomes

Evidence: crates/unastar/src/world/generator/pipeline.rs (`ChunkCache`, `DEFAULT_PROTO_CACHE`, `generate`, `generate_native`, `protos_are_generated_once_per_position`, `decoration_spills_into_neighbours_and_is_order_independent`, `heightmap_tracks_writes_in_the_region`)

### Requirement: Independent generators are a pure function of seed and position
Generators whose `decorates_in_order()` is false (Bedrock-style, flat, void) SHALL produce finished chunks that depend only on the seed and chunk position: request order, cache state and worker count MUST NOT change blocks or block actors. This is what makes parallel generation safe for them (decision 0031).

#### Scenario: Reverse request order
- **WHEN** two fresh pipelines for the same Bedrock-style seed generate chunks (6,-4) and (7,-4) in opposite orders
- **THEN** every block of both chunks is identical between the pipelines

#### Scenario: Neighbour actors merge in any order
- **WHEN** chunks (-1,-1) and (0,0) of a generator that places block actors are requested forwards and backwards
- **THEN** both orders give the same actors at the same positions, and repeating a request gives the same result

Evidence: crates/unastar/src/world/generator/pipeline.rs (`decoration_spills_into_neighbours_and_is_order_independent`, `pipeline_merges_proto_and_cached_neighbor_actors_independent_of_request_order`), crates/unastar/tests/bedrock_worldgen.rs (`bedrock_decoration_is_independent_of_chunk_request_order`), docs/decisions/0031-parallel-chunk-generation-is-allowed.md

### Requirement: Java-style decoration runs in one native session
The Java-style generator (native Java backend) SHALL report `decorates_in_order()` true, and its pipeline MUST decorate through one mutex-guarded native session in which each FEATURES origin completes at most once and observes the writes of origins completed before it. A finished chunk MUST complete its own origin and its eight neighbours first, then promote pending block actors as Java's FULL status does. Java-style output therefore depends on request history, not only on seed and position.

#### Scenario: Finished chunk completes its halo
- **WHEN** `generate_native` is called for a chunk of a native generator
- **THEN** the target origin and its eight neighbouring origins are decorated (each once) before the chunk is snapshotted

#### Scenario: FULL actor promotion
- **WHEN** a native chunk with pending actors is finished
- **THEN** its saved actors match the captured Java 1.21.11 FULL-status boundaries in `native_pending_full_actor_save_boundaries_1_21_11.json`

Evidence: crates/unastar/src/world/generator/terrain.rs (`decorates_in_order`), crates/unastar/src/world/generator/pipeline.rs (`decorate_origin`, `generate_native`, `native_full_snapshot`), crates/unastar/src/world/generator/pipeline/native.rs (`NativeSession`, `finished_native_actor_saves_match_actual_full_promotion`), crates/unastar/tests/fixtures/native_pending_full_actor_save_boundaries_1_21_11.json

### Requirement: Generation pool loads before it generates
`ChunkGenerationWorker` SHALL run on a fixed number of worker threads with 16 MiB stacks, and MUST generate a chunk only after an explicit storage miss; a storage read error MUST be returned to the caller and never replaced by generated terrain. A configured thread count of 0 MUST mean half the available cores clamped to `1..=8`.

#### Scenario: Missing chunk is generated
- **WHEN** the pool is asked for a chunk with no provider
- **THEN** it returns a column for that position with the "loaded" flag `false`

#### Scenario: Automatic thread count
- **WHEN** `generation_threads(0)` and `generation_threads(3)` are called
- **THEN** the first is within `1..=8` and the second is `3`

Evidence: crates/unastar/src/world/ecs/generation_worker.rs (`generation_threads`, `run_worker`, `pool_generates_requested_chunks`, `auto_thread_count_is_bounded`), crates/unastar/src/world/ecs/manager.rs

### Requirement: Exact output hashes gate optimisations
For each feature-enabled generator, `worldgen_output_hash` SHALL hash every fixture chunk (seed 12345, the 68 positions of `bds_1_26_51_seed_12345.json`) and compare with the committed goldens. A change that alters generated bytes MUST fail it; goldens MUST be updated only with separate correctness evidence, never as a performance exception.

#### Scenario: What the hash covers
- **WHEN** a fixture chunk is hashed
- **THEN** the SHA-256 covers the coordinates, all 24 length-framed disk subchunks and the disk and network biome bytes, and a second SHA-256 covers block actors sorted by position with canonical NBT key order

#### Scenario: Changed bytes
- **WHEN** a change alters any generated block, biome or actor in a fixture chunk
- **THEN** `java_fixture_chunk_bytes_are_unchanged` (with `java-worldgen`) or `bedrock_fixture_chunk_bytes_are_unchanged` (with `bedrock-worldgen`) fails and prints the new hashes

Evidence: crates/unastar/tests/worldgen_output_hash.rs (`java_fixture_chunk_bytes_are_unchanged`, `bedrock_fixture_chunk_bytes_are_unchanged`), crates/unastar/tests/fixtures/worldgen_output_hash_java.json, crates/unastar/tests/fixtures/worldgen_output_hash_bedrock.json
Verified by: `cargo test -p unastar --features java-worldgen,bedrock-worldgen --test worldgen_output_hash` (needs local installs; not run in CI)

### Requirement: Parity metrics ratchet against each generator's own reference
Each generator SHALL be measured against fixtures from its own truth source (Java 1.21.11 worlds for Java-style, BDS 1.26.51 worlds for Bedrock-style), and each metric MUST only move one way: a floor may only rise and an error ceiling only fall, in the same commit as the improvement (decision 0019). Only biome placement of the Java-style generator is gated against the BDS fixture.

#### Scenario: Java-style floors
- **WHEN** `java_style_vs_java_reference` measures seed 12345 finished chunks
- **THEN** it fails if any of `terrain_exact`, `terrain_within_2`, `surface_exact`, `surface_block` or `surface_biome` falls below its floor, or `terrain_mean_abs_error` or `histogram_l1` rises above its ceiling

#### Scenario: Java-style spawn
- **WHEN** the Java-style generator for seed 12345 finds the world spawn
- **THEN** it is exactly (96, 136, -32), the Java reference world's spawn

#### Scenario: Bedrock-style ratchets
- **WHEN** `bedrock_style_vs_bds_reference_ratchets` measures the BDS seed-12345 fixture
- **THEN** it fails if a terrain/surface/biome floor is undercut or the terrain or histogram error ceiling is exceeded

Evidence: crates/unastar/tests/worldgen_parity.rs (`java_style_vs_java_reference`, `java_style_biomes_vs_bds_reference`, `java_style_protos_vs_java_reference`, `java_style_spawn_matches_java_reference`), crates/unastar/tests/bedrock_worldgen.rs (`bedrock_style_vs_bds_reference_ratchets`, `foundation_matches_observed_bds_layer_boundaries`, `frozen_ocean_temperature_patches_ratchet_against_bds`), crates/unastar/src/world/generator/parity.rs, crates/unastar/tests/fixtures/java_1_21_11_seed_12345.json, crates/unastar/tests/fixtures/bds_1_26_51_seed_12345.json, docs/decisions/0019-gate-generators-with-fixtures-and-ratchets.md
Verified by: `cargo run -p unastar --example worldgen_parity_report`; Bedrock reference controls are produced with `tools/bds-worldgen-controls`

### Requirement: Allocation ratchet for generation performance
`worldgen_bench --ratchet <file>` SHALL compare a serial, stage-enabled run (`worldgen-profile`) against a committed ceiling and MUST fail if the total or any stage's allocation count or requested bytes increase. The ceiling MUST match kind, seed, chunk count, build profile and stage mode, or the comparison is refused. Wall time MUST be recorded but never gated.

#### Scenario: Allocation regression
- **WHEN** a change makes any stage allocate more often or request more bytes than `tools/worldgen-perf/ratchet-java.json` (or `ratchet-bedrock.json`)
- **THEN** `worldgen_bench --kind java --json --ratchet tools/worldgen-perf/ratchet-java.json` fails

Evidence: crates/unastar/examples/worldgen_bench.rs, tools/worldgen-perf/ratchet-java.json, tools/worldgen-perf/ratchet-bedrock.json, tools/worldgen-perf/README.md
Verified by: `tools/worldgen-perf`

### Requirement: Two numeric backends selected per generator
The noise layer SHALL keep two numeric backends: the calibrated backend (`NoiseRegistry::new`), used by the Bedrock-style generator's shared terrain, and the native Java-exact backend (`NoiseRegistry::with_router_settings`, returning `NativeNoiseRegistry`), used by the Java-style generator. Selecting one MUST NOT change the arithmetic of the other (decision 0032).

#### Scenario: Native backend matches captured Java bits
- **WHEN** the native registry samples legacy climate, blended noise or modern octave sums at the pinned seeds and positions
- **THEN** scalar and SIMD results equal the captured Java 1.21.11 bit patterns

#### Scenario: Backend per generator
- **WHEN** a `JavaGenerator` is built with `JavaGenerator::new(seed)`
- **THEN** it uses the native backend and decorates in order, while the Bedrock-style generator's terrain uses the calibrated `NoiseRegistry::new(seed)`

Evidence: crates/unastar/src/world/generator/density/types.rs (`NoiseRegistry::new`, `with_router_settings`, `NativeNoiseRegistry`, `legacy_java_climate_matches_native_scalar_and_simd_bits`, `java_blended_simd_preserves_scalar_bits_across_smear_and_height_boundaries`, `native_modern_router_uses_exact_octave_persistence`), crates/unastar/src/world/generator/terrain.rs (`TerrainBackend`), crates/unastar_noise/src/native_density.rs (`native_float_spline_segments_preserve_measured_bits`), docs/decisions/0032-two-worldgen-numeric-backends.md

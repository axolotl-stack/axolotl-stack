# unastar_noise — the worldgen AOT toolchain

Everything worldgen derives from data is compiled here at build time into straight-line
Rust (decision D6). Runtime generators live in `crates/unastar/src/world/generator/`.

## Layout

| Path | Role |
|---|---|
| `worldgen_data/` | The pinned Java version's `data/minecraft/worldgen` tree, unchanged (Java 1.21.11, see `java_data/README.md`). |
| `java_data/` | Java reports and Java→Bedrock maps (block states, collisions, biome ids, biome parameter points, biome tags), written by `tools/import_java_data.py`. |
| `build.rs` | Orchestrates the phases below; reruns when any input or `codegen/` changes. |
| `codegen/parser/<family>.rs` | Phase 1: JSON → IR (`serde`). Unknown or malformed input fails the build. |
| `codegen/data/<family>.rs` | Phase 1 for non-worldgen tables (`java_blocks`: resolve Java block states to Bedrock runtime ids and block classes at build time). |
| `codegen/analyzer/` | Phase 2: density-function graph analysis (dedup, flat/2D caches, topological order). |
| `codegen/emitter/<family>.rs` | Phase 3: IR → `TokenStream` (`quote`), pretty-printed with `prettyplease` to `OUT_DIR/<module>.rs`. |
| `codegen/emitter/mod.rs` | `emit_all`: one line per family; `GeneratedModules` writes each module and `OUT_DIR/mod.rs`. |
| `src/<family>/` | Runtime types and traits the generated code calls into (noise samplers, `block_class` flags, surface context, …). |

### Adding a data family

1. `codegen/parser/<family>.rs`: typed IR from the JSON, failing on anything unknown.
2. `codegen/emitter/<family>.rs`: `pub fn emit(...) -> TokenStream`; resolve block states
   with `codegen::data::java_blocks::JavaBlocks::resolve` and emit the Bedrock runtime id
   as a literal (no run-time block lookups).
3. One `modules.public("<family>", ...)` line in `emit_all`; the module is then
   `unastar_noise::<family>`.
4. Runtime support types in `src/<family>/`.

### Generated modules

| Module | Content |
|---|---|
| `noise_params` (glob) | `NoiseRef`, `NOISE_PARAMS` |
| `random_hashes` | md5-derived seeds of Java's named positional randoms (`ORE`, …) |
| `overworld_settings` | noise settings scalars, default block/fluid, blended noise, `SPAWN_TARGET` (quantized spawn climate points) |
| `overworld_compiled` (glob) | AOT overworld router: `compute_*`, `FlatCacheGrid`, `ColumnContext`, `ColumnContextGrid` (the 25 cell-corner columns of a chunk) |
| `biome_features` (glob) | `BiomeFeatures`, `GenerationStep` |
| `overworld_surface` | compiled overworld surface rule |
| `java_blocks` | `JAVA_TO_BEDROCK`, `JAVA_STATE_CLASSES`, `BEDROCK_STATE_CLASSES` |
| `java_biomes` | `JavaBiome` with climate and Bedrock biome id |
| `overworld_biome_source` | multi-noise R-tree search (`target`, `find`, `quantize`) |
| `carvers` | `OVERWORLD_CARVERS` with replaceable-block bitsets |

## Block identity

Generated code works in Bedrock runtime ids (`u32`, the canonical 1.26.51 palette ordinals
used by `unastar`'s chunks). Java block states are resolved at build time through
`java_data/bedrock_blocks.json`; every Java 1.21.11 state maps to exactly one Bedrock
state. `block_class` flags (air, fluid, water, leaves, blocks-motion, full top face) drive
heightmaps and the world-spawn search; they come from the Java collision shapes.

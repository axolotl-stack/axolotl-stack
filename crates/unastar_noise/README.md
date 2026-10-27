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
| `features::java_blocks` (`java_registry.rs`) | Java block registry for decoration: per-block properties, `blocks::*`/`states::*`/`tags::*` consts, tags and inline block lists as bitsets, per-state solid / sturdy-face / full-cube flags derived from `collisions.json` |
| `features::generated` (`features.rs`, `features_code.rs`) | Biome table and climate, every configured feature as a `cf_*` fn with a static config, every placed feature as a `pf_*` fn with its placement modifiers inlined, per-dimension step order (`OVERWORLD_FEATURES`, ...) and biome filters |
| `structures::generated` (`structures.rs`) | Processor lists, template pools, structures and structure sets as static data |
| `structures::template_data` (`templates.rs` + `templates.bin`) | Structure templates from the local server jar (feature `java-templates`) |
| `java_biomes` | `JavaBiome` with climate and Bedrock biome id |
| `overworld_biome_source` | multi-noise R-tree search (`target`, `find`, `quantize`) |
| `carvers` | `OVERWORLD_CARVERS` with replaceable-block bitsets |

## Native End density primitive

`end_islands::EndIslands::new(seed)` constructs the native legacy stream and
simplex permutation once. `sample(block_x, block_z)` returns the Y-independent
horizontal density; `sample_array` preserves scalar bits per lane without heap
allocation. Callers should initialize this only when an End router reaches it,
not eagerly for every Overworld noise registry.

The two native-bit regressions passed 528 measured Java 1.21.11 samples across
eight seeds, including disjoint holdouts, signed division boundaries, distant
float precision, wrapping radial squares and both i32 endpoints. This verifies
the primitive, not End router wiring or finished End chunks.

## Block identity

Generated code works in Bedrock runtime ids (`u32`, the canonical 1.26.51 palette ordinals
used by `unastar`'s chunks). Java block states are resolved at build time through
`java_data/bedrock_blocks.json`; every Java 1.21.11 state maps to exactly one Bedrock
state. `block_class` flags (air, fluid, water, leaves, blocks-motion, full top face) drive
heightmaps and the world-spawn search; they come from the Java collision shapes.

## Java decoration and structures

Features and structures run in **Java block-state space** (`features::JavaState`, the Java
1.21.11 state id) against the `features::FeatureWorld` trait; the runtime converts to
Bedrock ids at the region boundary (`unastar::world::generator::features`).

- `codegen/parser/feature.rs`, `codegen/parser/structure.rs`: typed IR of every vanilla
  configured/placed feature, structure, structure set, template pool and processor list,
  closed enums with `deny_unknown_fields` (an unknown type or field fails the build).
- `codegen/emitter/feature.rs`: placed features compile to nested loops and `if`s in
  Java's lazy stream order, so random draws happen in the same order as Java; Java's
  `FeatureSorter` order per dimension (feature index = seed input) is computed at build
  time from the biome parameter lists.
- `src/features/`: `WorldgenRandom` (Java bit-source semantics, decoration and feature
  seeds), value providers, predicates, survival rules, climate, the decoration driver
  (`decorate::decorate_chunk`) and one module per feature type in `kinds/`.
- `src/structures/`: set placement (random spread, concentric rings, frequency reduction,
  exclusion zones), `StructureManager` (starts per chunk, references within 8 chunks,
  per-step placement hook), the `StructurePiece` base, template placement with processors,
  and every structure type in `kinds/` (jigsaw, stronghold, mineshaft, monument, temples,
  igloo, buried treasure, mansion, ocean ruins, shipwrecks, ruined portals, nether
  fossils, fortress, end city).
- Structure templates are Mojang assets and are never committed: with the
  `java-templates` feature the build reads `data/minecraft/structure/*.nbt` from the jar
  named by `UNASTAR_JAVA_SERVER_JAR` (`unastar` forwards it as `java-worldgen`). Without it
  the template table is empty and `unastar` rejects `world.generator.kind = "java"`.
- Template block actors are compiled at build time to explicit Bedrock actor ids and
  NETWORK-endian NBT child tags. Root/coordinates and lifted loot/spawner identity
  are supplied at placement; marker/processor loot overrides retain other fields.
  Unknown actor ids and malformed conversion inputs fail the build. Other source
  metadata is preserved with its NBT types (including Java-only fields); native
  Bedrock interpretation of those fields is not implied.
- Output is a pure function of (seed, chunk): pieces never change while placed. Where
  Java's result depends on which chunk is decorated first, the code picks an
  order-independent rule; these and other deliberate deviations are listed in
  `thoughts/shared/research/java-quirks.md`.


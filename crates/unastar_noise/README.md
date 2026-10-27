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

### Density branch specialization

Density `range_choice` branches retain their own temporary scopes: unselected
noise subtrees are not hoisted into eager root statements. Four-Y SIMD evaluates
one branch for uniform lane masks, and both branches with per-lane selection for
mixed masks. Enclosing common expressions remain reusable, without leaking
branch-local names. Arithmetic precision and expression order are unchanged.
`tests/density_laziness.rs` covers range boundaries and uniform/mixed SIMD masks;
the integration chunk-byte/actor goldens remain the complete-output gate.
See the performance report for measured results versus unmeasured candidates.

### SIMD column memoization

`compute_final_density_4` takes a `FinalDensityCache4` reference after its column
context. Create one default cache per XZ column and reuse it across Y batches
with the same immutable noise source. The generator assigns fixed stack slots
to zero-Y-scale `Noise` leaves; there is no runtime hashing or allocation.
Each slot retains the exact vector result and distinguishes signed-zero lane
patterns and XZ coordinates. Scalar column values are not substituted for SIMD
results, so existing FMA precision and expression order remain unchanged.

### Density input correctness

Routers without `cache_2d` nodes use the same generated context/grid implementation
with zero fields, including local scalar and SIMD types; no Overworld aliases are
needed. The compiled End regression exercises a complete empty-cache scalar/SIMD
root against native bits. Unknown builtin references fail generation with their
name. `OldBlendedNoise` cannot be lowered into a 2D cache that loses its vertical
coordinate: these paths fail generation rather than fabricating zero or silently
sampling at Y=0; ordinary scalar/SIMD evaluation remains available.

### Runtime scratch storage

Java tree position sets keep their initial 16 bucket heads and tree-bin flags
inline. Logical table length still controls Java's load threshold, resizing,
treeification and iteration order; node storage and ordering are unchanged.
Block rotation snapshots the four original horizontal connection values into
an array before writing rotated properties, without a per-block scratch Vec.

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

`NoiseSource::sample_end_islands` supplies block-coordinate samples to every
scalar, four-Y SIMD, flat-cache, column-cache and standalone-cache emitter.
`NoiseRegistry` initializes the sampler lazily from the unmodified world seed;
ordinary Overworld/Bedrock registries do not construct the legacy End stream.
Four-Y lanes share X/Z, so a single scalar sample is splatted without changing
lane bits. A compiled generated regression checks direct nodes and inline spline
coordinates in all five expression environments, plus a complete empty-cache
root, against native seed-sensitive and wrapped-NaN fixtures. The production registry
has a separate native-seed regression. These are End correctness changes, not
output-preserving optimization speedups or proof of finished End chunk parity.

Scalar signatures also track actual column-cache use during lowering: a shared
inner node may reuse a `cache_2d` wrapper registered by another root even when
that wrapper is not a structural descendant. The compiled End regression covers
this cross-root alias without substituting zero or recomputing cached values.

## Native Java numeric source

`NoiseRegistry::with_router_settings(seed, legacy_random_source, blended)` accepts
the router's five blended-noise parameters in XZ scale, Y scale, XZ factor, Y
factor, smear order. Use its borrowed `java_source()` adapter for statically
dispatched Java scalar/SIMD arithmetic. The default `NoiseRegistry::new(seed)`
and ordinary registry `NoiseSource` implementation preserve the calibrated
terrain backend; selecting native Java math does not silently change Bedrock.

Native constructors distinguish legacy positional string hashes, fixed legacy
Nether temperature/vegetation streams, the zero legacy offset stream, and raw
world-seeded legacy blended noise. Name/octave string hashes are generated at
build time. Explicit `sample_java`/`sample_java_4` entrypoints use unfused
interpolation, centered coordinate wrapping and the promoted native float smear
epsilon; existing fused entrypoints keep their previous arithmetic.

Pinned seed-boundary/holdout regressions cover raw native climate bits in scalar
and SIMD lanes. A temporary actual-registry corpus smoke matched 984 native
Nether climate fields in both modes and 492 native raw Nether density fields;
1,968 blended SIMD lanes also matched their scalar bits. These numeric proofs do
not establish finished three-dimension chunk parity or complete surface behavior.

### Native cell interpolation

`AotEmitter::with_geometry(min_y, height, cell_width, cell_height)` emits a
dimension-specific `NativeCellSampler` with fixed-size, allocation-free source
planes and final-material storage. Only schema-marked `interpolated` inputs are
interpolated; nonlinear final-density operations execute afterward. Plane
advancement swaps indices rather than copying plane arrays.

Native cell-bulk evaluation uses XYZ interpolation order; active point/vein
evaluation uses YXZ. These orders are numerically distinct at measured material
boundaries. Native cache constructors and `compute_<root>_native_uncached`
entrypoints also preserve native f32 spline evaluation rather than the calibrated
backend's f64 polynomial lowering. Existing calibrated entrypoints are retained
as a separate backend, not redirected.

Regressions cover measured raw material/spline bits and an actual emitted
sampler's nonlinear material evaluation across negative chunks, descending Y,
SIMD source batches and plane reuse. Native material adds the mandatory
`NoiseSource::sample_beardifier(x,y,z)` contribution once after the router root,
inside the material cell cache. Plain sources explicitly return zero; structured
sources must supply their engine. Full native structure influence and the
three-dimension runtime cutover remain separate gates.

`NoiseSource::NATIVE_JAVA` is mandatory and statically selects the aquifer's
native cache constructors and root functions. `NoiseBasedAquifer` accepts a typed
source with the calibrated registry as its default type; existing calibrated
callers keep their arithmetic. Native ore decisions use `NativeOreVeinifier`,
which retains only the positional factory, with a toggle value and lazy ridged/gap
callbacks from the active cell sampler. It allocates no second corner grid.
Native richness comparison promotes the RNG's f32 draw to f64 rather than
rounding the threshold down to f32.

The ore producers in this retained-consumer branch return Bedrock runtime IDs.
A raw canonical Java-state consumer must use its emitted Java IDs at the producer;
the word “native” describes numeric/material decisions, not interchangeable
integer state domains. Do not copy a whole ore module across that cutover.

The native aquifer uses the measured Java 1.21.11 dry-fluid sentinel `-32512`.
The calibrated profile retains its previous sentinel. This is a semantic state
value, not pressure tuning: a near-i32-minimum dry level overflows the dry/wet
integer distance and can erase native stone barriers. Native regressions cover
both primary and second/third pressure-pair barriers and captured raw pressures.

`NativeColumnSampler::new(block_x, block_z, source)` owns the genuine
one-horizontal-cell profile: floor-divided block origins, two Z columns of source
planes, and just four quart evaluations for 4-wide cells or nine for 8-wide cells.
Its material engine is derived from the bulk sampler through typed AST changes,
not a second hand-maintained implementation. Base columns always use the empty
Beardifier marker, even if the supplied source also supports structures.
`initialize`, `advance_cell_x(0)`, `select_cell_yz(y,0)`, absolute X/Y updates and
`densities_4z` expose the same cell traversal contract; 8-wide cells use Z batches
0 and 4. This is not full-chunk generation followed by column extraction.

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
  Runtime templates are decoded individually into stable single-flight slots,
  avoiding eager allocation of every embedded template on the first structure.
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
- Concentric-ring points retain Java's sequential RNG/fork order. Biome relocation
  is resolved on demand only for points within seven chunks of a placement query;
  the 112-block search cannot move a point farther on either axis. Results are
  single-flight per point and independent of query order or generation worker.
- Short-lived jigsaw connector lists stay inline, with exact spill reservation for
  larger templates. Shuffle draws and stable selection-priority ordering are unchanged.

## Performance instrumentation

The opt-in `worldgen-profile` feature exposes exclusive, thread-local stage timers
and allocation attribution through `profile`. Ordinary builds compile scopes away;
the benchmark supplies the counting allocator. `unastar` forwards this feature to
its `worldgen_bench` example, which reports every Java decoration step, structure
starts/references/pieces/templates, terrain stages and Bedrock feature passes.
See [`tools/worldgen-perf`](../../tools/worldgen-perf/README.md) for deterministic
allocation ratchets, actual-generation-pool throughput, cold spawn-radius timing,
one-P-core alternating A/B measurements and non-admin Windows CPU profiling.



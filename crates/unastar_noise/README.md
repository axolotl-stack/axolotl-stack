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
| `java_blocks` | Canonical-to-Bedrock boundary mapping, native/Bedrock class tables, secondary-water projection flags, and actual native support/wall/center/behavior arrays validated from `block_physics.json` |
| `features::java_blocks` (`java_registry.rs`) | Java block/property registry, `blocks::*`/`states::*`/`tags::*` constants and interned sets; solid/sturdy/collision flags reuse the canonical native physics observations |
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

`with_router_settings` returns `NativeNoiseRegistry`, the
`NoiseRegistry<NativeDoublePerlinNoise>` specialization. Explicit native registry
fields should use that alias; default `NoiseRegistry` fields remain calibrated.
Native octave records keep their value factors outside the calibrated
`PerlinNoise` layout. Each octave bank retains its single vector: no extra
allocation, sampling-time profile branch or larger calibrated Perlin record.

Native constructors distinguish legacy positional string hashes, fixed legacy
Nether temperature/vegetation streams, the zero legacy offset stream, and raw
world-seeded legacy blended noise. Name/octave string hashes are generated at
build time. Explicit `sample_java`/`sample_java_4` entrypoints use unfused
interpolation, centered coordinate wrapping and the promoted native float smear
epsilon; existing fused entrypoints keep their previous arithmetic.

Modern native octaves derive persistence from the exact
`2^(length-1) / (2^length-1)` expression rather than the calibrated decimal table.
Native weighted sums retain amplitude and persistence separately so their order
is `(amplitude * sample) * persistence`; folding temperature's `1.5` amplitude
into persistence changes raw bits. Calibrated scalar/SIMD sums retain their
precombined weights and previous persistence table.

Pinned seed-boundary/holdout regressions cover raw native climate bits in scalar
and SIMD lanes. A temporary actual-registry corpus smoke matched 984 native
Nether climate fields in both modes and 492 native raw Nether density fields;
1,968 blended SIMD lanes also matched their scalar bits. These numeric proofs do
not establish finished three-dimension chunk parity or complete surface behavior.

The far Overworld aquifer trace also matched all 144 raw router fields across a
target and its eight cell corners, plus four actual material-cell densities,
bit-for-bit through a normal release CLI using the production registry and
generated sampler. This is a numeric-source proof, not full-chunk native parity.

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

Native and shared-backend ore producers return canonical Java states; the
Bedrock edition converts after generation (`DenseBlocks::convert_to_bedrock`).
The word “native” describes numeric/material decisions, not interchangeable
integer state domains.

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

## Native badlands clay bands

`surface::clay_bands` preserves the native orange-loop cursor check before
consuming another random gap. An orange placement at the final array index
advances the cursor to 192 and must end that loop without drawing again; an
extra draw changes all subsequent colour bands.

`tools/nativebadlandsprobe/NativeBadlandsOracle.java` captures the actual pinned
1.21.11 tagged-runtime `RandomState.surfaceSystem` band array and offset noise.
It also delegates native `generateBands` calls through the original native
random source, records the draw sequence, and checks that the replayed array
equals the constructor's array. Compile with the pinned runtime classpath;
run `nativebadlandsprobe.NativeBadlandsOracle <runtime-root>
<compiled-oracle-directory> <output.json> <seed> <x:y:z>...` with JDK21,
`-XX:ActiveProcessorCount=1 -Xmx1G`. It uses the existing compiled
`NativeWorldgenOracle` reflection support without changing that helper or
starting a world/server.

The regression retains the complete observed 192-band array for the signed
holdout seed `-734287144591`; a generator regression separately checks actual
SURFACE block consumers against immutable native chunk `(100,-100)` witnesses.
These checks are not a claim of full-world parity.

## Block identity

Native Java router settings, terrain, surface, carvers, features and structures use
canonical Java 1.21.11 state ids. `DenseBlocks` and chunks carry their identity
domain explicitly; no inverse of the many-to-one Bedrock map is used.
`java_data/bedrock_blocks.json` supplies the one-way output mapping to the
Bedrock 1.26.51 palette. Native class/support/behavior arrays are validated from
actual `block_physics.json` state observations, not inferred collision shapes.
The calibrated shared Bedrock terrain backend converts its explicitly bounded
material domain before Bedrock-specific geology and surface processing.

## Java decoration and structures

Features and structures run in **Java block-state space** (`features::JavaState`, the Java
1.21.11 state id) against the `features::FeatureWorld` trait. Canonical Java states and
native actors survive all generation stages; Bedrock conversion is an explicit final
output boundary, never a region read/write conversion.

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
  Compiled feature order and possible-biome masks are selected independently for
  Overworld, Nether and End. Decoration's `WorldgenRandom` remains Xoroshiro in all
  three; the distinct region-native random uses the preset's Xoroshiro/legacy
  positional factory, including native boolean/float/bounded-integer semantics.
  Region storage bounds are distinct from generator depth (Nether/End: 0..256
  storage, 128 noise depth).
  Placement height anchors use `WorldGenerationContext`: `below_top` is relative
  to `min(build height, generator depth)`, not the storage ceiling. Nether quartz
  therefore samples 10..117 even though the dimension stores blocks through Y255.
  Native sorter fixtures in `tests/data/native_feature_order_1_21_11.json` retain
  actual registered step indices for all three dimensions.
  `native_placed_quartz_1_21_11.json` records complete native quartz coordinates
  and RNG continuation for three signed seeds at two origins in isolated
  homogeneous worlds. `tools/NativeFeaturePrimitives.java` regenerates those
  observations through the actual tagged placed feature; the regression calls
  the generated Rust placement consumer, independently of chunk replay order.
- `src/structures/`: set placement (random spread, concentric rings, frequency reduction,
  exclusion zones), `StructureManager` (starts per chunk, references within 8 chunks,
  per-step placement hook), the `StructurePiece` base, template placement with processors,
  and every structure type in `kinds/` (jigsaw, stronghold, mineshaft, monument, temples,
  igloo, buried treasure, mansion, ocean ruins, shipwrecks, ruined portals, nether
  fossils, fortress, end city).
  The generator's statically dispatched `NativeStructureSource` supplies unzoomed
  biomes and actual no-structure base columns. Base heights use the native column
  states and each heightmap's own predicate, without density-root point sampling,
  eight-block skipping, or default-fluid substitutes. Native column primitives
  use the one-horizontal-cell `iterateNoiseColumn` traversal, not bulk NOISE
  chunk extraction; the dedicated oracle `columns` mode records those public
  base-column/base-height calls independently of feature captures.
  The `structure-order` oracle mode separately records actual runtime structure
  registry order and native dimension-specific `possibleStructureSets`; seeding
  still indexes the full registry's per-step order, not a dimension-filtered list.
- `structures::beardifier::Beardifier::for_chunk(starts, chunk_x, chunk_z)` captures
  native-ordered affecting starts once. `sample(x, y, z)` then uses compact boxes
  and junctions without allocation, RNG changes, or piece virtual dispatch.
  Ordinary pieces, rigid/terrain-matching pool projections, each piece's actual
  ground-level delta, strict junction bounds, inclusive piece-radius culling, and
  the Java 1.21.11 collected-geometry union inflated by 24 are retained. All five
  terrain adjustments are implemented; structure selection must continue to use
  **unadapted** native base columns to avoid a selection/density cycle.
  `tools/NativeBeardifierOracle.java` is an independent pinned native-API capture
  utility (package `nativebeardifier`, JDK21, one processor, `-Xmx1G`, no server
  or sockets). Its arguments are the external runtime root and output JSON path.
  The checked-in fixture records 15 deliberately constructed native collection
  cases, 7,800 consumer samples and 1,225 primitive samples, covering signed
  chunks, negative Y, nonzero ground deltas, projections, junction edges and
  affected-box clipping. Regressions consume real runtime piece metadata and
  that native capture; this does not establish generated-structure/full-world
  parity or claim that Rust checks have been run.
- Ruined-portal netherrack spread uses the native 14-entry distance table, with
  seven guaranteed-probability entries before its thinning edge. The table length
  bounds the diamond and therefore also controls subsequent random draws for
  magma, drip columns and overgrowth. `native_portal_spread_1_21_11.json` retains
  five native spread consumers covering warm/cold land, submerged ocean-floor
  placement with overgrowth, underground placement and Nether lava replacement.
  The regression checks ordered writes, final canonical states, live heightmap
  queries and the next random long; this is not a claim of full-world parity.
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



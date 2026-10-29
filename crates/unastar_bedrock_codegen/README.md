# unastar_bedrock_codegen

Build-time toolchain for unastar's Bedrock-style world generator, targeting BDS
1.26.51. It reads the vanilla behaviour packs of a local BDS install and compiles
feature rules, features and biome surface parameters into straight-line Rust
that `crates/unastar` includes (`world::generator::bedrock::generated`).

The compiled data path is exercised against the local install; **BDS world parity
is not established**. Runtime seeding, built-in feature geometry, scatter semantics
and surface noise still require reference-world measurements. See
`thoughts/shared/research/bds-quirks.md` for the explicit inference boundaries.

| Runtime area | Status | Needed BDS control |
|---|---|---|
| Shared Java noise/climate/biome mapping and spawn-center selection | unmeasured: to confirm by BDS fixture | Matched terrain/biome/spawn samples across seeds; safe-spawn checks use finished Bedrock chunks |
| Base bedrock/deepslate layers | Topology and transition boundaries measured in 1,024 columns; RNG/distribution unmeasured: to confirm by BDS fixture | `bds_1_26_51_seed_12345_geology.json`: contiguous bedrock thickness 2–5, stone absent through y=0, deepslate absent from y=8; capture more seeds and exact random draws |
| Chunk/pass RNG seeding and feature draw order | unmeasured: to confirm by BDS fixture | Marker placements across positive/negative chunks for one fixed seed |
| Scatter coordinate frames and distribution endpoints | BDS marker fixture confirms numeric/Molang offsets, nearest/half-away offset rounding and upper-exclusive uniform extents; other distributions remain unmeasured | Gaussian and grid controls |
| Molang noise and query height semantics | World-independent noise model matches 32 scalar carriers and 16 independent exact float patterns; stone/water controls confirm world-space heights and fluid distinction | Broader noise inputs and foliage/material height queries |
| JSON ore feature coordinates | Count-4/17/32 coal controls establish the internal (+8,+8) horizontal translation; exact vein shapes and RNG streams remain unmeasured | More shapes per count and measured feature RNG initialization |
| JSON tree/plant/geode/sculk/blob/decoration feature placement | unmeasured: to confirm by BDS fixture; one oak control is structurally consistent with the current crown model | Isolated features at known origins in controlled material |
| Built-in aquatic/plants/terrain features and legacy selectors | unmeasured: to confirm by BDS fixture | One built-in identifier per control, fixed seed and biome |
| Surface builders, noise seeds, mountain slopes and adjustment precedence | Fixed native frozen-ocean climate matches all 1,658 reference water/ice columns; iceberg shape and remaining builder fields are unmeasured | More seeds plus matched mesa/swamp/capped/end/gradient columns |
| Cave/underwater/nether carver geometry and probability | unmeasured: to confirm by BDS fixture | Isolated carver rules in stone and underwater regions |
| Template rotation, jigsaw expansion, structure placement and processors | unmeasured: to confirm by BDS fixture | Template/pool controls at known origins plus structure-set placements |
| Hard-coded structures without shipped placement data | Not implemented | Reference-world placement captures and controlled templates |

The synthetic/unit checks validate the implemented invariants, not equivalence
to BDS. Data-derived parameters are compiled; the unmeasured algorithms above
must not be treated as established vanilla behavior.

The BDS packs are under the Minecraft EULA: they are **read at build time and never
committed**. No vanilla data lives in this crate.

## Using it

`unastar` enables it with its `bedrock-worldgen` feature (off by default):

```sh
UNASTAR_BDS_DIR=/path/to/bds/server cargo build -p unastar --features bedrock-worldgen
```

`UNASTAR_BDS_DIR` must point at a BDS 1.26.51 server directory, the one that contains
`bedrock_server(.exe)`, `definitions/feature_rules`, `definitions/features` and
`behavior_packs/vanilla*`. The build fails with a message naming the variable when it is
missing or the layout is wrong. Block identities come from protocolgen's native data
(`crates/valentine_gen/protocolgen`, initialise the submodule). The build script reruns
when the variable or any file it read changes.

## Phases

| Phase | Module | What it does |
|---|---|---|
| Ingestion | `unastar_packs::{brarchive,json,packs}` | Shared `.brarchive` reader, JSON with comments, version stack `definitions` < `vanilla` < `vanilla_<version>` ascending up to 1.26.51; the last layer defining an identifier wins |
| Identity | `blocks`, `unastar_packs::nbt` | canonical palette (`canonical_block_states.nbt`), block tags, physical flags; descriptors resolve to runtime ids; pre-flattening names (`minecraft:log` + `old_log_type`, `minecraft:stone` + `stone_type`, `minecraft:tallgrass`, ...) go through `LEGACY_BLOCKS` / `STATE_RENAMES` |
| IR | `ir`, `model` | strict typed parsing of biomes, feature rules, features, spawn rules (unknown keys fail the build); reachability from overworld rules; built-in registry check |
| Molang | `unastar_rules::molang`, `molang` | Shared lexer/parser; worldgen-only binding/lowering, constant-folding (`variable.is_legacy` = 0) and `f32` Rust emission; no runtime interpreter |
| Emission | `emit` | `run_pass` with per-rule biome masks, one `rule_*` per rule, one `f_*` per reachable JSON feature, `BIOMES` surface data, `blocks`/`bb`/`props` constants |

`builtin` lists the built-in features (identifiers without a JSON definition) that the
runtime implements in Rust; a reachable built-in missing from the list fails the build.
`builtin_blocks` declares the blocks hand-written runtime code needs.

Legacy Java-format templates shipped with BDS can retain older identifiers:
`minecraft:chain` is upgraded to native `minecraft:iron_chain` before palette
resolution, preserving its axis under rotation. The conversion is exercised on
the local bastion stage templates; placement/pool behavior remains unmeasured.

Template palettes deduplicate **paired** primary/secondary permutations for all
four rotations; packed cells remain five bytes. Both native layer arrays must
match the template volume. A native `-1` index preserves its destination layer
independently, rather than becoming air
([format reference](https://wiki.bedrock.dev/nbt/mcstructure#block-indices)).
Java-template lowering uses shared
`unastar_packs::block_state::java_secondary_water` before primary projection:
wet states and intrinsic water plants retain source water, dry states explicitly
clear it, and primary water does not gain a duplicate liquid layer.
`KEEP_LAYER` and air remain distinct actions through emission and one atomic
runtime pair write; placement does not parse NBT or consume extra RNG draws.

Template block actors are decoded at build time through shared
`unastar_packs::{nbt,block_entity}`. The emitter stores sparse cell-indexed
actor records with native IDs and pre-encoded network NBT children; placement
only supplies transformed absolute coordinates and lifted loot/spawner fields.
Both template features and jigsaw pieces write the shared region's actor sink.
Processors replacing an actor's block type discard that actor; state-only
changes preserve it. Metadata introduces no RNG draws. Unknown actor identities
fail generation rather than being silently omitted.
The caller explicitly selects the actor source schema: Java loot paths/seeds
are lifted for native placement, while native Bedrock loot stays in the static
body unchanged. In particular, three captured natural BDS chests use NBT Int
`LootTableSeed`; applying Java's Long-only schema would reject those records.

Retaining Java-only fields does not establish that Bedrock interprets them.
Template entities, legacy hard-coded structure assembly and native behavioral
parity remain separate work.


## Tests

`cargo test -p unastar_bedrock_codegen`. Unit tests use synthetic palettes; shared
archive/version-stack tests live in `unastar-packs`, parser tests in `unastar-rules`.
`tests/bds_install.rs` checks the real install (counts, every reference resolves) and
skips with a message when `UNASTAR_BDS_DIR` is not set.

`cargo run -p unastar --features bedrock-worldgen --example bedrock_decoration_smoke`
applies each overworld biome's compiled surface and decoration rules to a shared
3x3 terrain region, printing the resulting block changes. This catches runtime
failures across all biome rule paths; it is not a parity test or a controlled
performance benchmark.

## Measured baseline (2026-10-25)

Merged runtime `7677b34c`, local BDS 26.51 inputs. The unchanged 68-chunk
seed-12345 BDS fixture contains 17,408 columns.
`cargo test -p unastar --features bedrock-worldgen --test bedrock_worldgen`
ratchets these measurements; it also checks request-order independence and
the observed foundation-layer boundaries.

### Controlled-correction ratchet exception

On the same merged source, temporarily reverting only four disproven
scatter/query assumptions exactly reproduces the previous baseline. Restoring
the BDS-proven behavior changes the aggregate measurements as follows:

| Parity metric | Old-behavior ablation | BDS-corrected |
|---|---:|---:|
| Terrain height exact | 0.8431181066176471 | 0.8354204963235294 |
| Terrain height within 2 blocks | 0.8824103860294118 | 0.8819508272058824 |
| Terrain mean absolute error (blocks) | 1.9171070772058822 | 1.9357766544117647 |
| Surface height exact | 0.8532284007352942 | 0.8574793198529411 |
| Surface block match | 0.8023322610294118 | 0.8057789522058824 |
| Surface biome match | 0.9950597426470589 | 0.9950022977941176 |
| Histogram L1 | 0.07409867366640406 | 0.07266664986780405 |

The coordinator authorized the terrain/within-two/error ratchet exception on
2026-10-25; surface, block and histogram ratchets are strengthened, and the biome
floor is preserved. No fixture, shared noise implementation or unrelated
generation stage changed in the ablation. The four controlled references in
`unastar/tests/fixtures/bds_1_26_51_feature_coordinates.json` are:

- Relative numeric/Molang coordinates: `numeric`, `literal`, `nested_literal`,
  `nested_random` at input x=-128, 0, 128.
- Upper-exclusive uniform extents: `uniform`, 128 attempts per origin.
- Nearest/half-away offset rounding: the six `round_*` cases.
- Fluid-inclusive world-space heightmap versus solid height:
  `heightmap_zero`, `above_top_solid_zero`, plus the four y=280 query controls.

Each correction has a failing-before/passing-after regression. **[INFERENCE]**
The lower `terrain_exact` likely exposes still-unmeasured feature geometry,
RNG/draw order or query behavior outside the controlled stone/water case;
the earlier wrong assumptions were compensating for part of that mismatch.

Benchmark of corrected runtime `c154b89d`: `worldgen_bench --kind java|bedrock`,
same binary with both `bedrock-worldgen,java-worldgen` enabled, 68 fixture chunks,
three rounds alternating Java then Bedrock. Windows affinity mask `4` pins one
P-core logical CPU; power-plan maximum processor state remains 99% (turbo
disabled). No other benchmark suite ran in this worker; other workers were
active. These are current baselines, not optimization speedup claims. Both
generators made 264 protos for 68 cold finished chunks and include their current
decoration/structure implementations. Bedrock legacy structures remain missing,
so the rows do not imply equivalent completeness or parity.

| Profile | Generator | Proto median (range), ms/chunk | Finished median (range), ms/chunk | Allocations/finished chunk | KiB allocated/finished chunk |
|---|---|---:|---:|---:|---:|
| Debug | Java | 70.97 (70.34–73.25) | 588.93 (587.83–609.27) | 2805 | 2511 |
| Debug | Bedrock | 66.32 (65.93–67.51) | 327.30 (326.97–331.31) | 1949 | 1417 |
| Release | Java | 8.33 (7.87–8.43) | 80.29 (79.28–86.37) | 2805 | 2511 |
| Release | Bedrock | 7.30 (6.89–7.83) | 37.12 (36.24–37.53) | 1949 | 1417 |

Release built with
`cargo build --release -p unastar --features bedrock-worldgen,java-worldgen --bin unastar --example worldgen_bench`.
The server's `kind = "bedrock"` configuration reached `Server ready` in an
isolated loopback-only startup smoke; no real-client visual check was performed
by this worker.

### Surface-builder comparison boundary (WD2)

The fixture covers `overworld` and `frozen_ocean` builders (ocean, jungle edge,
roofed forest and two lush-cave surface columns). It does not validate mesa,
swamp, capped, end or noise-gradient builders.

For the 1,658 frozen-ocean columns whose reference surface is water or ice,
the previous world-seeded model, shared Java
`biome_climate::cold_enough_to_snow` predicate and fixed native-MT model were
sampled at identical coordinates:

| Candidate freezing predicate | Correct water/ice classifications | Accuracy |
|---|---:|---:|
| Previous world-seeded Bedrock model | 672 | 40.53% |
| Shared Java frozen temperature modifier | 575 | 34.68% |
| Native MT fixed frozen modifier | 1658 | 100.00% |

Use the measured native model for this predicate: fixed MT3456 octaves
`[-2,-1,0]` and the same MT2345 biome-info field independently established by
Molang controls, with native float32 sampling. The public frozen-modifier
formula is unchanged; no threshold/salt was fitted to these columns.
The real `local_temperature` consumer regression fails at native ice column
`(1029,62,1056)` before the correction and passes every reference case after.
All cases are biome 46 at Y=62 across eight chunks. This validates the
warm-water/ice predicate, not iceberg geometry, snow placement, altitude
effects or another seed. Other surface fields remain explicitly unmeasured.

#### Iceberg geometry comparison

A separate WD2 experiment used the public Java `iceberg_surface`/`iceberg_pillar`/
`iceberg_pillar_roof` descriptors directly (first octaves -6/-6/-3 and
amplitude counts 3/4/1) with the existing named-noise seeding and builder formula.
The identical integrated CLI workload produced these displayed measurements:

| Geometry candidate | Terrain exact | Surface-height exact | Surface block | Histogram L1 |
|---|---:|---:|---:|---:|
| Retained geometry + measured native climate | 87.0% | 85.2% | 84.2% | 0.072 |
| Direct public Java iceberg descriptors | 85.8% | 83.5% | 82.8% | 0.077 |

The Java-descriptor experiment is rejected and its runtime changes removed.
The retained geometry is still unmeasured, not certified by winning this
comparison. Native iceberg field seeding/geometry and snow support require
isolated controls; no threshold or output golden was changed.


### Controlled worlds

`tools/bds-worldgen-controls/controls.py --out <new outside-repo directory>`
prepares 22 authored marker cases at chunk origins -8, 0 and 8: numeric,
literal-string, random, world/origin variables, uniform endpoints, fractional
rounding and nested scatter/height-query frames. It copies no Mojang assets.
`--run --runtime-helper
<TOOLING bds_runtime.py>` additionally runs a reserved BDS session, saves its
world/logs outside the repository, and requests isolated oak and coal features
with the [documented `/place feature` command](https://learn.microsoft.com/en-us/minecraft/creator/commands/commands/place).
Before `--run`, verify no `bedrock_server` process exists, announce
`BDS taken, ~N min` to the other BDS users, and verify the shared installation's
isolation policy. Restore properties/clean up and announce `BDS free` afterward.

`bds_worldgen_reference --markers` exports marker coordinates;
`--features` exports non-background blocks from the authored oak/coal control
height band; `--geology` exports base-layer counts and bedrock topology.
All modes snapshot `level.dat`/`db` before opening LevelDB, since even a
read-only caller can otherwise trigger log compaction. Stop BDS first.

`controls.py --coal-repeats 32` additionally resets an isolated stone volume
between repeated native coal placements and saves named structure records;
the manifest distinguishes successful console saves from verified exports and
does not invent a feature RNG seed. Scoreboard objective add/remove responses
bracket each command: `/say` does not provide a usable headless-console barrier.

`tools/bds-worldgen-controls/structure_controls.py` plans bounded natural
`locate structure` queries and saved volume tiles in disposable reference worlds.
It records full commands, responses, dimension and source origins; a successful
locate/save alone is not proof that a complete template was persisted.

`cargo run -p unastar --example bds_structure_reference -- list --stopped <world>`
lists persisted structure keys in a stopped-world snapshot.
`export --stopped --controls <manifest.json> <world> <outside-output.json> <names...>`
point-reads the selected records with checksums and exports complete typed NBT
plus authoritative raw little-endian bytes. Output under the repository or
source world is rejected. Format 1 has two List<Int> layers; observed native
format 2 has one or two IntArray layers. Missing layers are not invented, and
format-2 actor coordinates must match the absolute cell position. Corruption
fails the export rather than publishing a partial volume.

### Native ore coordinate controls

The local `minecraft:coal_ore_feature` has `count = 17`. Verified stopped-world
structure volumes contain the entire ore set inside a reset 32³ stone box,
with feature input Y=204, Z=8. The accepted captures are:

| Input X | Verified trials | Ore blocks per trial | X offsets | Z offsets |
|---:|---:|---:|---:|---:|
| -128 | 3 | 20–24 | 5–10 | 6–9 |
| 0 | 3 | 18–26 | 5–10 | 6–10 |
| 128 | 3 | 19–27 | 5–10 | 5–9 |
| 512 | 32 | 16–28 | 5–10 | 5–10 |

These are measured positions, not invented feature seeds. The old runtime's
horizontal support was disjoint from every observed voxel regardless of seed.
`bedrock::features::ore::place` now applies the internal legacy (+8,+8)
translation, preserving the feature input/return frame and existing RNG calls.
The signed-origin regression fails before that correction and passes after it.
Exact native RNG replay and the full sphere/discard algorithm are not established.

Full raw volumes and console manifests stay outside the repository under
`worldgen-controls/run-20261025-coal32-v2` and
`worldgen-controls/run-20261025-ore-count-origin-02`. Only the nine successfully
saved and independently exported native-count captures from the latter run
contribute evidence; its unregistered authored-count attempts do not.

Five further complete volumes from directly referenced authored ore features
vary only `count`, keeping the coal/stone replacement rule and input Y/Z:

| Count | Input X | Verified trials | Ore blocks | X offsets | Z offsets |
|---:|---|---:|---:|---:|---:|
| 4 | -128, 0 | 2 | 3 each | 7–8 | 7–8 |
| 4 | 128 | 1 | 0 | — | — |
| 32 | -128 | 1 | 89 | 3–12 | 5–9 |
| 32 | 0 | 1 | 99 | 3–11 | 6–9 |

These support the same translated horizontal frame and establish that `count`
is not a block budget. The zero-block count-4 trial returned the native
`Feature could not be placed at this location` outcome; its saved volume is
still complete. Raw manifests/volumes are outside the repo in
`worldgen-controls/run-20261025-ore-count-origin-05`. Its last planned count-32
trial was not captured: the strict repeated-fill guard stopped on native
`0 blocks filled`. The earlier rule-placement attempt in `...-04` supplies
no ore geometry evidence. These few additional shapes do not establish exact
sphere/discard behavior or RNG replay.

### Native Molang noise controls

`bds_1_26_51_molang_noise.json` contains authored measurements, not vanilla pack
or structure payloads:

| Control | Independently checked world seeds | Input pairs | Measurement |
|---|---|---:|---|
| Marker height `round(192 + 64 * query.noise(x,z))` | 12345 and 54321 | 16 per world | Identical 1/64-wide noise bins |
| Hexadecimal scalar carrier with sign/adaptive scale | -9876543210123 | 16 new pairs | Exact binary32 patterns; four independent literal calibrations passed |

The matching model is MT19937(2345), three constructor offset draws and a
forward modulo shuffle. Native float32 sampling uses left-associated
`t*t*t*t*dot`; regrouping as Java's `(t*t)*(t*t)*dot` or sampling in f64 changes
observed bits. `bedrock::molang_noise` const-evaluates the permutation and final
hash gradients, with no runtime RNG initialization, world-seed mix or per-world
table copy. The obsolete seed-variation assertion was removed, not repinned.
The real decorator regression failed at Y=217 versus native Y=207 before the
fix and now passes the scalar and exact-float controls.

Source manifests, authored packs, console logs and complete saved volumes stay
outside the repo in `worldgen-controls/run-20261026-noise-01` and `...-05`.
The precision-band run `...-02` independently disproved f64 sampling at large
inputs. `...-03` failed literal calibration; `...-04` exposed a tiny decimal
literal ambiguity. Neither supplies the certified exact-float fixture: `...-05`
uses an exact integer-ratio small calibration and validates every digit channel.
These controls do not establish decoration population seeds or draw order.

### Primitive-correction parity measurements (2026-10-26)

The same 68-chunk, 17,408-column seed-12345 fixture and `bedrock-worldgen`
feature flag were used throughout. The first four columns isolate the ore/noise
corrections; the last adds the measured fixed frozen-climate fields. Every
corrected implementation is restored. Values are rounded for display.

| Metric | Old ore / old noise | Corrected ore / old noise | Old ore / corrected noise | Ore + noise | + native frozen climate |
|---|---:|---:|---:|---:|---:|
| terrain exact | 0.835420 | 0.835593 | 0.833812 | 0.833869 | 0.870290 |
| terrain within 2 | 0.881951 | 0.882468 | 0.882813 | 0.881549 | 0.921530 |
| terrain mean absolute error | 1.935777 | 1.935087 | 1.929458 | 1.941234 | 1.249253 |
| surface exact | 0.857479 | 0.851333 | 0.857479 | 0.851448 | 0.851562 |
| surface block | 0.805779 | 0.801528 | 0.805779 | 0.801643 | 0.842429 |
| surface biome | 0.995002 | 0.994830 | 0.995002 | 0.994830 | 0.994830 |
| histogram L1 | 0.072667 | 0.072304 | 0.072387 | 0.072264 | 0.071887 |

The old/old row is the earlier offset-only ablation, which exactly recovered
every full-precision prior metric before the fixed-noise cutover.
Independent primitive controls disprove both old implementations; aggregate
surface/terrain metrics nevertheless contain compensating errors.

### D8 decision (2026-10-28)

The coordinator delegated D8. The ore-only column above shows the native-
calibrated ore frame alone lowers surface height exact (0.857479 to 0.851333)
and surface biome match (0.995002 to 0.994830); the frozen-climate fix leaves
both unchanged. Both regressions therefore come from a correction proven
against isolated native coal controls, so exactly those two floors were
re-baselined to the measured values (0.8515 and 0.9948). Every other floor of
`bedrock_style_vs_bds_reference_ratchets` was raised to the current
measurement, and the test is back in the gate.

### Kelp and seagrass sea-floor anchoring (2026-10-28)

Every vanilla rule feeding `kelp_feature` and the seagrass built-ins passes
`query.heightmap` (the first air above the water) as the input height, and the
nested seagrass scatters add a second heightmap on top. The built-ins used that
input directly, found air, and placed nothing in any ocean. They now start
above the column's highest motion-blocking block, as BDS evidently does (all
fixture kelp and seagrass is rooted on the floor). Plant support also uses
motion blocking, because the palette `solid` flag marks seagrass itself.

| Block (68-chunk fixture) | BDS | Before | After |
|---|---:|---:|---:|
| kelp | 4506 | 0 | 3272 |
| seagrass | 2584 | 0 | 2529 |
| water | 264317 | 255644 | 249843 |

Water falls because plants replace the primary water block; its remaining
deficit predates this change. Full-precision metrics after the change: terrain
exact 0.872989, within two 0.922564, error 1.236271, surface exact 0.851563,
surface block 0.842429, surface biome 0.994830, histogram L1 0.072594. Kelp
density stays about 27% low; feature RNG seeding remains unmeasured.

### Per-position rule biome checks (2026-10-28)

Rules used to run once per chunk when any of the chunk's 3D biomes matched
their filter, and every distributed placement then ran regardless of its own
biome. One pocket of lush caves therefore decorated the whole chunk volume:
in generated chunk (6,-4) lush-cave clay ore filled mountain stone up to
y 145. Each placement is now checked against the biome at its own position
(the chunk-level check remains a cheap prefilter). Every aggregate metric
improved, and the surface-biome floor returns to its pre-D8 value of 0.9950:

| Metric | Chunk-wide check | Per-position check |
|---|---:|---:|
| terrain exact | 0.872989 | 0.874138 |
| terrain within 2 | 0.922564 | 0.924173 |
| terrain mean absolute error | 1.236271 | 1.227194 |
| surface exact | 0.851563 | 0.852654 |
| surface block | 0.842429 | 0.843290 |
| surface biome | 0.994830 | 0.995060 |
| histogram L1 | 0.072594 | 0.068492 |

| Block (68-chunk fixture) | BDS | Chunk-wide | Per-position |
|---|---:|---:|---:|
| clay | 26162 | 48455 | 42540 |
| calcite | 538 | 1480 | under 914 |
| andesite | 54169 | 52163 | 58233 |
| granite | 47344 | 42951 | 46185 |

Clay, moss (23132 vs 14031), cave vines (5335 vs 2217) and big dripleaf
(1067 vs 190) are still over-generated; those remain open.

### Single-block deep sea floor (2026-10-28)

The overworld builder covered every deep ocean floor (below
`sea_level - sea_floor_depth - depth`) with a `depth`-thick layer of
`sea_floor_material`. The classic builder's deep-water branch places one
`sea_floor_material` block with foundation stone below, and the BDS fixture
agrees: ocean chunk (64,66) holds 495 gravel in total, most of it ore blobs,
where we generated 1389. The builder now places a single block.

| Metric | Gravel layer | Single block |
|---|---:|---:|
| terrain exact | 0.874138 | 0.874311 |
| terrain within 2 | 0.924173 | 0.922794 |
| terrain mean absolute error | 1.227194 | 1.232709 |
| surface exact | 0.852654 | 0.852654 |
| surface block | 0.843290 | 0.843290 |
| surface biome | 0.995060 | 0.995060 |
| histogram L1 | 0.068492 | 0.040459 |

| Block (68-chunk fixture) | BDS | Gravel layer | Single block |
|---|---:|---:|---:|
| gravel | 38197 | 73185 | 37412 |
| stone | 508985 | 478106 | 504814 |
| andesite | 54169 | 58233 | 61246 |

The coordinator approved re-baselining exactly `terrain_within_2` (0.9227)
and `terrain_mean_abs_error` (1.2328) to the measured values; every other
floor stays or rises (histogram L1 tightened to 0.0405).

**Second cause, still open.** Exactly 24 columns moved beyond two blocks, and
all 24 share one shape: an `underwater_magma_feature` blob replaced the single
gravel block (often the block below as well), exposing andesite, diorite or
coal ore, which the parity metric does not count as terrain, so measured
height drops to the first stone below. BDS reports a terrain block at the
magma's height in every one of those columns, so our underwater magma
(`partially_exposed_blob_feature`, radius 1 around the floor) is placed where
BDS keeps the floor; its position, replace set or density is unverified. The
old gravel layer had hidden this. The broader andesite/diorite excess is the
ore blobs now reaching stone directly under the floor.

### Open-space snap to surface (2026-10-28)

`snap_to_surface_feature` searched up to `vertical_search_range` blocks
through solid rock, first towards the surface and then away from it. Inputs
embedded in rock (most of the uniform -64..256 lush-cave placements, and the
underwater-magma inputs, which are at least two blocks below the surface)
therefore snapped onto any cave or ocean floor within reach. The walk now
starts at the input, only moves through open blocks (air, plus water where
allowed) towards the surface, and places nothing from inside rock, like the
equivalent Java environment scan. This is inferred from the fixture
histogram, not from a controlled native capture.

| Metric | Through-rock search | Open-space walk |
|---|---:|---:|
| terrain exact | 0.874311 | 0.878217 |
| terrain within 2 | 0.922794 | 0.924058 |
| terrain mean absolute error | 1.232709 | 1.221680 |
| histogram L1 | 0.040459 | 0.027691 |

| Block (68-chunk fixture) | BDS | Through-rock | Open-space |
|---|---:|---:|---:|
| moss_block | 14031 | 23124 | 15879 |
| cave_vines | 2217 | 5343 | 2547 |
| clay | 26162 | 42616 | 31591 |
| big_dripleaf | 190 | 1068 | 439 |
| moss_carpet | 924 | 1039 | 767 |

Still open after this change: magma 393 vs 142, big dripleaf 439 vs 190,
small dripleaf 80 vs 20, clay 31591 vs 26162, and the geode blocks (calcite
1097 vs 538, smooth basalt 1310 vs 721, amethyst 888 vs 366, budding
amethyst 106 vs 27), about twice BDS each. Surface metrics did not move.

### Kelp height measured from a natural BDS world (2026-10-28)

A fresh natural BDS 1.26.51 world of seed 12345 (ticking area over chunks
-5..4, no edits; preserved outside the repository with
`bds_worldgen_reference --aquatic` exports) reproduces the fixture's kelp
count in every exported chunk exactly. Across 39 chunks it has 407 kelp
columns of 3..25 blocks (mean 10.26), never shorter than 3 and never taller
than the water above the floor allows. Normalized by that room, lengths are
uniform: the model "uniform in `3..=min(25, room)`" predicts a mean of 10.10.
Our columns matched in number (414) but used the legacy `2..=11` heights
(mean 6.9, 2866 blocks against native 4175). With the measured rule the same
chunks hold 4065 blocks in the same 3..25 shape.

| Block (68-chunk fixture) | BDS | Legacy heights | Measured heights |
|---|---:|---:|---:|
| kelp | 4506 | 3152 | 4462 |
| water | 264317 | 249074 | 247764 |

Histogram L1 is unchanged to full precision: every added kelp block replaces
a water block, and the generated water deficit predates this change. Fixture
chunk (-4,-4) stays low (325 vs 517) because half of its native kelp grows in
a trench whose generated depth differs; that is terrain, not kelp.

### Feature rules have independent, identifier-seeded randoms (2026-10-28)

Two natural BDS 1.26.51 worlds of seed 12345 (chunks -5..4, no edits other
than an authored pack) each ran single-marker probe rules: `iterations` 1,
`x`/`z` uniform over the chunk, constant `y` in the air, one distinct marker
block per rule. All three probes placed exactly one marker in every exported
chunk.

| World | `first_pass` probes | `final_pass` probe |
|---|---|---|
| 1 | `wg_seed:a_probe`, `wg_seed:b_probe` (identical definitions) | `wg_seed:c_probe` |
| 2 | `wg_seed:x_probe` (the `a_probe` definition renamed), no `b_probe` | `wg_seed:c_probe` |

- `a_probe` and `b_probe` never shared a position (0 of 69 chunks), so two
  identical rules do not replay the same draws.
- `c_probe` kept its exact position in all 61 chunks common to both worlds,
  although world 2 removed one `first_pass` rule and renamed the other. A
  rule's draws therefore do not depend on how many draws other rules make.
- `x_probe` matched neither `a_probe` nor `b_probe` in any chunk (0 of 61),
  although its definition and its place in identifier order were those of
  `a_probe`. The seed depends on the rule identifier, not on a rule index.

A third world ran 50-iteration probes (`x`, `z`, `y` uniform over 16,
`coordinate_eval_order` `xzy`). For each such volume exactly one 32-bit
MT19937 seed reproduces all 50 positions, with plain `% 16` draws in `x`, `z`,
`y` order from the first output. Seeds recovered for `c_probe` in four chunks
and `d_probe` in one fix the formula the runtime now implements:

- population seed `s = level_seed ^ (chunk_x * px + chunk_z * pz)` (wrapping,
  chunk units), where `px` and `pz` are the first two non-negative 31-bit
  draws (`next_u32() >> 1`) of a random seeded with the level seed, each
  forced odd. The previous decorator used the full 32-bit draws.
- rule seed `((s << 6) + h + (s >> 2) + 0x9e3779b9) ^ s`, where `h` is the low
  32 bits of the 64-bit FNV-1 hash (multiply, then xor) of the rule
  identifier. `rules.rs` folds `h` at build time.
- the scatter starts a fresh MT19937 from the rule seed; the placed feature
  starts its own fresh MT19937 from the same seed.

These predict every probe position in all three worlds (69, 69 and 64
chunks; `first_pass` and `final_pass`; one and 50 iterations).
`decorate::tests::feature_rule_streams_match_native_probe_worlds` pins the
five recovered seeds and seven single-iteration positions. Carvers seed their
own per-source randoms from the world seed, so carving output does not move.
**Unmeasured:** how seeds wider than 32 bits fold into the level seed, and
the geology and structure streams that are still salted from the population
seed.

### Per-rule seeding re-baseline (2026-10-28)

With every rule on its own stream, chance-gated rules land in BDS's chunks:
the fixture's two geode chunks (-1,1) and (-4,3) had no generated geode
before and both have one now. Totals move towards BDS where the per-chunk
placement decides the count:

| Block (68-chunk fixture) | BDS | Shared stream | Per-rule streams |
|---|---:|---:|---:|
| clay | 26162 | 31591 | 27958 |
| kelp | 4506 | 4462 | 4764 |
| big dripleaf | 190 | 439 | 267 |
| magma | 142 | 393 | 367 |
| amethyst block | 366 | 888 | 1100 |

| Parity metric | Before | After |
|---|---:|---:|
| Terrain height exact | 0.8782 | 0.8791 |
| Terrain height within 2 | 0.9240 | 0.9224 |
| Terrain mean abs error | 1.2217 | 1.2287 |
| Surface height exact | 0.8526 | 0.8510 |
| Surface block match | 0.8432 | 0.8421 |
| Surface biome match | 0.9950 | 0.9947 |
| Histogram L1 | 0.0277 | 0.0203 |

The coordinator approved re-baselining the five lowered floors to these
measured values (D8): vegetation and ores now draw from the correct streams,
and what still differs is the feature algorithms themselves. Each generated
geode is about three times BDS's size (amethyst 477 vs 141 in chunk (-1,1))
and spills into neighbouring chunks; tree internals are the next suspect for
the surface metrics. **Unmeasured:** Molang `math.random` inside rule
distributions still draws from the rule's scatter stream.


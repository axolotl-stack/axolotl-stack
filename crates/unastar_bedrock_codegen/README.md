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
| Scatter coordinate frames and distribution endpoints | BDS marker fixture confirms numeric/Molang offsets and upper-exclusive uniform extents; other distributions remain unmeasured | Fractional rounding, height-query frames, Gaussian and grid controls |
| Molang noise and query height semantics | unmeasured: to confirm by BDS fixture | Marker heights above flat ground and fluids, noise threshold controls |
| JSON ore/tree/plant/geode/sculk/blob/decoration feature placement | unmeasured: to confirm by BDS fixture | One isolated feature at a known origin in controlled material |
| Built-in aquatic/plants/terrain features and legacy selectors | unmeasured: to confirm by BDS fixture | One built-in identifier per control, fixed seed and biome |
| Surface builders, noise seeds, mountain slopes and adjustment precedence | unmeasured: to confirm by BDS fixture | Matched columns for each builder with captured biome/material inputs |
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
| Identity | `blocks`, `nbt` | canonical palette (`canonical_block_states.nbt`), block tags, physical flags; descriptors resolve to runtime ids; pre-flattening names (`minecraft:log` + `old_log_type`, `minecraft:stone` + `stone_type`, `minecraft:tallgrass`, ...) go through `LEGACY_BLOCKS` / `STATE_RENAMES` |
| IR | `ir`, `model` | strict typed parsing of biomes, feature rules, features, spawn rules (unknown keys fail the build); reachability from overworld rules; built-in registry check |
| Molang | `unastar_rules::molang`, `molang` | Shared lexer/parser; worldgen-only binding/lowering, constant-folding (`variable.is_legacy` = 0) and `f32` Rust emission; no runtime interpreter |
| Emission | `emit` | `run_pass` with per-rule biome masks, one `rule_*` per rule, one `f_*` per reachable JSON feature, `BIOMES` surface data, `blocks`/`bb`/`props` constants |

`builtin` lists the built-in features (identifiers without a JSON definition) that the
runtime implements in Rust; a reachable built-in missing from the list fails the build.
`builtin_blocks` declares the blocks hand-written runtime code needs.

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

Runtime `7bbf024` / lint `4aa93c1`, shared core `6d8a964`, local BDS 26.51
inputs. The 68-chunk seed-12345 BDS fixture contains 17,408 columns.
`cargo test -p unastar --features bedrock-worldgen --test bedrock_worldgen`
ratchets these measurements; it also checks request-order independence and
the observed foundation-layer boundaries.

| Parity metric | Measured value |
|---|---:|
| Terrain height exact | 84.3118% |
| Terrain height within 2 blocks | 88.2410% |
| Terrain mean absolute error | 1.9171 blocks |
| Surface height exact | 85.3228% |
| Surface block match | 80.2332% |
| Surface biome match | 99.5060% |
| Histogram L1 | 0.074099 |

Benchmark: `worldgen_bench --kind java|bedrock`, same feature-enabled binary,
68 fixture chunks, three rounds alternating Java then Bedrock, Windows affinity
mask `4` (one P-core logical CPU), power-plan maximum processor state 99%
(turbo disabled). No other benchmark suite ran in this worker. Other workers
were active; the debug ranges show substantial drift, so these are baselines,
not optimization speedup claims. Finished costs include the cold pipeline's
neighbor ring: Bedrock made 264 protos for 68 requested chunks. At this core
revision the Java row has no decoration, so it is not an equivalent-completeness
comparison.

| Profile | Generator | Proto median (range), ms/chunk | Finished median (range), ms/chunk | Allocations/finished chunk | KiB allocated/finished chunk |
|---|---|---:|---:|---:|---:|
| Debug | Java | 102.72 (84.03–109.38) | 100.16 (83.89–110.89) | 107 | 595 |
| Debug | Bedrock | 114.17 (78.32–172.67) | 615.97 (413.37–695.70) | 2032 | 3000 |
| Release | Java | 9.37 (9.19–11.02) | 9.21 (8.86–11.94) | 107 | 595 |
| Release | Bedrock | 8.23 (7.80–9.96) | 40.46 (39.71–43.11) | 2032 | 3000 |

Release built with
`cargo build --release -p unastar --features bedrock-worldgen --bin unastar --example worldgen_bench`.
The server's `kind = "bedrock"` configuration reached `Server ready` in an
isolated loopback-only startup smoke; no real-client visual check was performed
by this worker.

### Surface-builder comparison boundary (WD2)

The fixture covers `overworld` and `frozen_ocean` builders (ocean, jungle edge,
roofed forest and two lush-cave surface columns). It does not validate mesa,
swamp, capped, end or noise-gradient builders.

For the 1,658 frozen-ocean columns whose reference surface is water or ice,
the current BDS temperature-patch model and the shared Java
`biome_climate::cold_enough_to_snow` predicate were sampled at identical
coordinates:

| Candidate freezing predicate | Correct water/ice classifications | Accuracy |
|---|---:|---:|
| Current Bedrock surface model | 672 | 40.53% |
| Shared Java frozen temperature modifier | 575 | 34.68% |

Neither predicate is established BDS behavior. Keep the BDS-data-backed builder
path rather than adopt Java's predicate on this evidence; its warm-water patch
noise and the final ice/snow placement still need controlled BDS measurements.

### Controlled worlds

`tools/bds-worldgen-controls/controls.py --out <new outside-repo directory>`
prepares ten authored marker cases at chunk origins -8, 0 and 8: numeric,
literal-string, random, world/origin variables, uniform endpoints and nested
scatter frames. It copies no Mojang assets. `--run --runtime-helper
<TOOLING bds_runtime.py>` additionally runs a reserved BDS session, saves its
world/logs outside the repository, and requests isolated oak and coal features
with the [documented `/place feature` command](https://learn.microsoft.com/en-us/minecraft/creator/commands/commands/place).
Acquire the coordinator's BDS slot and verify its loopback/isolation policy
before using `--run`.

`bds_worldgen_reference --markers` exports marker coordinates;
`--features` exports non-background blocks from the authored oak/coal control
height band; `--geology` exports base-layer counts and bedrock topology.
All modes snapshot `level.dat`/`db` before opening LevelDB, since even a
read-only caller can otherwise trigger log compaction. Stop BDS first.

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
| Molang noise and query height semantics | Stone/water controls confirm world-space heights: heightmap includes water, above-top-solid skips water; noise and other material classes remain unmeasured | Noise threshold controls and foliage/material height queries |
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

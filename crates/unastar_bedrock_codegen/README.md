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
| Scatter numeric/distribution offsets versus Molang positions, inclusive extents, Gaussian sampling | unmeasured: to confirm by BDS fixture | Single-block rules with constant, expression and distribution coordinates |
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

# unastar_bedrock_codegen

Build-time toolchain of unastar's **Bedrock-style** world generator (`kind = "bedrock"`,
BDS 1.26.51 parity). It reads the vanilla behaviour packs of a local BDS install and
compiles feature rules, features and biome surface parameters into straight-line Rust
that `crates/unastar` includes (`world::generator::bedrock::generated`).

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
| Ingestion | `brarchive`, `json`, `packs` | `.brarchive` containers (format in the module docs), JSON with comments, version stack `definitions` < `vanilla` < `vanilla_<version>` ascending up to 1.26.51; the last layer defining an identifier wins |
| Identity | `blocks`, `nbt` | canonical palette (`canonical_block_states.nbt`), block tags, physical flags; descriptors resolve to runtime ids; pre-flattening names (`minecraft:log` + `old_log_type`, `minecraft:stone` + `stone_type`, `minecraft:tallgrass`, ...) go through `LEGACY_BLOCKS` / `STATE_RENAMES` |
| IR | `ir`, `model` | strict typed parsing of biomes, feature rules, features, spawn rules (unknown keys fail the build); reachability from overworld rules; built-in registry check |
| Molang | `molang` | the subset used by worldgen data, constant-folded (`variable.is_legacy` = 0) and emitted as `f32` Rust; no runtime interpreter |
| Emission | `emit` | `run_pass` with per-rule biome masks, one `rule_*` per rule, one `f_*` per reachable JSON feature, `BIOMES` surface data, `blocks`/`bb`/`props` constants |

`builtin` lists the built-in features (identifiers without a JSON definition) that the
runtime implements in Rust; a reachable built-in missing from the list fails the build.
`builtin_blocks` declares the blocks hand-written runtime code needs.

## Tests

`cargo test -p unastar_bedrock_codegen`. Unit tests use synthetic archives and palettes.
`tests/bds_install.rs` checks the real install (counts, every reference resolves) and
skips with a message when `UNASTAR_BDS_DIR` is not set.

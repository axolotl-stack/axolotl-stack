# Java Edition data for the Java-style generator

`worldgen_data/` holds the pinned Java version's `data/minecraft/worldgen` tree unchanged.
This directory holds the other Java facts the AOT toolchain (`build.rs`) compiles in.

## Pinned versions

| Input | Version | Why |
|---|---|---|
| Java Edition | **1.21.11** (server.jar sha1 `64bb6d763bed0a9f1d632ec347938594144943ed`) | Its data generator output is byte-identical to `worldgen_data/` (`diff -rq` is empty), and it runs on JDK 21. The newest release (26.3) needs JDK 25 and would move data and reference worlds at once; upgrade both together in one slice. |
| Bedrock palette | 1.26.51 (`protocolgen` native `canonical_block_states.nbt`) | The ordinals are unastar's runtime block ids. |
| Java→Bedrock mapping | GeyserMC/mappings `81700ab543` (MIT, see `LICENSE-GEYSER-MAPPINGS`) | Generated for Java 26.3 / Bedrock 1.26.50; earlier commits predate Bedrock 1.26 state properties (e.g. stairs `minecraft:corner`). Its lists are indexed by Java 26.3 state ids, so the import keys them through the 26.3 block report. |

## Files

| File | Content |
|---|---|
| `blocks.json` | Every Java block: first state id, default state id, properties in state-id order (the cartesian product, last property fastest, regenerates every id). |
| `bedrock_blocks.json` | Java state id → Bedrock canonical ordinal (runtime id). Every Java state maps. |
| `collisions.json` | Java collision boxes per state (`[min_x, min_y, min_z, max_x, max_y, max_z]`), used for heightmap classification. |
| `bedrock_biomes.json` | Java biome → Bedrock numeric biome id. |
| `biome_parameters_overworld.json` | The data generator's overworld multi-noise parameter points (`reports/biome_parameters`), floats kept as decimal strings. |
| `tags/worldgen/biome/` | Biome tags from the data generator. |
| `tags/block/` | Block tags from the data generator (feature/structure predicates). |
| `block_kinds.json` | Java block id → block type (`definition.type` of `reports/blocks.json`), used by feature survival rules and structure processors. |
| `biome_parameters_nether.json` | The data generator's nether parameter list report (biome order for the nether's feature sorting). |

## Regenerating

```sh
# outside the repo
java -DbundlerMainClass=net.minecraft.data.Main -jar <1.21.11 server.jar> --reports --server --output <dg-1.21.11>
java -DbundlerMainClass=net.minecraft.data.Main -jar <26.3 server.jar> --reports --output <dg-26.3>   # JDK 25
# GeyserMC/mappings@81700ab543: blocks.nbt, collisions.nbt, biomes.json into <geyser>
python crates/unastar_noise/tools/import_java_data.py \
  --java-datagen <dg-1.21.11> --java-version 1.21.11 \
  --mapping-java-blocks <dg-26.3>/reports/blocks.json --mapping-java-version 26.3 \
  --geyser-dir <geyser> --geyser-commit 81700ab543 \
  --canonical-palette crates/valentine_gen/protocolgen/data/source/inputs/1.26.51/native/canonical_block_states.nbt \
  --bedrock-version 1.26.51 --out crates/unastar_noise/java_data
```

The import fails if any Java state has no Bedrock canonical state.

`block_kinds.json` is the `definition.type` of every entry of `<dg-1.21.11>/reports/blocks.json`
(`{"minecraft:stone": "minecraft:block", ...}`, sorted by id); `biome_parameters_nether.json` is
`<dg-1.21.11>/reports/biome_parameters/minecraft/nether.json` unchanged.

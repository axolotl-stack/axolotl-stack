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
| `block_physics.json` | Actual pinned native queries for every complete canonical state: full/center sturdy faces, connection exceptions, wall coverage, motion/solid/render/collision/replaceability flags and full upper collision face. Includes official asset pins and checksums of all four byte arrays. |
| `bedrock_biomes.json` | Java biome → Bedrock numeric biome id. |
| `biome_parameters_overworld.json` | The data generator's overworld multi-noise parameter points (`reports/biome_parameters`), floats kept as decimal strings. |
| `tags/worldgen/biome/` | Biome tags from the data generator. |
| `tags/block/` | Block tags from the data generator (feature/structure predicates). |
| `block_kinds.json` | Java block id → block type (`definition.type` of `reports/blocks.json`), used by feature survival rules and structure processors. |
| `biome_parameters_nether.json` | The data generator's nether parameter list report (biome order for the nether's feature sorting). |
| `dimension_type/` | Unchanged `data/minecraft/dimension_type/{overworld,the_nether,the_end}.json` from the pinned 1.21.11 version jar (SHA256 `ec47239a8de246335e1d54f6ac319bd35641778eb4b6a6da06372840d02fcebc`). |

## Regenerating

```sh
# outside the repo
java -DbundlerMainClass=net.minecraft.data.Main -jar <1.21.11 server.jar> --reports --server --output <dg-1.21.11>
java -DbundlerMainClass=net.minecraft.data.Main -jar <26.3 server.jar> --reports --output <dg-26.3>   # JDK 25
# GeyserMC/mappings@81700ab543: blocks.nbt and biomes.json into <geyser>
python crates/unastar_noise/tools/import_java_data.py \
  --java-datagen <dg-1.21.11> --java-version 1.21.11 \
  --mapping-java-blocks <dg-26.3>/reports/blocks.json --mapping-java-version 26.3 \
  --geyser-dir <geyser> --geyser-commit 81700ab543 \
  --canonical-palette crates/valentine_gen/protocolgen/data/source/inputs/1.26.51/native/canonical_block_states.nbt \
  --bedrock-version 1.26.51 --out crates/unastar_noise/java_data
```

The import fails if any Java state has no Bedrock canonical state.

### Native block physics

Use JDK 21 and the pinned runtime root containing `server.jar`,
`server-mappings.txt`, the extracted inner jar and its official libraries.
`<CP>` is only the inner jar plus those library jars, joined with the platform's
classpath separator; `<SEP>` is `;` on Windows and `:` elsewhere.

```text
javac -J-XX:ActiveProcessorCount=1 -J-Xmx512m -proc:none -cp "<CP>" -d <classes> crates/unastar_noise/tools/JavaBlockPhysics.java
java -XX:ActiveProcessorCount=1 -Xmx1G -Djava.security.manager=allow -cp "<classes><SEP><CP>" nativephysics.JavaBlockPhysics <runtime-root> <capture.json> crates/unastar_noise/tools/JavaBlockPhysics.java
python crates/unastar_noise/tools/import_native_physics.py --capture <capture.json> --out crates/unastar_noise/java_data/block_physics.json
```

The standalone exporter verifies all three official asset hashes, binds vanilla
tags before querying states, and forbids network/listener access. Tag binding is
required for native connection exceptions such as shulker boxes. It does not
construct a world or server. Its actual isolated-classpath replay reproduced all
29,671 canonical identities and all four observed bytes per state.

The build parser checks version, count, every state id/block/property identity
and all four array checksums before emission. Feature flags and terrain heightmap
classes reuse the same validated observations; collision boxes, suffix guesses
and reverse Bedrock mappings are not substitutes for these native queries.


`block_kinds.json` is the `definition.type` of every entry of `<dg-1.21.11>/reports/blocks.json`
(`{"minecraft:stone": "minecraft:block", ...}`, sorted by id); `biome_parameters_nether.json` is
`<dg-1.21.11>/reports/biome_parameters/minecraft/nether.json` unchanged.

`dimension_type/` is extracted from `versions/1.21.11/server-1.21.11.jar` inside the
server bundle. `worldgen_data/world_preset/normal.json` links each noise preset to
its dimension type at build time; generated `{overworld,nether,end}_settings`
modules expose `WORLD_MIN_Y`/`WORLD_HEIGHT` for legal build/storage bounds.
Their existing `MIN_Y`/`HEIGHT` describe only noise fill: Nether and End generate
128 noise blocks but store 256 blocks starting at y=0. Nether's
`dimension_type.logical_height` (128) is a separate portal/gameplay limit, not its
storage height. No dimension JSON is loaded by the runtime.

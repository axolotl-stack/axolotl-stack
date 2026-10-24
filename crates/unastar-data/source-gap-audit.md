# Gameplay Data Source Gap Audit

## Source trust model

Protocolgen's pinned native BDS 1.26.51 measurements own block palette order,
physical properties, tags, item IDs/stack limits, and biome IDs/definitions.
Vanilla behavior packs supply exposed gameplay components absent from native
measurements. Packet registries/captures own packet-only metadata. PMMP currently
supplies creative inventory only; Prismarine remains a cross-check, not gameplay truth.
Every consumed native file and packet registry projection is hashed separately in
`output/manifest.kdl`. See [README.md](README.md) for pins and generation commands.

## Current evidence and remaining gaps

| Domain | Native coverage | Remaining gap / policy |
| --- | --- | --- |
| Blocks | 1,477 identifiers, 22,091 typed states in canonical NBT order, sequential IDs and measured network hashes | Default/placement state is not measured; retain explicitly weak first-canonical-state policy, not a BDS-default claim |
| Block physics | Per-state destroy speed, explosion resistance, opacity and light; complete per-block material/property table and tags | Generated registry fields project first-state measurements; state-dependent values remain in KDL; dynamic collision/mining algorithms are not inferred |
| Block identity | Canonical ordinal and namespaced identifier | Removed unsourced legacy numeric IDs and translated names; registry ID is first canonical state ordinal |
| Items | 2,076 native IDs, stack limits, properties and tags | Native dump lacks packet component/version metadata; use separately hashed protocolgen registry NBT, checking IDs against native table |
| Biomes | 89 native IDs and full native definitions | Behavior-pack components/tags retain separate attribution; native-only biomes have no fabricated gameplay components |
| Creative inventory | PMMP groups, ordering, damage and block-state variants | Older community capture, not native 1.26.51 coverage; keep explicit provenance |
| Entities | Vanilla behavior-pack components plus local overrides | Native actor internals not imported |
| Actor identifier / biome packets | Validated BDS-capture ingestion lane available | Real capture artifacts are optional and must never be replaced by synthetic fixtures |
| Recipes / loot | Not normalized | Source recipes/loot from packs and packet formats from verified traces |

`jolyne` advertises `block_network_ids_are_hashes=false`: runtime chunks use
sequential native ordinals. `blocks.kdl` retains measured hashes and regression
coverage to prevent accidentally substituting hash order or an old palette.
`minecraft:wooden_door` is the native Bedrock oak door identifier, not `minecraft:oak_door`.

## Native analysis lane

1. Identify a concrete missing field before adding an extractor.
2. Extract factual tables offline; never couple runtime code to native offsets.
3. Normalize through `unastar-data` with source/version/hash/confidence metadata.
4. Fail on missing required identities, measurements or joins; do not invent fallbacks.
5. Do not commit proprietary binaries or code derived from them.

## Live BDS packet capture

Capture with `bds-extractor`, normalize with
`unastar-data-gen --bds-only --bds-extraction <capture.json>`, then generate with
`unastar-data-codegen --bds-packets-only`. Commit only real validated captures.
Synthetic fixtures test ingestion, never become runtime facts.

## References

- Protocolgen: `../valentine_gen/protocolgen/data/README.md`, `data/NOTICE.md`,
  `data/source/lock.json`, and `docs/outputs.md`.
- Native upstream: <https://github.com/didntpot/bedrock-data>
- PMMP provenance: <https://github.com/pmmp/BedrockData>
- Biome components: <https://bedrock.dev/docs/1.21.0.0/1.21.90.25/Biomes>

# Gameplay Data Source Gap Audit

This audit keeps `unastar-data` from promoting weak derived data into runtime
truth. It records which source should own each gameplay fact before a golden
artifact or generated Rust table is added.

## Source trust model

1. **Vanilla behavior/resource pack JSON**: authoritative for exposed
   data-driven definitions such as biome components, tags, surfaces, entity
   components, spawn rules, recipes, and loot tables when present.
2. **BDS/live protocol extraction**: authoritative for packet dictionaries and
   runtime data the server sends to clients, such as item registries, creative
   inventory, available actor identifiers, and basic biome definition packets.
3. **BDS mod or native extractor output**: authoritative for internal runtime
   tables not fully exposed by packs or protocol traces, such as block
   hardness, opacity, exact material traits, native tags, collision metadata,
   biome legacy IDs, and generator-only internals.
4. **Public community dumps**: acceptable bootstrap inputs only when their
   upstream source is known and recorded. PMMP/BedrockData is useful because it
   labels many files as vanilla packet traces, BDS-mod output, or binary
   analysis output.
5. **Valentine/Prismarine generated data**: protocol fallback and cross-check
   only for gameplay semantics. Do not create canonical `biomes.kdl`,
   behavior, or physics facts from Valentine biome data alone.
6. **Local overrides**: narrow corrections with source, version, confidence,
   and reason metadata.

## Current local evidence

- The repo has 87 vanilla behavior-pack biome definitions at
  `data/vanilla_bp/behavior_pack/biomes/*.biome.json`.
- The checked-in PMMP subset currently includes required item and creative
  inventory data, but does **not** include `biome_definitions.json`,
  `biome_id_map.json`, `block_properties_table.json`, or `item_tags.json`.
- `bds-extractor` builds and validates fixture JSON, so it is the right local
  entry point for packet/lifecycle extraction before adding a native extractor.

## Domain gap table

| Domain | Field class | Preferred source | Current status | Next action |
| --- | --- | --- | --- | --- |
| Biomes | Identifier, exposed components, tags, surface materials, climate component, generation-rule JSON | Vanilla behavior-pack biome JSON | Present locally | Parse into `biomes.kdl` before using Valentine fallback |
| Biomes | Legacy numeric IDs for chunk serialization | BDS mod / PMMP `biome_id_map.json`, cross-checked with BDS extraction | Missing locally | Import or generate from BDS-mod/native extractor output |
| Biomes | Packet payload shape for client biome definition lists | BDS live packet extraction / PMMP `biome_definitions.json` | Missing locally | Extend fixture extraction before runtime packet generation |
| Biomes | Client-side chunk generation internals not exposed in packets | Native client/BDS analysis output normalized through `unastar-data` | Missing | Defer until gap list proves pack JSON is insufficient |
| Blocks | String IDs, canonical state ID ranges, basic generated protocol palette | Valentine generated block data | Present | Already normalized in `blocks.kdl`; keep as protocol/canonical palette source |
| Blocks | Hardness, opacity/light, resistance, material traits, item mapping, tags | BDS mod/native extractor; PMMP `block_properties_table.json`, `item_tags.json` where available | Partially bootstrapped from Valentine; PMMP tables missing locally | Add explicit source confidence per field; replace weak fields with BDS-derived facts |
| Blocks | Exact collision/shape and behavior families | Native extractor or verified pack component data when exposed | Missing | Add `unknown`/family placeholders only when logged and source-attributed |
| Items | Network IDs, component-based flags, versions | BDS packet trace / PMMP `required_item_list.json` | Present locally and normalized | Keep generated artifact as authoritative for item registry packets |
| Items | Tags and component NBT semantics | PMMP `item_tags.json`, required-item component NBT, native extractor | Tags missing locally | Import tags before recipe/crafting semantics |
| Entities | Behavior-pack components, component groups, events | Vanilla behavior-pack JSON + local overrides | Present and generated | Continue runtime integration through generic interpreters |
| Entities | Available actor identifiers packet | BDS packet extraction / PMMP `entity_identifiers.nbt` | Extractor fixture mode exists; specific entity identifier source is not normalized | Add extractor fixture and artifact lane |
| Recipes/Loot | Recipe and loot tables | Vanilla packs plus PMMP packet traces for network recipe format | Not normalized | Add artifacts after item tags are sourced |

## Native/internals exploration lane

Native analysis should discover stable table ownership and extractor signatures,
not become the runtime architecture.

1. Start from a source-gap list with exact missing fields.
2. Identify the BDS/client structures that own those fields in IDA/Ghidra.
3. Build an offline extractor or BDS mod that emits factual JSON/KDL.
4. Normalize that output through `unastar-data` IR with source/version/hash
   metadata.
5. Commit extractor code and derived factual artifacts only when legally safe;
   do not commit proprietary binaries or copied decompiled code.
6. Keep Unastar runtime consuming generated tables and behavior kernels, never
   native offsets or decompiler-shaped structs directly.

## Immediate rule for biomes

Do not generate `biomes.kdl` from Valentine/Prismarine data alone. The first
biome artifact should be based on behavior-pack biome JSON, then enriched with
BDS/packet/native facts for legacy IDs and packet definitions.

## References

- PMMP/BedrockData documents which files come from packet traces, BDS mods, or
  binary analysis: <https://github.com/pmmp/BedrockData>
- Bedrock biome JSON schema documents biome components, tags, climate, and
  surface/generation fields: <https://bedrock.dev/docs/1.21.0.0/1.21.90.25/Biomes>

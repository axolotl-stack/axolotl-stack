# unastar-data

Generated, source-attributed gameplay data for Unastar.

The crate is the boundary between raw Bedrock data sources and the server runtime:

1. `datagen` reads Valentine block/protocol data, vanilla behavior-pack JSON, PMMP/BedrockData JSON, and local KDL overrides, then writes reviewable artifacts in `output/`.
2. `codegen` reads those golden artifacts and generates Rust types/tables under `src/`.
3. `unastar` consumes generated APIs; it should not parse PMMP, Prismarine, BDS, or behavior-pack data directly in hot runtime paths.

## Source priority

Gameplay facts should be merged in this order:

1. Valentine/generated protocol data for packet-facing IDs and canonical palette blobs.
2. BDS handshake extraction from `bds-extractor`.
3. Vanilla behavior/resource pack data in `data/vanilla_bp`.
4. PMMP/BedrockData or PMMP BDS-mapping outputs for packet-trace/native facts.
5. PrismarineJS `minecraft-data` where Bedrock coverage is usable.
6. Local KDL overrides with version/source notes.
7. Optional native extractor output, normalized through this crate before runtime use.

See [`source-gap-audit.md`](source-gap-audit.md) before adding new artifact
families. In particular, do not derive canonical biome gameplay data from
Valentine/Prismarine biome tables alone; use vanilla behavior-pack biome JSON
first, then enrich with BDS packet/native facts.

## Manifest

`output/manifest.kdl` records source paths, commits/hashes, confidence, and artifact hashes.
The currently vendored PMMP/BedrockData JSON inputs live under `data/upstream/pmmp/`
and are exposed through `unastar_data::pmmp` constants for legacy Unastar registries.
Current golden artifacts include:

- `entities.kdl` from vanilla behavior-pack entities plus local overrides.
- `blocks.kdl` from Valentine's generated Bedrock block definitions and canonical state ID ranges.
- `biomes.kdl` from vanilla behavior-pack biome JSON.
- `items.kdl` from PMMP/BedrockData `required_item_list.json`.
- `creative.kdl` from PMMP/BedrockData creative inventory JSON.
- Optional `biome_packets.kdl` from a validated `bds-extractor` JSON capture.

`biomes.kdl` intentionally contains only behavior-pack facts today. Enrich it
with BDS packet/native facts for legacy IDs and packet definitions instead of
silently accepting weaker generated fallback data.

Refresh only the manifest without regenerating artifacts:

```powershell
cargo run -p unastar-data-gen -- --manifest-only
```

Refresh only Valentine-derived block data plus the manifest:

```powershell
cargo run -p unastar-data-gen -- --blocks-only
```

Refresh only behavior-pack biome data plus the manifest:

```powershell
cargo run -p unastar-data-gen -- --biomes-only
```

Refresh only PMMP-derived item/creative data plus the manifest:

```powershell
cargo run -p unastar-data-gen -- --pmmp-only
```

Refresh only BDS-extractor packet/runtime facts plus the manifest:

```powershell
cargo run -p unastar-data-gen -- --bds-only --bds-extraction <path-to-bds-extractor-json>
```

Run full artifact generation and include BDS packet/runtime facts:

```powershell
cargo run -p unastar-data-gen -- --bds-extraction <path-to-bds-extractor-json>
```

List parsed vanilla entities without writing artifacts:

```powershell
cargo run -p unastar-data-gen -- --list
```

Run full artifact generation:

```powershell
cargo run -p unastar-data-gen --
```

If generated artifacts change, run `unastar-data-codegen` and review the generated Rust diff before committing.

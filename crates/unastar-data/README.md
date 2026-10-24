# unastar-data

Generated, source-attributed gameplay data for Unastar.

The crate is the boundary between raw Bedrock data sources and the server runtime:

1. `datagen` reads protocolgen's pinned native BDS 1.26.51 measurements, vanilla behavior-pack JSON, PMMP creative inventory, and local KDL overrides, then writes reviewable artifacts in `output/`.
   The vanilla pack is the `data/vanilla_bp` submodule, Mojang/bedrock-samples release
   `v1.26.50.4` (`46ba6ea`), the newest stable samples release for BDS 26.50/26.51
   (26.51 is a hotfix without a samples release). Every entity file must parse; datagen
   fails rather than drop one.
2. `codegen` reads those golden artifacts and generates Rust types/tables under `src/`.
3. `unastar` consumes generated APIs; it should not parse PMMP, Prismarine, BDS, or behavior-pack data directly in hot runtime paths.

## Source priority

Gameplay facts should be merged in this order:

1. Pinned protocolgen native BDS 1.26.51 inputs for canonical block state
   ordinals, physical measurements, native tags, item IDs/stack limits, and biome IDs/definitions.
2. Vanilla behavior/resource packs for exposed gameplay components absent from native tables.
3. BDS handshake extraction for packet/runtime facts sent during login.
4. Pinned protocolgen packet registry projections (Cloudburst) for item component flags/version.
5. PMMP/BedrockData with recorded upstream provenance, currently creative inventory only.
6. Prismarine `minecraft-data` for cross-checks only; it does not supply these block/item/biome artifacts.
7. Local overrides with version/source notes.

See [`source-gap-audit.md`](source-gap-audit.md) before adding new artifact
families. In particular, do not derive canonical biome gameplay data from
Valentine/Prismarine biome tables alone; use vanilla behavior-pack biome JSON
first, then enrich with BDS packet/native facts.

## Manifest

`output/manifest.kdl` records source paths, commits/hashes, confidence, and artifact hashes.
Native inputs live under
`../valentine_gen/protocolgen/data/source/inputs/1.26.51/native/`, pinned by
the protocolgen submodule. They are CC0 BDS measurements from
`didntpot/bedrock-data` release/1.26.51, commit
`bb896a46ba856c58383e3cfb962a38c45a0dea78`; see its README/LICENSE and protocolgen's
`data/NOTICE.md`. Every consumed native file has its own SHA-256/commit entry.
Text inputs are hashed with CRLF folded to LF (binary files verbatim), so the digests
equal protocolgen's `data/source/lock.json` on any checkout, including Windows
`core.autocrlf` ones.
`generated/data/registry/items.nbt` supplies packet metadata absent from native
item properties and is separately hashed (Cloudburst provenance in protocolgen's source lock).
Its contents match the `ItemRegistryPacket` BDS 1.26.51 sends (all 2,076 entries; checked
against a capture by `unastar`'s `vanilla_item_registry_packet_matches_bds_capture`).

Current golden artifacts:

- `entities.kdl`: 128 vanilla entities and local overrides.
- `blocks.kdl`: 1,477 native blocks and 22,091 states in **unaltered canonical
  NBT order**; each state retains typed properties, network hash and physical measurements.
  Native property tables and tags are also preserved. Generated block physical fields
  project the first state; state-dependent measurements remain in KDL.
- `biomes.kdl`: 89 native IDs/definitions, enriched with explicitly attributed
  behavior-pack components where available.
- `items.kdl`: 2,076 native IDs/stack limits/properties/tags plus packet registry metadata.
  The 78 items whose registry component data is not the empty compound carry it as
  `component_nbt` (base64 of the exact network NBT, sliced from `items.nbt`, never
  re-encoded); codegen exposes it as `ItemData::component_nbt`.
- `creative.kdl`: PMMP creative inventory.
- Optional `biome_packets.kdl` / `entity_identifiers.kdl`: validated BDS captures.

Native data does **not** identify each block's placement/default state. As before,
`default_state_id` selects the first canonical state, explicitly marked
`weak_first_canonical_state_not_measured_default`; it is not a measured BDS default.
Block `id` is now the first canonical state ordinal, **not** a Prismarine legacy ID;
identifiers and `by_runtime_id` are the stable lookup contracts. Names are identifiers,
not unsourced translated display names. Opacity is native `opaqueFullBlock`;
hardness/resistance map directly to `destroySpeed`/`explosionResistance` without Java scaling.
Jolyne negotiates sequential block IDs (`block_network_ids_are_hashes=false`);
hashes are retained for validation, not substituted for ordinals.

Initialize both source submodules before generation:

```powershell
git submodule update --init crates/valentine_gen/protocolgen crates/unastar-data/data/vanilla_bp
```

Refresh only the manifest without regenerating artifacts:

```powershell
cargo run -p unastar-data-gen -- --manifest-only
```

Refresh only native block data plus the manifest:

```powershell
cargo run -p unastar-data-gen -- --blocks-only
```

Refresh native biome data with behavior-pack enrichment plus the manifest:

```powershell
cargo run -p unastar-data-gen -- --biomes-only
```

Refresh native items and PMMP creative data plus the manifest:

```powershell
cargo run -p unastar-data-gen -- --pmmp-only
```

Refresh only BDS-extractor packet/runtime facts plus the manifest:

```powershell
cargo run -p unastar-data-gen -- --bds-only --bds-extraction <path-to-bds-extractor-json>
```

Generate or refresh the Rust consumer surface for BDS packet artifacts:

```powershell
cargo run -p unastar-data-codegen -- --bds-packets-only
```

Generate or refresh the Rust consumer surface for block artifacts:

```powershell
cargo run -p unastar-data-codegen -- --blocks-only
```

Generate or refresh the Rust consumer surface for creative inventory artifacts:

```powershell
cargo run -p unastar-data-codegen -- --creative-only
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

Regenerate all Rust consumers after artifact generation:

```powershell
cargo run -p unastar-data-codegen --
```

Run datagen and codegen twice to check determinism. Neither embeds wall-clock time;
leave `UNASTAR_DATA_GENERATED_AT` unset (or pin it to the same value).

## BDS capture workflow

Use `bds-extractor` for packet/runtime facts that are sent by BDS during login:

```powershell
cargo run -p bds-extractor -- `
  --addr 127.0.0.1:19132 `
  --name BDSExtractor `
  --output .tmp/bds-capture.json
```

Then normalize and generate:

```powershell
cargo run -p unastar-data-gen -- `
  --bds-only `
  --bds-extraction .tmp/bds-capture.json `
  --output crates/unastar-data/output

cargo run -p unastar-data-codegen -- --bds-packets-only
```

Commit BDS-derived artifacts only when the capture came from a real validated
BDS source. Synthetic fixtures are for tests only and must not be checked in as
runtime facts.

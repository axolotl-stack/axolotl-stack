# unastar-data

Generated, source-attributed gameplay data for Unastar.

The crate is the boundary between raw Bedrock data sources and the server runtime:

1. `datagen` reads protocolgen's native BDS 1.26.51 measurements and open-data
   catalogues, then writes reviewable artefacts in `output/`. `codegen` builds
   the existing native runtime tables from those artefacts.
2. `build.rs` reads the pinned bedrock-samples submodule (`data/vanilla_bp`,
   `v1.26.50.4`, `46ba6ea`) and native biome measurements. It writes normalised
   actor definitions, enriched biomes, canonical loot JSON and item/recipe document
   bundles into Cargo's `OUT_DIR`, embeds them
   and decodes it once at startup. Pack-derived JSON/KDL and per-entity Rust
   are not committed (D2/D13); `output/pack-manifest.json` records counts/hashes.
3. `unastar` consumes the decoded typed APIs, not raw pack files in tick paths.

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

- `actor_definitions.json` (**OUT_DIR only**): 128 normalised definitions,
  including full component bodies, component groups, event trees, properties,
  description metadata and format versions. `actor_definitions::{vanilla,get}`
  decodes via the shared `unastar-packs::actor` loader. No `_defaults.kdl`,
  per-entity bundles or generated spawn functions remain.
- `actor_component_registry.json` (**OUT_DIR only**): 410 parser-kind names
  plus canonical name patterns and `ActorDocument` member names from stable
  Mojang schemas in the samples metadata and `data/upstream/mojang/entity_forms`.
  Build/runtime actor loading uses the same registry
  (`actor_definitions::parser()`); addon names register before loading. Only
  its count/hash is committed in the manifest.
- `blocks.kdl`: 1,477 native blocks and 22,091 states in **unaltered canonical
  NBT order**; each state retains typed properties, network hash and physical measurements.
  Native property tables and tags are also preserved. Generated block physical fields
  project the first state; state-dependent measurements remain in KDL.
- `biomes.json` (**OUT_DIR only**): 89 native IDs/definitions enriched with
  complete pack components, tags and climate; `biomes::{vanilla,get,iter}`.
- `items.kdl`: 2,076 native IDs/stack limits/properties/tags plus packet registry metadata.
  The 78 items whose registry component data is not the empty compound carry it as
  `component_nbt` (base64 of the exact network NBT, sliced from `items.nbt`, never
  re-encoded). Codegen emits the typed `ItemData` schema in `src/items.rs` and
  compact rows in `src/items.json`, decoded once by `items::vanilla()`; no
  per-item Rust constants remain. Native offhand/head capability tags and reviewed
  effective bucket stack limits are applied by datagen, with drift checks.
- `creative.kdl`: PMMP creative inventory.
- `loot_tables.json` (**OUT_DIR only**): 508 canonical tables from the vanilla
  pack plus `data/overrides/loot_tables/blocks.json`, decoded at startup by
  `loot::vanilla() -> &unastar_rules::loot::LootTables`. Tables retain the
  complete canonical JSON alongside typed pools/entries/conditions/functions;
  unknown functions/conditions are explicit `Other` values and evaluation
  returns an error rather than silently ignoring them.
  The authored block tables retain per-table `//source` notes and Java-format
  extensions (`alternatives`, `apply_bonus`, `limit_count`, binomial providers,
  `table_bonus`, `block_state_property`, `inverted`). These block facts remain
  a D4 deviation until E9 sweeps replace them; the D2 per-entry-static
  deviation is removed. I11 owns vanilla loot completion and BDS RNG parity.
- Optional `biome_packets.kdl` / `entity_identifiers.kdl`: validated BDS captures.
- `actor_identifiers.json`: measured BDS 1.26.51 capture facts (137 rows,
  protocol 2193), including JSON-less built-in actors. `actor_identifiers::vanilla`
  retains `rid`, `id`, `bid`, `hasspawnegg`, `summonable` and capture row order.
  The artefact records TOOLING's source path/commit and raw-packet SHA-256.
- `shapes.json`: every state's collision, outline and liquid-clip geometry,
  deduplicated by shape, from protocolgen `7b2e47c`. The geometry is
  [CloudburstMC/Data](https://github.com/CloudburstMC/Data), Apache-2.0; its
  pinned revision and the input hashes are recorded in the artefact and
  `manifest.kdl` (see protocolgen `generated/data/semantic_sources.json` and
  `data/NOTICE.md`). The 30 unavailable liquid-clip shapes remain explicitly
  unavailable, distinct from known-empty shapes. Runtime `shapes` decodes once.

Item/recipe pack families use the same shared `unastar-packs` reader as the
actor/loot pipeline. `item_components::decode_vanilla()` retains typed legacy and
modern item components; `recipes::decode_vanilla()` parses shaped, shapeless,
furnace, brewing and smithing documents. The game server freezes one
`recipes::RecipeRegistry` at startup and shares it through `RecipeRegistryResource`.
`furnace_result(identifier, aux)` borrows a canonical `ResultItem` without JSON
parsing, allocations or tag walks. `with_items(recipes, items)` permits an
addon-capable item snapshot before freeze. Lower numeric recipe priority wins;
exact and wildcard furnace inputs share the same priority ordering.

Legacy furnace spellings (`minecraft:coal:1`, `minecraft:fish`, etc.) normalize
through `data/measured/recipe_legacy_identities.json`, a small projection of the
pinned 1.26.51 CraftingData capture; no raw/cooked mob table is maintained.
Regenerate from this crate directory with:

```powershell
python tools/extract_recipe_identities.py C:/Users/jvigu/orca/bds-runtime/captures/1.26.51/20261025T172246Z/member/join/00054-CraftingDataPacket.json
```

This registry prerequisite supplies furnace lookup; it does not advertise a
CraftingData packet or execute crafting yet.

Generated `blocks::BlockData` also carries the native block-level physics
(`friction`, `material`, `blocks_motion`, `liquid`, `solid`, `falling`,
`requires_correct_tool_for_drops`, `visual_shape`) and the per-state property
table (`properties` + `state_values`, queried with `state_value` /
`with_state_value`).

Native data does **not** identify each block's placement/default state. As before,
`default_state_id` selects the first canonical state, explicitly marked
`weak_first_canonical_state_not_measured_default`; it is not a measured BDS default.
Block `id` is now the first canonical state ordinal, **not** a Prismarine legacy ID;
identifiers and `by_runtime_id` are the stable lookup contracts. Names are identifiers,
not unsourced translated display names. Opacity is native `opaqueFullBlock`;
hardness/resistance map directly to `destroySpeed`/`explosionResistance` without Java scaling.
Jolyne negotiates sequential block IDs (`block_network_ids_are_hashes=false`);
hashes are retained for validation, not substituted for ordinals.

Initialize both source submodules **before building**, not only generation:

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

Actor and biome artefacts regenerate automatically when their submodule inputs
change. Refresh their committed hash/count manifest with `--manifest-only`.
The D9 comparison uses the same pack-stack/brarchive reader as worldgen:

```powershell
$env:UNASTAR_BDS_DIR = 'C:/path/to/BDS-1.26.51'
cargo test -p unastar-packs --test vanilla bedrock_samples_match_the_bds_layer_stack
```

The samples' 128 entities equal the BDS layered definitions; BDS alone contains
`minecraft:agent`. Without the environment variable the install test prints a
skip message. This data comparison does not establish crafted-pack parser or
gameplay parity with BDS.

Regenerate the committed, openly licensed shape family:

```powershell
cargo run -p unastar-data-gen -- --shapes-only
```

Refresh native items and PMMP creative data plus the manifest:

```powershell
cargo run -p unastar-data-gen -- --pmmp-only
```

Loot tables regenerate on pack/override changes during the build. Refresh
their committed hash/count manifest:

```powershell
cargo run -p unastar-data-gen -- --manifest-only
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

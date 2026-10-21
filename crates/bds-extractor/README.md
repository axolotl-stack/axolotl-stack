# bds-extractor

`bds-extractor` connects to a Minecraft Bedrock Dedicated Server (BDS) as a
temporary client and writes login and post-spawn packet/runtime data to a
validated JSON file.

The output is an input to `unastar-data-gen`; runtime crates should not parse
the JSON directly.

## Capture from a local BDS

Start a matching BDS instance in offline/dev mode, then run:

```powershell
cargo run -p bds-extractor -- `
  --addr 127.0.0.1:19132 `
  --name BDSExtractor `
  --timeout 30 `
  --output .tmp/bds-capture.json
```

The extractor joins with a random self-signed identity, retains Jolyne's play
stream, and drains definition packets queued during login and received after
spawn. `--timeout` bounds connection/login and, separately, collection of all
required definitions (not an idle timeout that resets on each packet). It sends
a Disconnect packet on completion or collection failure, with a two-second
shutdown bound. No output is written if extraction or validation fails.

## Output contract

Required sections are `metadata`, `items` (ItemRegistry), `blocks` (StartGame
block properties), `creative` (CreativeContent), `biomes` (BiomeDefinitionList),
and `entities` (AvailableActorIdentifiers). Missing post-spawn sections and an
empty item registry return `ValidationError::MissingSection`, naming the
section; malformed contents return `ValidationError::InvalidData`.
The JSON decoder accepts absent/null post-spawn fields only to provide these
typed diagnostics: they are **not optional in a successful capture**.

`blocks.properties` may be empty: StartGame carries custom block properties,
not the complete vanilla block palette. Empty creative groups/entries are valid
when the server sends an empty CreativeContent packet. Per-item `nbt_base64`
is genuinely optional and omitted when no component NBT is supplied.
CraftingData/recipes, item tags, canonical block states and other registries
are outside this extractor's output contract; unrelated play packets are ignored.

## Relationship to protocolgen

For pinned vanilla game data used by workspace generation, prefer
`crates/valentine_gen/protocolgen/generated/data` and protocolgen's native BDS
capture tooling (`docs/outputs.md` in that submodule). Its
`generated/<version>/vanilla-data` captures include overlapping biome,
actor-identifier, creative-content and item-registry packet payloads, plus
crafting data, recipes, canonical block states and other registries that this
JSON format does not expose. At the pinned protocolgen revision, `outputs.md`
lists committed vanilla-data snapshots for 1.26.40 and 1.26.44; do not relabel
those as 1.26.51. `make vanilla-data BDS_BINARY=…` captures a matching pinned BDS.

Use this extractor for a particular running server (including custom content),
for validating Jolyne's client receive path, or when the existing extractor-JSON
normalization workflow below is specifically needed. These formats are not
interchangeable: packet payloads/NBT in vanilla-data are not extractor JSON.

## Normalize into source-attributed artifacts

Do not check in captures from throwaway or synthetic sources. For a real capture
that you intend to use as a source input, normalize it through `unastar-data`:

```powershell
cargo run -p unastar-data-gen -- `
  --bds-only `
  --bds-extraction .tmp/bds-capture.json `
  --output crates/unastar-data/output
```

Then generate the Rust consumer surface:

```powershell
cargo run -p unastar-data-codegen -- --bds-packets-only
```

Review the resulting `crates/unastar-data/output/biome_packets.kdl`,
`crates/unastar-data/output/entity_identifiers.kdl`, `output/manifest.kdl`, and
`crates/unastar-data/src/bds_packets.rs` before committing. The manifest must
include `source "bds_extractor_capture"` for any BDS packet artifacts.

## Fixture mode

Fixture mode validates and rewrites an existing extractor JSON file without
connecting to a server:

```powershell
cargo run -p bds-extractor -- `
  --fixture crates/bds-extractor/tests/fixtures/minimal-extracted-data.json `
  --output .tmp/bds-fixture-roundtrip.json
```

This is for smoke tests and schema validation. It should not be treated as a
real source of runtime IDs.

## Source-boundary rules

- `bds-extractor` JSON is a raw capture format, not a runtime API.
- `unastar-data-gen` owns conversion from capture JSON to reviewable KDL.
- `unastar-data-codegen` owns generated Rust consumer tables.
- Behavior-pack facts (`entities.kdl`, `biomes.kdl`) stay separate from packet
  facts (`entity_identifiers.kdl`, `biome_packets.kdl`) until an explicit typed
  joiner is added.
- Do not use Valentine, Prismarine, or synthetic fixtures to populate BDS packet
  artifacts.

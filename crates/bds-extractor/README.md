# bds-extractor

`bds-extractor` connects to a Minecraft Bedrock Dedicated Server (BDS) as a
temporary client and writes the packet/runtime data captured during login to a
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

The extractor joins with a random self-signed identity, captures the login data
that Jolyne exposes, validates it, writes JSON, and disconnects.

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

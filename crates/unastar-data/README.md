# unastar-data

Generated, source-attributed gameplay data for Unastar.

The crate is the boundary between raw Bedrock data sources and the server runtime:

1. `datagen` reads vanilla behavior-pack JSON plus local KDL overrides and writes reviewable artifacts in `output/`.
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

## Manifest

`output/manifest.kdl` records source paths, commits/hashes, confidence, and artifact hashes.
Refresh only the manifest without regenerating entities:

```powershell
cargo run -p unastar-data-gen -- --manifest-only
```

List parsed vanilla entities without writing artifacts:

```powershell
cargo run -p unastar-data-gen -- --list
```

Run full entity artifact generation:

```powershell
cargo run -p unastar-data-gen --
```

If generated artifacts change, run `unastar-data-codegen` and review the generated Rust diff before committing.

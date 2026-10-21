# `valentine`

`valentine` is the Bedrock protocol surface for the workspace.

It re-exports generated version crates behind feature flags and keeps the shared Bedrock codec/runtime in `bedrock_core`.

## Current Workspace Version

The checked-in workspace targets Bedrock **26.51**, network protocol **2193**.
The canonical protocolgen manifest labels this release `1.26.51`; Valentine
uses that schema label for its module and Cargo feature names.

- `bedrock_1_26_51` (default feature)
- `valentine::bedrock::protocol::v1_26_51::*`
- `valentine::bedrock::version::v1_26_51::*`
- `valentine::bedrock::v1_26_51::*` (compatibility alias)

## Layout

- `src/bedrock/`: shared Bedrock-facing API, version aliases, codec/context/error re-exports
- `bedrock_core/`: shared codec/runtime primitives used by every generated version crate
- `bedrock_versions/v1_26_51/`: generated protocol crate for the current version

## Import Paths

Prefer:

```rust
use valentine::bedrock::version::v1_26_51::*;
```

Compatibility aliases still exist:

```rust
use valentine::bedrock::v1_26_51::*;
use valentine::bedrock::protocol::v1_26_51::*;
```

Use `protocol::vX_Y_Z` when you explicitly want the raw generated version crate/module layout.

## Regenerating Protocol Code

From the repo root:

```bash
git submodule update --init --recursive
cargo run -p valentine_gen -- --latest
cargo fmt --all
```

Protocol code is generated from the pinned
[`bedrock-mc/protocolgen`](../valentine_gen/protocolgen) canonical manifest by
default. The checked-in protocolgen submodule and generated Valentine sources
must be advanced together. To use a different canonical manifest explicitly:

```bash
cargo run -p valentine_gen -- --latest --protocolgen-manifest /path/to/manifest.json
```

The legacy schema frontends remain available through `--source endstone`,
`--source mojang`, and `--source prismarine`. Protocolgen only supplies protocol
schemas, so block, item, entity, and biome data generation must still select the
appropriate data source explicitly.

Generate multiple versions when you want cross-version type/packet dedup to be considered in a single run:

```bash
cargo run -p valentine_gen -- --versions 1.21.120,1.21.124,1.26.30
```

## Notes

- Bedrock strings are decoded lossily on purpose for wire compatibility with existing implementations such as `gophertunnel`.
- Prefer importing from `bedrock::version::vX_Y_Z` in application code unless you specifically need the raw generated protocol crate or a compatibility alias.

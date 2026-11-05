# valentine_gen — agent guide

Extends the root [AGENTS.md](../../AGENTS.md). The contract this crate must keep is
[openspec/specs/protocol-generator/spec.md](../../openspec/specs/protocol-generator/spec.md);
how it works is in [README.md](README.md) ("Generator pipeline", corrections sections).

## Rules

- **Never edit generated output** (`crates/valentine/bedrock_versions/v1_26_51`,
  `crates/valentine/src/bedrock`). Behaviour that must survive regeneration goes in the parser,
  IR analysis or emitter. CI regenerates and fails on any diff.
- **Never edit the pinned protocolgen manifest** (`protocolgen/generated/1.26.51/`, a submodule).
  Capture-proven defects go in `src/parser/protocolgen_corrections.rs`, keyed on packet + field,
  pinning the node they replace and the protocol version whose captures prove them; write the
  upstream report in `PROTOCOLGEN_ISSUES.md`. Mojang/Endstone corrections live in `overrides/`
  and `overrides-endstone/`; every entry is fail-closed and carries a `why`.
- **Never infer wire ordinals or codecs.** Fail generation instead of guessing.
- `generated/1.26.50/` is Preview 25 (protocol 2187), not stable. Only `1.26.51` (protocol 2193)
  is supported.
- Emit with `quote` / `syn` / `proc-macro2` / `prettyplease`, not strings. Emitted Rust uses
  explicit imports and clear module boundaries; avoid glob ambiguity.
- Packet argument typing has **one source of truth**: signatures, codec args and packet dispatch
  all reuse the resolved analysis from `src/generator/resolver.rs`. Don't recompute it elsewhere.
- Cross-version dedup only with explicit, inspectable canonical paths. If reuse changes, check the
  generated version crate's `Cargo.toml` wiring.

## Map

| File | Role |
|---|---|
| `src/main.rs` | CLI entrypoint and source selection |
| `src/parser/` | Source frontends (protocolgen default; prismarine, Mojang, Endstone) → IR |
| `src/ir.rs` | Internal representation |
| `src/generator/resolver.rs`, `analysis.rs` | Container, dependency and codec-arg analysis |
| `src/generator/definitions.rs` | Type/container emission and canonical registration |
| `src/generator/codec.rs`, `primitives.rs` | `BedrockCodec` generation |
| `src/generator/borrowed.rs` | Borrowed packet views |
| `src/generator/mod.rs` | Module coordination and version-crate output |

When traversal, naming and token emission start to mix, split out a helper module.

## Validate

```sh
cargo clippy -p valentine_gen --all-targets -- -D warnings
cargo test -p valentine_gen
cargo run -p valentine_gen -- --latest && cargo fmt --all
cargo test -p valentine          # roundtrip and codec tests against the regenerated crate
git diff --stat crates/valentine # every generated change must be explained by your generator change
```

Use the `regen-protocol` skill for the full sequence.

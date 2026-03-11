# `valentine_gen`

`valentine_gen` generates the Bedrock protocol crates consumed by `valentine`.

It reads `minecraft-data` and `bedrock-data`, resolves protocol schema dependencies into typed Rust structures, emits formatted Rust with `quote` + `syn` + `prettyplease`, and updates the `valentine` workspace wiring.

## What It Generates

- `crates/valentine/bedrock_versions/vX_Y_Z/`
- `crates/valentine/src/bedrock/protocol/mod.rs`
- `crates/valentine/src/bedrock/version.rs`
- `crates/valentine/Cargo.toml` feature/dependency entries

## Generator Pipeline

1. Parse `protocol.json` into the internal IR in `src/ir.rs`.
2. Analyze containers in `src/generator/resolver.rs`.
3. Resolve discriminator/argument types once and reuse that analysis for:
   - packet signatures
   - `BedrockCodec::Args` generation
   - mcpe packet dispatch generation
   - nested packet/type argument forwarding
4. Emit `proto.rs`, `types.rs`, `mcpe.rs`, `common.rs`, and version `lib.rs`.
5. Register canonical definitions so later versions generated in the same run can reuse identical types/packets and add the necessary inter-version crate dependencies automatically.

## Setup

```bash
git submodule update --init --recursive
```

## Usage

Generate the default/latest Bedrock version:

```bash
cargo run -p valentine_gen -- --latest
```

Generate specific versions:

```bash
cargo run -p valentine_gen -- --versions 1.21.120
cargo run -p valentine_gen -- --versions 1.21.120,1.21.124,1.26.0
```

Generate only protocol code:

```bash
cargo run -p valentine_gen -- --latest --proto
```

Generate everything:

```bash
cargo run -p valentine_gen -- --all
```

List supported Bedrock versions:

```bash
cargo run -p valentine_gen -- --list-versions
```

Enable debug logging:

```bash
cargo run -p valentine_gen -- --latest --log debug
```

## Maintenance Notes

- Cross-version dedup only applies to versions processed in the same generator invocation.
- Bedrock strings intentionally use tolerant byte-to-string decoding to match protocol behavior seen in existing implementations.
- When changing generated output shape, update the analysis phase first rather than patching generated files by hand.
- Generated packet/controller args may be more strongly typed than the raw stored schema field. Keep those concerns separate when modifying discriminator logic.

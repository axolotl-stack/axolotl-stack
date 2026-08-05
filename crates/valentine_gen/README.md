# `valentine_gen`

`valentine_gen` generates the Bedrock protocol crates consumed by `valentine`.

It reads either PrismarineJS `minecraft-data` or Mojang's official
`bedrock-protocol-docs`, resolves protocol schema dependencies into typed Rust
structures, emits formatted Rust with `quote` + `syn` + `prettyplease`, and
updates the `valentine` workspace wiring.

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

The default source remains PrismarineJS and is unchanged:

```bash
cargo run -p valentine_gen -- --source prismarine --latest --proto
```

Mojang schemas are selected explicitly. The source currently supports protocol
generation only; item/block/entity data still comes from PrismarineJS:

```bash
cargo run -p valentine_gen -- --source mojang --versions 1.26.30 --proto
```

Use `--mojang-docs <DIR>` and `--overrides <DIR>` to point at alternate
checkouts/directories. Use `--output-dir <DIR>` for a scratch generation; this
is useful for reviewing output without touching the checked-in version crates:

```bash
cargo run -p valentine_gen -- \
  --source mojang --versions 1.26.30 --proto \
  --output-dir C:/tmp/valentine-mojang
```

### Mojang version mapping

The pinned `bedrock-protocol-docs` submodule is commit
[`ba81d713aa983bb6bc26fe662a9934c5de1838a5`](https://github.com/Mojang/bedrock-protocol-docs/commit/ba81d713aa983bb6bc26fe662a9934c5de1838a5),
the `r/26_u3` snapshot. Its schema metadata and
[`changelog_1001_05_18_26.md`](https://github.com/Mojang/bedrock-protocol-docs/blob/ba81d713aa983bb6bc26fe662a9934c5de1838a5/changelog_1001_05_18_26.md)
identify Minecraft 1.26.30 as network protocol 1001. Valentine calls this
same target `v1_26_30`; the two projects use different release/tag naming and
network-version histories, so the generator reads `x-minecraft-version` and
`x-protocol-version` from the schemas rather than guessing from a tag. The
next `r/26_u4` snapshot changes the protocol numbering, so it is intentionally
not used here.

### Mojang corrections

`crates/valentine_gen/overrides/bpd-fixer.json` is applied in memory before
Mojang parsing. It records the source link and reason for requiredness fixes,
legacy enum values, discriminator enum corrections, the known double-optional
presence-byte fields, and the global compressed `oneOf` discriminator rule.
The parser recognizes `+double-optional` as two presence bytes. Add future
schema corrections to the data file (or another JSON file in the same
directory); never patch generated Rust output.

Mojang definition IDs are global hashes. The r/26_u3 hash
`#/definitions/3172631924` is the builtin `CompoundTag`, not a missing
per-file definition; the parser maps it directly to Valentine’s
`Primitive::Nbt`. Valentine currently exposes one NBT codec using its Network
Little-Endian convention, so fixed-width LE NBT call sites share that IR alias
until dialect-specific NBT types are added.

## Usage

Generate the default/latest Bedrock version:

```bash
cargo run -p valentine_gen -- --latest
```

Generate specific versions:

```bash
cargo run -p valentine_gen -- --versions 1.21.120
cargo run -p valentine_gen -- --versions 1.21.120,1.21.124,1.26.30
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

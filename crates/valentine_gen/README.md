# `valentine_gen`

`valentine_gen` generates the Bedrock protocol crates consumed by `valentine`.
It accepts either PrismarineJS `minecraft-data` or Mojang's official
`bedrock-protocol-docs`, lowers the selected source into a shared IR, and emits
the version crate and Valentine workspace wiring.

## Setup and sources

```bash
git submodule update --init --recursive
```

Prismarine remains the default source. Mojang generation is selected explicitly
and requires a scratch output directory, so it cannot overwrite the checked-in
version crates:

```bash
cargo run -p valentine_gen -- --source prismarine --latest --proto
cargo run -p valentine_gen -- \
  --source mojang --versions 1.26.40 --proto \
  --output-dir C:/tmp/valentine-mojang \
  --emit-wire-manifest C:/tmp/valentine-2168-wire.json
```

`--emit-wire-manifest <FILE>` requires one selected version and writes a stable,
machine-readable view of the lowered IR. Each packet contains its ordered wire
operations with references and nested containers recursively expanded. Primitive
operations stay primitive; arrays record their count prefix, options record the
Bool presence boundary, unions record their control codec and per-value payload,
and recursive references become explicit cycle markers. This makes conformance
comparison independent of generated Rust syntax and is suitable for a future CI
gate.

`--mojang-docs <DIR>` selects any other docs checkout without changing the
submodule pin. For example, maintainers can inspect Mojang main/protocol 2169 in
a scratch checkout while the repository remains pinned to the live-server 2168
target. `--overrides <DIR>` similarly selects an alternate correction set.

## Mojang version mapping and pin policy

The `bedrock-protocol-docs` gitlink is
`0e00fe80f4f3c71572ff6429de40146d1f4412fc` (`automated/1.26.40`). Its schemas
stamp `x-minecraft-version: 1.26.40-beta.0` and `x-protocol-version: 2168`;
Valentine normalizes the prerelease metadata suffix to crate/module version
`v1_26_40` while retaining protocol 2168.

This is intentionally not Mojang main/`r/26_u4` (1.26.50/protocol 2169).
Cinnabar is a client and must advertise the protocol accepted by live 1.26.40
servers. A 1.26.40 server rejects RequestNetworkSettings advertising 2169, and
using the newer layouts would also misparse TextData and persona animated
texture enums. Cinnabar's preceding target is protocol 1001, verified against
live BDS 1.26.32.2; protocol 2168 is its next interoperable target. Both
independent hand-written implementations used as oracles—gophertunnel and
Cloudburst—implement 2168, while neither implements 2169.

Ignoring version/format stamp lines, 968 of 971 JSON schemas are identical
between this pin and main. The complete known 2168-to-2169 wire delta is:

- `RequestNetworkSettingsPacketPayload.json`: protocol constant 2168 becomes
  2169.
- `TextDataPayload.json`: 2169 adds `LineGapHeight` (`F32LE`) at ordinal 4 and
  shifts the following field.
- `persona__AnimatedTextureType.json`: 2169 adds a leading `None`, shifting all
  following discriminants by one.

Main also has `__protocoldoc.json`; this pin does not need it because its corpus
has zero dangling references.

## Mojang corrections and conformance

`overrides/bpd-fixer.json` and `overrides/enum-ordinals.json` are applied in
memory before lowering. Every operation is fail-closed and carries a `why`
citing the protocol 2168 gophertunnel or Cloudburst source used to establish
the correction. The 186 string-enum occurrences have explicit values; the
generator never infers missing wire ordinals. Field-level `Enum-as-Value`
metadata supplies the contextual scalar codec, including Compression, while
the referenced enum supplies its members.

Compression is normally signed zig-zag for signed integer types and ordinary
varint for unsigned types. Eight-bit fields are deliberately not trusted to
that mechanical rule: all 24 ambiguous occurrences in this corpus are pinned
individually to their protocol 2168 oracle codec. See
[`MOJANG_PARITY.md`](MOJANG_PARITY.md) for the evidence table and the
gophertunnel/Cloudburst conformance inventory.

Mojang fixed-width numerics are little-endian unless marked `Big Endian`.
`No size compression` arrays use `U32LE` lengths, and equal `minItems` and
`maxItems` become fixed arrays. Mojang's global CompoundTag hash
`3172631924` maps to Valentine's network little-endian NBT primitive.

## Generator pipeline

1. Parse the selected source into the IR in `src/ir.rs`.
2. Analyze containers and codec arguments in `src/generator/resolver.rs`.
3. Optionally emit the structured wire manifest from the fully lowered IR.
4. Emit `proto.rs`, `types.rs`, `mcpe.rs`, `common.rs`, `borrowed.rs`, and the
   version crate manifest.
5. Register canonical definitions for reuse among versions generated in the
   same invocation.

The test suite generates Mojang output in a temporary directory and runs
`cargo check` on the generated crate.

## Common commands

```bash
cargo run -p valentine_gen -- --latest
cargo run -p valentine_gen -- --versions 1.21.120,1.21.124
cargo run -p valentine_gen -- --latest --proto
cargo run -p valentine_gen -- --all
cargo run -p valentine_gen -- --list-versions
cargo run -p valentine_gen -- --latest --log debug
```

When behavior must survive regeneration, change the parser, IR analysis, or
emitter rather than generated Rust.

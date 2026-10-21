# `valentine_gen`

`valentine_gen` generates the Bedrock protocol crates consumed by `valentine`.
It lowers protocolgen's reconciled canonical manifest into a shared IR by
default, then emits the version crate and Valentine workspace wiring. Legacy
PrismarineJS, Mojang, and Endstone frontends remain available explicitly.

## Setup and sources

```bash
git submodule update --init --recursive
```

The pinned protocolgen submodule is the default source. This command regenerates
the checked-in stable Bedrock 26.51 protocol (protocolgen schema label
`1.26.51`, network protocol `2193`) from its canonical manifest:

```bash
cargo run -p valentine_gen -- --latest
cargo fmt --all
```

Minecraft's player-facing version numbering uses the year prefix (`26.51`).
Protocolgen manifests retain the `1.26.51` schema label used for generated
module/feature names. The `1.26.50` manifest in protocolgen is Preview 25,
protocol 2187; it is not the stable 26.50 release (protocol 2193).

An alternate canonical manifest may be selected with
`--protocolgen-manifest <FILE>`. Protocolgen provides protocol schemas only.
Prismarine remains the source for block, item, entity, and biome data.

Legacy protocol frontends are selected explicitly. Mojang generation requires a
scratch output directory, so it cannot overwrite the checked-in version crates:

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

Protocolgen input is already a canonical wire manifest, so
`--emit-wire-manifest` is intentionally rejected with the default source.

Canonical unions always retain their control codec and every data or void
variant. Inline union names are disambiguated from named payloads with a
`Union` suffix: `DisconnectPacketMessagesUnion` contains either
`DisconnectPacketMessages(DisconnectPacketMessages)` (control 0, show message)
or `Empty1` (control 1, hide screen). A payload name collision must never select
the payload codec in place of the union codec. The differential gate is
`cargo test -p valentine --test union_conformance`; it compares BDS wire vectors
and both variants against the pinned protocolgen Rust contract crate.

`--mojang-docs <DIR>` selects a separate docs checkout without changing the
pinned canonical input. Keep scratch comparisons isolated from checked-in
outputs, and verify the exact protocol version against the stable protocol
changelog before using a schema as a release target.

## Mojang version mapping and pin policy

The only supported target is Bedrock 26.51 (protocolgen schema label
`1.26.51`, network protocol 2193). The canonical manifest and its source
fingerprints are pinned under `protocolgen/generated/1.26.51/`, from the
`cinnabar-mc/protocolgen` submodule. Older Valentine version crates (1.26.0,
1.26.40, 1.26.44) have been removed; regenerate from their protocolgen
manifests if a compatibility crate is ever needed again.

Do not treat `generated/1.26.50/` as the stable release: it captures Preview 25
and protocol 2187. Stable 26.50 and 26.51 use protocol 2193; the 26.51 hotfix
has no packet changes relative to stable 26.50.

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

## Endstone corrections

`overrides-endstone/` is the correction set for the Endstone dumps and is
expected to stay small: that corpus is extracted from a running server, so a
needed correction usually means the dumper is wrong and is worth reporting
upstream. The corrections are applied over `packets/`, `types/` and `enums/`
together, keyed `<directory>/<document name>`, because a shared type such as the
item `User Data Buffer` is described once under `types/` and reached by every
packet that embeds it.

The one systematic correction is binary buffers. BDS declares a byte buffer as a
C++ `std::string`, which has no encoding guarantee, and the dumper reports that
faithfully as `string`. Valentine decodes strings lossily for gophertunnel
parity, so a `String` turns every non-UTF-8 byte into U+FFFD and re-encodes it as
`EF BF BD`. Encode and decode corrupt symmetrically, so a value round-trip still
passes while the wire is wrong. `overrides-endstone/binary-buffers.json` retypes
each such field as `uint8` with a `uvarint32` repeat prefix, which is byte-for-
byte the same framing and matches gophertunnel's `io.ByteSlice`. Fields that
genuinely carry text stay strings; the file lists both sets with citations.

Corrections against this corpus use `patch_field`, which edits one entry of a
document's `fields` array in place. Mojang's `patch_property` cannot reach them
(these documents use an ordered array, not a `properties` map), and restating a
whole document with `replace_schema` would silently freeze its other fields at
today's dump.

## Generator pipeline

1. Parse the selected source into the IR in `src/ir.rs`.
2. Analyze containers and codec arguments in `src/generator/resolver.rs`.
3. Optionally emit the structured wire manifest from the fully lowered IR.
4. Emit `proto.rs`, `types.rs`, `mcpe.rs`, `common.rs`, `borrowed.rs`, and the
   version crate manifest.
5. Register canonical definitions for reuse among versions generated in the
   same invocation.

Generated Rust is parsed and formatted before its destination is touched, then
staged in a sibling temporary file and atomically renamed into place. A failed
format or write preserves the previous generated module.

Prismarine data-only generation merges existing live version declarations into
its manifest and surface updates. A version absent from minecraft-data is kept
when its version crate still exists, so refreshing legacy data cannot remove the
default protocolgen-backed crate or its feature.

Surface refreshes also regenerate `valentine/src/bedrock/borrowed.rs` with
version-independent core borrowing primitives only. Packet views are exported by
their generated version crate; obsolete hand-written packet views and retired
feature gates must not survive a version removal.

Codec emission derives conservative minimum wire sizes for collection bounds
and detects named-reference cycles to insert the shared recursive-value budget.
The same bounds apply to borrowed collections; recursive borrowed raw fallbacks
use the owned codec on materialization.

Owned frame dispatch uses a non-inlined typed payload helper. Each packet's
decode temporaries and trailing-byte rejection stay inside that helper, before
conversion to the dispatch enum. This preserves exact payload consumption while
preventing the largest packet codecs from imposing their stack frame and Windows
stack probes on every small packet. Use `valentine`'s `mcpe_frame_decode` benchmark
and generated assembly to check this boundary; interleave benchmark executables
because machine load and hybrid-core scheduling can obscure small differences.

The test suite generates Mojang output in a temporary directory and runs
`cargo check` on the generated crate.

## Common commands

```bash
cargo run -p valentine_gen -- --latest
cargo run -p valentine_gen -- --protocolgen-manifest /path/to/manifest.json
cargo run -p valentine_gen -- --source endstone --latest --proto
cargo run -p valentine_gen -- --source prismarine --latest --items --blocks
cargo run -p valentine_gen -- --list-versions
cargo run -p valentine_gen -- --latest --log debug
```

When behavior must survive regeneration, change the parser, IR analysis, or
emitter rather than generated Rust.

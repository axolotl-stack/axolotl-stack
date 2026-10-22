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

`DisconnectPacket.messages` is a `DisconnectPacketMessagesUnion`. Construct
`DisconnectPacketMessagesUnion::DisconnectPacketMessages(DisconnectPacketMessages {
message, filtered_message })` to show a kick message, or `Empty1` to hide the
disconnect screen. The union owns the wire control varint; encoding its payload
struct alone is not a valid disconnect body. `tests/union_conformance.rs`
cross-checks both variants against protocolgen's Rust contract.

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

## Decode resource policy

Owned and borrowed collection decoders validate statically known minimum wire
sizes before allocating. Unknown-size values grow storage only as items decode;
zero-byte schema values remain valid. `BedrockCodec::MIN_ENCODED_SIZE` defaults
to zero for external implementations and may be overridden with a conservative
wire-size lower bound (not Rust's in-memory `size_of`).

Generated recursive types share a synchronous, thread-local budget of 64 active
recursive value decodes. Exceeding it returns `RecursiveDepthExceeded`; success,
errors, and unwinding restore the budget. Typed borrowed recursive unions enforce
the same budget during decoding, before conversion to an owned packet.

NBT accepts segmented `Buf` inputs without consuming following fields. Values
wholly in the first chunk retain the contiguous extraction path; split values
copy only the bytes belonging to that NBT value.

Owned MCPE frame decoding rejects unread payload bytes with
`TrailingPacketBytes`, matching [gophertunnel's default invalid-packet
handling](https://github.com/Sandertv/gophertunnel/blob/master/minecraft/packet.go).
There are no packet-specific trailing-data exceptions.

## Shared byte payloads and typed views

Generated binary buffer fields use `bytes::Bytes`, including chunk payloads,
login requests, and item user-data buffers. Decoding from `Bytes` shares the input
allocation; other `Buf` implementations may copy when extracting a buffer.
Construct fields from a `Vec<u8>` with `.into()` and request a mutable copy with
`.to_vec()` only when needed.

For a loop over inner frames, construct `bedrock::borrowed::BorrowedReader` once
over the immutable input and call `McpePacketData::decode_inner_reader` or
`BorrowedMcpePacket::decode_inner_reader`. Framing borrows the source; only fields
that retain bytes clone its shared handle. The borrowed method returns the
unconsumed payload count, which framed callers must require to be zero.
`decode_inner_bytes` remains convenient for an owned `Bytes` cursor; generic
`Buf` entry points still support segmented inputs. Once an entry length is
validated, shared-input entry points consume that complete entry even when its
header or payload fails, leaving the next entry intact.

Text fields use `bedrock::codec::BedrockString` (`bytestring::ByteString`), an
immutable, UTF-8-guaranteed `Bytes`-backed string. Decode validates once, shares
valid input, and normalizes malformed UTF-8 lossily before construction. Reads
through `&*field` do not validate again. Construct fields with `.into()`; clone
to share storage, or use `.to_string()` when an application needs mutable text.

Construct `McpePacketData` through the generated `From<Packet>` conversion
(`packet.into()`) rather than adding `Box::new` manually. The generator selects
inline or boxed packet and union storage from their layouts without changing the
wire representation.

Every current packet has a typed `Borrowed*Packet` view. Union arms recursively
retain `BorrowedStr` and `Bytes`; converting to owned validates textual strings
without copying valid shared input. Borrowed views preserve invalid UTF-8 bytes
on re-encoding, while owned conversion replaces them lossily.

`encode_inner_bytes_mut` predicts the frame prefix width from reusable capacity
and writes the header and body in place. It moves the body only when the prefix
width prediction is wrong; payload bytes still must be copied once into a
contiguous output buffer.

## Notes

- Bedrock strings are decoded lossily on purpose for wire compatibility with existing implementations such as `gophertunnel`.
- Prefer importing from `bedrock::version::vX_Y_Z` in application code unless you specifically need the raw generated protocol crate or a compatibility alias.

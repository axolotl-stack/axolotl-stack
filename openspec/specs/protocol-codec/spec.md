# protocol-codec Specification

## Purpose
The Bedrock wire codec: the shared runtime primitives in `crates/valentine/bedrock_core` (the `BedrockCodec`/`BedrockSized` traits, scalar and varint codecs, strings, NBT, bitsets, borrowed readers and `DecodeError`) and the contracts that generated version crates such as `crates/valentine/bedrock_versions/v1_26_51` uphold when they compose those primitives into packets and MCPE frames. `crates/valentine/src` re-exports both behind version feature flags. Transport (RakNet/NetherNet), batching/compression and session state are out of scope; generation of the packet code is covered by `protocol-generator`.

## Requirements

### Requirement: Explicit numeric byte order
The codec SHALL encode the little-endian wrapper types (`U16LE`, `I16LE`, `U32LE`, `I32LE`, `U64LE`, `I64LE`, `F32LE`, `F64LE`) in little-endian byte order, and SHALL encode bare Rust numeric types (`u16`, `i16`, `u32`, `i32`, `u64`, `i64`, `f32`, `f64`) in big-endian byte order. Fixed-width decodes MUST fail with `DecodeError::UnexpectedEof` instead of panicking when fewer bytes remain than the type's width.

#### Scenario: Little-endian wrapper layout
- **WHEN** `U32LE(0x0403_0201)` is encoded
- **THEN** the output bytes are `01 02 03 04`

#### Scenario: Short fixed-width input
- **WHEN** a `u32` is decoded from a buffer holding fewer than four bytes
- **THEN** decoding returns `UnexpectedEof` with `needed: 4`

Evidence: crates/valentine/bedrock_core/src/bedrock/codec.rs (`le_int_newtype!`, tests `u32le_encoding_is_little_endian`, `u32_decode_insufficient_buffer`), crates/valentine/tests/binary_buffer_roundtrip.rs (`game_rule_scalar_payloads_are_little_endian`)

### Requirement: Varint, varlong and zig-zag encoding
The codec SHALL encode unsigned varints (`VarUInt`, `VarULong`, `protocol::wire`) as LEB128 with seven payload bits per byte, and `ZigZag32`/`ZigZag64` as zig-zag-mapped unsigned varints. Unsigned decoding MUST reject encodings longer than 5/10 bytes, payload bits beyond the type's width, and overlong encodings. The signed `VarInt`/`VarLong` wrappers MUST reject encodings longer than 5/10 bytes.

#### Scenario: Full unsigned range round-trips
- **WHEN** `u32::MAX` and `u64::MAX` are encoded as unsigned varints and decoded
- **THEN** the decoded values equal the originals and all bytes are consumed

#### Scenario: Overlong unsigned encoding
- **WHEN** an unsigned varint encodes a value with a trailing zero continuation group (for example `80 00`)
- **THEN** decoding fails instead of returning `0`

#### Scenario: Zig-zag sign mapping
- **WHEN** `ZigZag32(-1)` is encoded
- **THEN** the output is the single byte `01`

Evidence: crates/valentine/bedrock_core/src/protocol/wire.rs, crates/valentine/bedrock_core/src/bedrock/codec.rs (tests `unsigned_varints_preserve_the_full_wire_range`, `unsigned_varints_reject_payload_bits_beyond_their_width`, `unsigned_varints_reject_noncanonical_overlong_encodings`, `zigzag32_encoding`, `varint_too_long`, `varlong_too_long`), crates/valentine/tests/bedrock_roundtrip.rs (`varint_roundtrips_typical_boundaries`)

### Requirement: Lossy string decoding
The codec SHALL decode Bedrock text (`String` and `BedrockString`) as a var-u32 length-prefixed byte string and MUST NOT reject non-UTF-8 payloads; malformed UTF-8 SHALL be replaced with U+FFFD, matching gophertunnel's tolerant behaviour. Valid UTF-8 decoded into `BedrockString` from a `Bytes` input SHALL share the input allocation. A declared length larger than the remaining input MUST fail with `StringLengthExceeded`.

#### Scenario: Invalid UTF-8 is normalized
- **WHEN** a string field carrying the bytes `61 FF 62` is decoded and re-encoded
- **THEN** decoding succeeds and the re-encoded payload is `61 EF BF BD 62` with a length prefix of 5

#### Scenario: Valid UTF-8 shares input
- **WHEN** a valid UTF-8 string is decoded from a `Bytes` buffer
- **THEN** the decoded string's data pointer lies inside the input buffer

#### Scenario: Truncated string
- **WHEN** a string declares a length greater than the bytes remaining
- **THEN** decoding returns `StringLengthExceeded`

Evidence: crates/valentine/bedrock_core/src/bedrock/codec.rs (`decode_utf8_lossy_owned`, `decode_utf8_lossy_bytes`, tests `shared_text_handles_utf8_and_lossy_segment_boundaries`, `string_decode_insufficient_buffer`), crates/valentine/tests/borrowed_typed_coverage.rs (`owned_text_shares_valid_utf8_and_normalizes_invalid_utf8`)

### Requirement: Collection preallocation bounded by remaining input
Collection decoders SHALL check a declared count against the remaining input before reserving storage when the element's `BedrockCodec::MIN_ENCODED_SIZE` is positive, failing with `ArrayLengthExceeded` if `count * MIN_ENCODED_SIZE` exceeds the remaining bytes or overflows. Elements with a zero (unknown) minimum SHALL grow storage fallibly as items decode, and zero-byte elements MUST stay decodable. `MIN_ENCODED_SIZE` SHALL default to zero.

#### Scenario: Impossible count is rejected before allocating
- **WHEN** a `Vec<u8>` declares 1,000,000 elements but only one byte follows
- **THEN** decoding returns `ArrayLengthExceeded` without reserving storage for the declared count

#### Scenario: Generated owned and borrowed arrays apply the same bound
- **WHEN** a generated skin image declares a `u32::MAX` byte count followed by one byte
- **THEN** both the owned type and its borrowed view return `ArrayLengthExceeded`

#### Scenario: Zero-size elements
- **WHEN** `vec![(); 5]` is encoded and decoded
- **THEN** the round trip succeeds

Evidence: crates/valentine/bedrock_core/src/bedrock/codec.rs (`prepare_decode_vec`, `reserve_decode_item`, `impl BedrockCodec for Vec<T>`, tests `generic_vec_rejects_impossible_wire_count`, `zero_width_collection_values_remain_decodable`), crates/valentine/tests/decode_safety.rs (`owned_and_borrowed_arrays_reject_impossible_lengths`)

### Requirement: Recursive value depth budget
Generated recursive types SHALL enter a shared, thread-local budget of `MAX_RECURSIVE_VALUE_DEPTH` (64) active recursive value decodes via `RecursiveDecodeGuard`, and exceeding it MUST fail with `RecursiveDepthExceeded`. The budget SHALL be restored when a guarded decode returns successfully, returns an error, or unwinds. Borrowed typed views of recursive values SHALL enforce the same budget during borrowed decoding, before any conversion to owned packets.

#### Scenario: Excessive nesting is rejected
- **WHEN** a `CerealDynamicValue` list or map nested 128 levels deep is decoded
- **THEN** decoding returns `RecursiveDepthExceeded`

#### Scenario: Budget is restored after failure
- **WHEN** a shallow nested value is decoded on the same thread after a depth failure
- **THEN** it decodes successfully and re-encodes to the original bytes

#### Scenario: Borrowed packet decode
- **WHEN** a borrowed MCPE packet whose payload contains an over-deep recursive value is decoded
- **THEN** borrowed decoding returns `RecursiveDepthExceeded`

Evidence: crates/valentine/bedrock_core/src/bedrock/codec.rs (`MAX_RECURSIVE_VALUE_DEPTH`, `RecursiveDecodeGuard`), crates/valentine/tests/decode_safety.rs (`recursive_values_reject_excessive_depth_and_restore_budget`, `borrowed_packet_decode_enforces_recursive_budget`)

### Requirement: NBT scanning over segmented buffers
The codec SHALL decode `Nbt` (network little-endian, rooted), `Nbt::decode_little_endian` (fixed-width little-endian) and `NbtCompoundBody` (root-less compound children ending in TAG_End) by validating the structure and retaining the exact wire bytes. Decoding MUST succeed when the value spans several `Buf` chunks and MUST consume only that value's bytes. Nesting deeper than 512 containers MUST fail with `NbtDepthExceeded`, and unknown tags with `UnknownNbtTag`.

#### Scenario: Split value does not consume the next field
- **WHEN** a rooted NBT value is decoded from a chained buffer split at any byte offset and followed by one unrelated byte
- **THEN** the decoded bytes equal the original value and exactly one byte remains unread

#### Scenario: Depth limit
- **WHEN** an NBT value with 512 nested containers and one with 513 are decoded
- **THEN** the first succeeds and the second returns `NbtDepthExceeded`

#### Scenario: Compound body reads every child
- **WHEN** a `NbtCompoundBody` with two named children and TAG_End is decoded and then wrapped with `to_nbt`
- **THEN** both children are retained and the wrapped bytes decode as a rooted `Nbt` with nothing remaining

Evidence: crates/valentine/bedrock_core/src/bedrock/codec.rs (`decode_nbt_bytes`, `NbtRecordingBuf`, `MAX_NBT_DEPTH`, tests `nbt_decodes_segmented_buffers_without_consuming_following_value`, `compound_body_reads_all_children_up_to_tag_end`, `network_nbt_allows_512_nested_containers_and_rejects_513`, `nbt_depth_limit_counts_lists`), crates/valentine/tests/capture_conformance.rs

### Requirement: MCPE frame layout
Generated version crates SHALL frame game packets as `[0xFE] [var-u32 length] [var-u32 header] [body]`, where the header packs the packet ID in bits 0–9, the sender sub-client in bits 10–11 and the target sub-client in bits 12–13. Decoding MUST fail with `InvalidMagicByte` for a leading byte other than `0xFE`, with `PacketLengthExceeded` when the declared length exceeds the available input, and with `InvalidPacketId` for an ID that names no packet.

#### Scenario: Wrong marker
- **WHEN** a game frame starting with a byte other than `0xFE` is decoded
- **THEN** decoding returns `InvalidMagicByte { expected: 0xfe, .. }`

#### Scenario: Consecutive frames
- **WHEN** two encoded frames are concatenated and the first is decoded
- **THEN** the remaining input equals exactly the second frame, which then decodes to the same packet

Evidence: crates/valentine/bedrock_versions/v1_26_51/src/mcpe.rs (`decode_game_frame`, `decode_inner`, `decode_bounded_payload`), crates/valentine/bedrock_core/src/bedrock/borrowed.rs (`RawMcpeFrame::decode`), crates/valentine/tests/bedrock_roundtrip.rs (`owned_mcpe_frame_leaves_next_frame_unconsumed`)

### Requirement: Owned frame decoding rejects trailing bytes
Owned MCPE packet decoding SHALL fail with `TrailingPacketBytes { remaining }` when the typed packet decoder does not consume its entire declared payload, with no packet-specific exceptions. The check MUST apply to every owned entry point (generic `Buf`, `Bytes`, and `BorrowedReader` paths) and MUST NOT count bytes that belong to subsequent batch entries.

#### Scenario: Padded frame
- **WHEN** a valid `DisconnectPacket` frame has its declared length increased by 3 and 3 extra bytes appended to its payload
- **THEN** owned decoding returns `TrailingPacketBytes { remaining: 3 }`

Evidence: crates/valentine/bedrock_versions/v1_26_51/src/mcpe.rs (`decode_small_payload`), crates/valentine_gen/src/generator/mod.rs, crates/valentine/tests/bedrock_roundtrip.rs (`owned_mcpe_frame_rejects_trailing_payload_bytes`)

### Requirement: Shared-input entries are consumed atomically
Once an entry's declared length has been validated, the shared-input entry points (`decode_inner_bytes`, `decode_inner_reader`, and the borrowed `decode_inner_with_remaining`/`decode_inner_reader`) SHALL advance past the complete entry even when its header or payload is rejected, so the next entry stays decodable. An invalid length prefix SHALL consume only the prefix.

#### Scenario: Malformed entry followed by a valid one
- **WHEN** a batch holds a truncated `DisconnectPacket` entry followed by a valid entry and the first is decoded
- **THEN** decoding returns an error, the cursor is positioned at the start of the valid entry, and decoding again yields the valid packet with no input left

Evidence: crates/valentine/bedrock_versions/v1_26_51/src/mcpe.rs (`decode_inner_bytes`, `decode_inner_reader`), crates/valentine/bedrock_versions/v1_26_51/src/borrowed.rs (`BorrowedMcpePacket::decode_entry`), crates/valentine/tests/borrowed_decode.rs (`shared_frame_errors_preserve_the_next_entry`)

### Requirement: Borrowed decoding
Borrowed decoding (`BorrowedReader`, `BorrowedStr`, generated `Borrowed*Packet` views) SHALL slice `Bytes` input without copying; retained fields share the source allocation. `BorrowedStr` SHALL keep raw wire bytes, including invalid UTF-8, until owned conversion applies lossy normalization. Borrowed MCPE decoding SHALL report, not reject, unconsumed payload bytes, and `into_owned` MUST equal owned decoding of the same input.

#### Scenario: Bounded frame cursor
- **WHEN** `BorrowedReader::take_frame(2)` is taken over a 4-byte source and one byte is retained via `copy_to_bytes`
- **THEN** the retained bytes point into the source allocation, the frame exposes no bytes of the outer reader, and the outer reader still holds the remaining 2 bytes

#### Scenario: Borrowed and owned agree
- **WHEN** a corpus batch is decoded both with `McpePacketData::decode_inner_reader` and `BorrowedMcpePacket::decode_inner_reader`
- **THEN** every borrowed entry reports 0 remaining bytes, is fully typed (no raw payload), and materializes to the same header and data as the owned decode

Evidence: crates/valentine/bedrock_core/src/bedrock/borrowed.rs (test `bounded_reader_shares_fields_without_exposing_the_next_frame`), crates/valentine/tests/borrowed_decode.rs, crates/valentine/tests/borrowed_typed_coverage.rs (`corpus_packets_decode_as_typed_views_and_match_owned`, `borrowed_union_strings_share_input_and_convert_lossily`)

### Requirement: Union and enum discriminant handling
Generated unions SHALL encode their control value followed by the selected variant's payload and MUST reject an unknown control value with `InvalidEnumValue` and a truncated control or payload with an error, in both owned and borrowed decoding. Generated scalar enums SHALL be open: an unrecognised wire value SHALL decode into an `Unknown` variant that carries the raw value and re-encodes it unchanged.

#### Scenario: Unknown union control
- **WHEN** a `DisconnectPacket` body carries union control value `2`
- **THEN** owned decoding and the `DisconnectPacketMessagesUnionView` borrowed decoding both fail

#### Scenario: Every union variant round-trips
- **WHEN** each `DisconnectPacketMessagesUnion` variant is encoded
- **THEN** the bytes match the protocolgen contract crate's encoding and decode back to the same variant

Evidence: crates/valentine/tests/union_conformance.rs, crates/valentine/tests/borrowed_typed_coverage.rs (`borrowed_union_rejects_unknown_controls_and_truncated_payloads`), crates/valentine/bedrock_versions/v1_26_51/src/types.rs

### Requirement: Encode/decode round-trip fidelity
Decoding a valid packet encoding and re-encoding it SHALL reproduce the original bytes, and `BedrockSized::encoded_size` SHALL equal the bytes `encode` writes. Captured BDS fixtures and the protocolgen corpus MUST round-trip byte-for-byte. Binary buffer fields (`bytes::Bytes`) MUST round-trip every byte value and SHALL share the input allocation when decoded from `Bytes`.

#### Scenario: Captured packets
- **WHEN** each captured `LevelEventGenericPacket` fixture is decoded and re-encoded
- **THEN** the re-encoded frame equals the captured bytes

#### Scenario: Binary chunk payload
- **WHEN** a `LevelChunkPacket` payload containing every byte value 0–255 is encoded and decoded
- **THEN** the payload is unchanged

#### Scenario: Frame length prefix width change
- **WHEN** adjacent entries are encoded into one buffer where an entry's length prefix needs more than one byte
- **THEN** each entry decodes correctly and adjacent entries are not corrupted

Evidence: crates/valentine/tests/capture_conformance.rs, crates/valentine/tests/binary_buffer_roundtrip.rs, crates/valentine/tests/bedrock_roundtrip.rs, crates/valentine/tests/start_game_roundtrip.rs, crates/valentine/tests/borrowed_typed_coverage.rs

### Requirement: Decode error taxonomy
All decode failures SHALL be reported as a `DecodeError` variant (defined in `bedrock_core/src/bedrock/error.rs`) naming the failure class, and decoding malformed input MUST return an error rather than panic. Converting a `DecodeError` into `std::io::Error` SHALL map `UnexpectedEof` to `ErrorKind::UnexpectedEof` and every other variant to `ErrorKind::InvalidData`.

#### Scenario: Truncated packet body
- **WHEN** a `DisconnectPacket` payload is truncated mid-field
- **THEN** decoding returns a `DecodeError` and does not panic

#### Scenario: io::Error mapping
- **WHEN** a `DecodeError::TrailingPacketBytes` is converted into `std::io::Error`
- **THEN** its kind is `InvalidData`

Evidence: crates/valentine/bedrock_core/src/bedrock/error.rs, crates/valentine/tests/bedrock_roundtrip.rs (`packet_disconnect_rejects_truncated_payload`)

### Requirement: Bounded continuation bitsets
The codec SHALL encode fixed-width bitsets as seven payload bits per continuation byte, emitting a single `00` byte for an empty set, and SHALL reject on encode any set bit outside the declared width. Decoding MUST fail with `InvalidBitset` when a payload byte carries bits beyond the declared width or a continuation extends past it.

#### Scenario: Empty bitset
- **WHEN** an all-zero bitset is encoded
- **THEN** the output is the single byte `00`

#### Scenario: Excess bits
- **WHEN** a bitset byte sets a bit beyond the declared width
- **THEN** decoding returns `InvalidBitset`

Evidence: crates/valentine/bedrock_core/src/bedrock/codec.rs (`encode_bitset`, `decode_bitset`, tests `continuation_bitset_round_trips_declared_boundary_bits`, `continuation_bitset_zero_is_one_byte_and_rejects_excess_bits`)

### Requirement: Versioned public surface
`valentine` SHALL expose each generated version crate behind a Cargo feature named `bedrock_<major>_<minor>_<patch>`, re-exported at `valentine::bedrock::protocol::vX_Y_Z` and `valentine::bedrock::version::vX_Y_Z`, the latter also carrying `GAME_VERSION`, `PROTOCOL_VERSION` and `INFO`. The default feature SHALL be `bedrock_1_26_51`, exposing game version `1.26.51` and network protocol `2193`.

#### Scenario: Default version metadata
- **WHEN** `valentine` is built with default features
- **THEN** `valentine::bedrock::version::v1_26_51::PROTOCOL_VERSION` is `2193` and `GAME_VERSION` is `"1.26.51"`

Evidence: crates/valentine/Cargo.toml, crates/valentine/src/bedrock/version.rs, crates/valentine/src/bedrock/protocol/mod.rs, crates/valentine/tests/version_support.rs

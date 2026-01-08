# Comprehensive Testing Strategy for axolotl-stack

## Overview

This plan establishes a testing strategy for the axolotl-stack Minecraft Bedrock server monorepo. The goal is to improve stability, correctness, and maintainability through systematic unit tests, integration tests, and optional mutation testing.

## Current State Analysis

### Workspace Structure (17 crates)

| Crate | Purpose | Current Tests | Priority |
|-------|---------|---------------|----------|
| **tokio-raknet** | RakNet UDP protocol | 2 integration tests, ~30 inline unit tests | **HIGH** |
| **valentine** | Bedrock protocol codec | 2 test files (roundtrip tests) | **HIGH** |
| **valentine_bedrock_core** | Core codec traits | None | **HIGH** |
| **valentine_bedrock_1_21_130** | Protocol v1.21.130 | None (generated code) | MEDIUM |
| **jolyne** | Connection layer (auth, encryption) | ~4 inline tests | **HIGH** |
| **tokio-nethernet** | WebRTC/NetherNet transport | 2 test files | **HIGH** |
| **axolotl-xbl** | Xbox Live API client | 1 test file (7 tests) | **HIGH** |
| **unastar** | Main server (ECS, world, entities) | ~20 inline tests, 1 bench | **CRITICAL** |
| **unastar_noise** | World generation noise | ~15 inline tests | HIGH |
| **unastar-data** | Generated game data | None | LOW |
| **unastar-api** | Plugin API | None | MEDIUM |
| **unastar-api-macros** | Proc macros | None | MEDIUM |
| **valentine_gen** | Protocol code generator | None | MEDIUM |
| **axelerator** | Xbox friends relay server | None | MEDIUM |
| **bds-extractor** | BDS data extractor | None | LOW |
| **example-plugin** | Example WASM plugin | None | LOW |

### Key Discoveries

1. **Existing Test Patterns** - The codebase has excellent roundtrip test patterns in `valentine/tests/` that should be extended
2. **Inline Tests** - Many crates have `#[cfg(test)]` modules with good coverage of protocol types
3. **Benchmarks** - `tokio-raknet` has 7 benchmarks, `unastar` has 1, `tokio-nethernet` has 2
4. **No Mocking** - Currently no mock infrastructure for network/database testing
5. **No CI Testing** - No visible GitHub Actions or CI configuration

## Desired End State

After implementing this plan:
- Each crate has unit tests for core logic with >80% line coverage
- Integration tests verify end-to-end flows (client-server handshake, world generation)
- Mutation testing identifies weak spots in critical paths
- CI runs tests on every PR
- Mocking infrastructure enables isolated testing of network/database code

### Verification

```bash
# All tests pass
cargo test --workspace

# Benchmarks run without errors
cargo bench --workspace

# Mutation testing on critical crates
cargo mutants -p tokio-raknet -p valentine -p jolyne --timeout 120

# Coverage report (using cargo-tarpaulin or llvm-cov)
cargo tarpaulin --workspace --out Html
```

## What We're NOT Doing

- Testing generated code in `valentine_bedrock_1_21_130` (generated from protocol.json)
- Testing generated code in `unastar-data` (generated game data)
- 100% mutation kill rate (aim for 70%+ on critical code)
- Property-based testing (future enhancement)
- Fuzzing (future enhancement, but highly recommended for protocol parsing)

---

# Phase 1: tokio-raknet Testing (Priority: HIGH)

## Overview
RakNet is the foundation of the networking stack. Bugs here affect everything.

## Current State
- **Existing**: 2 integration tests (`integration_handshake.rs`, `reproduce_handshake_bug.rs`)
- **Inline tests**: ~30 tests across protocol types
- **Benchmarks**: 7 benchmarks for codec, ack, split, varint, etc.

## Changes Required

### 1.1 Protocol Type Unit Tests

**File**: `crates/tokio-raknet/src/protocol/types/mod.rs` (new test module)

```rust
// Example: Add to src/protocol/types/sequence.rs

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::{BytesMut, Buf};

    #[test]
    fn sequence_number_wrapping() {
        // Sequence numbers are 24-bit, should wrap at 2^24
        let max = SequenceNumber(0xFFFFFF);
        let next = max.wrapping_add(1);
        assert_eq!(next.0, 0);
    }

    #[test]
    fn sequence_number_comparison_handles_wrap() {
        // When sequences wrap, comparison should still work
        let old = SequenceNumber(0xFFFFFE);
        let new = SequenceNumber(0x000002);
        assert!(new.is_newer_than(old));
    }

    #[test]
    fn sequence_roundtrip() {
        let seq = SequenceNumber(0x123456);
        let mut buf = BytesMut::new();
        seq.encode(&mut buf);

        let decoded = SequenceNumber::decode(&mut buf.freeze()).unwrap();
        assert_eq!(seq, decoded);
    }
}
```

**File**: `crates/tokio-raknet/src/protocol/types/varint.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_boundary_values() {
        // Test VarInt encoding at size boundaries
        let test_cases = [
            (0u32, 1),      // 1 byte
            (127, 1),       // max 1 byte
            (128, 2),       // min 2 bytes
            (16383, 2),     // max 2 bytes
            (16384, 3),     // min 3 bytes
            (u32::MAX, 5),  // max size
        ];

        for (value, expected_bytes) in test_cases {
            let mut buf = BytesMut::new();
            VarInt(value).encode(&mut buf);
            assert_eq!(buf.len(), expected_bytes, "VarInt({}) should be {} bytes", value, expected_bytes);

            // Roundtrip
            let decoded = VarInt::decode(&mut buf.freeze()).unwrap();
            assert_eq!(decoded.0, value);
        }
    }

    #[test]
    fn varint_rejects_overlong_encoding() {
        // 5 bytes encoding a small number should be rejected or normalized
        let overlong = &[0x80, 0x80, 0x80, 0x80, 0x00]; // Encodes 0 in 5 bytes
        // Depending on implementation, this should either decode to 0 or error
    }
}
```

### 1.2 Session State Machine Tests

**File**: `crates/tokio-raknet/src/session/tests.rs` (new file)

```rust
//! Unit tests for RakNet session state machine

use super::*;

/// Test SplitAssembler correctly reassembles fragmented packets
#[test]
fn split_assembler_reassembles_in_order() {
    let mut assembler = SplitAssembler::new(1000); // 1KB max

    let split_id = 42u16;
    let total_parts = 3u32;

    // Receive parts in order
    assembler.insert(split_id, 0, total_parts, Bytes::from("Hello "));
    assert!(assembler.try_assemble(split_id).is_none());

    assembler.insert(split_id, 1, total_parts, Bytes::from("World"));
    assert!(assembler.try_assemble(split_id).is_none());

    assembler.insert(split_id, 2, total_parts, Bytes::from("!"));
    let complete = assembler.try_assemble(split_id).expect("Should be complete");
    assert_eq!(complete, Bytes::from("Hello World!"));
}

#[test]
fn split_assembler_reassembles_out_of_order() {
    let mut assembler = SplitAssembler::new(1000);
    let split_id = 42u16;
    let total_parts = 3u32;

    // Receive parts out of order
    assembler.insert(split_id, 2, total_parts, Bytes::from("!"));
    assembler.insert(split_id, 0, total_parts, Bytes::from("Hello "));
    assembler.insert(split_id, 1, total_parts, Bytes::from("World"));

    let complete = assembler.try_assemble(split_id).expect("Should be complete");
    assert_eq!(complete, Bytes::from("Hello World!"));
}

#[test]
fn split_assembler_rejects_oversized() {
    let mut assembler = SplitAssembler::new(10); // 10 byte limit
    let split_id = 1u16;

    // Try to insert packet that would exceed limit
    let large_data = Bytes::from(vec![0u8; 20]);
    let result = assembler.try_insert(split_id, 0, 1, large_data);
    assert!(result.is_err());
}

/// Test ACK queue coalescing
#[test]
fn ack_queue_coalesces_ranges() {
    let mut queue = AckQueue::new();

    // Add consecutive sequence numbers
    queue.add(SequenceNumber(1));
    queue.add(SequenceNumber(2));
    queue.add(SequenceNumber(3));
    queue.add(SequenceNumber(5)); // Gap
    queue.add(SequenceNumber(6));

    let ranges = queue.drain_ranges();
    // Should produce [1-3, 5-6] not [1,2,3,5,6]
    assert_eq!(ranges.len(), 2);
    assert_eq!(ranges[0], (SequenceNumber(1), SequenceNumber(3)));
    assert_eq!(ranges[1], (SequenceNumber(5), SequenceNumber(6)));
}

/// Test ReliableTracker retry behavior
#[test]
fn reliable_tracker_marks_for_retry() {
    let mut tracker = ReliableTracker::new();
    let now = Instant::now();

    // Send packet
    let seq = SequenceNumber(1);
    tracker.track_send(seq, Bytes::from("data"), now);

    // Before timeout, no retries
    let retries = tracker.get_retries(now + Duration::from_millis(100));
    assert!(retries.is_empty());

    // After timeout, should retry
    let retries = tracker.get_retries(now + Duration::from_secs(2));
    assert_eq!(retries.len(), 1);
}

#[test]
fn reliable_tracker_stops_retry_after_ack() {
    let mut tracker = ReliableTracker::new();
    let now = Instant::now();

    let seq = SequenceNumber(1);
    tracker.track_send(seq, Bytes::from("data"), now);

    // ACK the packet
    tracker.acknowledge(seq);

    // Should not retry after ack
    let retries = tracker.get_retries(now + Duration::from_secs(10));
    assert!(retries.is_empty());
}
```

### 1.3 Packet Roundtrip Tests

**File**: `crates/tokio-raknet/src/protocol/packet/tests.rs` (new file)

```rust
//! Roundtrip tests for all RakNet packet types

use super::*;
use bytes::BytesMut;

fn roundtrip<T: RaknetPacket>(packet: T) {
    let mut buf = BytesMut::new();
    packet.encode(&mut buf);
    let encoded = buf.freeze();

    let mut reader = encoded.clone();
    let decoded = T::decode(&mut reader).expect("decode failed");

    // Re-encode and compare bytes
    let mut buf2 = BytesMut::new();
    decoded.encode(&mut buf2);
    assert_eq!(encoded, buf2.freeze(), "roundtrip produced different bytes");
}

#[test]
fn open_connection_request1_roundtrip() {
    roundtrip(OpenConnectionRequest1 {
        magic: DEFAULT_UNCONNECTED_MAGIC,
        protocol_version: 11,
        padding: EoBPadding(1400),
    });
}

#[test]
fn open_connection_request2_roundtrip() {
    roundtrip(OpenConnectionRequest2 {
        magic: DEFAULT_UNCONNECTED_MAGIC,
        server_addr: "127.0.0.1:19132".parse().unwrap(),
        mtu: 1400,
        cookie: Some(0x12345678),
        client_proof: false,
        client_guid: 0xDEADBEEF,
    });
}

#[test]
fn connected_ping_roundtrip() {
    roundtrip(ConnectedPing {
        client_timestamp: 1234567890,
    });
}

#[test]
fn connected_pong_roundtrip() {
    roundtrip(ConnectedPong {
        client_timestamp: 1234567890,
        server_timestamp: 9876543210,
    });
}

// ... more packet types
```

### 1.4 Integration Tests

**File**: `crates/tokio-raknet/tests/reliability_test.rs` (new file)

```rust
//! Integration tests for RakNet reliability features

use bytes::Bytes;
use futures::StreamExt;
use std::time::Duration;
use tokio::time::timeout;
use tokio_raknet::{RaknetListener, RaknetStream};

/// Test that reliable packets are delivered even with simulated packet loss
#[tokio::test]
async fn reliable_delivery_under_packet_loss() {
    let mut listener = RaknetListener::bind("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr();

    let server = tokio::spawn(async move {
        let mut conn = listener.accept().await.unwrap();
        let mut received = Vec::new();

        while let Some(Ok(packet)) = conn.next().await {
            received.push(packet);
            if received.len() == 10 {
                break;
            }
        }
        received
    });

    let client = tokio::spawn(async move {
        let mut conn = RaknetStream::connect(addr).await.unwrap();

        // Send 10 reliable packets
        for i in 0..10 {
            conn.send_reliable(Bytes::from(format!("packet-{}", i))).await.unwrap();
        }

        // Keep connection alive for retries
        tokio::time::sleep(Duration::from_secs(2)).await;
    });

    let (received, _) = tokio::join!(server, client);
    let received = received.unwrap();

    // All 10 packets should arrive
    assert_eq!(received.len(), 10);
    for (i, packet) in received.iter().enumerate() {
        assert_eq!(*packet, Bytes::from(format!("packet-{}", i)));
    }
}

/// Test that ordered packets arrive in order
#[tokio::test]
async fn ordered_delivery_preserves_order() {
    let mut listener = RaknetListener::bind("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr();

    let server = tokio::spawn(async move {
        let mut conn = listener.accept().await.unwrap();
        let mut received = Vec::new();

        for _ in 0..100 {
            if let Some(Ok(packet)) = conn.next().await {
                received.push(packet);
            }
        }
        received
    });

    let client = tokio::spawn(async move {
        let mut conn = RaknetStream::connect(addr).await.unwrap();

        // Send 100 ordered packets
        for i in 0..100u32 {
            conn.send_ordered(Bytes::from(i.to_be_bytes().to_vec()), 0).await.unwrap();
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    });

    let (received, _) = tokio::join!(server, client);
    let received = received.unwrap();

    // Verify order
    for (i, packet) in received.iter().enumerate() {
        let value = u32::from_be_bytes(packet[..4].try_into().unwrap());
        assert_eq!(value, i as u32, "Packet {} out of order", i);
    }
}

/// Test MTU negotiation
#[tokio::test]
async fn mtu_negotiation() {
    let mut listener = RaknetListener::bind("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr();

    let server = tokio::spawn(async move {
        let conn = listener.accept().await.unwrap();
        conn.mtu()
    });

    let client = tokio::spawn(async move {
        let conn = RaknetStream::connect(addr).await.unwrap();
        conn.mtu()
    });

    let (server_mtu, client_mtu) = tokio::join!(server, client);

    // Both should agree on MTU
    assert_eq!(server_mtu.unwrap(), client_mtu.unwrap());
    // MTU should be reasonable
    assert!(server_mtu.unwrap() >= 500);
    assert!(server_mtu.unwrap() <= 1500);
}
```

## Success Criteria

### Automated Verification
- [x] `cargo test -p tokio-raknet` passes with 0 failures (110 tests passing)
- [ ] `cargo clippy -p tokio-raknet` has no warnings (pre-existing warning in open_connection.rs)
- [ ] New tests increase line coverage to >70%

### Manual Verification
- [ ] Connect a real Minecraft client through RakNet
- [ ] Large packets (>MTU) are correctly fragmented and reassembled
- [ ] Connection survives temporary network interruption

---

# Phase 2: valentine Protocol Testing (Priority: HIGH)

## Overview
Valentine handles all Minecraft Bedrock protocol serialization. Every packet must roundtrip correctly.

## Current State
- **Existing**: `tests/bedrock_roundtrip.rs`, `tests/start_game_roundtrip.rs`
- **Pattern**: Good `assert_roundtrip<T>()` helper

## Changes Required

### 2.1 Extend Roundtrip Tests for All Packets

**File**: `crates/valentine/tests/all_packets_roundtrip.rs` (new file)

```rust
//! Comprehensive roundtrip tests for all packet types
//!
//! This file tests that every packet type can be encoded and decoded
//! without data loss.

use bytes::{Buf, BytesMut};
use valentine::bedrock::codec::BedrockCodec;
use valentine::bedrock::protocol::v1_21_130::*;

/// Generic roundtrip test helper
fn assert_roundtrip<T>(value: T, args: T::Args)
where
    T: BedrockCodec + PartialEq + std::fmt::Debug,
    T::Args: Clone,
{
    let mut buf = BytesMut::new();
    value.encode(&mut buf).expect("encode failed");
    let encoded = buf.freeze();

    assert!(!encoded.is_empty(), "encoded packet is empty");

    let mut reader = encoded.clone();
    let decoded = T::decode(&mut reader, args).expect("decode failed");

    assert_eq!(value, decoded, "roundtrip changed value");
    assert!(!reader.has_remaining(), "trailing bytes: {}", reader.remaining());
}

// ============================================================================
// Login/Handshake Packets
// ============================================================================

#[test]
fn login_packet_roundtrip() {
    let packet = LoginPacket {
        protocol_version: 712,
        tokens: LoginTokens {
            identity: "eyJ...".to_string(), // Minimal valid JWT structure
            client: "eyJ...".to_string(),
        },
    };
    assert_roundtrip(packet, ());
}

#[test]
fn server_to_client_handshake_roundtrip() {
    let packet = ServerToClientHandshakePacket {
        jwt: "eyJ...signed_jwt...".to_string(),
    };
    assert_roundtrip(packet, ());
}

#[test]
fn client_to_server_handshake_roundtrip() {
    let packet = ClientToServerHandshakePacket {};
    assert_roundtrip(packet, ());
}

#[test]
fn play_status_roundtrip() {
    for status in [
        PlayStatusPacketStatus::LoginSuccess,
        PlayStatusPacketStatus::FailedClient,
        PlayStatusPacketStatus::FailedSpawn,
        PlayStatusPacketStatus::PlayerSpawn,
        PlayStatusPacketStatus::FailedInvalidTenant,
        PlayStatusPacketStatus::FailedVanillaEdu,
        PlayStatusPacketStatus::FailedIncompatible,
        PlayStatusPacketStatus::FailedServerFull,
    ] {
        let packet = PlayStatusPacket { status };
        assert_roundtrip(packet, ());
    }
}

// ============================================================================
// World Packets
// ============================================================================

#[test]
fn level_chunk_roundtrip_empty() {
    let packet = LevelChunkPacket {
        chunk_position: ChunkPos { x: 0, z: 0 },
        dimension_id: 0,
        sub_chunk_count: 0,
        cache_enabled: false,
        blob_ids: vec![],
        payload: bytes::Bytes::new(),
    };
    assert_roundtrip(packet, ());
}

#[test]
fn level_chunk_roundtrip_with_data() {
    let packet = LevelChunkPacket {
        chunk_position: ChunkPos { x: 100, z: -50 },
        dimension_id: 0,
        sub_chunk_count: 16,
        cache_enabled: true,
        blob_ids: vec![0x123456789ABCDEF0, 0xFEDCBA9876543210],
        payload: bytes::Bytes::from(vec![1, 2, 3, 4, 5]),
    };
    assert_roundtrip(packet, ());
}

#[test]
fn set_time_roundtrip() {
    let packet = SetTimePacket { time: 12000 }; // Noon
    assert_roundtrip(packet, ());
}

// ============================================================================
// Entity Packets
// ============================================================================

#[test]
fn add_player_roundtrip() {
    let packet = AddPlayerPacket {
        uuid: uuid::Uuid::new_v4(),
        username: "TestPlayer".to_string(),
        runtime_entity_id: 12345,
        platform_chat_id: "".to_string(),
        position: Vec3F { x: 0.0, y: 64.0, z: 0.0 },
        velocity: Vec3F { x: 0.0, y: 0.0, z: 0.0 },
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        held_item: ItemStack::empty(),
        gamemode: GameMode::Survival,
        metadata: EntityMetadata::default(),
        // ... other fields with defaults
    };
    assert_roundtrip(packet, ());
}

#[test]
fn move_player_roundtrip() {
    for mode in [
        MovePlayerPacketMode::Normal,
        MovePlayerPacketMode::Reset,
        MovePlayerPacketMode::Teleport,
        MovePlayerPacketMode::Rotation,
    ] {
        let packet = MovePlayerPacket {
            runtime_entity_id: 1,
            position: Vec3F { x: 100.5, y: 65.0, z: -200.25 },
            pitch: 45.0,
            yaw: 90.0,
            head_yaw: 90.0,
            mode,
            on_ground: true,
            riding_runtime_entity_id: 0,
            tick: 1000,
        };
        assert_roundtrip(packet, ());
    }
}

// ============================================================================
// Inventory Packets
// ============================================================================

#[test]
fn inventory_content_roundtrip_empty() {
    let packet = InventoryContentPacket {
        window_id: 0,
        items: vec![],
        // ...
    };
    assert_roundtrip(packet, ());
}

#[test]
fn inventory_content_roundtrip_with_items() {
    let packet = InventoryContentPacket {
        window_id: 0,
        items: vec![
            ItemStack::new(1, 64, 0), // Stone x64
            ItemStack::empty(),
            ItemStack::new(4, 32, 0), // Cobblestone x32
        ],
    };
    assert_roundtrip(packet, ());
}

// ============================================================================
// Command Packets
// ============================================================================

#[test]
fn available_commands_roundtrip() {
    let packet = AvailableCommandsPacket {
        commands: vec![
            Command {
                name: "help".to_string(),
                description: "Shows help".to_string(),
                flags: 0,
                permission_level: 0,
                aliases: vec![],
                overloads: vec![],
            },
            Command {
                name: "gamemode".to_string(),
                description: "Changes gamemode".to_string(),
                flags: 0,
                permission_level: 1,
                aliases: vec!["gm".to_string()],
                overloads: vec![
                    CommandOverload {
                        parameters: vec![
                            CommandParameter {
                                name: "mode".to_string(),
                                param_type: CommandParamType::Int,
                                optional: false,
                            },
                        ],
                    },
                ],
            },
        ],
        // ...
    };
    assert_roundtrip(packet, ());
}

// ============================================================================
// Edge Cases & Error Conditions
// ============================================================================

#[test]
fn text_packet_all_types() {
    for typ in [
        TextPacketType::Raw,
        TextPacketType::Chat,
        TextPacketType::Translation,
        TextPacketType::Popup,
        TextPacketType::Jukebox,
        TextPacketType::Tip,
        TextPacketType::System,
        TextPacketType::Whisper,
        TextPacketType::Announcement,
        TextPacketType::Json,
    ] {
        let packet = TextPacket {
            type_: typ,
            needs_translation: false,
            // ... appropriate content for type
        };
        assert_roundtrip(packet, ());
    }
}

#[test]
fn varint_edge_cases() {
    use valentine::bedrock::codec::VarInt;

    // Test boundary values
    for value in [0, 1, 127, 128, 16383, 16384, i32::MAX] {
        assert_roundtrip(VarInt(value), ());
    }
}

#[test]
fn string_with_unicode() {
    use valentine::bedrock::codec::BedrockString;

    let strings = [
        "",
        "hello",
        "Hello, World!",
        "emoji: 😀🎮⛏️",
        "中文测试",
        "日本語テスト",
        "mixed: Hello世界🌍",
        "a".repeat(1000), // Long string
    ];

    for s in strings {
        assert_roundtrip(BedrockString(s.to_string()), ());
    }
}
```

### 2.2 Codec Trait Unit Tests

**File**: `crates/valentine/bedrock_core/src/bedrock/codec_tests.rs` (new file)

```rust
//! Unit tests for BedrockCodec implementations

use super::codec::*;
use bytes::{Buf, BufMut, BytesMut};

#[test]
fn zigzag32_encoding() {
    // ZigZag encoding: 0 -> 0, -1 -> 1, 1 -> 2, -2 -> 3, 2 -> 4, ...
    let cases = [
        (0i32, 0u32),
        (-1, 1),
        (1, 2),
        (-2, 3),
        (2, 4),
        (i32::MIN, u32::MAX),
        (i32::MAX, u32::MAX - 1),
    ];

    for (signed, unsigned) in cases {
        assert_eq!(zigzag_encode(signed), unsigned);
        assert_eq!(zigzag_decode(unsigned), signed);
    }
}

#[test]
fn nbt_roundtrip() {
    use super::codec::Nbt;

    let nbt = Nbt::compound([
        ("string", Nbt::String("hello".into())),
        ("int", Nbt::Int(42)),
        ("list", Nbt::List(vec![Nbt::Int(1), Nbt::Int(2), Nbt::Int(3)])),
        ("nested", Nbt::compound([("inner", Nbt::Byte(1))])),
    ]);

    let mut buf = BytesMut::new();
    nbt.encode(&mut buf).unwrap();

    let decoded = Nbt::decode(&mut buf.freeze(), ()).unwrap();
    assert_eq!(nbt, decoded);
}

#[test]
fn block_position_encoding() {
    // BlockCoordinates use specific bit packing
    let pos = BlockCoordinates { x: 100, y: 64, z: -200 };

    let mut buf = BytesMut::new();
    pos.encode(&mut buf).unwrap();

    let decoded = BlockCoordinates::decode(&mut buf.freeze(), ()).unwrap();
    assert_eq!(pos, decoded);
}

#[test]
fn uuid_encoding() {
    let uuid = uuid::Uuid::new_v4();

    let mut buf = BytesMut::new();
    uuid.encode_bedrock(&mut buf).unwrap();

    // Bedrock UUIDs are little-endian
    assert_eq!(buf.len(), 16);

    let decoded = uuid::Uuid::decode_bedrock(&mut buf.freeze()).unwrap();
    assert_eq!(uuid, decoded);
}
```

## Success Criteria

### Automated Verification
- [ ] `cargo test -p valentine` passes
- [ ] Every packet type has at least one roundtrip test
- [ ] `cargo test -p valentine_bedrock_core` passes

### Manual Verification
- [ ] Client can complete full login sequence
- [ ] World data transfers without corruption

---

# Phase 3: jolyne Connection Layer Testing (Priority: HIGH)

## Overview
Jolyne handles authentication, encryption, and the high-level connection state machine.

## Changes Required

### 3.1 Batch Encoding/Decoding Tests

**File**: `crates/jolyne/src/batch_tests.rs` (new file)

```rust
//! Tests for packet batching and compression

use super::batch::*;
use bytes::Bytes;

#[test]
fn batch_encode_single_packet() {
    let packets = vec![Bytes::from("packet1")];
    let encoded = encode_batch(&packets, CompressionLevel::None).unwrap();

    let decoded = decode_batch(&encoded).unwrap();
    assert_eq!(decoded, packets);
}

#[test]
fn batch_encode_multiple_packets() {
    let packets = vec![
        Bytes::from("packet1"),
        Bytes::from("packet2"),
        Bytes::from("longer packet with more data"),
    ];
    let encoded = encode_batch(&packets, CompressionLevel::None).unwrap();

    let decoded = decode_batch(&encoded).unwrap();
    assert_eq!(decoded, packets);
}

#[test]
fn batch_with_compression() {
    // Create compressible data
    let packets = vec![
        Bytes::from(vec![0u8; 1000]),
        Bytes::from(vec![1u8; 1000]),
    ];

    let uncompressed = encode_batch(&packets, CompressionLevel::None).unwrap();
    let compressed = encode_batch(&packets, CompressionLevel::Default).unwrap();

    // Compressed should be smaller
    assert!(compressed.len() < uncompressed.len());

    // Both should decode to same result
    let decoded = decode_batch(&compressed).unwrap();
    assert_eq!(decoded, packets);
}

#[test]
fn batch_empty() {
    let packets: Vec<Bytes> = vec![];
    let encoded = encode_batch(&packets, CompressionLevel::None).unwrap();
    let decoded = decode_batch(&encoded).unwrap();
    assert!(decoded.is_empty());
}
```

### 3.2 Authentication Flow Tests

**File**: `crates/jolyne/src/auth/tests.rs` (new file)

```rust
//! Unit tests for authentication logic

use super::*;

#[test]
fn ecdh_key_exchange() {
    // Generate two key pairs
    let server_key = EcdhKeyPair::generate();
    let client_key = EcdhKeyPair::generate();

    // Perform key exchange
    let server_shared = server_key.derive_shared_secret(&client_key.public_key());
    let client_shared = client_key.derive_shared_secret(&server_key.public_key());

    // Both should derive the same shared secret
    assert_eq!(server_shared, client_shared);
}

#[test]
fn encryption_key_derivation() {
    let shared_secret = [0u8; 32]; // Example shared secret
    let salt = [1u8; 16];

    let key = derive_encryption_key(&shared_secret, &salt);

    // Key should be deterministic
    let key2 = derive_encryption_key(&shared_secret, &salt);
    assert_eq!(key, key2);

    // Different salt should produce different key
    let salt2 = [2u8; 16];
    let key3 = derive_encryption_key(&shared_secret, &salt2);
    assert_ne!(key, key3);
}

#[test]
fn encryption_roundtrip() {
    let key = [0u8; 32];
    let mut cipher = BedrockCipher::new(&key);

    let plaintext = b"Hello, encrypted world!";
    let encrypted = cipher.encrypt(plaintext);

    // Create new cipher with same key (reset counter)
    let mut cipher2 = BedrockCipher::new(&key);
    let decrypted = cipher2.decrypt(&encrypted).unwrap();

    assert_eq!(plaintext.as_slice(), decrypted.as_ref());
}

#[test]
fn jwt_validation() {
    // Test with mock JWT
    let jwt = create_test_jwt("test_xuid", "test_identity");

    let claims = validate_client_jwt(&jwt).unwrap();
    assert_eq!(claims.xuid, "test_xuid");
}

#[test]
fn jwt_rejects_expired() {
    let expired_jwt = create_expired_jwt();

    let result = validate_client_jwt(&expired_jwt);
    assert!(result.is_err());
}
```

### 3.3 Connection State Machine Tests

**File**: `crates/jolyne/tests/connection_flow.rs` (new file)

```rust
//! Integration tests for full connection flow

use jolyne::*;
use tokio_raknet::{RaknetListener, RaknetStream};

/// Mock client that follows the handshake protocol
struct MockClient {
    stream: BedrockStream<RaknetStream>,
}

impl MockClient {
    async fn connect(addr: std::net::SocketAddr) -> anyhow::Result<Self> {
        let raknet = RaknetStream::connect(addr).await?;
        let stream = BedrockStream::new(raknet);
        Ok(Self { stream })
    }

    async fn handshake(&mut self) -> anyhow::Result<()> {
        // Send login packet
        self.stream.send_login().await?;

        // Expect ServerToClientHandshake
        let packet = self.stream.recv().await?;
        assert!(matches!(packet, McpePacket::ServerToClientHandshake(_)));

        // Send ClientToServerHandshake
        self.stream.send_client_handshake().await?;

        // Expect PlayStatus::LoginSuccess
        let packet = self.stream.recv().await?;
        match packet {
            McpePacket::PlayStatus(p) => {
                assert!(matches!(p.status, PlayStatusPacketStatus::LoginSuccess));
            }
            _ => panic!("Expected PlayStatus"),
        }

        Ok(())
    }
}

#[tokio::test]
async fn full_handshake_flow() {
    // Start server
    let listener = BedrockListener::bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let addr = listener.local_addr();

    let server = tokio::spawn(async move {
        let conn = listener.accept().await.unwrap();
        // Server auto-handles handshake
        conn
    });

    let client = tokio::spawn(async move {
        let mut client = MockClient::connect(addr).await.unwrap();
        client.handshake().await.unwrap();
        client
    });

    let (server_conn, client_conn) = tokio::join!(server, client);

    // Both should complete successfully
    assert!(server_conn.is_ok());
    assert!(client_conn.is_ok());
}
```

## Success Criteria

### Automated Verification
- [ ] `cargo test -p jolyne` passes
- [ ] Encryption roundtrip tests pass
- [ ] Batch encoding tests pass

### Manual Verification
- [ ] Real client can authenticate
- [ ] Encryption works with official client

---

# Phase 4: unastar Server Core Testing (Priority: CRITICAL)

## Overview
Unastar is the main server application with ECS, world generation, storage, and entity systems.

## Changes Required

### 4.1 World Generation Unit Tests

**File**: `crates/unastar/src/world/generator/tests.rs` (new file)

```rust
//! Unit tests for world generation

use super::*;

#[test]
fn noise_is_deterministic() {
    let seed = 12345u64;

    let noise1 = OverworldNoise::new(seed);
    let noise2 = OverworldNoise::new(seed);

    // Same seed should produce identical noise values
    for x in -10..10 {
        for z in -10..10 {
            let v1 = noise1.sample_2d(x as f64, z as f64);
            let v2 = noise2.sample_2d(x as f64, z as f64);
            assert_eq!(v1, v2, "Noise not deterministic at ({}, {})", x, z);
        }
    }
}

#[test]
fn height_map_reasonable() {
    let gen = OverworldGenerator::new(42);

    for x in -100..100 {
        for z in -100..100 {
            let height = gen.get_height(x, z);

            // Height should be within world bounds
            assert!(height >= -64, "Height {} too low at ({}, {})", height, x, z);
            assert!(height <= 320, "Height {} too high at ({}, {})", height, x, z);
        }
    }
}

#[test]
fn biome_assignment_consistent() {
    let gen = OverworldGenerator::new(42);

    // Same coordinates should always produce same biome
    let biome1 = gen.get_biome(100, 64, 200);
    let biome2 = gen.get_biome(100, 64, 200);
    assert_eq!(biome1, biome2);
}

#[test]
fn chunk_generation_produces_valid_chunk() {
    let gen = OverworldGenerator::new(42);
    let chunk = gen.generate_chunk(0, 0);

    // Chunk should have correct dimensions
    assert_eq!(chunk.sub_chunks.len(), 24); // 384 / 16

    // Should have some non-air blocks
    let mut has_solid = false;
    for subchunk in &chunk.sub_chunks {
        for block in subchunk.blocks.iter() {
            if *block != BlockState::AIR {
                has_solid = true;
                break;
            }
        }
    }
    assert!(has_solid, "Chunk should have some solid blocks");
}

#[test]
fn flat_world_generator() {
    let gen = FlatWorldGenerator::new(vec![
        BlockState::BEDROCK,
        BlockState::DIRT,
        BlockState::DIRT,
        BlockState::GRASS_BLOCK,
    ]);

    let chunk = gen.generate_chunk(0, 0);

    // Verify layer structure
    assert_eq!(chunk.get_block(0, -64, 0), BlockState::BEDROCK);
    assert_eq!(chunk.get_block(0, -63, 0), BlockState::DIRT);
    assert_eq!(chunk.get_block(0, -62, 0), BlockState::DIRT);
    assert_eq!(chunk.get_block(0, -61, 0), BlockState::GRASS_BLOCK);
    assert_eq!(chunk.get_block(0, -60, 0), BlockState::AIR);
}
```

### 4.2 Chunk Storage Tests

**File**: `crates/unastar/src/storage/tests.rs` (new file)

```rust
//! Unit tests for chunk storage

use super::*;
use tempfile::TempDir;

#[test]
fn chunk_save_load_roundtrip() {
    let temp = TempDir::new().unwrap();
    let storage = ChunkStorage::open(temp.path()).unwrap();

    // Create test chunk
    let mut chunk = Chunk::new(0, 0);
    chunk.set_block(0, 64, 0, BlockState::STONE);
    chunk.set_block(1, 64, 1, BlockState::DIAMOND_ORE);

    // Save
    storage.save_chunk(&chunk).unwrap();

    // Load
    let loaded = storage.load_chunk(0, 0).unwrap().unwrap();

    // Verify
    assert_eq!(loaded.get_block(0, 64, 0), BlockState::STONE);
    assert_eq!(loaded.get_block(1, 64, 1), BlockState::DIAMOND_ORE);
}

#[test]
fn player_data_persistence() {
    let temp = TempDir::new().unwrap();
    let storage = PlayerStorage::open(temp.path()).unwrap();

    let uuid = uuid::Uuid::new_v4();
    let data = PlayerData {
        position: Vec3::new(100.0, 64.0, 200.0),
        rotation: Vec2::new(45.0, 90.0),
        gamemode: GameMode::Creative,
        inventory: Inventory::default(),
    };

    storage.save_player(&uuid, &data).unwrap();
    let loaded = storage.load_player(&uuid).unwrap().unwrap();

    assert_eq!(data.position, loaded.position);
    assert_eq!(data.gamemode, loaded.gamemode);
}

#[test]
fn morton_encoding_bijective() {
    use super::morton::*;

    for x in -100..100 {
        for z in -100..100 {
            let encoded = morton_encode_2d(x, z);
            let (dx, dz) = morton_decode_2d(encoded);
            assert_eq!((x, z), (dx, dz), "Morton encoding not bijective");
        }
    }
}
```

### 4.3 ECS System Tests

**File**: `crates/unastar/src/ecs/tests.rs` (new file)

```rust
//! Unit tests for ECS systems

use bevy_ecs::prelude::*;
use super::*;

#[test]
fn player_movement_system() {
    let mut world = World::new();

    // Spawn player
    let player = world.spawn((
        Transform::new(Vec3::new(0.0, 64.0, 0.0)),
        Velocity::new(Vec3::new(1.0, 0.0, 0.0)),
        Player::default(),
    )).id();

    // Run movement system
    let mut schedule = Schedule::default();
    schedule.add_systems(update_positions);
    schedule.run(&mut world);

    // Check position updated
    let transform = world.get::<Transform>(player).unwrap();
    assert!(transform.position.x > 0.0);
}

#[test]
fn chunk_loading_triggers_on_player_move() {
    let mut world = World::new();
    world.insert_resource(ChunkManager::new());

    // Spawn player at origin
    let player = world.spawn((
        Transform::new(Vec3::new(0.0, 64.0, 0.0)),
        Player::default(),
        ChunkViewDistance(8),
    )).id();

    // Run chunk loading system
    let mut schedule = Schedule::default();
    schedule.add_systems(update_player_chunks);
    schedule.run(&mut world);

    // Chunks around player should be queued
    let manager = world.resource::<ChunkManager>();
    assert!(manager.is_chunk_loaded(0, 0));
}

#[test]
fn entity_despawn_on_chunk_unload() {
    let mut world = World::new();
    world.insert_resource(ChunkManager::new());

    // Spawn entity in chunk (0, 0)
    let entity = world.spawn((
        Transform::new(Vec3::new(8.0, 64.0, 8.0)),
        ChunkPosition::new(0, 0),
        Mob::default(),
    )).id();

    // Unload chunk
    let mut manager = world.resource_mut::<ChunkManager>();
    manager.unload_chunk(0, 0);

    // Run cleanup system
    let mut schedule = Schedule::default();
    schedule.add_systems(despawn_entities_in_unloaded_chunks);
    schedule.run(&mut world);

    // Entity should be despawned
    assert!(world.get_entity(entity).is_none());
}
```

### 4.4 Inventory System Tests

**File**: `crates/unastar/src/item/tests.rs` (new file)

```rust
//! Unit tests for inventory and item handling

use super::*;

#[test]
fn item_stack_merge() {
    let mut stack1 = ItemStack::new(Item::STONE, 32);
    let stack2 = ItemStack::new(Item::STONE, 20);

    let remaining = stack1.try_merge(&stack2);

    assert_eq!(stack1.count, 52);
    assert!(remaining.is_none());
}

#[test]
fn item_stack_merge_overflow() {
    let mut stack1 = ItemStack::new(Item::STONE, 60);
    let stack2 = ItemStack::new(Item::STONE, 20);

    let remaining = stack1.try_merge(&stack2);

    assert_eq!(stack1.count, 64); // Max stack size
    assert_eq!(remaining.unwrap().count, 16); // Overflow
}

#[test]
fn item_stack_different_items_no_merge() {
    let mut stack1 = ItemStack::new(Item::STONE, 32);
    let stack2 = ItemStack::new(Item::DIRT, 20);

    let remaining = stack1.try_merge(&stack2);

    assert_eq!(stack1.count, 32); // Unchanged
    assert_eq!(remaining.unwrap().count, 20); // Full stack returned
}

#[test]
fn inventory_add_item() {
    let mut inv = Inventory::new(36); // Player inventory

    let stack = ItemStack::new(Item::DIAMOND, 10);
    let remaining = inv.add_item(stack);

    assert!(remaining.is_none());
    assert_eq!(inv.count_item(Item::DIAMOND), 10);
}

#[test]
fn inventory_add_item_to_existing() {
    let mut inv = Inventory::new(36);

    // Add initial stack
    inv.add_item(ItemStack::new(Item::STONE, 32));

    // Add more
    inv.add_item(ItemStack::new(Item::STONE, 20));

    // Should merge into existing slot
    assert_eq!(inv.count_item(Item::STONE), 52);
    assert!(inv.slots.iter().filter(|s| s.is_some()).count() == 1);
}
```

## Success Criteria

### Automated Verification
- [ ] `cargo test -p unastar` passes
- [ ] World generation produces deterministic output
- [ ] Storage roundtrip preserves all data

### Manual Verification
- [ ] Generated world looks correct in-game
- [ ] Player data persists across restarts

---

# Phase 5: axolotl-xbl Xbox Live Testing (Priority: HIGH)

## Overview
Xbox Live integration requires careful testing of cryptographic signing and API mocking.

## Changes Required

### 5.1 Mock HTTP Client

**File**: `crates/axolotl-xbl/src/testing.rs` (new file)

```rust
//! Testing utilities for axolotl-xbl

use std::collections::HashMap;
use async_trait::async_trait;

/// Mock HTTP client for testing
pub struct MockHttpClient {
    responses: HashMap<String, MockResponse>,
}

pub struct MockResponse {
    pub status: u16,
    pub body: String,
}

impl MockHttpClient {
    pub fn new() -> Self {
        Self { responses: HashMap::new() }
    }

    pub fn mock_endpoint(&mut self, url: &str, response: MockResponse) {
        self.responses.insert(url.to_string(), response);
    }
}

#[async_trait]
impl HttpClient for MockHttpClient {
    async fn post(&self, url: &str, body: &str) -> Result<Response, Error> {
        let response = self.responses.get(url)
            .ok_or(Error::NotFound)?;

        Ok(Response {
            status: response.status,
            body: response.body.clone(),
        })
    }
}
```

### 5.2 Signing Tests

**File**: `crates/axolotl-xbl/src/auth/signing_tests.rs` (new file)

```rust
//! Additional signing tests

use super::signing::*;

#[test]
fn signature_format_correct() {
    let key = SigningKeyPair::generate();
    let sig = key.sign_request("POST", "/test", "", b"body");

    let decoded = base64::decode(&sig).unwrap();

    // Check structure
    assert_eq!(decoded.len(), 76);
    assert_eq!(&decoded[0..4], &[0, 0, 0, 1]); // Version

    // Timestamp should be reasonable (within last minute)
    let timestamp = i64::from_be_bytes(decoded[4..12].try_into().unwrap());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    assert!((now - timestamp / 10000000).abs() < 60);
}

#[test]
fn signature_includes_all_components() {
    let key = SigningKeyPair::generate();

    // Different methods should produce different signatures
    let sig1 = key.sign_request("GET", "/test", "", b"body");
    let sig2 = key.sign_request("POST", "/test", "", b"body");
    assert_ne!(sig1, sig2);

    // Different paths should produce different signatures
    let sig3 = key.sign_request("POST", "/test1", "", b"body");
    let sig4 = key.sign_request("POST", "/test2", "", b"body");
    assert_ne!(sig3, sig4);
}
```

## Success Criteria

### Automated Verification
- [ ] `cargo test -p axolotl-xbl` passes
- [ ] Signing produces correct format
- [ ] Mock HTTP tests work

### Manual Verification
- [ ] Real Xbox Live authentication works
- [ ] Friends list loads correctly

---

# Phase 6: tokio-nethernet WebRTC Testing (Priority: HIGH)

## Overview
NetherNet provides WebRTC transport, requiring careful testing of signaling and data channels.

## Current State
- `tests/integration_test.rs` - Full connection test with mock signaling
- `tests/discovery_test.rs` - Discovery protocol tests

## Changes Required

### 6.1 Discovery Crypto Tests

**File**: `crates/tokio-nethernet/src/discovery/crypto_tests.rs` (new file)

```rust
//! Tests for discovery encryption

use super::crypto::*;

#[test]
fn discovery_encryption_roundtrip() {
    let key = [0u8; 16];
    let plaintext = b"discovery packet data";

    let encrypted = encrypt_discovery(plaintext, &key);
    let decrypted = decrypt_discovery(&encrypted, &key).unwrap();

    assert_eq!(plaintext.as_slice(), decrypted.as_slice());
}

#[test]
fn discovery_key_derivation() {
    let password = "test-server-123";
    let key = derive_discovery_key(password);

    // Same password should produce same key
    let key2 = derive_discovery_key(password);
    assert_eq!(key, key2);

    // Different password should produce different key
    let key3 = derive_discovery_key("different-password");
    assert_ne!(key, key3);
}
```

### 6.2 Signaling Tests

**File**: `crates/tokio-nethernet/src/signaling_tests.rs` (new file)

```rust
//! Tests for signaling protocol

use super::*;

#[test]
fn signal_serialization() {
    let signals = vec![
        Signal::new_offer("v=0\r\no=..."),
        Signal::new_answer("v=0\r\no=..."),
        Signal::new_ice_candidate("candidate:..."),
    ];

    for signal in signals {
        let json = serde_json::to_string(&signal).unwrap();
        let parsed: Signal = serde_json::from_str(&json).unwrap();
        assert_eq!(signal, parsed);
    }
}
```

## Success Criteria

### Automated Verification
- [ ] `cargo test -p tokio-nethernet` passes
- [ ] Discovery encryption works
- [ ] Signaling serialization correct

### Manual Verification
- [ ] Connect via NetherNet from real client
- [ ] Data transfers reliably

---

# Phase 7: Mutation Testing Setup

## Overview
Set up cargo-mutants for continuous quality improvement.

## Installation

```bash
cargo install cargo-mutants
```

## Configuration

**File**: `.cargo/mutants.toml` (new file)

```toml
# Mutation testing configuration

# Timeout per mutant (seconds)
timeout = 120

# Parallel jobs
jobs = 4

# Exclude paths
exclude_globs = [
    "tests/**",
    "benches/**",
    "examples/**",
    "**/generated/**",
    "crates/valentine/bedrock_versions/**",  # Generated code
    "crates/unastar-data/**",                 # Generated data
]

# Minimum test time before considering a mutant killed
minimum_test_time = 0.5
```

## Usage

```bash
# Run on specific crate
cargo mutants -p tokio-raknet

# Run on critical paths only
cargo mutants -p tokio-raknet --file src/session/split_assembler.rs

# Run on changed files (for CI)
cargo mutants --in-diff origin/main

# Generate report
cargo mutants -p tokio-raknet --json > mutants-report.json
```

## Priority Order for Mutation Testing

1. **tokio-raknet** - Protocol correctness is critical
2. **valentine** - Packet encoding must be exact
3. **jolyne** - Authentication/encryption bugs are severe
4. **axolotl-xbl** - Security-sensitive signing code
5. **unastar** - World generation correctness

## Interpreting Results

- **Killed mutants**: Tests caught the bug - good!
- **Surviving mutants**: Potential missing test - investigate
- **Timeout mutants**: Infinite loop or very slow - skip
- **Equivalent mutants**: Change doesn't affect behavior - ignore

---

# Phase 8: CI Integration

## GitHub Actions Workflow

**File**: `.github/workflows/test.yml` (new file)

```yaml
name: Test

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

env:
  CARGO_TERM_COLOR: always
  RUST_BACKTRACE: 1

jobs:
  test:
    runs-on: ubuntu-latest

    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy

      - name: Cache cargo
        uses: Swatinem/rust-cache@v2

      - name: Check formatting
        run: cargo fmt --all -- --check

      - name: Clippy
        run: cargo clippy --workspace --all-targets -- -D warnings

      - name: Build
        run: cargo build --workspace

      - name: Test
        run: cargo test --workspace

      - name: Test (all features)
        run: cargo test --workspace --all-features

  mutation-test:
    runs-on: ubuntu-latest
    if: github.event_name == 'pull_request'

    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Install cargo-mutants
        run: cargo install cargo-mutants

      - name: Run mutation tests on diff
        run: cargo mutants --in-diff origin/main -p tokio-raknet -p valentine -p jolyne --timeout 120 -j 2
        continue-on-error: true

      - name: Upload results
        uses: actions/upload-artifact@v4
        with:
          name: mutation-results
          path: mutants.out/
```

---

# Testing Conventions & Few-Shot Examples

## Test Module Structure

```rust
// src/my_module.rs

pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

// Tests go in the same file in a submodule
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_positive_numbers() {
        assert_eq!(add(2, 3), 5);
    }

    #[test]
    fn add_negative_numbers() {
        assert_eq!(add(-2, -3), -5);
    }

    #[test]
    fn add_mixed_signs() {
        assert_eq!(add(-2, 3), 1);
    }

    #[test]
    fn add_overflow_wraps() {
        // Document edge case behavior
        assert_eq!(add(i32::MAX, 1), i32::MIN);
    }
}
```

## Integration Test Structure

```rust
// tests/integration_test.rs

use my_crate::*;

/// Test description as doc comment
#[tokio::test]
async fn test_name_describes_what_is_tested() {
    // 1. Setup
    let server = TestServer::start().await;
    let client = TestClient::connect(server.addr()).await;

    // 2. Action
    client.send_message("hello").await;

    // 3. Assert
    let response = client.recv().await;
    assert_eq!(response, "hello back");

    // 4. Cleanup (automatic via Drop)
}
```

## Mocking External Dependencies

```rust
// Use trait objects for mockability
#[async_trait]
pub trait HttpClient: Send + Sync {
    async fn get(&self, url: &str) -> Result<Response>;
    async fn post(&self, url: &str, body: &[u8]) -> Result<Response>;
}

// Real implementation
pub struct ReqwestClient {
    client: reqwest::Client,
}

#[async_trait]
impl HttpClient for ReqwestClient {
    // ... real implementation
}

// Mock for testing
#[cfg(test)]
pub struct MockHttpClient {
    responses: std::collections::HashMap<String, Response>,
}

#[cfg(test)]
#[async_trait]
impl HttpClient for MockHttpClient {
    async fn get(&self, url: &str) -> Result<Response> {
        self.responses.get(url)
            .cloned()
            .ok_or(Error::NotFound)
    }

    async fn post(&self, url: &str, _body: &[u8]) -> Result<Response> {
        self.responses.get(url)
            .cloned()
            .ok_or(Error::NotFound)
    }
}

// In tests:
#[test]
fn test_with_mock() {
    let mut mock = MockHttpClient::new();
    mock.responses.insert(
        "https://api.example.com/data".to_string(),
        Response { status: 200, body: "test data".to_string() }
    );

    let service = MyService::new(Box::new(mock));
    let result = service.fetch_data().await;

    assert_eq!(result, "test data");
}
```

## Property-Based Testing (Future)

```rust
// Using proptest crate
use proptest::prelude::*;

proptest! {
    #[test]
    fn varint_roundtrip(value in any::<u32>()) {
        let mut buf = BytesMut::new();
        VarInt(value).encode(&mut buf);

        let decoded = VarInt::decode(&mut buf.freeze()).unwrap();
        prop_assert_eq!(decoded.0, value);
    }

    #[test]
    fn chunk_coords_in_range(x in -1000000..1000000i32, z in -1000000..1000000i32) {
        let chunk = ChunkPos::new(x, z);
        prop_assert!(chunk.x() == x);
        prop_assert!(chunk.z() == z);
    }
}
```

---

## References

- Existing tests: `crates/valentine/tests/`, `crates/tokio-raknet/tests/`
- Mutation testing: [cargo-mutants documentation](https://mutants.rs)
- Bedrock protocol: `crates/valentine_gen/minecraft-data/`

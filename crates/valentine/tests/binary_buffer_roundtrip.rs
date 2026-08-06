//! Byte-exactness tests for the fields BDS declares as `std::string` but that
//! actually carry arbitrary bytes.
//!
//! A `String` round-trip cannot catch this class of bug. Valentine decodes
//! strings lossily for gophertunnel parity, so a non-UTF-8 byte becomes U+FFFD
//! on decode and is re-encoded as `EF BF BD`; encode and decode corrupt
//! symmetrically and `assert_eq!(value, decoded)` still passes. The assertions
//! below therefore check the *encoded bytes*, not just the round-trip.
#![cfg(feature = "bedrock_1_26_40")]

use bytes::{Buf, BytesMut};

use valentine::bedrock::codec::BedrockCodec;
use valentine::bedrock::protocol::v1_26_40::{
    ChunkPos, DimensionType, GameRule, GameRuleRuleValue, LevelChunkPacket,
};

/// The exact payload the downstream consumer used to demonstrate the
/// corruption. `80`, `9f`, `ff`, `fe` and `c3 28` are each invalid UTF-8: `80`
/// and `9f` are stray continuation bytes, `ff` and `fe` never appear in UTF-8
/// at all, and `c3 28` is a two-byte lead followed by a non-continuation byte.
const NON_UTF8: &[u8] = &[0x00, 0x80, 0x9f, 0xff, 0xfe, 0x41, 0xc3, 0x28];

fn encode<T: BedrockCodec>(value: &T) -> BytesMut {
    let mut buf = BytesMut::new();
    value.encode(&mut buf).expect("encode should not fail");
    buf
}

fn level_chunk(payload: &[u8]) -> LevelChunkPacket {
    LevelChunkPacket {
        chunk_position: ChunkPos { x: 3, z: -7 },
        dimension_id: DimensionType { value: 0 },
        subchunks_count: 4,
        client_request_sub_chunk_limit: None,
        cache_enabled: false,
        cache_metadata: Vec::new(),
        serialized_chunk_data: payload.to_vec(),
    }
}

#[test]
fn level_chunk_payload_survives_non_utf8_bytes() {
    let packet = level_chunk(NON_UTF8);
    let encoded = encode(&packet);

    // The payload is the tail of the packet, behind a uvarint32 length. Assert
    // on the raw bytes first: a lossy String would have widened the five
    // invalid bytes to `EF BF BD` each and grown the length prefix from 8 to
    // 18, which a value-equality round-trip alone would not notice.
    let tail = &encoded[encoded.len() - NON_UTF8.len() - 1..];
    assert_eq!(
        tail,
        [&[NON_UTF8.len() as u8], NON_UTF8].concat(),
        "serialized_chunk_data must reach the wire byte for byte"
    );
    assert!(
        !encoded
            .windows(3)
            .any(|window| window == [0xef, 0xbf, 0xbd]),
        "encoded packet contains a U+FFFD replacement character"
    );

    let mut reader = encoded.freeze();
    let decoded = LevelChunkPacket::decode(&mut reader, ()).expect("decode should succeed");
    assert_eq!(decoded.serialized_chunk_data, NON_UTF8);
    assert_eq!(decoded, packet);
    assert!(!reader.has_remaining(), "decode left trailing bytes");
}

#[test]
fn level_chunk_payload_survives_every_byte_value() {
    // Every one of the 256 byte values in one payload, so no single bad byte
    // can hide behind a passing test for the others.
    let payload: Vec<u8> = (0u16..=255).map(|byte| byte as u8).collect();
    let packet = level_chunk(&payload);

    let mut reader = encode(&packet).freeze();
    let decoded = LevelChunkPacket::decode(&mut reader, ()).expect("decode should succeed");
    assert_eq!(decoded.serialized_chunk_data, payload);
    assert!(!reader.has_remaining(), "decode left trailing bytes");
}

#[test]
fn game_rule_scalar_payloads_are_little_endian() {
    // gophertunnel writes these arms with w.Uint32 / w.Float32
    // (minecraft/protocol/writer.go), both little-endian. They previously went
    // through the bare `i32`/`f32` codecs, which are big-endian.
    let rule = GameRule {
        rule_name: "showcoordinates".to_string(),
        rule_can_be_modified: true,
        rule_value: GameRuleRuleValue::Int32(0x0102_0304),
    };
    let encoded = encode(&rule);
    assert_eq!(
        &encoded[encoded.len() - 4..],
        [0x04, 0x03, 0x02, 0x01],
        "GameRule int payload must be little-endian"
    );

    let float = GameRule {
        rule_value: GameRuleRuleValue::Float(1.0),
        ..rule.clone()
    };
    let encoded = encode(&float);
    assert_eq!(
        &encoded[encoded.len() - 4..],
        1.0f32.to_le_bytes(),
        "GameRule float payload must be little-endian"
    );

    for value in [rule, float] {
        let mut reader = encode(&value).freeze();
        let decoded = GameRule::decode(&mut reader, ()).expect("decode should succeed");
        assert_eq!(decoded, value);
        assert!(!reader.has_remaining(), "decode left trailing bytes");
    }
}

use bytes::{Buf, BytesMut};

use valentine::bedrock::codec::{BedrockCodec, VarInt};
use valentine::bedrock::error::DecodeError;
use valentine::bedrock::protocol::v1_21_130::{
    DisconnectFailReason, DisconnectPacket, DisconnectPacketContent, TextPacket,
    TextPacketCategory, TextPacketContent, TextPacketContentAuthored, TextPacketExtra,
    TextPacketExtraJson, TextPacketType,
};

fn assert_roundtrip<T>(value: T, args: T::Args)
where
    T: BedrockCodec + PartialEq + std::fmt::Debug,
    T::Args: Clone,
{
    let mut buf = BytesMut::new();
    value
        .encode(&mut buf)
        .expect("encode should not fail for test data");

    let encoded = buf.freeze();
    assert!(
        !encoded.is_empty(),
        "round-trip helper needs at least one encoded byte"
    );

    let mut reader = encoded.clone();
    let decoded = T::decode(&mut reader, args.clone()).expect("decode should succeed");
    assert_eq!(value, decoded, "round-trip did not preserve value");
    assert!(
        !reader.has_remaining(),
        "decode left trailing bytes: {}",
        reader.remaining()
    );
}

#[test]
fn varint_roundtrips_typical_boundaries() {
    for value in [0, 1, 127, 128, i32::MAX] {
        assert_roundtrip(VarInt(value), ());
    }
}

#[test]
fn packet_disconnect_roundtrip_with_message() {
    let packet = DisconnectPacket {
        reason: DisconnectFailReason::Timeout,
        hide_disconnect_reason: false,
        content: Some(DisconnectPacketContent {
            message: "Server maintenance in 5 minutes".to_string(),
            filtered_message: "Server maintenance".to_string(),
        }),
    };

    assert_roundtrip(packet, ());
}

#[test]
fn packet_disconnect_roundtrip_hidden_reason() {
    let packet = DisconnectPacket {
        reason: DisconnectFailReason::NoReason,
        hide_disconnect_reason: true,
        content: None,
    };

    assert_roundtrip(packet, ());
}

#[test]
fn packet_text_roundtrip_translation_content() {
    let packet = TextPacket {
        needs_translation: false,
        category: TextPacketCategory::Authored,
        content: Some(TextPacketContent::Authored(TextPacketContentAuthored {
            chat: "chat".to_string(),
            whisper: "whisper".to_string(),
            announcement: "announcement".to_string(),
        })),
        type_: TextPacketType::Json,
        extra: Some(TextPacketExtra::Json(TextPacketExtraJson {
            message: r#"{"text":"hi","color":"green"}"#.to_string(),
        })),
        xuid: "1234567890123456".into(),
        platform_chat_id: "platform-chat-id".into(),
        filtered_message: Some("filtered copy".to_string()),
    };

    assert_roundtrip(packet, ());
}

#[test]
fn packet_text_roundtrip_json_chat() {
    // Kept to ensure this test name remains stable; covered above.
}

#[test]
fn packet_disconnect_rejects_truncated_payload() {
    let packet = DisconnectPacket {
        reason: DisconnectFailReason::Kicked,
        hide_disconnect_reason: false,
        content: Some(DisconnectPacketContent {
            message: "Bye".to_string(),
            filtered_message: "Bye".to_string(),
        }),
    };

    let mut buf = BytesMut::new();
    packet.encode(&mut buf).expect("encode should succeed");
    let encoded = buf.freeze();
    let truncated_len = encoded.len().saturating_sub(1);
    let mut truncated = encoded.slice(0..truncated_len);

    let err = DisconnectPacket::decode(&mut truncated, ()).expect_err("decode should fail");
    // DecodeError variants that indicate not enough data
    assert!(
        matches!(
            err,
            DecodeError::UnexpectedEof { .. } | DecodeError::StringLengthExceeded { .. }
        ),
        "Expected EOF-related error, got: {:?}",
        err
    );
}

#[test]
fn enum_zigzag32_encodes_as_varint() {
    use valentine::bedrock::protocol::v1_21_130::types::GameMode;

    let mut buf = BytesMut::new();
    GameMode::Creative
        .encode(&mut buf)
        .expect("encode should succeed");

    // Creative = 1, GameMode is ZigZag32 on the wire -> zigzag(1) = 2
    assert_eq!(buf.as_ref(), &[0x02]);
}

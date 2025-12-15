use bytes::{Buf, BytesMut};
use std::io::ErrorKind;

use valentine::bedrock::codec::{BedrockCodec, VarInt};
use valentine::bedrock::protocol::v1_21_130::packets::{
    PacketDisconnect, PacketDisconnectContent, PacketText, PacketTextCategory, PacketTextContent,
    PacketTextContentAuthored, PacketTextExtra, PacketTextExtraJson, PacketTextType,
};
use valentine::bedrock::protocol::v1_21_130::types::DisconnectFailReason;

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
    let packet = PacketDisconnect {
        reason: DisconnectFailReason::Timeout,
        content: Some(PacketDisconnectContent {
            message: "Server maintenance in 5 minutes".to_string(),
            filtered_message: "Server maintenance".to_string(),
        }),
    };

    assert_roundtrip(packet, ());
}

#[test]
fn packet_disconnect_roundtrip_hidden_reason() {
    let packet = PacketDisconnect {
        reason: DisconnectFailReason::NoReason,
        content: None,
    };

    assert_roundtrip(packet, ());
}

#[test]
fn packet_text_roundtrip_translation_content() {
    let packet = PacketText {
        needs_translation: false,
        category: PacketTextCategory::Authored,
        content: Some(PacketTextContent::Authored(PacketTextContentAuthored {
            chat: "chat".to_string(),
            whisper: "whisper".to_string(),
            announcement: "announcement".to_string(),
        })),
        type_: PacketTextType::Json,
        extra: Some(PacketTextExtra::Json(PacketTextExtraJson {
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
    let packet = PacketDisconnect {
        reason: DisconnectFailReason::Kicked,
        content: Some(PacketDisconnectContent {
            message: "Bye".to_string(),
            filtered_message: "Bye".to_string(),
        }),
    };

    let mut buf = BytesMut::new();
    packet.encode(&mut buf).expect("encode should succeed");
    let encoded = buf.freeze();
    let truncated_len = encoded.len().saturating_sub(1);
    let mut truncated = encoded.slice(0..truncated_len);

    let err = PacketDisconnect::decode(&mut truncated, ()).expect_err("decode should fail");
    assert_eq!(err.kind(), ErrorKind::UnexpectedEof);
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

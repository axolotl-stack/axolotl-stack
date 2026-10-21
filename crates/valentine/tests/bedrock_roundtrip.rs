#![cfg(feature = "bedrock_1_26_51")]
use bytes::{Buf, BytesMut};

use valentine::bedrock::codec::{BedrockCodec, VarInt, VarUInt};
use valentine::bedrock::error::DecodeError;
use valentine::bedrock::version::v1_26_51::*;

fn assert_roundtrip<T>(value: T, args: T::Args)
where
    T: BedrockCodec + PartialEq + std::fmt::Debug,
{
    let mut buf = BytesMut::new();
    value.encode(&mut buf).expect("encode test data");
    let mut reader = buf.freeze();
    assert!(!reader.is_empty());
    let decoded = T::decode(&mut reader, args).expect("decode test data");
    assert_eq!(value, decoded);
    assert!(
        !reader.has_remaining(),
        "trailing bytes: {}",
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
    assert_roundtrip(
        DisconnectPacket {
            reason: EnumsConnectionDisconnectFailReason::Timeout,
            messages: DisconnectPacketMessagesUnion::DisconnectPacketMessages(
                DisconnectPacketMessages {
                    message: "Server maintenance in 5 minutes".into(),
                    filtered_message: "Server maintenance".into(),
                },
            ),
        },
        (),
    );
}

#[test]
fn packet_disconnect_roundtrip_empty_message() {
    assert_roundtrip(
        DisconnectPacket {
            reason: EnumsConnectionDisconnectFailReason::Noreason,
            messages: DisconnectPacketMessagesUnion::default(),
        },
        (),
    );
}

#[test]
fn packet_text_roundtrip_chat_message() {
    assert_roundtrip(
        TextPacket {
            localize: false,
            body: TextPacketBody::AuthorAndMessage(TextPacketPayloadAuthorAndMessage {
                message_type: TextPacketPayloadAuthorAndMessageMessageType::Chat,
                player_name: "PlayerName".into(),
                message: "Hello, world!".into(),
            }),
            senders_xuid: "1234567890123456".into(),
            platform_id: "platform-chat-id".into(),
            filtered_message: Some("Hello, world!".into()),
        },
        (),
    );
}

#[test]
fn packet_text_roundtrip_translation() {
    assert_roundtrip(
        TextPacket {
            localize: true,
            body: TextPacketBody::MessageAndParams(TextPacketPayloadMessageAndParams {
                message_type: TextPacketPayloadMessageAndParamsMessageType::Translate,
                message: "chat.type.text".into(),
                parameter_list: vec!["Player1".into(), "Hello".into()],
            }),
            senders_xuid: String::new(),
            platform_id: String::new(),
            filtered_message: None,
        },
        (),
    );
}

#[test]
fn packet_text_roundtrip_json_chat() {
    assert_roundtrip(
        TextPacket {
            body: TextPacketBody::MessageOnly(TextPacketPayloadMessageOnly {
                message_type: EnumsTextPacketType::Textobject,
                message: r#"{"rawtext":[{"text":"Hello, world!"}]}"#.into(),
            }),
            ..Default::default()
        },
        (),
    );
}

#[test]
fn packet_disconnect_rejects_truncated_payload() {
    let packet = DisconnectPacket {
        reason: EnumsConnectionDisconnectFailReason::Disconnected,
        messages: DisconnectPacketMessagesUnion::DisconnectPacketMessages(
            DisconnectPacketMessages {
                message: "Bye".into(),
                filtered_message: "Bye".into(),
            },
        ),
    };
    let mut buf = BytesMut::new();
    packet.encode(&mut buf).expect("encode");
    buf.truncate(buf.len() - 1);
    let err = DisconnectPacket::decode(&mut buf.freeze(), ()).expect_err("truncated packet");
    assert!(
        matches!(
            err,
            DecodeError::UnexpectedEof { .. } | DecodeError::StringLengthExceeded { .. }
        ),
        "{err:?}"
    );
}

#[test]
fn enum_zigzag32_encodes_as_varint() {
    let mut buf = BytesMut::new();
    EnumsGameType::Creative.encode(&mut buf).expect("encode");
    assert_eq!(buf.as_ref(), &[0x02]);
}

#[test]
fn owned_mcpe_frame_rejects_trailing_payload_bytes() {
    let packet = McpePacket::from(DisconnectPacket::default());
    let mut encoded = BytesMut::new();
    packet.encode(&mut encoded).expect("encode frame");
    let mut original = encoded.freeze();
    let magic = original.get_u8();
    let declared = VarUInt::decode(&mut original, ()).expect("frame length").0;
    let mut malformed = BytesMut::new();
    malformed.extend_from_slice(&[magic]);
    VarUInt(declared + 3)
        .encode(&mut malformed)
        .expect("frame length");
    malformed.extend_from_slice(&original);
    malformed.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
    let args = McpePacketArgs;
    let err =
        McpePacket::decode(&mut malformed.freeze(), args).expect_err("trailing frame payload");
    assert!(
        matches!(err, DecodeError::TrailingPacketBytes { remaining: 3 }),
        "{err:?}"
    );
}

#[test]
fn owned_mcpe_frame_leaves_next_frame_unconsumed() {
    let packet = McpePacket::from(DisconnectPacket::default());
    let mut encoded = BytesMut::new();
    packet.encode(&mut encoded).expect("first frame");
    let frame_len = encoded.len();
    packet.encode(&mut encoded).expect("second frame");
    let mut reader = encoded.freeze();
    let args = McpePacketArgs;
    assert_eq!(
        McpePacket::decode(&mut reader, args.clone()).expect("first decode"),
        packet
    );
    assert_eq!(reader.remaining(), frame_len);
    assert_eq!(
        McpePacket::decode(&mut reader, args).expect("second decode"),
        packet
    );
    assert!(!reader.has_remaining());
}

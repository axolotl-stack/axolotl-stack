use bytes::{Buf, BytesMut};

use valentine::bedrock::borrowed::{
    BorrowedDisconnectPacket, BorrowedLoginPacket, BorrowedTextPacket, RawMcpeFrame,
};
use valentine::bedrock::codec::BedrockCodec;
use valentine::bedrock::context::BedrockSession;
use valentine::bedrock::version::v1_26_30::*;

fn sample_login_packet() -> LoginPacket {
    LoginPacket {
        protocol_version: 776,
        tokens: LoginTokens {
            identity: "{\"chain\":[{\"extraData\":{\"displayName\":\"Player\"}}]}".repeat(2),
            client: "{\"ClientRandomId\":1,\"ServerAddress\":\"127.0.0.1:19132\"}".repeat(2),
        },
    }
}

fn sample_text_packet() -> TextPacket {
    TextPacket {
        type_: TextPacketType::Chat,
        needs_translation: false,
        category: TextPacketCategory::Authored,
        content: Some(TextPacketContent::Chat(TextPacketContentAnnouncement {
            source_name: "PlayerName".to_string(),
            message: "Hello, world!".repeat(4),
        })),
        xuid: "1234567890123456".to_string(),
        platform_chat_id: "platform-chat-id".to_string(),
        filtered_message: Some("Hello, world!".repeat(4)),
    }
}

fn sample_disconnect_packet() -> DisconnectPacket {
    DisconnectPacket {
        reason: DisconnectFailReason::Timeout,
        hide_disconnect_reason: false,
        content: Some(DisconnectPacketContent {
            message: "Server maintenance in 5 minutes".to_string(),
            filtered_message: "Server maintenance".to_string(),
        }),
    }
}

fn encode_to_bytes<T: BedrockCodec>(value: &T) -> bytes::Bytes {
    let mut buf = BytesMut::new();
    value.encode(&mut buf).expect("encode should succeed");
    buf.freeze()
}

#[test]
fn borrowed_login_decode_roundtrips_strings() {
    let packet = sample_login_packet();
    let mut encoded = encode_to_bytes(&packet);

    let decoded = BorrowedLoginPacket::decode(&mut encoded).expect("borrowed login decode");

    assert_eq!(decoded.protocol_version, packet.protocol_version);
    assert_eq!(
        decoded.tokens.identity.as_str().expect("identity utf8"),
        packet.tokens.identity
    );
    assert_eq!(
        decoded.tokens.client.as_str().expect("client utf8"),
        packet.tokens.client
    );
    assert!(!encoded.has_remaining());
}

#[test]
fn borrowed_disconnect_decode_roundtrips_strings() {
    let packet = sample_disconnect_packet();
    let mut encoded = encode_to_bytes(&packet);

    let decoded =
        BorrowedDisconnectPacket::decode(&mut encoded).expect("borrowed disconnect decode");

    assert_eq!(decoded.reason, packet.reason);
    assert_eq!(
        decoded.hide_disconnect_reason,
        packet.hide_disconnect_reason
    );
    let content = decoded.content.expect("disconnect content");
    let expected = packet.content.expect("expected disconnect content");
    assert_eq!(
        content.message.as_str().expect("message utf8"),
        expected.message
    );
    assert_eq!(
        content.filtered_message.as_str().expect("filtered utf8"),
        expected.filtered_message
    );
    assert!(!encoded.has_remaining());
}

#[test]
fn borrowed_text_decode_roundtrips_strings() {
    let packet = sample_text_packet();
    let mut encoded = encode_to_bytes(&packet);

    let decoded = BorrowedTextPacket::decode(&mut encoded).expect("borrowed text decode");

    assert_eq!(decoded.type_, packet.type_);
    assert_eq!(decoded.needs_translation, packet.needs_translation);
    assert_eq!(decoded.category, packet.category);
    assert_eq!(decoded.xuid.as_str().expect("xuid utf8"), packet.xuid);
    assert_eq!(
        decoded.platform_chat_id.as_str().expect("platform utf8"),
        packet.platform_chat_id
    );
    assert_eq!(
        decoded
            .filtered_message
            .as_ref()
            .expect("filtered message")
            .as_str()
            .expect("filtered utf8"),
        packet.filtered_message.expect("expected filtered message")
    );
    assert!(!encoded.has_remaining());
}

#[test]
fn borrowed_mcpe_frame_slices_payload() {
    let session = BedrockSession { shield_item_id: 0 };
    let args = McpePacketArgs::from(&session);
    let packet = McpePacket::from(sample_text_packet());
    let encoded = encode_to_bytes(&packet);
    let expected = McpePacket::decode(&mut encoded.clone(), args).expect("owned decode");

    let mut raw = encoded;
    let frame = RawMcpeFrame::decode(&mut raw).expect("raw frame decode");

    assert_eq!(frame.header.id_raw, expected.header.id as u32);
    assert_eq!(frame.header.from_subclient, expected.header.from_subclient);
    assert_eq!(frame.header.to_subclient, expected.header.to_subclient);
    assert!(!frame.payload.is_empty());
    assert!(!raw.has_remaining());
}

#[test]
fn borrowed_mcpe_frame_dispatches_login_packet() {
    let packet = McpePacket::from(sample_login_packet());
    let mut encoded = encode_to_bytes(&packet);

    let borrowed = BorrowedMcpePacket::decode_game_frame(&mut encoded).expect("borrowed mcpe");

    assert_eq!(borrowed.packet_id(), McpePacketName::PacketLogin);
    match borrowed.data {
        BorrowedMcpePacketData::PacketLogin(login) => {
            assert_eq!(login.protocol_version, 776);
            assert!(
                login
                    .tokens
                    .identity
                    .as_str()
                    .expect("identity utf8")
                    .contains("Player")
            );
        }
        other => panic!("expected borrowed login packet, got {other:?}"),
    }
    assert!(!encoded.has_remaining());
}

#[test]
fn borrowed_mcpe_packet_materializes_to_owned() {
    let session = BedrockSession { shield_item_id: 0 };
    let args = McpePacketArgs::from(&session);
    let packet = McpePacket::from(sample_login_packet());
    let expected = McpePacket::decode(&mut encode_to_bytes(&packet), args.clone()).expect("owned");

    let mut encoded = encode_to_bytes(&packet);
    let borrowed = BorrowedMcpePacket::decode_game_frame(&mut encoded).expect("borrowed mcpe");
    let owned = borrowed.into_owned(args).expect("materialize owned");

    assert_eq!(owned, expected);
    assert!(!encoded.has_remaining());
}

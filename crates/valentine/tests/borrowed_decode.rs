#![cfg(feature = "bedrock_1_26_51")]
use bytes::{Buf, BytesMut};

use valentine::bedrock::borrowed::RawMcpeFrame;
use valentine::bedrock::codec::BedrockCodec;
use valentine::bedrock::version::v1_26_51::*;

fn sample_login_packet() -> LoginPacket {
    LoginPacket {
        client_network_version: PROTOCOL_VERSION,
        connection_request: b"\0\xffopaque connection request\x80".repeat(4),
    }
}

fn sample_text_packet() -> TextPacket {
    TextPacket {
        localize: false,
        body: TextPacketBody::AuthorAndMessage(TextPacketPayloadAuthorAndMessage {
            message_type: TextPacketPayloadAuthorAndMessageMessageType::Chat,
            player_name: "PlayerName".into(),
            message: "Hello, world!".repeat(4),
        }),
        senders_xuid: "1234567890123456".into(),
        platform_id: "platform-chat-id".into(),
        filtered_message: Some("Hello, world!".repeat(4)),
    }
}

fn encode_to_bytes<T: BedrockCodec>(value: &T) -> bytes::Bytes {
    let mut buf = BytesMut::new();
    value.encode(&mut buf).expect("encode should succeed");
    buf.freeze()
}

#[test]
fn borrowed_login_decode_roundtrips_connection_request() {
    let packet = sample_login_packet();
    let mut encoded = encode_to_bytes(&packet);
    let decoded = BorrowedLoginPacket::decode(&mut encoded).expect("borrowed login decode");
    assert_eq!(
        decoded.client_network_version,
        packet.client_network_version
    );
    assert_eq!(
        decoded.connection_request.as_ref(),
        packet.connection_request
    );
    assert!(!encoded.has_remaining());
    assert_eq!(LoginPacket::from(decoded), packet);
}

#[test]
fn borrowed_disconnect_decode_roundtrips_strings() {
    let packet = DisconnectPacket {
        reason: EnumsConnectionDisconnectFailReason::Timeout,
        messages: DisconnectPacketMessagesUnion::DisconnectPacketMessages(
            DisconnectPacketMessages {
                message: "Server maintenance in 5 minutes".into(),
                filtered_message: "Server maintenance".into(),
            },
        ),
    };
    let mut encoded = encode_to_bytes(&packet);
    // The current schema exposes a borrowed view for messages, not the whole packet.
    let reason = EnumsConnectionDisconnectFailReason::decode(&mut encoded, ()).expect("reason");
    let control = valentine::bedrock::codec::VarUInt::decode(&mut encoded, ()).expect("control");
    assert_eq!(control.0, 0);
    let DisconnectPacketMessagesUnion::DisconnectPacketMessages(expected) = &packet.messages else {
        panic!("expected visible disconnect");
    };
    let messages = DisconnectPacketMessagesView::decode(&mut encoded).expect("borrowed messages");
    assert_eq!(reason, packet.reason);
    assert_eq!(
        messages.message.as_str().expect("message utf8"),
        expected.message
    );
    assert_eq!(
        messages.filtered_message.as_str().expect("filtered utf8"),
        expected.filtered_message
    );
    assert!(!encoded.has_remaining());
}

#[test]
fn borrowed_text_decode_roundtrips_strings() {
    let packet = sample_text_packet();
    let mut encoded = encode_to_bytes(&packet);
    let decoded = BorrowedTextPacket::decode(&mut encoded).expect("borrowed text decode");
    assert_eq!(decoded.localize, packet.localize);
    assert_eq!(decoded.body, packet.body);
    assert_eq!(
        decoded.senders_xuid.as_str().expect("xuid utf8"),
        packet.senders_xuid
    );
    assert_eq!(
        decoded.platform_id.as_str().expect("platform utf8"),
        packet.platform_id
    );
    assert_eq!(
        decoded
            .filtered_message
            .as_ref()
            .expect("filtered message")
            .as_str()
            .expect("filtered utf8"),
        packet.filtered_message.as_ref().unwrap()
    );
    assert!(!encoded.has_remaining());
    assert_eq!(TextPacket::from(decoded), packet);
}

#[test]
fn borrowed_mcpe_frame_slices_payload() {
    let args = McpePacketArgs;
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
    let login = sample_login_packet();
    let packet = McpePacket::from(login.clone());
    let mut encoded = encode_to_bytes(&packet);
    let borrowed = BorrowedMcpePacket::decode_game_frame(&mut encoded).expect("borrowed mcpe");
    assert_eq!(borrowed.packet_id(), McpePacketName::LoginPacket);
    match borrowed.data {
        BorrowedMcpePacketData::LoginPacket(decoded) => {
            assert_eq!(decoded.client_network_version, login.client_network_version);
            assert_eq!(
                decoded.connection_request.as_ref(),
                login.connection_request
            );
        }
        other => panic!("expected borrowed login packet, got {other:?}"),
    }
    assert!(!encoded.has_remaining());
}

#[test]
fn borrowed_mcpe_packet_materializes_to_owned() {
    let args = McpePacketArgs;
    let packet = McpePacket::from(sample_login_packet());
    let expected = McpePacket::decode(&mut encode_to_bytes(&packet), args.clone()).expect("owned");
    let mut encoded = encode_to_bytes(&packet);
    let borrowed = BorrowedMcpePacket::decode_game_frame(&mut encoded).expect("borrowed mcpe");
    assert_eq!(
        borrowed.into_owned(args).expect("materialize owned"),
        expected
    );
    assert!(!encoded.has_remaining());
}

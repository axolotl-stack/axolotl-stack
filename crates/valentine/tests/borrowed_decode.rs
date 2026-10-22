#![cfg(feature = "bedrock_1_26_51")]
use bytes::{Buf, BytesMut};

use valentine::bedrock::borrowed::RawMcpeFrame;
use valentine::bedrock::codec::BedrockCodec;
use valentine::bedrock::version::v1_26_51::*;

fn sample_login_packet() -> LoginPacket {
    LoginPacket {
        client_network_version: PROTOCOL_VERSION,
        connection_request: b"\0\xffopaque connection request\x80".repeat(4).into(),
    }
}

fn sample_text_packet() -> TextPacket {
    TextPacket {
        localize: false,
        body: TextPacketBody::AuthorAndMessage(TextPacketPayloadAuthorAndMessage {
            message_type: TextPacketPayloadAuthorAndMessageMessageType::Chat,
            player_name: "PlayerName".into(),
            message: "Hello, world!".repeat(4).into(),
        }),
        senders_xuid: "1234567890123456".into(),
        platform_id: "platform-chat-id".into(),
        filtered_message: Some("Hello, world!".repeat(4).into()),
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
        &*expected.message
    );
    assert_eq!(
        messages.filtered_message.as_str().expect("filtered utf8"),
        &*expected.filtered_message
    );
    assert!(!encoded.has_remaining());
}

#[test]
fn borrowed_text_decode_roundtrips_strings() {
    let packet = sample_text_packet();
    let mut encoded = encode_to_bytes(&packet);
    let decoded = BorrowedTextPacket::decode(&mut encoded).expect("borrowed text decode");
    assert_eq!(decoded.localize, packet.localize);
    assert_eq!(TextPacketBody::from(decoded.body.clone()), packet.body);
    assert_eq!(
        decoded.senders_xuid.as_str().expect("xuid utf8"),
        &*packet.senders_xuid
    );
    assert_eq!(
        decoded.platform_id.as_str().expect("platform utf8"),
        &*packet.platform_id
    );
    assert_eq!(
        decoded
            .filtered_message
            .as_ref()
            .expect("filtered message")
            .as_str()
            .expect("filtered utf8"),
        &**packet.filtered_message.as_ref().unwrap()
    );
    assert!(!encoded.has_remaining());
    assert_eq!(TextPacket::from(decoded), packet);
}

#[test]
fn borrowed_mcpe_frame_slices_payload() {
    let args = McpePacketArgs;
    let packet = McpePacket::from(sample_text_packet());
    let encoded = encode_to_bytes(&packet);
    let expected = McpePacket::decode_bytes(&mut encoded.clone(), args).expect("owned decode");
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
    let expected =
        McpePacket::decode_bytes(&mut encode_to_bytes(&packet), args.clone()).expect("owned");
    let mut encoded = encode_to_bytes(&packet);
    let borrowed = BorrowedMcpePacket::decode_game_frame(&mut encoded).expect("borrowed mcpe");
    assert_eq!(
        borrowed.into_owned(args).expect("materialize owned"),
        expected
    );
    assert!(!encoded.has_remaining());
}

#[test]
fn shared_frame_errors_preserve_the_next_entry() {
    let valid = McpePacket::from(DisconnectPacket::default());
    let mut next = BytesMut::new();
    valid.data.encode_inner_bytes_mut(&mut next, 0, 0).unwrap();
    let next = next.freeze();

    let mut truncated_message = BytesMut::new();
    valentine::protocol::wire::write_var_u32(
        &mut truncated_message,
        McpePacketName::DisconnectPacket as u32,
    );
    truncated_message.extend_from_slice(&[0, 0, 3, b'a']);
    for malformed in [
        bytes::Bytes::from_static(&[0x80]),
        truncated_message.freeze(),
    ] {
        let mut joined = BytesMut::new();
        valentine::protocol::wire::write_var_u32(&mut joined, malformed.len() as u32);
        joined.extend_from_slice(&malformed);
        joined.extend_from_slice(&next);
        let mut cursor = joined.freeze();
        let mut owned_cursor = cursor.clone();
        assert!(McpePacketData::decode_inner_bytes(&mut owned_cursor, McpePacketArgs).is_err());
        assert_eq!(owned_cursor, next);
        let (header, data) =
            McpePacketData::decode_inner_bytes(&mut owned_cursor, McpePacketArgs).unwrap();
        assert_eq!(McpePacket { header, data }, valid);
        assert!(owned_cursor.is_empty());
        assert!(BorrowedMcpePacket::decode_inner_with_remaining(&mut cursor).is_err());
        assert_eq!(cursor, next);
        let (packet, remaining) =
            BorrowedMcpePacket::decode_inner_with_remaining(&mut cursor).unwrap();
        assert_eq!(remaining, 0);
        assert_eq!(packet.into_owned(McpePacketArgs).unwrap(), valid);
        assert!(cursor.is_empty());
    }
}

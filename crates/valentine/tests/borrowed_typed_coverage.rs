#![cfg(feature = "bedrock_1_26_51")]

use bytes::{Buf, BytesMut};
use valentine::bedrock::borrowed::BorrowedReader;
use valentine::bedrock::version::v1_26_51::*;

#[test]
fn corpus_packets_decode_as_typed_views_and_match_owned() {
    let corpus = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../valentine_gen/protocolgen/testdata/bench");
    let manifest: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(corpus.join("manifest.json")).unwrap()).unwrap();
    for entry in manifest {
        let name = entry["name"].as_str().unwrap();
        let file = entry["file"].as_str().unwrap();
        let batch = bytes::Bytes::from(std::fs::read(corpus.join(file)).unwrap());
        assert_eq!(batch[0], 0xfe, "{name}: batch marker");
        let entries = batch.slice(1..);
        let mut owned_input = BorrowedReader::new(&entries, entries.len());
        let mut borrowed_input = BorrowedReader::new(&entries, entries.len());
        let mut output = BytesMut::new();
        output.extend_from_slice(&[0xfe]);
        while owned_input.has_remaining() {
            let owned =
                McpePacketData::decode_inner_reader(&mut owned_input, McpePacketArgs).unwrap();
            let (borrowed, remaining) =
                BorrowedMcpePacket::decode_inner_reader(&mut borrowed_input)
                    .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(remaining, 0, "{name}: trailing borrowed payload");
            assert!(
                borrowed.data.raw_payload().is_none(),
                "{name}: untyped payload"
            );
            let materialized = borrowed.into_owned(McpePacketArgs).unwrap();
            assert_eq!(
                (materialized.header, &materialized.data),
                (owned.0, &owned.1),
                "{name}"
            );
            materialized
                .data
                .encode_inner_bytes_mut(
                    &mut output,
                    materialized.header.from_subclient,
                    materialized.header.to_subclient,
                )
                .unwrap();
        }
        assert!(!borrowed_input.has_remaining(), "{name}: trailing frame");
        assert_eq!(output.as_ref(), batch.as_ref(), "{name}: re-encoded wire");
    }
}

#[test]
fn owned_text_shares_valid_utf8_and_normalizes_invalid_utf8() {
    use valentine::bedrock::codec::BedrockCodec;

    let wire = bytes::Bytes::from_static(b"\x00\x03abc\x00");
    let owned = DisconnectPacketMessagesUnion::decode(&mut wire.clone(), ()).unwrap();
    let DisconnectPacketMessagesUnion::DisconnectPacketMessages(messages) = owned else {
        panic!("expected messages arm");
    };
    assert_eq!(messages.message.as_ptr(), wire[2..].as_ptr());

    let mut invalid = bytes::Bytes::from_static(b"\x00\x03a\xffb\x00");
    let owned = DisconnectPacketMessagesUnion::decode(&mut invalid, ()).unwrap();
    let mut normalized = BytesMut::new();
    owned.encode(&mut normalized).unwrap();
    assert_eq!(normalized.as_ref(), b"\x00\x05a\xef\xbf\xbdb\x00");
}

#[test]
fn borrowed_union_strings_share_input_and_convert_lossily() {
    let wire = bytes::Bytes::from_static(b"\x00\x03a\xffb\x04safe");
    let mut input = wire.clone();
    let view = DisconnectPacketMessagesUnionView::decode(&mut input).expect("typed union");
    assert!(input.is_empty());
    let DisconnectPacketMessagesUnionView::DisconnectPacketMessages(messages) = &view else {
        panic!("expected messages arm");
    };
    assert_eq!(messages.message.as_bytes().as_ptr(), wire[2..].as_ptr());
    assert_eq!(messages.message.as_bytes(), b"a\xffb");
    let mut encoded = BytesMut::new();
    view.encode(&mut encoded).expect("encode view");
    assert_eq!(encoded.freeze(), wire);
    let owned = DisconnectPacketMessagesUnion::from(view);
    let DisconnectPacketMessagesUnion::DisconnectPacketMessages(messages) = owned else {
        panic!("expected owned messages arm");
    };
    assert_eq!(messages.message, "a\u{fffd}b");
}

#[test]
fn borrowed_union_rejects_unknown_controls_and_truncated_payloads() {
    for wire in [&[2][..], &[0x80][..], &[0, 3, b'a'][..]] {
        assert!(
            DisconnectPacketMessagesUnionView::decode(&mut bytes::Bytes::copy_from_slice(wire))
                .is_err()
        );
    }
}

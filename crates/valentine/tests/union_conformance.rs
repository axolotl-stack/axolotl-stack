#![cfg(feature = "bedrock_1_26_51")]

use protocolgen_contract::wire::{Decode, Encode};
use valentine::bedrock::codec::{BedrockCodec, BedrockSized};
use valentine::bedrock::version::v1_26_51::{
    DisconnectPacket, DisconnectPacketMessages, DisconnectPacketMessagesUnion,
    EnumsConnectionDisconnectFailReason,
};

#[test]
fn disconnect_matches_contract_for_every_variant() {
    let cases = [
        (
            vec![0x6a, 0, 0, 0],
            DisconnectPacketMessagesUnion::default(),
        ),
        (vec![0x6a, 1], DisconnectPacketMessagesUnion::Empty1),
        (
            b"\x6a\x00\x03bye\x04safe".to_vec(),
            DisconnectPacketMessagesUnion::DisconnectPacketMessages(DisconnectPacketMessages {
                message: "bye".into(),
                filtered_message: "safe".into(),
            }),
        ),
    ];
    for (body, messages) in cases {
        let expected = DisconnectPacket {
            reason: EnumsConnectionDisconnectFailReason::Loginpacketnocert,
            messages,
        };
        let mut reader = body.as_slice();
        let packet = DisconnectPacket::decode(&mut reader, ()).unwrap();
        assert!(reader.is_empty(), "union control must not be dropped");
        assert_eq!(packet, expected);
        let mut encoded = Vec::new();
        expected.encode(&mut encoded).unwrap();
        assert_eq!(encoded, body);
        assert_eq!(expected.encoded_size(), body.len());
        let contract = protocolgen_contract::packets::Disconnect::decode_exact(&body).unwrap();
        assert_eq!(encoded, contract.encode_to_vec().unwrap());
        // Also cross-check the affected union independently of its packet wrapper.
        let contract_union =
            protocolgen_contract::types::DisconnectMessages::decode_exact(&body[1..]).unwrap();
        let mut union_bytes = Vec::new();
        expected.messages.encode(&mut union_bytes).unwrap();
        assert_eq!(union_bytes, contract_union.encode_to_vec().unwrap());
    }
}

#[test]
fn disconnect_rejects_unknown_and_truncated_union_controls() {
    for body in [&[0x6a, 2][..], &[0x6a, 0x80][..], &[0x6a, 0, 0][..]] {
        let mut reader = body;
        assert!(DisconnectPacket::decode(&mut reader, ()).is_err());
        assert!(protocolgen_contract::packets::Disconnect::decode_exact(body).is_err());
    }
}

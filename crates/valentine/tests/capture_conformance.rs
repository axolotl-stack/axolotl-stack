#![cfg(feature = "bedrock_1_26_51")]
//! Regression tests from packets captured off BDS 1.26.51 by the
//! `tools/client-diff` conformance harness: each must decode with no trailing
//! bytes and re-encode byte-for-byte through both the owned and borrowed paths.

use bytes::{Buf, Bytes};
use valentine::bedrock::codec::{BedrockCodec, Nbt};
use valentine::bedrock::version::v1_26_51::*;

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("fixture hex"))
        .collect()
}

/// Prefixes the batch-entry length the captures omit.
fn frame(packet: &[u8]) -> Bytes {
    let mut framed = Vec::with_capacity(packet.len() + 5);
    let mut len = u32::try_from(packet.len()).unwrap();
    while len >= 0x80 {
        framed.push(len as u8 | 0x80);
        len >>= 7;
    }
    framed.push(len as u8);
    framed.extend_from_slice(packet);
    framed.into()
}

/// One captured packet per LevelEventGeneric event ID seen in the captures.
fn level_event_generic_fixtures() -> Vec<(i32, Vec<u8>)> {
    include_str!("fixtures/conformance/level_event_generic.jsonl")
        .lines()
        .map(|line| {
            let value: serde_json::Value = serde_json::from_str(line).expect("fixture json");
            (
                i32::try_from(value["event_id"].as_i64().unwrap()).unwrap(),
                hex(value["hex"].as_str().unwrap()),
            )
        })
        .collect()
}

#[test]
fn captured_level_event_generic_packets_roundtrip_exactly() {
    let fixtures = level_event_generic_fixtures();
    assert_eq!(fixtures.len(), 9, "one fixture per captured event ID");
    for (event_id, packet) in fixtures {
        let (header, data) =
            McpePacketData::decode_inner_bytes(&mut frame(&packet), McpePacketArgs)
                .unwrap_or_else(|err| panic!("event {event_id}: owned decode: {err:?}"));
        let McpePacketData::LevelEventGenericPacket(generic) = &data else {
            panic!("event {event_id}: decoded {:?}", data.packet_id());
        };
        assert_eq!(generic.event_id, event_id);
        // The body is the whole rest of the packet, ending in TAG_End, and
        // reads as a complete compound once given a root header.
        assert_eq!(generic.__ctd__.0.last(), Some(&0));
        let mut rooted = generic.__ctd__.to_nbt().0;
        Nbt::decode(&mut rooted, ()).expect("event data as rooted compound");
        assert!(!rooted.has_remaining());

        let mut encoded = Vec::new();
        data.encode_inner(&mut encoded, header.from_subclient, header.to_subclient)
            .unwrap();
        assert_eq!(encoded, frame(&packet), "event {event_id}: owned re-encode");

        let (borrowed, remaining) =
            BorrowedMcpePacket::decode_inner_with_remaining(&mut frame(&packet))
                .unwrap_or_else(|err| panic!("event {event_id}: borrowed decode: {err:?}"));
        assert_eq!(remaining, 0, "event {event_id}: borrowed trailing bytes");
        let mut payload = Vec::new();
        borrowed.data.encode_payload(&mut payload).unwrap();
        assert_eq!(
            payload,
            packet[1..],
            "event {event_id}: borrowed re-encode (one-byte header)"
        );
        assert_eq!(borrowed.data.into_owned(McpePacketArgs).unwrap(), data);
    }
}

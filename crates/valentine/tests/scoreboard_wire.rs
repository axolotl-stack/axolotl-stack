#![cfg(feature = "bedrock_1_26_51")]
//! Independent byte fixtures for packet action, common rows and identity suffixes.

use bytes::{Buf, Bytes, BytesMut};
use valentine::bedrock::codec::{BedrockCodec, BedrockSized};
use valentine::bedrock::version::v1_26_51::*;

// Action 0, three rows: player 10, entity -5 and fake player "Hi". Each
// row repeats the common id/objective/little-endian score before its byte tag.
const CHANGE: &[u8] = &[
    0, 3, 2, 1, b's', 4, 3, 2, 1, 1, 20, 3, 1, b's', 255, 255, 255, 255, 2, 9, 6, 1, b's', 7, 0, 0,
    0, 3, 2, b'H', b'i',
];
// Action 1: complete common rows, with no identity tags or optional-string flags.
const REMOVE: &[u8] = &[1, 2, 1, 1, b's', 4, 3, 2, 1, 0, 0, 0, 0, 0, 0];

/// Semantic values specified independently of the encoder under test.
fn change() -> SetScorePacket {
    SetScorePacket {
        action: SetScoreAction::Change,
        entries: vec![
            ScoreEntry {
                scoreboard_id: 1,
                objective_name: "s".into(),
                score: 0x01020304,
                identity: Some(ScoreIdentity::Player(10)),
            },
            ScoreEntry {
                scoreboard_id: -2,
                objective_name: "s".into(),
                score: u32::MAX,
                identity: Some(ScoreIdentity::Entity(-5)),
            },
            ScoreEntry {
                scoreboard_id: 3,
                objective_name: "s".into(),
                score: 7,
                identity: Some(ScoreIdentity::FakePlayer("Hi".into())),
            },
        ],
    }
}
/// Removal preserves common score bytes while omitting all identity suffixes.
fn remove() -> SetScorePacket {
    SetScorePacket {
        action: SetScoreAction::Remove,
        entries: vec![
            ScoreEntry {
                scoreboard_id: -1,
                objective_name: "s".into(),
                score: 0x01020304,
                identity: None,
            },
            ScoreEntry {
                scoreboard_id: 0,
                objective_name: "".into(),
                score: 0,
                identity: None,
            },
        ],
    }
}

#[test]
fn change_and_remove_match_independent_body_bytes_in_owned_and_borrowed_paths() {
    for (packet, fixture) in [(change(), CHANGE), (remove(), REMOVE)] {
        let mut encoded = BytesMut::new();
        packet.encode(&mut encoded).unwrap();
        assert_eq!(encoded.as_ref(), fixture);
        assert_eq!(packet.encoded_size(), fixture.len());
        let mut bytes = Bytes::copy_from_slice(fixture);
        assert_eq!(SetScorePacket::decode(&mut bytes, ()).unwrap(), packet);
        assert!(!bytes.has_remaining());
        let mut bytes = Bytes::copy_from_slice(fixture);
        let view = SetScorePacketView::decode(&mut bytes).unwrap();
        assert!(!bytes.has_remaining());
        assert_eq!(view.encoded_size(), fixture.len());
        let mut encoded = BytesMut::new();
        view.encode(&mut encoded).unwrap();
        assert_eq!(encoded.as_ref(), fixture);
        assert_eq!(SetScorePacket::from(view), packet);
    }
}

#[test]
fn empty_operations_still_include_the_packet_action_byte() {
    for (action, fixture) in [
        (SetScoreAction::Change, &[0, 0][..]),
        (SetScoreAction::Remove, &[1, 0][..]),
    ] {
        let packet = SetScorePacket {
            action,
            entries: vec![],
        };
        let mut encoded = BytesMut::new();
        packet.encode(&mut encoded).unwrap();
        assert_eq!(encoded.as_ref(), fixture);
        assert_eq!(
            SetScorePacket::decode(&mut Bytes::copy_from_slice(fixture), ()).unwrap(),
            packet
        );
    }
}

#[test]
fn discriminator_bytes_reject_unknown_and_overlong_varint_spellings() {
    for fixture in [
        &[2, 0][..],
        &[128, 0, 0][..],
        &[0, 1, 2, 1, b's', 0, 0, 0, 0, 4, 20][..],
        &[0, 1, 2, 1, b's', 0, 0, 0, 0, 129, 0, 20][..],
    ] {
        assert!(SetScorePacket::decode(&mut Bytes::copy_from_slice(fixture), ()).is_err());
        assert!(SetScorePacketView::decode(&mut Bytes::copy_from_slice(fixture)).is_err());
    }
}

#[test]
fn incomplete_bodies_and_impossible_counts_fail_before_allocation() {
    for fixture in [CHANGE, REMOVE] {
        for end in 0..fixture.len() {
            assert!(
                SetScorePacket::decode(&mut Bytes::copy_from_slice(&fixture[..end]), ()).is_err(),
                "owned accepted truncated body at {end}"
            );
            assert!(
                SetScorePacketView::decode(&mut Bytes::copy_from_slice(&fixture[..end])).is_err(),
                "borrowed accepted truncated body at {end}"
            );
        }
    }
    let huge_count = &[0, 255, 255, 255, 255, 15];
    assert!(SetScorePacket::decode(&mut Bytes::from_static(huge_count), ()).is_err());
    assert!(SetScorePacketView::decode(&mut Bytes::from_static(huge_count)).is_err());
}

#[test]
fn inconsistent_action_identity_pairs_reject_without_partial_output() {
    for mut packet in [change(), remove()] {
        packet.action = if packet.action == SetScoreAction::Change {
            SetScoreAction::Remove
        } else {
            SetScoreAction::Change
        };
        let mut bytes = BytesMut::from(&b"unchanged"[..]);
        assert!(packet.encode(&mut bytes).is_err());
        assert_eq!(&bytes[..], b"unchanged");
    }
    let mut view = SetScorePacketView::decode(&mut Bytes::copy_from_slice(CHANGE)).unwrap();
    view.action = SetScoreAction::Remove;
    let mut bytes = BytesMut::new();
    assert!(view.encode(&mut bytes).is_err());
    assert!(bytes.is_empty());
}

#[test]
fn borrowed_strings_remain_lossless_until_owned_conversion() {
    let fixture = &[0, 1, 2, 1, 255, 0, 0, 0, 0, 3, 1, 254];
    let view = SetScorePacketView::decode(&mut Bytes::copy_from_slice(fixture)).unwrap();
    assert_eq!(view.entries[0].objective_name.as_bytes(), &[255]);
    let mut bytes = BytesMut::new();
    view.encode(&mut bytes).unwrap();
    assert_eq!(&bytes[..], fixture);
    let owned = SetScorePacket::from(view);
    assert_eq!(&*owned.entries[0].objective_name, "\u{fffd}");
    assert_eq!(
        SetScorePacket::decode(&mut Bytes::copy_from_slice(fixture), ()).unwrap(),
        owned
    );
}

#[test]
fn real_packet_dispatch_uses_correct_score_codec() {
    let packet = McpePacket::from(change());
    let mut bytes = BytesMut::new();
    packet.encode(&mut bytes).unwrap();
    let mut bytes = bytes.freeze();
    let decoded = McpePacket::decode(&mut bytes, McpePacketArgs).unwrap();
    assert_eq!(decoded, packet);
    assert!(!bytes.has_remaining());
    let mut bytes = BytesMut::new();
    packet.encode(&mut bytes).unwrap();
    let mut bytes = bytes.freeze();
    let decoded = BorrowedMcpePacket::decode_game_frame(&mut bytes).unwrap();
    let BorrowedMcpePacketData::SetScorePacket(view) = decoded.data else {
        panic!("score dispatch");
    };
    assert_eq!(SetScorePacket::from(view), change());
    assert!(!bytes.has_remaining());
}

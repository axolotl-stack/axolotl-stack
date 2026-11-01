#![cfg(feature = "bedrock_1_26_51")]
//! Protocol 2193 entry variants, independently specified from the encoder.

use bytes::{Buf, Bytes, BytesMut};
use valentine::bedrock::codec::{BedrockCodec, BedrockSized};
use valentine::bedrock::version::v1_26_51::*;

// Four independent rows: remove, player, entity and fake player. There is no
// packet-wide action. Each row begins with its varuint variant and action string.
const MIXED: &[u8] = &[
    4, 0, 6, b'r', b'e', b'm', b'o', b'v', b'e', 1, 1, 1, b's', 1, 12, b'c', b'h', b'a', b'n',
    b'g', b'e', b'p', b'l', b'a', b'y', b'e', b'r', 2, 1, b's', 255, 255, 255, 255, 20, 2, 12,
    b'c', b'h', b'a', b'n', b'g', b'e', b'e', b'n', b't', b'i', b't', b'y', 3, 1, b's', 4, 3, 2, 1,
    9, 3, 16, b'c', b'h', b'a', b'n', b'g', b'e', b'f', b'a', b'k', b'e', b'p', b'l', b'a', b'y',
    b'e', b'r', 6, 1, b's', 7, 0, 0, 0, 2, b'H', b'i',
];

/// All variants carry their own operation rather than selecting a packet action.
fn mixed() -> SetScorePacket {
    SetScorePacket {
        score_info: vec![
            SetScorePacketScoreInfoItem::RemoveScore(RemoveScorejson {
                action: "remove".into(),
                scoreboard_id: ScoreboardId { scoreboard_id: -1 },
                objective_name: Some("s".into()),
            }),
            SetScorePacketScoreInfoItem::ChangePlayerScore(ChangePlayerScorejson {
                action: "changeplayer".into(),
                scoreboard_id: ScoreboardId { scoreboard_id: 1 },
                objective_name: "s".into(),
                score_value: -1,
                player_unique_id: PlayerScoreboardId {
                    player_unique_id: 10,
                },
            }),
            SetScorePacketScoreInfoItem::ChangeEntityScore(ChangeEntityScorejson {
                action: "changeentity".into(),
                scoreboard_id: ScoreboardId { scoreboard_id: -2 },
                objective_name: "s".into(),
                score_value: 0x01020304,
                actor_id: ActorUniqueId {
                    actor_unique_id: -5,
                },
            }),
            SetScorePacketScoreInfoItem::ChangeFakePlayerScore(ChangeFakePlayerScorejson {
                action: "changefakeplayer".into(),
                scoreboard_id: ScoreboardId { scoreboard_id: 3 },
                objective_name: "s".into(),
                score_value: 7,
                fake_player_name: "Hi".into(),
            }),
        ],
    }
}

#[test]
fn mixed_score_entries_match_protocol_2193_bytes_and_encoded_sizes() {
    let packet = mixed();
    let mut output = BytesMut::new();
    packet.encode(&mut output).unwrap();
    assert_eq!(output.as_ref(), MIXED);
    assert_eq!(packet.encoded_size(), MIXED.len());
    let mut input = Bytes::from_static(MIXED);
    assert_eq!(SetScorePacket::decode(&mut input, ()).unwrap(), packet);
    assert!(!input.has_remaining());
}

#[test]
fn borrowed_score_entries_preserve_each_variant_and_optional_objective() {
    let mut input = Bytes::from_static(MIXED);
    let view = SetScorePacketView::decode(&mut input).unwrap();
    assert!(!input.has_remaining());
    assert_eq!(view.encoded_size(), MIXED.len());
    let mut output = BytesMut::new();
    view.encode(&mut output).unwrap();
    assert_eq!(output.as_ref(), MIXED);
    assert_eq!(SetScorePacket::from(view), mixed());
}

#[test]
fn removals_distinguish_absent_and_empty_objectives_without_score_values() {
    let bytes = [
        2, 0, 6, b'r', b'e', b'm', b'o', b'v', b'e', 2, 0, 0, 6, b'r', b'e', b'm', b'o', b'v',
        b'e', 4, 1, 0,
    ];
    let mut input = Bytes::copy_from_slice(&bytes);
    let packet = SetScorePacket::decode(&mut input, ()).unwrap();
    let [
        SetScorePacketScoreInfoItem::RemoveScore(first),
        SetScorePacketScoreInfoItem::RemoveScore(second),
    ] = packet.score_info.as_slice()
    else {
        panic!("remove variants");
    };
    assert_eq!(first.objective_name, None);
    assert_eq!(second.objective_name.as_deref(), Some(""));
    let mut output = BytesMut::new();
    packet.encode(&mut output).unwrap();
    assert_eq!(output.as_ref(), bytes);
}

#[test]
fn empty_score_updates_are_one_count_byte() {
    let mut output = BytesMut::new();
    SetScorePacket::default().encode(&mut output).unwrap();
    assert_eq!(output.as_ref(), [0]);
}

#[test]
fn truncated_variants_and_unknown_tags_fail_without_accepting_partial_rows() {
    for end in 0..MIXED.len() {
        let mut owned = Bytes::copy_from_slice(&MIXED[..end]);
        let mut borrowed = owned.clone();
        assert!(
            SetScorePacket::decode(&mut owned, ()).is_err(),
            "owned prefix {end}"
        );
        assert!(
            SetScorePacketView::decode(&mut borrowed).is_err(),
            "borrowed prefix {end}"
        );
    }
    for bytes in [&[1, 4][..], &[255, 255, 255, 255, 15][..]] {
        assert!(SetScorePacket::decode(&mut Bytes::copy_from_slice(bytes), ()).is_err());
        assert!(SetScorePacketView::decode(&mut Bytes::copy_from_slice(bytes)).is_err());
    }
}

#[test]
fn real_packet_dispatch_uses_the_same_entry_variant_codec() {
    let packet = McpePacket::from(mixed());
    let mut output = BytesMut::new();
    packet.encode(&mut output).unwrap();
    let encoded = output.freeze();
    let mut owned = encoded.clone();
    let decoded = McpePacket::decode(&mut owned, McpePacketArgs).unwrap();
    assert_eq!(decoded, packet);
    assert!(!owned.has_remaining());
    let mut borrowed = encoded;
    let decoded = BorrowedMcpePacket::decode_game_frame(&mut borrowed).unwrap();
    let BorrowedMcpePacketData::SetScorePacket(view) = decoded.data else {
        panic!("score dispatch");
    };
    assert_eq!(SetScorePacket::from(view), mixed());
    assert!(!borrowed.has_remaining());
}

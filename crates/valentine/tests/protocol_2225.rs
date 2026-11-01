#![cfg(feature = "bedrock_1_26_60")]
//! Version-specific wire vectors, including complete empty and occupied slots.

use bytes::{Buf, BytesMut};
use valentine::bedrock::borrowed::BorrowedReader;
use valentine::bedrock::codec::{BedrockCodec, BedrockSized};
use valentine::bedrock::version::v1_26_60::*;

const REQUEST_NETWORK_SETTINGS: &[u8] = &[0, 0, 8, 177];
const NETWORK_SETTINGS: &[u8] = &[0, 1, 1, 0, 1, 7, 0, 0, 0, 63];
const LOGIN: &[u8] = &[
    0, 0, 8, 177, 18, 99, 111, 100, 101, 99, 45, 111, 110, 108, 121, 45, 112, 97, 121, 108, 111,
    97, 100,
];
const START_GAME: &[u8] = &[
    84, 43, 0, 0, 0, 160, 63, 0, 0, 141, 66, 0, 0, 48, 192, 0, 0, 0, 0, 0, 0, 0, 0, 57, 48, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 4, 2, 136, 1, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 13, 107, 101, 101, 112, 105, 110, 118, 101, 110, 116, 111, 114,
    121, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 7, 49, 46, 50,
    54, 46, 54, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 116, 101, 115, 116, 45,
    108, 101, 118, 101, 108, 13, 80, 114, 111, 116, 111, 99, 111, 108, 32, 116, 101, 115, 116, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 7, 49, 46, 50, 54, 46, 54, 48, 10, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];
const SET_SCORE: &[u8] = &[
    2, 3, 16, 99, 104, 97, 110, 103, 101, 102, 97, 107, 101, 112, 108, 97, 121, 101, 114, 18, 7,
    115, 105, 100, 101, 98, 97, 114, 4, 0, 0, 0, 10, 83, 99, 111, 114, 101, 32, 108, 105, 110, 101,
    0, 6, 114, 101, 109, 111, 118, 101, 20, 0,
];
const INVENTORY_CONTENT: &[u8] = &[
    0, 2, 8, 1, 5, 0, 3, 1, 142, 1, 135, 173, 75, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 12, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];
const INVENTORY_SLOT: &[u8] = &[
    0, 8, 1, 12, 0, 0, 8, 1, 5, 0, 3, 1, 142, 1, 135, 173, 75, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];
const MOB_EQUIPMENT: &[u8] = &[
    43, 8, 1, 5, 0, 3, 1, 142, 1, 135, 173, 75, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 8, 0,
];
const LEVEL_CHUNK: &[u8] = &[2, 3, 0, 0, 1, 30, 0, 0, 3, 0, 1, 2, 1];

/// Reject a partial body and verify both the sized and actual encoding.
fn exact<T: BedrockCodec<Args = ()> + BedrockSized>(body: &[u8]) -> T {
    let mut input = body;
    let packet = T::decode(&mut input, ()).expect("complete 2225 vector");
    assert!(input.is_empty(), "must consume every wire field");
    let mut output = Vec::new();
    packet.encode(&mut output).unwrap();
    assert_eq!(output, body);
    assert_eq!(packet.encoded_size(), body.len());
    assert!(
        T::decode(&mut &body[..body.len() - 1], ()).is_err(),
        "truncated body must fail"
    );
    packet
}

#[test]
fn current_login_and_world_vectors_are_exact() {
    assert_eq!(PROTOCOL_VERSION, 2225);
    assert_eq!(GAME_VERSION, "1.26.60");
    assert_eq!(
        exact::<RequestNetworkSettingsPacket>(REQUEST_NETWORK_SETTINGS).client_network_version,
        2225
    );
    let network = exact::<NetworkSettingsPacket>(NETWORK_SETTINGS);
    assert_eq!(network.compression_threshold, 256);
    assert_eq!(
        network.compression_algorithm,
        EnumsPacketCompressionAlgorithm::Snappy
    );
    let login = exact::<LoginPacket>(LOGIN);
    assert_eq!(login.client_network_version, 2225);
    assert_eq!(login.connection_request.as_ref(), b"codec-only-payload");
    let start = exact::<StartGamePacket>(START_GAME);
    assert_eq!(start.entity_id.actor_unique_id, 42);
    assert_eq!(start.runtime_id.actor_runtime_id, 43);
    assert_eq!(start.level_name, "Protocol test");
    assert_eq!(start.settings.base_game_version, "1.26.60");
    assert_eq!(start.server_version, "1.26.60");
    assert_eq!(start.settings.game_difficulty, Difficultyjson::Normal);
    assert_eq!(start.settings.rule_data.rules_list.len(), 1);
    assert!(matches!(
        start.settings.rule_data.rules_list[0].rule_value,
        GameRulejsonRuleValue::Variant1(false)
    ));
}

#[test]
fn current_inventory_and_equipment_preserve_net_ids_and_slot_framing() {
    let content = exact::<InventoryContentPacket>(INVENTORY_CONTENT);
    assert_eq!(content.slots.len(), 2);
    assert_eq!(content.slots[0].id, 264);
    assert_eq!(content.slots[0].stacksize, 5);
    assert_eq!(content.slots[0].net_id_variant, Some(71));
    assert_eq!(content.slots[0].block_runtime_id, 1_234_567);
    assert_eq!(content.slots[1].id, 0);
    assert_eq!(content.slots[1].stacksize, 0);
    let slot = exact::<InventorySlotPacket>(INVENTORY_SLOT);
    assert_eq!(slot.slot, 8);
    assert!(slot.full_container_name.is_some());
    assert!(slot.storage_item.is_none());
    let equipment = exact::<MobEquipmentPacket>(MOB_EQUIPMENT);
    assert_eq!(equipment.target_runtime_id.actor_runtime_id, 43);
    assert_eq!(equipment.item, content.slots[0]);
    assert_eq!(equipment.selected_slot, 8);
}

#[test]
fn scoreboard_entry_controls_and_new_chunk_tail_are_exact() {
    let score = exact::<SetScorePacket>(SET_SCORE);
    assert_eq!(score.score_info.len(), 2);
    let SetScorePacketScoreInfoItem::ChangeFakePlayerScore(line) = &score.score_info[0] else {
        panic!("fake-player entry")
    };
    assert_eq!(line.objective_name, "sidebar");
    assert_eq!(line.fake_player_name, "Score line");
    assert!(matches!(
        score.score_info[1],
        SetScorePacketScoreInfoItem::RemoveScore(_)
    ));
    let chunk = exact::<LevelChunkPacket>(LEVEL_CHUNK);
    assert!(chunk.is_client_biome_update);
    assert_eq!(chunk.serialized_chunk_data.as_ref(), &[0, 1, 2]);
}

#[test]
fn all_current_vectors_use_typed_owned_and_borrowed_mcpe_framing() {
    let cases: &[(McpePacketName, &[u8])] = &[
        (
            McpePacketName::RequestNetworkSettingsPacket,
            REQUEST_NETWORK_SETTINGS,
        ),
        (McpePacketName::NetworkSettingsPacket, NETWORK_SETTINGS),
        (McpePacketName::LoginPacket, LOGIN),
        (McpePacketName::StartGamePacket, START_GAME),
        (McpePacketName::SetScorePacket, SET_SCORE),
        (McpePacketName::InventoryContentPacket, INVENTORY_CONTENT),
        (McpePacketName::InventorySlotPacket, INVENTORY_SLOT),
        (McpePacketName::MobEquipmentPacket, MOB_EQUIPMENT),
        (McpePacketName::LevelChunkPacket, LEVEL_CHUNK),
    ];
    for (name, body) in cases {
        let mut payload = BytesMut::new();
        valentine::bedrock::codec::VarUInt(*name as u32)
            .encode(&mut payload)
            .unwrap();
        payload.extend_from_slice(body);
        let mut frame = BytesMut::new();
        valentine::bedrock::codec::VarUInt(payload.len() as u32)
            .encode(&mut frame)
            .unwrap();
        frame.extend_from_slice(&payload);
        let frame = frame.freeze();
        let mut owned_input = BorrowedReader::new(&frame, frame.len());
        let (header, packet) =
            McpePacketData::decode_inner_reader(&mut owned_input, McpePacketArgs).unwrap();
        assert!(!owned_input.has_remaining());
        let mut borrowed_input = BorrowedReader::new(&frame, frame.len());
        let (borrowed, remaining) =
            BorrowedMcpePacket::decode_inner_reader(&mut borrowed_input).unwrap();
        assert_eq!(remaining, 0, "{name:?}");
        assert!(!borrowed_input.has_remaining());
        assert!(borrowed.data.raw_payload().is_none());
        let materialized = borrowed.into_owned(McpePacketArgs).unwrap();
        assert_eq!(materialized.header, header);
        assert_eq!(materialized.data, packet);
        let mut output = BytesMut::new();
        packet.encode_inner_bytes_mut(&mut output, 0, 0).unwrap();
        assert_eq!(output, frame, "{name:?}");
    }
}

#[test]
fn numeric_enum_aliases_encode_identically_and_decode_canonically() {
    let mut survival = Vec::new();
    GameTypejson::Survival.encode(&mut survival).unwrap();
    let mut alias = Vec::new();
    GameTypejson::WorldDefault.encode(&mut alias).unwrap();
    assert_eq!(survival, alias);
    assert_eq!(
        GameTypejson::decode(&mut alias.as_slice(), ()).unwrap(),
        GameTypejson::Survival
    );
}

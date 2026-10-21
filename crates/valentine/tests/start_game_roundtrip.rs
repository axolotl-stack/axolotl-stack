#![cfg(feature = "bedrock_1_26_51")]
use bytes::{Buf, BytesMut};
use valentine::bedrock::codec::{BedrockCodec, BedrockSized, Nbt};
use valentine::bedrock::version::v1_26_51::*;

#[test]
fn jolyne_start_game_roundtrip() {
    let packet = StartGamePacket {
        entity_id: ActorUniqueId { actor_unique_id: 1 },
        runtime_id: ActorRuntimeId {
            actor_runtime_id: 2,
        },
        game_type: EnumsGameType::Creative,
        position: Vec3 {
            x: 0.0,
            y: 64.0,
            z: -1.0,
        },
        rotation: Vec2 { x: 0.0, y: 90.0 },
        settings: LevelSettings {
            seed: 12345,
            spawn_settings: SpawnSettings {
                user_defined_biome_name: "plains".into(),
                dimension: 0,
                ..Default::default()
            },
            generator_type: EnumsGeneratorType::Overworld,
            game_type: EnumsGameType::Survival,
            game_difficulty: EnumsSharedTypesLegacyDifficulty::Easy,
            multiplayer_game_intent: true,
            lan_broadcast_intent: true,
            commands_enabled: true,
            player_permissions: EnumsPlayerPermissionLevel::Member,
            server_chunk_tick_range: 4,
            base_game_version: GAME_VERSION.into(),
            nether_type: true,
            override_force_experimental_gameplay: Some(false),
            ..Default::default()
        },
        level_name: "World".into(),
        block_properties: vec![ServerBlockProperty {
            block_name: "minecraft:stone".into(),
            block_definition: Nbt::default(),
        }],
        player_property_data: Nbt::default(),
        ..Default::default()
    };

    let mut buf = BytesMut::new();
    packet.encode(&mut buf).expect("encode failed");
    assert_eq!(buf.len(), packet.encoded_size());
    let mut reader = buf.freeze();
    let decoded = StartGamePacket::decode(&mut reader, ()).expect("decode failed");
    assert_eq!(packet, decoded);
    assert!(
        !reader.has_remaining(),
        "trailing bytes: {}",
        reader.remaining()
    );
}

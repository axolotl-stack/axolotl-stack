use bytes::{Buf, BytesMut};
use valentine::bedrock::codec::BedrockCodec;
use valentine::bedrock::protocol::v1_21_130::*;
use valentine_bedrock_core::bedrock::codec::Nbt;

#[test]
fn jolyne_start_game_roundtrip() {
    let packet = StartGamePacket {
        entity_id: 1,
        runtime_entity_id: 2,
        player_gamemode: GameMode::Creative,
        player_position: Vec3F {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        rotation: Vec2F { x: 0.0, z: 0.0 },
        seed: 12345,
        biome_type: 0,
        biome_name: "plains".to_string(),
        dimension: StartGamePacketDimension::Overworld,
        generator: 1,
        world_gamemode: GameMode::Survival,
        hardcore: false,
        difficulty: 1,
        spawn_position: BlockCoordinates { x: 0, y: 0, z: 0 },
        achievements_disabled: false,
        editor_world_type: StartGamePacketEditorWorldType::NotEditor,
        created_in_editor: false,
        exported_from_editor: false,
        day_cycle_stop_time: 0,
        edu_offer: 0,
        edu_features_enabled: false,
        edu_product_uuid: "".to_string(),
        rain_level: 0.0,
        lightning_level: 0.0,
        has_confirmed_platform_locked_content: false,
        is_multiplayer: true,
        broadcast_to_lan: true,
        xbox_live_broadcast_mode: 0,
        platform_broadcast_mode: 0,
        enable_commands: true,
        is_texturepacks_required: false,
        gamerules: vec![],
        experiments: vec![],
        experiments_previously_used: false,
        bonus_chest: false,
        map_enabled: false,
        permission_level: PermissionLevel::Member,
        server_chunk_tick_range: 4,
        has_locked_behavior_pack: false,
        has_locked_resource_pack: false,
        is_from_locked_world_template: false,
        msa_gamertags_only: false,
        is_from_world_template: false,
        is_world_template_option_locked: false,
        only_spawn_v_1_villagers: false,
        persona_disabled: false,
        custom_skins_disabled: false,
        emote_chat_muted: false,
        game_version: "1.21.0".to_string(),
        limited_world_width: 0,
        limited_world_length: 0,
        is_new_nether: true,
        edu_resource_uri: EducationSharedResourceUri {
            button_name: "".to_string(),
            link_uri: "".to_string(),
        },
        experimental_gameplay_override: false,
        chat_restriction_level: StartGamePacketChatRestrictionLevel::None,
        disable_player_interactions: false,
        server_identifier: "".to_string(),
        world_identifier: "".to_string(),
        scenario_identifier: "".to_string(),
        owner_identifier: "".to_string(),
        level_id: "".to_string(),
        world_name: "World".to_string(),
        premium_world_template_id: "".to_string(),
        is_trial: false,
        rewind_history_size: 0,
        server_authoritative_block_breaking: false,
        current_tick: 0,
        enchantment_seed: 0,
        block_properties: vec![BlockPropertiesItem {
            name: "minecraft:stone".to_string(),
            state: Nbt::default(),
        }],
        multiplayer_correlation_id: "".to_string(),
        server_authoritative_inventory: false,
        engine: "".to_string(),
        property_data: Nbt::default(),
        block_pallette_checksum: 0,
        world_template_id: uuid::Uuid::nil(),
        client_side_generation: false,
        block_network_ids_are_hashes: false,
        server_controlled_sound: false,
    };

    let mut buf = BytesMut::new();
    packet.encode(&mut buf).expect("encode failed");
    let encoded = buf.freeze();

    let mut reader = encoded.clone();
    let decoded = StartGamePacket::decode(&mut reader, ()).expect("decode failed");

    assert_eq!(packet, decoded);
    assert!(
        !reader.has_remaining(),
        "trailing bytes: {}",
        reader.remaining()
    );
}

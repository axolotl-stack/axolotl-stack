use bytes::{Buf, Bytes, BytesMut};
use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};

use valentine::bedrock::borrowed::{
    BorrowedDisconnectPacket, BorrowedLoginPacket, BorrowedTextPacket, RawMcpeFrame,
};
use valentine::bedrock::codec::{BedrockCodec, BedrockSized};
use valentine::bedrock::context::BedrockSession;
use valentine::bedrock::version::v1_26_30::*;
use valentine_bedrock_core::bedrock::codec::Nbt;

fn sample_disconnect_packet() -> DisconnectPacket {
    DisconnectPacket {
        reason: DisconnectFailReason::Timeout,
        hide_disconnect_reason: false,
        content: Some(DisconnectPacketContent {
            message: "Server maintenance in 5 minutes".to_string(),
            filtered_message: "Server maintenance".to_string(),
        }),
    }
}

fn sample_login_packet() -> LoginPacket {
    LoginPacket {
        protocol_version: 776,
        tokens: LoginTokens {
            identity: "{\"chain\":[{\"extraData\":{\"displayName\":\"Player\"}}]}".repeat(4),
            client: "{\"ClientRandomId\":1,\"ServerAddress\":\"127.0.0.1:19132\"}".repeat(3),
        },
    }
}

fn sample_text_packet() -> TextPacket {
    TextPacket {
        type_: TextPacketType::Chat,
        needs_translation: false,
        category: TextPacketCategory::Authored,
        content: Some(TextPacketContent::Chat(TextPacketContentAnnouncement {
            source_name: "PlayerName".to_string(),
            message: "Hello, world!".repeat(8),
        })),
        xuid: "1234567890123456".to_string(),
        platform_chat_id: "platform-chat-id".to_string(),
        filtered_message: Some("Hello, world!".repeat(8)),
    }
}

fn sample_start_game_packet() -> StartGamePacket {
    StartGamePacket {
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
        game_version: "1.26.30".to_string(),
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
        server_editor_connection_policy: 0,
        allow_anonymous_block_drops_in_editor_worlds: false,
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
        is_chat_logging: false,
        server_join_info: None,
    }
}

fn encode_to_bytes<T: BedrockCodec>(value: &T) -> Bytes {
    let mut buf = BytesMut::new();
    value.encode(&mut buf).expect("encode should succeed");
    buf.freeze()
}

fn bench_disconnect(c: &mut Criterion) {
    let packet = sample_disconnect_packet();
    let encoded = encode_to_bytes(&packet);
    let encoded_size = packet.encoded_size();

    c.bench_function("disconnect_encode", |b| {
        b.iter(|| {
            let mut buf = BytesMut::new();
            black_box(&packet).encode(&mut buf).unwrap();
            black_box(buf)
        });
    });

    c.bench_function("disconnect_encode_fresh_sized", |b| {
        b.iter(|| {
            let mut buf = BytesMut::with_capacity(encoded_size);
            black_box(&packet).encode(&mut buf).unwrap();
            black_box(buf)
        });
    });

    c.bench_function("disconnect_encode_reuse_buf", |b| {
        let mut buf = BytesMut::with_capacity(encoded_size);
        b.iter(|| {
            buf.clear();
            black_box(&packet).encode(&mut buf).unwrap();
            black_box(buf.len())
        });
    });

    c.bench_function("disconnect_encode_reuse_buf_copy", |b| {
        let mut buf = BytesMut::with_capacity(encoded_size);
        b.iter(|| {
            buf.clear();
            black_box(&packet).encode(&mut buf).unwrap();
            let copied = buf.as_ref().to_vec();
            black_box(copied)
        });
    });

    c.bench_function("disconnect_decode", |b| {
        b.iter_batched(
            || encoded.clone(),
            |mut reader| {
                let decoded = DisconnectPacket::decode(&mut reader, ()).unwrap();
                black_box(decoded);
                assert!(!reader.has_remaining());
            },
            BatchSize::SmallInput,
        );
    });

    c.bench_function("disconnect_borrowed_decode", |b| {
        b.iter_batched(
            || encoded.clone(),
            |mut reader| {
                let decoded = BorrowedDisconnectPacket::decode(&mut reader).unwrap();
                black_box(decoded);
                assert!(!reader.has_remaining());
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_text(c: &mut Criterion) {
    let packet = sample_text_packet();
    let encoded = encode_to_bytes(&packet);

    c.bench_function("text_encode", |b| {
        b.iter(|| {
            let mut buf = BytesMut::new();
            black_box(&packet).encode(&mut buf).unwrap();
            black_box(buf)
        });
    });

    c.bench_function("text_decode", |b| {
        b.iter_batched(
            || encoded.clone(),
            |mut reader| {
                let decoded = TextPacket::decode(&mut reader, ()).unwrap();
                black_box(decoded);
                assert!(!reader.has_remaining());
            },
            BatchSize::SmallInput,
        );
    });

    c.bench_function("text_borrowed_decode", |b| {
        b.iter_batched(
            || encoded.clone(),
            |mut reader| {
                let decoded = BorrowedTextPacket::decode(&mut reader).unwrap();
                black_box(decoded);
                assert!(!reader.has_remaining());
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_login(c: &mut Criterion) {
    let packet = sample_login_packet();
    let encoded = encode_to_bytes(&packet);

    c.bench_function("login_encode", |b| {
        b.iter(|| {
            let mut buf = BytesMut::new();
            black_box(&packet).encode(&mut buf).unwrap();
            black_box(buf)
        });
    });

    c.bench_function("login_decode", |b| {
        b.iter_batched(
            || encoded.clone(),
            |mut reader| {
                let decoded = LoginPacket::decode(&mut reader, ()).unwrap();
                black_box(decoded);
                assert!(!reader.has_remaining());
            },
            BatchSize::SmallInput,
        );
    });

    c.bench_function("login_borrowed_decode", |b| {
        b.iter_batched(
            || encoded.clone(),
            |mut reader| {
                let decoded = BorrowedLoginPacket::decode(&mut reader).unwrap();
                black_box(decoded);
                assert!(!reader.has_remaining());
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_start_game(c: &mut Criterion) {
    let packet = sample_start_game_packet();
    let encoded = encode_to_bytes(&packet);

    c.bench_function("start_game_encode", |b| {
        b.iter(|| {
            let mut buf = BytesMut::new();
            black_box(&packet).encode(&mut buf).unwrap();
            black_box(buf)
        });
    });

    c.bench_function("start_game_decode", |b| {
        b.iter_batched(
            || encoded.clone(),
            |mut reader| {
                let decoded = StartGamePacket::decode(&mut reader, ()).unwrap();
                black_box(decoded);
                assert!(!reader.has_remaining());
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_mcpe_frame(c: &mut Criterion) {
    let session = BedrockSession { shield_item_id: 0 };
    let packet = McpePacket::from(sample_text_packet());
    let args = McpePacketArgs::from(&session);
    let encoded = encode_to_bytes(&packet);
    let borrowed_login = McpePacket::from(sample_login_packet());
    let borrowed_login_encoded = encode_to_bytes(&borrowed_login);

    c.bench_function("mcpe_frame_encode", |b| {
        b.iter(|| {
            let mut buf = BytesMut::new();
            black_box(&packet).encode(&mut buf).unwrap();
            black_box(buf)
        });
    });

    c.bench_function("mcpe_frame_encode_fast", |b| {
        b.iter(|| {
            let mut buf = BytesMut::new();
            McpePacket::encode_bytes_mut(black_box(&packet), &mut buf).unwrap();
            black_box(buf)
        });
    });

    c.bench_function("mcpe_frame_decode", |b| {
        b.iter_batched(
            || encoded.clone(),
            |mut reader| {
                let decoded = McpePacket::decode(&mut reader, args.clone()).unwrap();
                black_box(decoded);
                assert!(!reader.has_remaining());
            },
            BatchSize::SmallInput,
        );
    });

    c.bench_function("mcpe_raw_frame_decode", |b| {
        b.iter_batched(
            || encoded.clone(),
            |mut reader| {
                let decoded = RawMcpeFrame::decode(&mut reader).unwrap();
                black_box(decoded);
                assert!(!reader.has_remaining());
            },
            BatchSize::SmallInput,
        );
    });

    c.bench_function("mcpe_borrowed_login_frame_decode", |b| {
        b.iter_batched(
            || borrowed_login_encoded.clone(),
            |mut reader| {
                let decoded = BorrowedMcpePacket::decode_game_frame(&mut reader).unwrap();
                black_box(decoded);
                assert!(!reader.has_remaining());
            },
            BatchSize::SmallInput,
        );
    });
}

fn codec_benches(c: &mut Criterion) {
    bench_disconnect(c);
    bench_login(c);
    bench_text(c);
    bench_start_game(c);
    bench_mcpe_frame(c);
}

criterion_group!(benches, codec_benches);
criterion_main!(benches);

use bytes::{Buf, Bytes, BytesMut};
use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};

use valentine::bedrock::borrowed::RawMcpeFrame;
use valentine::bedrock::codec::{BedrockCodec, BedrockSized};
use valentine::bedrock::version::v1_26_40::*;
use valentine_bedrock_core::bedrock::codec::Nbt;

fn sample_disconnect_packet() -> DisconnectPacket {
    DisconnectPacket {
        reason: EnumsConnectionDisconnectFailReason::Timeout,
        messages: DisconnectPacketMessages {
            message: "Server maintenance in 5 minutes".to_string(),
            filtered_message: "Server maintenance".to_string(),
        },
    }
}

fn sample_login_packet() -> LoginPacket {
    LoginPacket {
        client_network_version: PROTOCOL_VERSION,
        connection_request: br#"{\"chain\":[{\"extraData\":{\"displayName\":\"Player\"}}]}"#
            .repeat(4),
    }
}

fn sample_text_packet() -> TextPacket {
    TextPacket {
        localize: false,
        message_category: 0,
        body: TextPacketBody::Chat(TextPacketPayloadAuthorAndMessage {
            player_name: "PlayerName".to_string(),
            message: "Hello, world!".repeat(8),
        }),
        senders_xuid: "1234567890123456".to_string(),
        platform_id: "platform-chat-id".to_string(),
        filtered_message: Some("Hello, world!".repeat(8)),
    }
}

fn sample_start_game_packet() -> StartGamePacket {
    StartGamePacket {
        entity_id: ActorUniqueId { actor_unique_id: 1 },
        runtime_id: ActorRuntimeId {
            actor_runtime_id: 2,
        },
        game_type: EnumsGameType::Creative,
        position: Vec3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        rotation: Vec2 { x: 0.0, y: 0.0 },
        settings: LevelSettings {
            seed: 12345,
            game_type: EnumsGameType::Creative,
            ..Default::default()
        },
        level_name: "World".to_string(),
        level_current_time: 0,
        enchantment_seed: 0,
        block_properties: vec![ServerBlockProperty {
            block_name: "minecraft:stone".to_string(),
            block_definition: Nbt::default(),
        }],
        multiplayer_correlation_id: "".to_string(),
        server_version: "1.26.40".to_string(),
        world_template_id: uuid::Uuid::nil(),
        ..Default::default()
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
                let reason = EnumsConnectionDisconnectFailReason::decode(&mut reader, ()).unwrap();
                let messages = DisconnectPacketMessagesView::decode(&mut reader).unwrap();
                black_box((reason, messages));
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
    let packet = McpePacket::from(sample_text_packet());
    let args = McpePacketArgs;
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

//! Resource pack exchange tests: two codec-level connections pumped into
//! each other, covering ordering, chunk pacing, size and hash checks, and
//! refusals on both sides.

use super::*;
use crate::config::ConnectionOptions;
use crate::error::ProtocolError;
use crate::proto::conn::{Datagram, Framing};
use crate::valentine::McpePacketData;

fn conn() -> Connection {
    Connection::new(Framing::RakNet, ConnectionOptions::default())
}

/// Moves every sealed transmit from `from` into `to` and returns the decoded packets.
fn pump(from: &mut Connection, to: &mut Connection) -> Vec<McpePacket> {
    while let Some(transmit) = from.poll_transmit() {
        to.handle_datagram(Datagram::Contiguous(transmit.payload))
            .unwrap();
    }
    let mut out = Vec::new();
    while let Some(raw) = to.next_packet().unwrap() {
        out.push(raw.decode().unwrap());
    }
    out
}

fn pack(len: usize) -> Arc<ResourcePack> {
    let content: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
    Arc::new(ResourcePack::new(Uuid::from_u128(7), "1.2.3", content.into()).unwrap())
}

fn server_packs(packs: Vec<Arc<ResourcePack>>, interval: Duration) -> Arc<ServerPacks> {
    Arc::new(ServerPacks {
        packs,
        chunk_size: 1000,
        chunk_interval: interval,
        ..ServerPacks::default()
    })
}

/// Runs both halves against each other; returns the client's downloads and
/// the order of server packet kinds.
fn run(
    packs: Arc<ServerPacks>,
    policy: ClientPackPolicy,
) -> Result<(Vec<DownloadedPack>, Vec<&'static str>), JolyneError> {
    let now = Instant::now();
    let (mut server_conn, mut client_conn) = (conn(), conn());
    let mut server = ServerPackExchange::start(packs, &mut server_conn, now)?;
    let mut client = ClientPackExchange::new(policy);
    let mut kinds = Vec::new();
    let mut clock = now;
    for _ in 0..10_000 {
        for packet in pump(&mut server_conn, &mut client_conn) {
            kinds.push(match &packet.data {
                McpePacketData::ResourcePacksInfoPacket(_) => "info",
                McpePacketData::ResourcePackDataInfoPacket(_) => "data_info",
                McpePacketData::ResourcePackChunkDataPacket(_) => "chunk",
                McpePacketData::ResourcePackStackPacket(_) => "stack",
                _ => "other",
            });
            client.on_packet(&packet, &mut client_conn, clock)?;
        }
        let mut done = false;
        for packet in pump(&mut client_conn, &mut server_conn) {
            done |= server.on_packet(&packet, &mut server_conn, clock)? == PackProgress::Done;
        }
        if done {
            return Ok((client.into_downloaded().0, kinds));
        }
        if let Some(at) = server.timer() {
            clock = at;
            server.on_timer(&mut server_conn, clock)?;
        }
    }
    panic!("exchange did not finish");
}

#[test]
fn transfers_verifies_and_orders_stack_after_downloads() {
    let offered = pack(2_500);
    let (downloaded, kinds) = run(
        server_packs(vec![offered.clone()], Duration::ZERO),
        ClientPackPolicy::default(),
    )
    .unwrap();
    assert_eq!(downloaded.len(), 1);
    assert_eq!(&downloaded[0].content, offered.content());
    assert_eq!(&downloaded[0].sha256, offered.sha256());
    assert_eq!(
        kinds,
        ["info", "data_info", "chunk", "chunk", "chunk", "stack"],
        "stack only after the client finished downloading"
    );
}

#[test]
fn empty_offer_skips_to_stack_and_builtins_are_accepted() {
    let (downloaded, kinds) = run(
        Arc::new(ServerPacks::default()),
        ClientPackPolicy::default(),
    )
    .unwrap();
    assert!(downloaded.is_empty());
    assert_eq!(kinds, ["info", "stack"]);

    let mut client_conn = conn();
    let mut client = ClientPackExchange::new(ClientPackPolicy::default());
    let now = Instant::now();
    client
        .on_packet(
            &McpePacket::from(ResourcePacksInfoPacket::default()),
            &mut client_conn,
            now,
        )
        .unwrap();
    let (uuid, version) = BUILTIN_PACKS[0];
    let stack = McpePacket::from(ResourcePackStackPacket {
        texture_pack_list: vec![PackInstanceId {
            pack_id: uuid.into(),
            version: version.into(),
            sub_pack_name: "".into(),
        }],
        ..Default::default()
    });
    assert_eq!(
        client.on_packet(&stack, &mut client_conn, now).unwrap(),
        PackProgress::Done
    );
}

#[test]
fn stack_entries_must_be_downloaded_builtin_or_declined() {
    let mut client_conn = conn();
    let mut client = ClientPackExchange::new(ClientPackPolicy::default());
    let now = Instant::now();
    client
        .on_packet(
            &McpePacket::from(ResourcePacksInfoPacket::default()),
            &mut client_conn,
            now,
        )
        .unwrap();
    let stack = McpePacket::from(ResourcePackStackPacket {
        texture_pack_list: vec![PackInstanceId {
            pack_id: "00000000-0000-0000-0000-000000000001".into(),
            version: "1.0.0".into(),
            sub_pack_name: "".into(),
        }],
        ..Default::default()
    });
    assert!(matches!(
        client.on_packet(&stack, &mut client_conn, now),
        Err(JolyneError::Protocol(ProtocolError::Pack(
            PackError::MissingStackPack(_)
        )))
    ));
}

#[test]
fn declined_packs_are_not_downloaded() {
    let policy = ClientPackPolicy {
        filter: Some(Arc::new(|_: &PackOffer| false)),
        ..ClientPackPolicy::default()
    };
    let (downloaded, kinds) = run(server_packs(vec![pack(1_500)], Duration::ZERO), policy).unwrap();
    assert!(downloaded.is_empty());
    assert_eq!(kinds, ["info", "stack"]);
}

#[test]
fn chunk_pacing_holds_replies_on_a_timer() {
    let now = Instant::now();
    let interval = Duration::from_millis(200);
    let mut server_conn = conn();
    let mut server = ServerPackExchange::start(
        server_packs(vec![pack(3_000)], interval),
        &mut server_conn,
        now,
    )
    .unwrap();
    let request = |chunk| {
        McpePacket::from(ResourcePackChunkRequestPacket {
            resource_name: format!("{}_1.2.3", Uuid::from_u128(7)).into(),
            chunk,
        })
    };
    server
        .on_packet(
            &response(ResourcePackClientResponsePacketResponse::Downloading(
                ResourcePackClientResponseDownloadingjson {
                    response_type: "downloading".into(),
                    downloading_packs: vec![format!("{}_1.2.3", Uuid::from_u128(7)).into()],
                },
            )),
            &mut server_conn,
            now,
        )
        .unwrap();
    let batches = server_conn.metrics().batches_out;
    for chunk in 0..3 {
        server
            .on_packet(&request(chunk), &mut server_conn, now)
            .unwrap();
    }
    assert_eq!(
        server_conn.metrics().batches_out,
        batches + 1,
        "only chunk 0 sent"
    );
    assert_eq!(server.timer(), Some(now + interval));
    server
        .on_timer(&mut server_conn, now + interval / 2)
        .unwrap();
    assert_eq!(server_conn.metrics().batches_out, batches + 1);
    server.on_timer(&mut server_conn, now + interval).unwrap();
    assert_eq!(server_conn.metrics().batches_out, batches + 2);
    server
        .on_timer(&mut server_conn, now + interval * 2)
        .unwrap();
    assert_eq!(server_conn.metrics().batches_out, batches + 3);
    assert_eq!(server.timer(), None);
}

#[test]
fn server_rejects_out_of_order_unknown_and_refused() {
    let now = Instant::now();
    let mut server_conn = conn();
    let packs = Arc::new(ServerPacks {
        required: true,
        ..(*server_packs(vec![pack(3_000)], Duration::ZERO)).clone()
    });
    let mut server = ServerPackExchange::start(packs, &mut server_conn, now).unwrap();
    let downloading = |name: String| {
        response(ResourcePackClientResponsePacketResponse::Downloading(
            ResourcePackClientResponseDownloadingjson {
                response_type: "downloading".into(),
                downloading_packs: vec![name.into()],
            },
        ))
    };
    assert!(matches!(
        server.on_packet(&downloading("nope_1.0.0".into()), &mut server_conn, now),
        Err(JolyneError::Protocol(ProtocolError::Pack(
            PackError::UnknownPack(_)
        )))
    ));
    let name = format!("{}_1.2.3", Uuid::from_u128(7));
    server
        .on_packet(&downloading(name.clone()), &mut server_conn, now)
        .unwrap();
    let skip = McpePacket::from(ResourcePackChunkRequestPacket {
        resource_name: name.clone().into(),
        chunk: 1,
    });
    assert!(matches!(
        server.on_packet(&skip, &mut server_conn, now),
        Err(JolyneError::Protocol(ProtocolError::Pack(
            PackError::ChunkOutOfOrder {
                index: 1,
                expected: 0,
                ..
            }
        )))
    ));
    let refused = response(ResourcePackClientResponsePacketResponse::Cancel(
        Default::default(),
    ));
    assert!(matches!(
        server.on_packet(&refused, &mut server_conn, now),
        Err(JolyneError::Protocol(ProtocolError::Pack(
            PackError::Refused
        )))
    ));
}

#[test]
fn client_bounds_offers_chunk_size_and_hash() {
    let now = Instant::now();
    let offer_info = |size| {
        McpePacket::from(ResourcePacksInfoPacket {
            resource_packs: vec![pack(10).info()]
                .into_iter()
                .map(|mut info| {
                    info.pack_size = size;
                    info
                })
                .collect(),
            ..Default::default()
        })
    };
    let small = ClientPackPolicy {
        max_pack_size: 100,
        ..ClientPackPolicy::default()
    };
    let mut client = ClientPackExchange::new(small);
    assert!(matches!(
        client.on_packet(&offer_info(101), &mut conn(), now),
        Err(JolyneError::Protocol(ProtocolError::Pack(
            PackError::PackTooLarge { .. }
        )))
    ));

    let mut client_conn = conn();
    let mut client = ClientPackExchange::new(ClientPackPolicy::default());
    client
        .on_packet(&offer_info(10), &mut client_conn, now)
        .unwrap();
    let name = format!("{}_1.2.3", Uuid::from_u128(7));
    let data_info = |chunk_size, hash: &[u8]| {
        McpePacket::from(ResourcePackDataInfoPacket {
            resource_name: name.clone().into(),
            chunk_size,
            numberof_chunks: 1,
            file_size: 10,
            file_hash: Bytes::copy_from_slice(hash),
            is_premium_pack: false,
            pack_type: PackType::Resources as u8,
        })
    };
    assert!(matches!(
        client.on_packet(&data_info(0, &[0; 32]), &mut client_conn, now),
        Err(JolyneError::Protocol(ProtocolError::Pack(
            PackError::InvalidChunkSize { size: 0, .. }
        )))
    ));
    client
        .on_packet(&data_info(10, &[0; 32]), &mut client_conn, now)
        .unwrap();
    let chunk = McpePacket::from(ResourcePackChunkDataPacket {
        resource_name: name.clone().into(),
        chunk_id: 0,
        byte_offset: 0,
        chunk_data: pack(10).content().clone(),
    });
    assert!(matches!(
        client.on_packet(&chunk, &mut client_conn, now),
        Err(JolyneError::Protocol(ProtocolError::Pack(
            PackError::HashMismatch(_)
        )))
    ));
}

/// Regression: the client trusted `ResourcePackDataInfo.file_size` and
/// allocated it up front, bypassing the advertised-size caps.
#[test]
fn data_info_must_match_the_advertised_size() {
    let now = Instant::now();
    let mut client_conn = conn();
    let mut client = ClientPackExchange::new(ClientPackPolicy::default());
    let mut info = pack(10).info();
    info.pack_size = 10;
    client
        .on_packet(
            &McpePacket::from(ResourcePacksInfoPacket {
                resource_packs: vec![info],
                ..Default::default()
            }),
            &mut client_conn,
            now,
        )
        .unwrap();
    let inflated = McpePacket::from(ResourcePackDataInfoPacket {
        resource_name: format!("{}_1.2.3", Uuid::from_u128(7)).into(),
        chunk_size: 1024,
        numberof_chunks: 512 * 1024,
        file_size: 512 * 1024 * 1024,
        file_hash: Bytes::copy_from_slice(&[0; 32]),
        is_premium_pack: false,
        pack_type: PackType::Resources as u8,
    });
    assert!(matches!(
        client.on_packet(&inflated, &mut client_conn, now),
        Err(JolyneError::Protocol(ProtocolError::Pack(
            PackError::Invalid(_, _)
        )))
    ));
}

#[test]
fn stack_finished_before_stack_is_rejected() {
    let now = Instant::now();
    let mut server_conn = conn();
    let mut server =
        ServerPackExchange::start(server_packs(vec![], Duration::ZERO), &mut server_conn, now)
            .unwrap();
    let finished = response(
        ResourcePackClientResponsePacketResponse::ResourcePackStackFinished(Default::default()),
    );
    assert!(server.on_packet(&finished, &mut server_conn, now).is_err());
}

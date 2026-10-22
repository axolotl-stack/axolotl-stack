//! The server and client cores joined against each other in memory: no
//! transport, no clock, every transition observable.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::BytesMut;
use p384::SecretKey;
use parking_lot::Mutex;

use super::client::{ClientHandshake, ClientLogin, ClientSettings, ClientSpawn};
use super::conn::{Connection, Datagram, Framing};
use super::custom::{CustomPacket, PacketRegistry, UnknownPacketPolicy, packet_name};
use super::observe::{Direction, PacketEvent};
use super::phase::{Owner, Phase, Step};
use super::raw::{PacketHeader, RawPacket};
use super::server::{ServerHandshake, ServerSession, ServerSpawn, WorldReady};
use crate::config::{BedrockListenerConfig, ClientConfig, ConnectionOptions};
use crate::error::{HandshakePhase, JolyneError, LoginRefusal, ProtocolError};
use crate::valentine::{
    ActorRuntimeId, EnumsConnectionDisconnectFailReason, McpePacket, McpePacketName,
    PlayStatusPacketStatus, RequestChunkRadiusPacket, RequestNetworkSettingsPacket,
    SetLocalPlayerAsInitializedPacket, TextPacket,
};
use crate::world::{WorldJoinParams, WorldTemplate};

const PEER: SocketAddr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), 1);

fn advance<P: Phase>(phase: P) -> Step<P, P::Next> {
    match phase.advance(Instant::now()) {
        Ok(step) => step,
        Err(failure) => panic!("{:?} failed: {}", P::PHASE, failure.error),
    }
}

fn wait<P: Phase>(phase: P) -> P {
    match advance(phase) {
        Step::Wait(phase) => phase,
        Step::Next(_) => panic!("{:?} advanced early", P::PHASE),
    }
}

fn next<P: Phase>(phase: P) -> P::Next {
    match advance(phase) {
        Step::Next(next) => next,
        Step::Wait(_) => panic!("{:?} did not complete", P::PHASE),
    }
}

fn error_of<P: Phase>(phase: P, now: Instant) -> JolyneError {
    match phase.advance(now) {
        Err(failure) => failure.error,
        Ok(_) => panic!("{:?} should have failed", P::PHASE),
    }
}

/// Moves every transmit `from` produced into `to`.
fn pump(from: &mut impl Owner, to: &mut impl Owner) {
    while let Some(transmit) = from.poll_transmit() {
        to.connection()
            .handle_datagram(Datagram::Contiguous(transmit.payload))
            .expect("peer decodes every batch");
    }
}

type Log = Arc<Mutex<Vec<McpePacketName>>>;

fn recording(log: &Log) -> ConnectionOptions {
    let log = log.clone();
    ConnectionOptions {
        observer: Some(Arc::new(move |event: PacketEvent<'_>| {
            if event.direction == Direction::Outbound {
                log.lock()
                    .push(packet_name(event.header.id).expect("known id"));
            }
        })),
        ..ConnectionOptions::default()
    }
}

struct Harness {
    config: Arc<BedrockListenerConfig>,
    server_key: SecretKey,
    client_key: SecretKey,
    sent: Log,
}

impl Harness {
    fn new(config: BedrockListenerConfig) -> Self {
        let sent = Log::default();
        Self {
            config: Arc::new(BedrockListenerConfig {
                connection: recording(&sent),
                ..config
            }),
            server_key: SecretKey::random(&mut rand::thread_rng()),
            client_key: SecretKey::random(&mut rand::thread_rng()),
            sent,
        }
    }

    fn start(&self) -> (ServerHandshake, ClientSettings) {
        let now = Instant::now();
        let server = ServerHandshake::new(
            Connection::new(Framing::RakNet, self.config.connection.clone()),
            self.config.clone(),
            now,
        );
        let client_config = ClientConfig::default();
        let client = ClientSettings::new(
            Connection::new(Framing::RakNet, client_config.connection.clone()),
            &client_config,
            now,
        )
        .unwrap();
        (server, client)
    }

    fn login_request(&self) -> bytes::Bytes {
        let (chain, token) = crate::auth::client::generate_self_signed_chain(
            &self.client_key,
            "Tester",
            uuid::Uuid::new_v4(),
        )
        .unwrap();
        crate::auth::client::encode_login_request(&chain, &token).into()
    }

    /// Runs settings, login and encryption; returns both sides at the
    /// resource pack phase.
    async fn through_login(
        &self,
    ) -> Result<(super::server::PacksReady, super::client::ClientPacks), JolyneError> {
        let (mut server, mut client) = self.start();
        pump(&mut client, &mut server);
        let mut server = next(server);
        pump(&mut server, &mut client);
        let login: ClientLogin = next(client);
        let mut client: ClientHandshake = login
            .login(
                self.client_key.clone(),
                self.login_request(),
                Instant::now(),
            )
            .map_err(|failure| failure.error)?;
        pump(&mut client, &mut server);
        let request = next(server);
        let identity =
            crate::stream::server::verify_login(request.connection_request(), false, true).await?;
        let authenticated = request
            .authenticated(identity, PEER, Instant::now())
            .map_err(|failure| failure.error)?;
        let mut encryption = match authenticated.start_encryption(&self.server_key, Instant::now())
        {
            Ok(Step::Wait(encryption)) => encryption,
            Ok(Step::Next(_)) => panic!("encryption is enabled"),
            Err(failure) => return Err(failure.error),
        };
        pump(&mut encryption, &mut client);
        let mut client = wait(client);
        pump(&mut client, &mut encryption);
        let mut ready = next(encryption);
        pump(&mut ready, &mut client);
        Ok((ready, next(client)))
    }

    /// Runs the whole join up to the play sessions.
    async fn join(&self, params: WorldJoinParams) -> (ServerSession, super::client::ClientJoined) {
        let (spawn, client) = self.through_packs(params).await;
        finish_spawn(spawn, client)
    }

    async fn through_packs(&self, params: WorldJoinParams) -> (ServerSpawn, ClientSpawn) {
        let (ready, mut client) = self.through_login().await.unwrap();
        let mut packs = ready
            .start_packs(Instant::now())
            .map_err(|f| f.error)
            .unwrap();
        pump(&mut client, &mut packs); // ClientCacheStatus: deferred by the server
        pump(&mut packs, &mut client); // ResourcePacksInfo
        let mut client = wait(client);
        pump(&mut client, &mut packs); // DownloadingFinished
        let mut packs = wait(packs);
        pump(&mut packs, &mut client); // ResourcePackStack
        let mut client: ClientSpawn = next(client);
        pump(&mut client, &mut packs); // Completed
        let world: WorldReady = next(packs);
        let spawn = world
            .start_game(params, Instant::now())
            .map_err(|f| f.error)
            .unwrap();
        (spawn, client)
    }
}

fn finish_spawn(
    mut spawn: ServerSpawn,
    mut client: ClientSpawn,
) -> (ServerSession, super::client::ClientJoined) {
    pump(&mut spawn, &mut client); // StartGame .. CreativeContent
    let mut client = wait(client); // sends RequestChunkRadius
    pump(&mut client, &mut spawn);
    let mut spawn = wait(spawn); // radius, publisher, PlayerSpawn
    pump(&mut spawn, &mut client);
    let mut joined = next(client); // sends SetLocalPlayerAsInitialized
    pump(&mut joined.session, &mut spawn);
    (next(spawn), joined)
}

fn params(runtime_id: i64) -> WorldJoinParams {
    WorldTemplate::default().to_join_params(runtime_id)
}

/// ROADMAP M3: the server's packet order through PlayerSpawn, with the stack
/// only after the client's first pack response and the structure packets
/// before StartGame (vanilla/BDS ordering; the client disconnects with
/// "Missing structure data from server." without JigsawStructureData).
#[tokio::test]
async fn join_sequence_order_matches_vanilla() {
    use McpePacketName as N;
    let harness = Harness::new(BedrockListenerConfig {
        online_mode: false,
        ..Default::default()
    });
    let mut params = params(42);
    params.dimension_data = Some(Arc::new(crate::valentine::DimensionDataPacket::default()));
    let (server, client) = harness.join(params).await;
    assert_eq!(server.session.runtime_id(), 42);
    assert_eq!(client.session.session.runtime_id(), 42);
    assert_eq!(server.session.chunk_radius(), Some(16));
    assert_eq!(client.session.session.chunk_radius(), Some(16));
    assert_eq!(
        *harness.sent.lock(),
        [
            N::NetworkSettingsPacket,
            N::ServerToClientHandshakePacket,
            N::PlayStatusPacket,
            N::ResourcePacksInfoPacket,
            N::ResourcePackStackPacket,
            N::DimensionDataPacket,
            N::JigsawStructureDataPacket,
            N::VoxelShapesPacket,
            N::StartGamePacket,
            N::ItemRegistryPacket,
            N::BiomeDefinitionListPacket,
            N::AvailableActorIdentifiersPacket,
            N::CreativeContentPacket,
            N::ChunkRadiusUpdatedPacket,
            N::NetworkChunkPublisherUpdatePacket,
            N::PlayStatusPacket,
        ]
    );
    // The client keeps the structure packets for proxies to replay.
    let game_data = &client.game_data;
    assert!(game_data.dimension_data.is_some());
    assert_eq!(
        game_data.jigsaw_structures,
        Some(crate::world::empty_jigsaw_structures())
    );
    assert_eq!(
        game_data.voxel_shapes,
        Some(crate::valentine::VoxelShapesPacket::default())
    );
}

#[tokio::test]
async fn packets_before_their_phase_are_replayed_into_play_in_order() {
    let harness = Harness::new(BedrockListenerConfig {
        online_mode: false,
        ..Default::default()
    });
    let (mut server, mut client) = harness.join(params(7)).await;
    // The client's early ClientCacheStatus reached the server session.
    let first = server.connection().next_packet().unwrap().unwrap();
    assert!(first.is(McpePacketName::ClientCacheStatusPacket));
    // The client keeps definitions and the radius update for play.
    let mut client_seen = Vec::new();
    while let Some(packet) = client.session.connection().next_packet().unwrap() {
        client_seen.push(packet.name().unwrap());
    }
    assert_eq!(
        client_seen,
        [
            McpePacketName::BiomeDefinitionListPacket,
            McpePacketName::AvailableActorIdentifiersPacket,
            McpePacketName::CreativeContentPacket,
            McpePacketName::ChunkRadiusUpdatedPacket,
            McpePacketName::NetworkChunkPublisherUpdatePacket,
        ]
    );
}

#[tokio::test]
async fn compression_and_encryption_are_active_in_play() {
    for algorithm in [
        super::batch::BatchCompression::Deflate,
        super::batch::BatchCompression::Snappy,
        super::batch::BatchCompression::None,
    ] {
        let harness = Harness::new(BedrockListenerConfig {
            online_mode: false,
            compression: crate::config::CompressionConfig {
                algorithm,
                level: 7,
                threshold: 0,
            },
            ..Default::default()
        });
        let (mut server, mut client) = harness.join(params(1)).await;
        assert!(server.connection().is_encrypted() && client.session.connection().is_encrypted());
        server
            .connection()
            .queue_packet(&McpePacket::from(TextPacket::default()), Instant::now())
            .unwrap();
        let transmit = server.connection().poll_transmit().unwrap();
        assert_eq!(transmit.payload[0], 0xFE);
        client
            .session
            .connection()
            .handle_datagram(Datagram::Contiguous(transmit.payload))
            .unwrap();
        let mut delivered = false;
        while let Some(packet) = client.session.connection().next_packet().unwrap() {
            delivered |= packet.is(McpePacketName::TextPacket);
        }
        assert!(delivered, "text not delivered with {algorithm:?}");
    }
}

#[tokio::test]
async fn runtime_id_mismatch_is_rejected() {
    let harness = Harness::new(BedrockListenerConfig {
        online_mode: false,
        ..Default::default()
    });
    let (mut spawn, mut client) = harness.through_packs(params(5)).await;
    pump(&mut spawn, &mut client);
    let now = Instant::now();
    client
        .connection()
        .queue_packet(
            &McpePacket::from(SetLocalPlayerAsInitializedPacket {
                player_id: ActorRuntimeId {
                    actor_runtime_id: 6,
                },
            }),
            now,
        )
        .unwrap();
    pump(&mut client, &mut spawn);
    assert!(matches!(
        error_of(spawn, now),
        JolyneError::Protocol(ProtocolError::RuntimeIdMismatch {
            expected: 5,
            got: 6
        })
    ));
}

#[tokio::test]
async fn chunk_radius_requires_one_and_honours_the_server_override() {
    let harness = Harness::new(BedrockListenerConfig {
        online_mode: false,
        ..Default::default()
    });
    let request = |radius| {
        McpePacket::from(RequestChunkRadiusPacket {
            chunk_radius: radius,
            max_chunk_radius: 0,
        })
    };
    let (mut spawn, mut client) = harness.through_packs(params(1)).await;
    client
        .connection()
        .queue_packet(&request(0), Instant::now())
        .unwrap();
    pump(&mut client, &mut spawn);
    assert!(matches!(
        error_of(spawn, Instant::now()),
        JolyneError::Protocol(ProtocolError::InvalidChunkRadius(0))
    ));

    let (spawn, client) = harness
        .through_packs(WorldJoinParams {
            chunk_radius: Some(4),
            ..params(1)
        })
        .await;
    let (server, client) = finish_spawn(spawn, client);
    assert_eq!(server.session.chunk_radius(), Some(4));
    assert_eq!(client.session.session.chunk_radius(), Some(4));
}

#[tokio::test]
async fn each_stage_has_a_mandatory_deadline() {
    let harness = Harness::new(BedrockListenerConfig {
        online_mode: false,
        ..Default::default()
    });
    let (server, _client) = harness.start();
    let late = Instant::now() + harness.config.login_timeout;
    assert!(matches!(
        error_of(server, late),
        JolyneError::Protocol(ProtocolError::Timeout(HandshakePhase::NetworkSettings))
    ));

    let (spawn, _client) = harness.through_packs(params(1)).await;
    let late = Instant::now() + harness.config.spawn_timeout;
    assert!(matches!(
        error_of(spawn, late),
        JolyneError::Protocol(ProtocolError::Timeout(HandshakePhase::Spawn))
    ));
}

#[tokio::test]
async fn protocol_mismatch_answers_play_status_and_client_reports_refusal() {
    let harness = Harness::new(BedrockListenerConfig::default());
    let (mut server, mut client) = harness.start();
    let _ = client.connection().poll_transmit(); // drop the correct request
    let mut raw = Connection::new(Framing::RakNet, ConnectionOptions::default());
    raw.queue_packet(
        &McpePacket::from(RequestNetworkSettingsPacket {
            client_network_version: 1,
        }),
        Instant::now(),
    )
    .unwrap();
    while let Some(transmit) = raw.poll_transmit() {
        server
            .connection()
            .handle_datagram(Datagram::Contiguous(transmit.payload))
            .unwrap();
    }
    let failure = match server.advance(Instant::now()) {
        Err(failure) => failure,
        Ok(_) => panic!("old protocol accepted"),
    };
    assert!(matches!(
        failure.error,
        JolyneError::Protocol(ProtocolError::IncompatibleProtocol {
            client_protocol: 1,
            ..
        })
    ));
    assert_eq!(
        failure.error.disconnect_reason(),
        None,
        "PlayStatus already sent"
    );
    let mut server = *failure.state;
    pump(&mut server, &mut client);
    assert!(matches!(
        error_of(client, Instant::now()),
        JolyneError::LoginRefused(PlayStatusPacketStatus::LoginfailedClientold)
    ));
}

#[tokio::test]
async fn server_full_and_admission_refusals_reach_the_client_typed() {
    let harness = Harness::new(BedrockListenerConfig {
        online_mode: false,
        ..Default::default()
    });
    let (mut server, mut client) = harness.start();
    pump(&mut client, &mut server);
    let mut server = next(server);
    pump(&mut server, &mut client);
    let mut client = next(client)
        .login(
            harness.client_key.clone(),
            harness.login_request(),
            Instant::now(),
        )
        .map_err(|f| f.error)
        .unwrap();
    pump(&mut client, &mut server);
    let request = next(server);
    let failure = request.refuse_server_full(Instant::now());
    assert!(matches!(
        failure.error,
        JolyneError::Refused(LoginRefusal::ServerFull)
    ));
    let mut refused = *failure.state;
    pump(&mut refused, &mut client);
    assert!(matches!(
        error_of(client, Instant::now()),
        JolyneError::LoginRefused(PlayStatusPacketStatus::LoginfailedServerfullsubclient)
    ));

    let denying = Harness::new(BedrockListenerConfig {
        online_mode: false,
        admission: Some(Arc::new(
            |_: SocketAddr, _: &crate::auth::AuthenticatedLogin| Err("whitelist".to_string()),
        )),
        ..Default::default()
    });
    let (mut server, mut client) = denying.start();
    pump(&mut client, &mut server);
    let mut server = next(server);
    pump(&mut server, &mut client);
    let mut client = next(client)
        .login(
            denying.client_key.clone(),
            denying.login_request(),
            Instant::now(),
        )
        .map_err(|f| f.error)
        .unwrap();
    pump(&mut client, &mut server);
    let request = next(server);
    let identity = crate::stream::server::verify_login(request.connection_request(), false, true)
        .await
        .unwrap();
    let failure = match request.authenticated(identity, PEER, Instant::now()) {
        Err(failure) => failure,
        Ok(_) => panic!("admission hook ignored"),
    };
    let mut refused = *failure.state;
    pump(&mut refused, &mut client);
    match error_of(client, Instant::now()) {
        JolyneError::Disconnected(disconnect) => {
            assert_eq!(
                disconnect.reason,
                EnumsConnectionDisconnectFailReason::Kicked
            );
            assert_eq!(disconnect.message.as_deref(), Some("whitelist"));
        }
        other => panic!("expected Disconnected, got {other}"),
    }
}

struct Ping(u32);

impl CustomPacket for Ping {
    const ID: u16 = 1000;

    fn decode(body: &[u8]) -> Result<Self, valentine::bedrock::error::DecodeError> {
        let bytes: [u8; 4] =
            body.try_into()
                .map_err(|_| valentine::bedrock::error::DecodeError::UnexpectedEof {
                    needed: 4,
                    available: body.len(),
                })?;
        Ok(Self(u32::from_le_bytes(bytes)))
    }

    fn encode(&self, out: &mut BytesMut) -> Result<(), std::io::Error> {
        out.extend_from_slice(&self.0.to_le_bytes());
        Ok(())
    }
}

#[test]
fn unknown_and_custom_packets_follow_the_configured_policy() {
    let now = Instant::now();
    let send = |packet: &RawPacket, options: ConnectionOptions| {
        let mut from = Connection::new(Framing::RakNet, ConnectionOptions::default());
        let mut to = Connection::new(Framing::RakNet, options);
        from.queue_raw(packet, now).unwrap();
        let result =
            to.handle_datagram(Datagram::Contiguous(from.poll_transmit().unwrap().payload));
        (result, to)
    };
    let unknown = RawPacket::new(PacketHeader::new(1000), &[1, 0, 0, 0]).unwrap();

    // Server default: reject (gophertunnel AllowUnknownPackets=false).
    let (result, _) = send(&unknown, ConnectionOptions::default());
    assert!(matches!(
        result,
        Err(JolyneError::Protocol(ProtocolError::UnknownPacket(1000)))
    ));
    assert_eq!(
        JolyneError::from(ProtocolError::UnknownPacket(1000)).disconnect_reason(),
        Some(EnumsConnectionDisconnectFailReason::Unknownpacket)
    );

    // Client default / proxies: passthrough, byte-identical.
    let (result, mut to) = send(&unknown, ClientConfig::default().connection);
    result.unwrap();
    let passed = to.next_packet().unwrap().unwrap();
    assert_eq!(passed.inner_frame(), unknown.inner_frame());
    assert_eq!(to.metrics().unknown_packets, 1);

    // Registered custom packet: typed decode through the user codec, even
    // under the server's reject policy.
    let mut registry = PacketRegistry::default();
    registry.register::<Ping>().unwrap();
    let (result, mut to) = send(
        &RawPacket::from_custom(&Ping(77)).unwrap(),
        ConnectionOptions {
            unknown_packets: UnknownPacketPolicy::Reject,
            custom_packets: registry,
            ..ConnectionOptions::default()
        },
    );
    result.unwrap();
    let packet = to.next_packet().unwrap().unwrap();
    assert_eq!(packet.decode_custom::<Ping>().unwrap().0, 77);
    assert!(matches!(
        packet.decode(),
        Err(JolyneError::NonValentinePacket(_))
    ));
}

#[test]
fn deadline_elapses_exactly_at_login_timeout() {
    let config = Arc::new(BedrockListenerConfig {
        login_timeout: Duration::from_secs(3),
        ..Default::default()
    });
    let now = Instant::now();
    let server = ServerHandshake::new(
        Connection::new(Framing::RakNet, ConnectionOptions::default()),
        config,
        now,
    );
    assert_eq!(server.poll_timeout(), now + Duration::from_secs(3));
    let server = match server.advance(now + Duration::from_millis(2999)) {
        Ok(Step::Wait(server)) => server,
        _ => panic!("still within the deadline"),
    };
    assert!(server.advance(now + Duration::from_secs(3)).is_err());
}

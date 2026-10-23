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

/// Moves every transmit `from` produced into `to`; returns how many batches.
fn pump(from: &mut impl Owner, to: &mut impl Owner) -> usize {
    let mut batches = 0;
    while let Some(transmit) = from.poll_transmit() {
        to.connection()
            .handle_datagram(Datagram::Contiguous(transmit.payload))
            .expect("peer decodes every batch");
        batches += 1;
    }
    batches
}

/// Every outbound packet: its name and payload (after the packet header).
type Log = Arc<Mutex<Vec<(McpePacketName, Vec<u8>)>>>;

fn recording(log: &Log) -> ConnectionOptions {
    let log = log.clone();
    ConnectionOptions {
        observer: Some(Arc::new(move |event: PacketEvent<'_>| {
            if event.direction == Direction::Outbound {
                let name = packet_name(event.header.id).expect("known id");
                log.lock().push((name, event.payload.to_vec()));
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
    framing: Framing,
    /// Signs an online (provider) login instead of an offline one.
    provider: Option<SecretKey>,
    /// The key the online login claims; `None` is the client's own key. A
    /// replayed Login claims a key the sender does not hold.
    login_key: Option<SecretKey>,
    client: ClientConfig,
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
            framing: Framing::RakNet,
            provider: None,
            login_key: None,
            client: ClientConfig::default(),
        }
    }

    /// An online-mode harness whose logins carry a test-provider token.
    fn online(config: BedrockListenerConfig, framing: Framing) -> Self {
        Self {
            framing,
            provider: Some(crate::auth::security_tests::key(2)),
            ..Self::new(BedrockListenerConfig {
                online_mode: true,
                ..config
            })
        }
    }

    fn start(&self) -> (ServerHandshake, ClientSettings) {
        let now = Instant::now();
        let server = ServerHandshake::new(
            Connection::new(self.framing, self.config.connection.clone()),
            self.config.clone(),
            now,
        );
        let client = ClientSettings::new(
            Connection::new(self.framing, self.client.connection.clone()),
            &self.client,
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
        crate::auth::client::encode_login_request(&chain, &token)
            .unwrap()
            .into()
    }

    /// Verifies the login like the async layer: offline, or against the test
    /// provider's key set.
    async fn verify(
        &self,
        request: &super::server::LoginRequest,
    ) -> Result<crate::auth::AuthenticatedLogin, JolyneError> {
        match &self.provider {
            None => {
                crate::stream::server::verify_login(request.connection_request(), false, true).await
            }
            Some(provider) => {
                use crate::auth::security_tests::{claims, client_data, sign, verifier_for};
                let key = self.login_key.as_ref().unwrap_or(&self.client_key);
                let token = sign(provider, &claims(key));
                Ok(verifier_for(&[(provider, None)]).validate(&token, &client_data(key))?)
            }
        }
    }

    /// Runs settings and login; returns the verified, admitted server side
    /// and the client waiting for the handshake.
    async fn through_authentication(
        &self,
    ) -> Result<(super::server::Authenticated, ClientHandshake), JolyneError> {
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
        let identity = self.verify(&request).await?;
        let authenticated = request
            .authenticated(identity, PEER, Instant::now())
            .map_err(|failure| failure.error)?;
        Ok((authenticated, client))
    }

    /// Runs settings, login and encryption; returns both sides at the
    /// resource pack phase.
    async fn through_login(
        &self,
    ) -> Result<(super::server::PacksReady, super::client::ClientPacks), JolyneError> {
        let (authenticated, mut client) = self.through_authentication().await?;
        let mut encryption =
            match authenticated.start_encryption(&self.server_key, None, Instant::now()) {
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

/// BDS 1.26.51's server-to-client packets through `PlayStatus(PlayerSpawn)`:
/// `(batch number, packet name)` per packet.
fn bds_join_trace() -> Vec<(u32, String)> {
    include_str!("testdata/bds-1.26.51-join-s2c.txt")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| {
            let (batch, name) = line.split_once(' ').expect("batch and packet");
            (batch.parse().expect("batch number"), name.to_owned())
        })
        .collect()
}

/// ROADMAP M3: the server's packets through PlayerSpawn appear in the order
/// BDS 1.26.51 sends them (a recorded trace, not a hand-written list): the
/// stack only after the client's first pack response, the structure packets
/// before StartGame (the client disconnects with "Missing structure data
/// from server." without JigsawStructureData), and the StartGame group in
/// one BDS batch. BDS sends packets jolyne leaves to the application
/// (player list, abilities, chunks, ...), so jolyne's sequence must be an
/// order-preserving subsequence. BDS sends no `DimensionData` for vanilla
/// dimensions; it is checked to precede the structure packets.
#[tokio::test]
async fn join_sequence_follows_the_bds_capture() {
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
    let sent: Vec<_> = harness.sent.lock().iter().map(|(name, _)| *name).collect();
    let dimension = sent
        .iter()
        .position(|name| *name == N::DimensionDataPacket)
        .expect("DimensionData sent");
    assert_eq!(sent[dimension + 1], N::JigsawStructureDataPacket);

    let bds = bds_join_trace();
    let mut cursor = 0;
    let mut matched = Vec::new();
    for name in sent.iter().filter(|name| **name != N::DimensionDataPacket) {
        let wanted = format!("{name:?}");
        let offset = bds[cursor..]
            .iter()
            .position(|(_, bds_name)| *bds_name == wanted)
            .unwrap_or_else(|| panic!("{wanted} out of BDS order after {matched:?}"));
        cursor += offset;
        matched.push((bds[cursor].0, *name));
        cursor += 1;
    }
    assert_eq!(
        cursor,
        bds.len(),
        "jolyne's last packet is BDS's PlayStatus(PlayerSpawn)"
    );
    let start_game_batches: std::collections::BTreeSet<_> = matched
        .iter()
        .filter(|(_, name)| {
            matches!(
                name,
                N::JigsawStructureDataPacket
                    | N::VoxelShapesPacket
                    | N::StartGamePacket
                    | N::ItemRegistryPacket
                    | N::BiomeDefinitionListPacket
                    | N::AvailableActorIdentifiersPacket
                    | N::CreativeContentPacket
            )
        })
        .map(|(batch, _)| *batch)
        .collect();
    assert_eq!(start_game_batches.len(), 1, "{matched:?}");

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

/// BDS sends `StartGame` with the structure and definition packets in one
/// batch, and gophertunnel answers `RequestChunkRadius` with
/// `ChunkRadiusUpdated`, `NetworkChunkPublisherUpdate` and
/// `PlayStatus(PlayerSpawn)` in one flush; under the default immediate flush
/// mode each packet used to leave in its own batch.
#[tokio::test]
async fn start_game_and_spawn_answers_leave_as_single_batches() {
    let harness = Harness::new(BedrockListenerConfig {
        online_mode: false,
        ..Default::default()
    });
    assert_eq!(
        harness.config.connection.flush,
        crate::config::FlushMode::Immediate
    );
    let mut params = params(5);
    params.dimension_data = Some(Arc::new(crate::valentine::DimensionDataPacket::default()));
    let (mut spawn, mut client) = harness.through_packs(params).await;
    assert_eq!(pump(&mut spawn, &mut client), 1); // DimensionData .. CreativeContent
    let mut client = wait(client);
    pump(&mut client, &mut spawn);
    let mut spawn = wait(spawn);
    assert_eq!(pump(&mut spawn, &mut client), 1); // radius, publisher, PlayerSpawn
    let _joined = next(client);
}

/// The real 26.52 client dropped the connection with `BadPacket`
/// (`InitialConnection-90`) on a `ResourcePacksInfo` whose world template
/// version was `""`: since 26.50 that field is a validated cereal
/// `SemVersion`. Pins the payloads the server sends between the encryption
/// handshake and the client's first pack response to BDS 1.26.51's bytes
/// (captured with `tools/client-diff/gt-client`). `ResourcePackStack` keeps
/// the base game version the pre-sans-IO server sent to accepted clients
/// (BDS sends `*`).
#[tokio::test]
async fn post_encryption_packets_match_bds_bytes() {
    use McpePacketName as N;
    let harness = Harness::new(BedrockListenerConfig {
        online_mode: false,
        ..Default::default()
    });
    harness.through_packs(params(1)).await;
    let sent = harness.sent.lock();
    let handshake = sent
        .iter()
        .position(|(name, _)| *name == N::ServerToClientHandshakePacket)
        .expect("handshake sent");
    let mut bds_info = vec![0u8; 4]; // required, addons, scripts, vibrant visuals
    bds_info.extend([0u8; 16]); // nil world template UUID
    bds_info.extend(b"\x050.0.0"); // world template version
    bds_info.push(0); // no packs (varuint count)
    let mut stack = vec![0u8, 0]; // not required, no packs
    stack.extend(b"\x071.26.51"); // base game version
    stack.extend([0, 0, 0, 0, 0, 0]); // no experiments, never toggled, no editor packs
    assert_eq!(
        sent[handshake + 1..handshake + 4],
        [
            (N::PlayStatusPacket, vec![0, 0, 0, 0]), // LoginSuccess (i32 BE)
            (N::ResourcePacksInfoPacket, bds_info),
            (N::ResourcePackStackPacket, stack),
        ]
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
async fn huge_configured_timeouts_mean_far_deadlines_not_overflow() {
    // `Duration::MAX` as "never" used to overflow `Instant + Duration`.
    let harness = Harness::new(BedrockListenerConfig {
        online_mode: false,
        login_timeout: Duration::MAX,
        resource_pack_timeout: Duration::MAX,
        spawn_timeout: Duration::MAX,
        ..Default::default()
    });
    let far = Instant::now() + Duration::from_secs(50 * 365 * 24 * 60 * 60);
    let (server, _client) = harness.start();
    assert!(server.poll_timeout() > far);
    let (spawn, _client) = harness.through_packs(params(1)).await;
    assert!(spawn.poll_timeout() > far);

    let client_config = ClientConfig {
        join_timeout: Duration::MAX,
        ..ClientConfig::default()
    };
    let client = ClientSettings::new(
        Connection::new(Framing::RakNet, client_config.connection.clone()),
        &client_config,
        Instant::now(),
    )
    .unwrap();
    assert!(client.poll_timeout() > far);

    let mut conn = Connection::new(
        Framing::RakNet,
        ConnectionOptions {
            flush: crate::config::FlushMode::Interval(Duration::MAX),
            ..ConnectionOptions::default()
        },
    );
    let packet = McpePacket::from(crate::valentine::RequestChunkRadiusPacket {
        chunk_radius: 4,
        max_chunk_radius: 4,
    });
    conn.queue_packet(&packet, Instant::now()).unwrap();
    assert!(conn.poll_timeout().is_some_and(|at| at > far));
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

/// Failure disconnects carry a fixed sentence per reason, never the error's
/// own text (token/signature details, decode previews).
#[test]
fn failure_disconnects_never_send_internal_error_text() {
    use crate::error::AuthError;
    let now = Instant::now();
    for (error, reason, message) in [
        (
            JolyneError::from(AuthError::BadSignature("x5u=MHYwEAYH secret".into())),
            EnumsConnectionDisconnectFailReason::Notauthenticated,
            "Authentication failed.",
        ),
        (
            ProtocolError::DecompressionFailed("preview 0a0b0c".into()).into(),
            EnumsConnectionDisconnectFailReason::Badpacket,
            "Received an invalid packet.",
        ),
    ] {
        let mut server = Connection::new(Framing::RakNet, ConnectionOptions::default());
        let mut client = Connection::new(Framing::RakNet, ConnectionOptions::default());
        server.queue_failure_disconnect(&error, now);
        let transmit = server.poll_transmit().expect("disconnect queued");
        client
            .handle_datagram(Datagram::Contiguous(transmit.payload))
            .unwrap();
        let Err(JolyneError::Disconnected(disconnect)) = client.next_packet() else {
            panic!("expected a disconnect for {error}");
        };
        assert_eq!(disconnect.reason, reason);
        assert_eq!(disconnect.message.as_deref(), Some(message));
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

fn encryption_of(
    harness: &Harness,
    authenticated: super::server::Authenticated,
) -> super::server::ServerEncryption {
    match authenticated.start_encryption(&harness.server_key, None, Instant::now()) {
        Ok(Step::Wait(encryption)) => encryption,
        Ok(Step::Next(_)) => panic!("encryption is enabled"),
        Err(failure) => panic!("start_encryption failed: {}", failure.error),
    }
}

/// A verified online token is not an authenticated identity until the client
/// proves it holds the token's key by completing the encryption handshake.
#[tokio::test]
async fn online_identity_is_authenticated_only_after_the_encryption_handshake() {
    use crate::auth::Trust;
    let harness = Harness::online(BedrockListenerConfig::default(), Framing::RakNet);
    let (authenticated, mut client) = harness.through_authentication().await.unwrap();
    let identity = &authenticated.login().identity;
    assert_eq!(identity.trust(), Trust::AwaitingKeyProof);
    assert_eq!((identity.xuid(), identity.playfab_id()), (None, None));

    let mut encryption = encryption_of(&harness, authenticated);
    pump(&mut encryption, &mut client);
    let mut client = wait(client);
    pump(&mut client, &mut encryption);
    let ready = next(encryption);
    let identity = &ready.login().identity;
    assert!(identity.is_authenticated());
    assert_eq!(identity.xuid(), Some("2535428123456789"));
    assert_eq!(identity.playfab_id(), Some("ABCDEF0123456789"));
}

/// A replayed Login (valid token for a key the sender does not hold) cannot
/// complete the handshake, stays unauthenticated, and the wait for
/// `ClientToServerHandshake` is bounded by the login deadline.
#[tokio::test]
async fn replayed_online_login_never_authenticates_and_times_out_at_the_login_deadline() {
    use crate::auth::Trust;
    let harness = Harness {
        login_key: Some(crate::auth::security_tests::key(9)),
        ..Harness::online(BedrockListenerConfig::default(), Framing::RakNet)
    };
    let (authenticated, mut client) = harness.through_authentication().await.unwrap();
    let mut encryption = encryption_of(&harness, authenticated);
    pump(&mut encryption, &mut client);
    // The replaying client derives a different secret: its handshake reply
    // fails the checksum and is never accepted.
    let mut client = wait(client);
    let transmit = client.connection().poll_transmit().unwrap();
    assert!(matches!(
        encryption
            .connection()
            .handle_datagram(Datagram::Contiguous(transmit.payload)),
        Err(JolyneError::Protocol(
            ProtocolError::ChecksumMismatch { .. }
        ))
    ));
    let encryption = wait(encryption);
    assert_eq!(encryption.login().identity.trust(), Trust::AwaitingKeyProof);
    let late = Instant::now() + harness.config.login_timeout;
    assert!(matches!(
        error_of(encryption, late),
        JolyneError::Protocol(ProtocolError::Timeout(HandshakePhase::Encryption))
    ));
}

/// Without Bedrock encryption, RakNet cannot prove the client's key, so an
/// online server refuses rather than trusting a possibly replayed Login.
#[tokio::test]
async fn online_mode_without_encryption_is_refused_on_raknet() {
    let harness = Harness::online(
        BedrockListenerConfig {
            encryption_enabled: false,
            ..Default::default()
        },
        Framing::RakNet,
    );
    let (authenticated, _client) = harness.through_authentication().await.unwrap();
    let error = match authenticated.start_encryption(&harness.server_key, None, Instant::now()) {
        Err(failure) => failure.error,
        Ok(_) => panic!("online RakNet login accepted without encryption"),
    };
    assert!(matches!(
        error,
        JolyneError::Auth(crate::error::AuthError::OnlineModeRequiresEncryption)
    ));
    assert_eq!(
        error.disconnect_reason(),
        Some(EnumsConnectionDisconnectFailReason::Notauthenticated)
    );
}

/// On a message transport with encryption off, an online login is
/// authenticated only when the transport proved the login's key (NetherNet's
/// DTLS-bound identity); otherwise it is treated as offline.
#[tokio::test]
async fn unencrypted_online_login_trusts_only_a_transport_proven_key() {
    use crate::auth::Trust;
    let harness = Harness::online(
        BedrockListenerConfig {
            encryption_enabled: false,
            ..Default::default()
        },
        Framing::Unprefixed,
    );
    let own = harness.client_key.public_key();
    let other = crate::auth::security_tests::key(9).public_key();
    for (transport_key, expected) in [
        (None, Trust::Unauthenticated),
        (Some(&other), Trust::Unauthenticated),
        (Some(&own), Trust::Authenticated),
    ] {
        let (authenticated, _client) = harness.through_authentication().await.unwrap();
        let ready = match authenticated.start_encryption(
            &harness.server_key,
            transport_key,
            Instant::now(),
        ) {
            Ok(Step::Next(ready)) => ready,
            Ok(Step::Wait(_)) => panic!("encryption is disabled"),
            Err(failure) => panic!("start_encryption failed: {}", failure.error),
        };
        let identity = &ready.login().identity;
        assert_eq!(identity.trust(), expected);
        assert_eq!(identity.xuid().is_some(), expected == Trust::Authenticated);
    }
}

#[tokio::test]
async fn client_can_require_encryption() {
    for require_encryption in [false, true] {
        let mut harness = Harness::new(BedrockListenerConfig {
            online_mode: false,
            encryption_enabled: false,
            ..Default::default()
        });
        harness.client.require_encryption = require_encryption;
        let (authenticated, mut client) = harness.through_authentication().await.unwrap();
        let mut ready =
            match authenticated.start_encryption(&harness.server_key, None, Instant::now()) {
                Ok(Step::Next(ready)) => ready,
                _ => panic!("encryption is disabled"),
            };
        pump(&mut ready, &mut client); // PlayStatus(LoginSuccess), unencrypted
        match client.advance(Instant::now()) {
            Ok(Step::Next(_)) => assert!(!require_encryption),
            Err(failure) => {
                assert!(require_encryption);
                assert!(matches!(
                    failure.error,
                    JolyneError::Protocol(ProtocolError::InvalidHandshake(_))
                ));
            }
            Ok(Step::Wait(_)) => panic!("LoginSuccess was not handled"),
        }
    }
}

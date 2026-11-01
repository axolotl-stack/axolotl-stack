use super::*;

#[test]
fn aggregate_status_does_not_reset_to_one_listeners_local_player_count() {
    let config = UnastarConfig::default();
    let status = RuntimeStatus::new(&config);
    status.players.store(7, Ordering::Relaxed);
    let advertised = status.status(2, 3);
    assert_eq!(advertised.player_count, 7);
    assert_eq!(advertised.max_players, config.server.max_players as usize);
    assert_eq!(advertised.server_name, config.server.motd);
}

use jolyne::stream::client::ClientHandshakeConfig;
use jolyne::stream::{
    BedrockStream, Client, Handshake, NetherNetTransport, Play, RakNetTransport, Transport,
};
use jolyne::valentine::{McpePacketData, PlayStatusPacket, PlayStatusPacketStatus, TextPacket};
use jolyne::{ClientConfig, WorldTemplate};
use std::net::SocketAddr;
use tokio_nethernet::{NetherNetDialer, NetherNetDialerConfig, TransportConfig};

/// A real native accept loop with bounded events and a disposable player store.
struct Harness {
    state: AcceptLoopState,
    events: mpsc::Receiver<NetworkEvent>,
    _directory: tempfile::TempDir,
}
impl Harness {
    fn new(online: bool, capacity: usize) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let mut config = UnastarConfig::default();
        config.server.online_mode = online;
        config.server.max_players = capacity as u32;
        config.server.handshake_timeout_seconds = 1;
        config.world.generator = crate::world::WorldGenerator::SuperFlat;
        config.spawn_rules[0].previous_position = false;
        let template = Arc::new(WorldTemplate::default());
        let dimension = template
            .start_game_template
            .settings
            .spawn_settings
            .dimension;
        let (event_tx, events) = mpsc::channel(32);
        let (tick_tx, _) = broadcast::channel(16);
        let state = AcceptLoopState {
            worlds: BTreeMap::from([(
                dimension,
                JoinWorldTemplate {
                    template,
                    state: Default::default(),
                    world_config: config.world,
                },
            )]),
            primary: dimension,
            key: SecretKey::random(&mut rand::thread_rng()),
            player_data_store: Arc::new(PlayerDataStore::new(directory.path().to_owned())),
            player_provider: None,
            config: Arc::new(config),
            event_tx,
            tick_tx,
            connection_gate: Arc::new(Semaphore::new(capacity)),
            next_session_id: Arc::new(AtomicU64::new(1)),
        };
        Self {
            state,
            events,
            _directory: directory,
        }
    }
    /// Use the production binder with strictly local ICE addresses for tests.
    async fn endpoint(&self, tasks: &mut NetworkTasks) -> SocketAddr {
        let endpoint = crate::config::NetherNetConfig {
            bind_address: "127.0.0.1:0".parse().unwrap(),
            ..Default::default()
        };
        let status = Arc::new(RuntimeStatus::new(&self.state.config));
        let (handler, listener) = bind_nethernet(
            &endpoint,
            BedrockListenerConfig {
                online_mode: self.state.config.server.online_mode,
                max_players: self.state.config.server.max_players as usize,
                status: Some(status),
                status_refresh: Duration::from_millis(100),
                ..Default::default()
            },
            TransportConfig::loopback_v4(),
        )
        .await
        .unwrap();
        let address = handler.local_addr().unwrap();
        tasks.endpoint = Some(handler);
        tasks
            .tasks
            .push(spawn_accept_loop(listener, self.state.clone()));
        address
    }
    /// A concurrent RakNet listener shares the same gate and session sequence.
    async fn raknet(&self, tasks: &mut NetworkTasks) -> SocketAddr {
        let listener = BedrockListener::raknet()
            .addr("127.0.0.1:0")
            .config(BedrockListenerConfig {
                online_mode: self.state.config.server.online_mode,
                max_players: self.state.config.server.max_players as usize,
                ..Default::default()
            })
            .bind()
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        tasks
            .tasks
            .push(spawn_accept_loop(listener, self.state.clone()));
        address
    }
    /// Complete the same streamed spawn boundary that the ECS normally handles.
    async fn joined(&mut self) -> (u64, mpsc::Sender<jolyne::valentine::McpePacket>) {
        let selected = loop {
            match self.events.recv().await.unwrap() {
                NetworkEvent::WorldSelected {
                    session_id,
                    dimension,
                } => break (session_id, dimension),
                NetworkEvent::Joined { .. } => panic!("join preceded world selection"),
                _ => {}
            }
        };
        match self.events.recv().await.unwrap() {
            NetworkEvent::Joined {
                session_id,
                runtime_id,
                xuid,
                outbound_tx,
                ..
            } => {
                assert_eq!(session_id, selected.0);
                assert_eq!(selected.1, self.state.primary);
                assert_eq!(runtime_id as u64, session_id);
                assert!(xuid.is_none());
                outbound_tx
                    .send(
                        PlayStatusPacket {
                            status: PlayStatusPacketStatus::PlayerSpawn,
                        }
                        .into(),
                    )
                    .await
                    .unwrap();
                self.state.tick_tx.send(()).unwrap();
                (session_id, outbound_tx)
            }
            other => panic!("{other:?}"),
        }
    }
}

/// Sign the SDP assertion using the same key that signs this test's offline Login.
async fn endpoint_transport(
    address: SocketAddr,
    login: &ClientHandshakeConfig,
    signed: bool,
) -> NetherNetTransport {
    try_endpoint_transport(address, login, signed)
        .await
        .unwrap()
}

/// A full listener may close a rejected connection before dial completes.
async fn try_endpoint_transport(
    address: SocketAddr,
    login: &ClientHandshakeConfig,
    signed: bool,
) -> Result<NetherNetTransport, tokio_nethernet::NetherNetError> {
    let identity = signed.then(|| {
        Arc::new(
            tokio_nethernet::identity::Identity::generate_server(
                p384::ecdsa::SigningKey::from(login.identity_key.clone()),
                "localhost",
            )
            .unwrap(),
        )
    });
    let dialer = NetherNetDialer::connect_with_signaling(
        tokio_nethernet::endpoint::Client::new(),
        NetherNetDialerConfig {
            transport: TransportConfig::loopback_v4(),
            identity,
            ..Default::default()
        },
    );
    let stream = dialer.dial(format!("http://{address}")).await?;
    Ok(NetherNetTransport::new(stream))
}

/// Run the normal client protocol over either real native transport.
async fn client_join<T: Transport>(
    transport: T,
    login: ClientHandshakeConfig,
) -> Result<BedrockStream<Play, Client, T>, jolyne::JolyneError> {
    let stream = BedrockStream::<Handshake, Client, _>::from_transport(
        transport,
        ClientConfig {
            join_timeout: Duration::from_secs(5),
            ..Default::default()
        },
    )
    .await?;
    stream.join(login).await.map(|(play, _)| play)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn endpoint_and_raknet_join_share_routing_capacity_and_unique_session_ids() {
    tokio::time::timeout(Duration::from_secs(15), async {
        let mut harness = Harness::new(false, 2);
        let mut tasks = NetworkTasks::default();
        let endpoint = harness.endpoint(&mut tasks).await;
        let raknet = harness.raknet(&mut tasks).await;
        let status = tokio_nethernet::endpoint::Client::new()
            .status(&format!("http://{endpoint}"))
            .await
            .unwrap();
        assert_eq!(status.server_name, harness.state.config.server.motd);
        assert_eq!(
            status.protocol,
            i64::from(jolyne::valentine::PROTOCOL_VERSION)
        );
        assert_eq!(status.max_player_count, 2);
        let login = ClientHandshakeConfig::random(endpoint, "Endpoint");
        let transport = endpoint_transport(endpoint, &login, true).await;
        let pending = tokio::spawn(client_join(transport, login));
        let (first, _outbound1) = harness.joined().await;
        let mut nethernet = pending.await.unwrap().unwrap();
        assert_eq!(nethernet.runtime_id(), first);
        let stream = tokio_raknet::RaknetStream::connect(raknet).await.unwrap();
        let pending = tokio::spawn(client_join(
            RakNetTransport::new(stream),
            ClientHandshakeConfig::random(raknet, "UDP"),
        ));
        let (second, _outbound2) = harness.joined().await;
        let _raknet = pending.await.unwrap().unwrap();
        assert_ne!(first, second);
        assert_eq!(harness.state.connection_gate.available_permits(), 0);
        nethernet
            .send_packet(
                TextPacket {
                    body: jolyne::valentine::types::TextPacketBody::MessageOnly(
                        jolyne::valentine::types::TextPacketPayloadMessageOnly {
                            message_type: jolyne::valentine::EnumsTextPacketType::Raw,
                            message: "real endpoint payload".into(),
                        },
                    ),
                    ..Default::default()
                }
                .into(),
            )
            .await
            .unwrap();
        loop {
            if let NetworkEvent::Packet {
                session_id,
                packet,
                packet_args,
            } = harness.events.recv().await.unwrap()
                && let McpePacketData::TextPacket(text) =
                    packet.into_owned(packet_args).unwrap().data
            {
                assert_eq!(session_id, first);
                let jolyne::valentine::types::TextPacketBody::MessageOnly(body) = text.body else {
                    panic!("wrong text body")
                };
                assert_eq!(body.message, "real endpoint payload");
                break;
            }
        }
        let login = ClientHandshakeConfig::random(endpoint, "Full");
        match try_endpoint_transport(endpoint, &login, true).await {
            Ok(transport) => assert!(client_join(transport, login).await.is_err()),
            Err(tokio_nethernet::NetherNetError::ConnectionClosed) => {}
            Err(error) => panic!("unexpected capacity rejection: {error}"),
        }
        assert_eq!(harness.state.next_session_id.load(Ordering::Relaxed), 3);
        tasks.stop().await;
        assert!(tokio::net::TcpStream::connect(endpoint).await.is_err());
        tokio::time::timeout(Duration::from_secs(1), async {
            while harness.state.connection_gate.available_permits() != 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    })
    .await
    .expect("real native dual-transport join timed out");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn online_endpoint_rejects_guest_login_without_any_join_or_account_event() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut harness = Harness::new(true, 1);
        let mut tasks = NetworkTasks::default();
        let endpoint = harness.endpoint(&mut tasks).await;
        let login = ClientHandshakeConfig::random(endpoint, "Unverified");
        let transport = endpoint_transport(endpoint, &login, true).await;
        assert!(client_join(transport, login).await.is_err());
        assert!(harness.events.try_recv().is_err());
        tasks.stop().await;
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn endpoint_rejects_unsigned_sdp_before_native_login() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut harness = Harness::new(false, 1);
        let mut tasks = NetworkTasks::default();
        let endpoint = harness.endpoint(&mut tasks).await;
        let dialer = NetherNetDialer::connect_with_signaling(
            tokio_nethernet::endpoint::Client::new(),
            NetherNetDialerConfig {
                transport: TransportConfig::loopback_v4(),
                ..Default::default()
            },
        );
        assert!(dialer.dial(format!("http://{endpoint}")).await.is_err());
        assert_eq!(harness.state.next_session_id.load(Ordering::Relaxed), 1);
        assert_eq!(harness.state.connection_gate.available_permits(), 1);
        assert!(harness.events.try_recv().is_err());
        tasks.stop().await;
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn idle_handshake_timeout_releases_shared_capacity_and_shutdown_releases_http() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let harness = Harness::new(false, 1);
        let mut tasks = NetworkTasks::default();
        let endpoint = harness.endpoint(&mut tasks).await;
        let raknet = harness.raknet(&mut tasks).await;
        let _stalled = tokio_raknet::RaknetStream::connect(raknet).await.unwrap();
        while harness.state.connection_gate.available_permits() != 0 {
            tokio::task::yield_now().await;
        }
        tokio::time::sleep(Duration::from_millis(1150)).await;
        assert_eq!(harness.state.connection_gate.available_permits(), 1);
        tasks.stop().await;
        assert!(tokio::net::TcpStream::connect(endpoint).await.is_err());
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn endpoint_status_refresh_tracks_the_shared_provider_and_preserves_game_mode() {
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut config = UnastarConfig::default();
        config.server.default_gamemode = "adventure".into();
        config.server.max_players = 24;
        let status = Arc::new(RuntimeStatus::new(&config));
        status.players.store(7, Ordering::Relaxed);
        let endpoint = crate::config::NetherNetConfig {
            bind_address: "127.0.0.1:0".parse().unwrap(),
            ..Default::default()
        };
        let (handler, mut listener) = bind_nethernet(
            &endpoint,
            BedrockListenerConfig {
                status: Some(status.clone()),
                status_refresh: Duration::from_millis(100),
                ..Default::default()
            },
            TransportConfig::loopback_v4(),
        )
        .await
        .unwrap();
        let address = format!("http://{}", handler.local_addr().unwrap());
        let client = tokio_nethernet::endpoint::Client::new();
        let first = client.status(&address).await.unwrap();
        assert_eq!(
            (first.player_count, first.max_player_count, first.game_type),
            (7, 24, 2)
        );
        status.players.store(12, Ordering::Relaxed);
        assert!(
            tokio::time::timeout(Duration::from_millis(150), listener.accept())
                .await
                .is_err()
        );
        let second = client.status(&address).await.unwrap();
        assert_eq!(
            (
                second.player_count,
                second.max_player_count,
                second.game_type
            ),
            (12, 24, 2)
        );
        handler.close();
    })
    .await
    .unwrap();
}

/// Connect only the transport; no game Login or account authentication is performed.
async fn persisted_endpoint_key(path: &std::path::Path) -> p384::ecdsa::VerifyingKey {
    let endpoint = crate::config::NetherNetConfig {
        bind_address: "127.0.0.1:0".parse().unwrap(),
        signing_key_path: Some(path.to_owned()),
        ..Default::default()
    };
    let (handler, listener) = bind_nethernet(
        &endpoint,
        BedrockListenerConfig::default(),
        TransportConfig::loopback_v4(),
    )
    .await
    .unwrap();
    let identity = Arc::new(
        tokio_nethernet::identity::Identity::generate_server(
            p384::ecdsa::SigningKey::random(&mut rand::thread_rng()),
            "localhost",
        )
        .unwrap(),
    );
    let dialer = NetherNetDialer::connect_with_signaling(
        tokio_nethernet::endpoint::Client::new(),
        NetherNetDialerConfig {
            identity: Some(identity),
            transport: TransportConfig::loopback_v4(),
            ..Default::default()
        },
    );
    let connection = tokio::time::timeout(
        Duration::from_secs(5),
        dialer.dial(format!("http://{}", handler.local_addr().unwrap())),
    )
    .await
    .unwrap()
    .unwrap();
    let verified = connection.remote_identity().expect("signed server answer");
    let key = verified.public_key;
    drop(connection);
    drop(listener);
    handler.close();
    key
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_endpoint_restart_retains_the_verified_server_key() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("keys/server.der");
    let first = persisted_endpoint_key(&path).await;
    let second = persisted_endpoint_key(&path).await;
    assert_eq!(first, second);
}

#[tokio::test]
async fn invalid_configured_key_fails_before_http_listener_is_bound() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server.der");
    tokio_nethernet::identity::ServerSigningKey::load_or_create(&path).unwrap();
    std::fs::write(&path, b"invalid private key").unwrap();
    let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = socket.local_addr().unwrap();
    drop(socket);
    let endpoint = crate::config::NetherNetConfig {
        bind_address: address,
        signing_key_path: Some(path.clone()),
        ..Default::default()
    };
    assert!(
        bind_nethernet(
            &endpoint,
            BedrockListenerConfig::default(),
            TransportConfig::loopback_v4()
        )
        .await
        .is_err()
    );
    let _available = tokio::net::TcpListener::bind(address).await.unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"invalid private key");
}

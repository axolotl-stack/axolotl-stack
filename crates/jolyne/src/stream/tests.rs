//! The async layer over an in-memory transport.

use std::net::{Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use p384::SecretKey;
use tokio::sync::mpsc;
use tokio_util::sync::PollSender;

use super::transport::{Datagram, Framing, Transmit, Transport};
use super::*;
use crate::config::{BedrockListenerConfig, ClientConfig};
use crate::error::{HandshakePhase, ProtocolError};
use crate::stream::client::ClientHandshakeConfig;
use crate::valentine::{McpePacketName, TextPacket};
use crate::world::WorldTemplate;

/// A bounded in-memory message pipe (capacity = backpressure point).
struct Memory {
    tx: PollSender<Transmit>,
    rx: mpsc::Receiver<Transmit>,
    sent: Arc<std::sync::atomic::AtomicUsize>,
}

#[derive(Debug, thiserror::Error)]
#[error("memory pipe closed")]
struct Closed;

fn pipe(capacity: usize) -> (Memory, Memory) {
    let (a_tx, a_rx) = mpsc::channel(capacity);
    let (b_tx, b_rx) = mpsc::channel(capacity);
    let memory = |tx, rx| Memory {
        tx: PollSender::new(tx),
        rx,
        sent: Default::default(),
    };
    (memory(a_tx, b_rx), memory(b_tx, a_rx))
}

impl Transport for Memory {
    type Error = Closed;
    const FRAMING: Framing = Framing::RakNet;

    fn poll_send_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Closed>> {
        self.tx.poll_reserve(cx).map_err(|_| Closed)
    }

    fn start_send(mut self: Pin<&mut Self>, message: Transmit) -> Result<(), Closed> {
        self.sent.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.tx.send_item(message).map_err(|_| Closed)
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Closed>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Closed>> {
        self.tx.close();
        Poll::Ready(Ok(()))
    }

    fn poll_recv(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Datagram, Closed>>> {
        self.rx
            .poll_recv(cx)
            .map(|message| message.map(|message| Ok(Datagram::Contiguous(message.payload))))
    }

    fn peer_addr(&self) -> SocketAddr {
        SocketAddr::from((Ipv4Addr::LOCALHOST, 19132))
    }

    fn latency(&self) -> Option<Duration> {
        Some(Duration::from_millis(3))
    }
}

fn offline() -> Arc<BedrockListenerConfig> {
    Arc::new(BedrockListenerConfig {
        online_mode: false,
        ..Default::default()
    })
}

async fn joined(
    config: Arc<BedrockListenerConfig>,
) -> (
    BedrockStream<Play, Server, Memory>,
    BedrockStream<Play, Client, Memory>,
) {
    let (server_io, client_io) = pipe(64);
    let server = tokio::spawn(async move {
        let key = SecretKey::random(&mut rand::thread_rng());
        BedrockStream::<Handshake, Server, _>::from_transport(server_io, config)
            .accept_join_sequence(&WorldTemplate::default(), &key)
            .await
            .map(|(play, _)| play)
    });
    let client =
        BedrockStream::<Handshake, Client, _>::from_transport(client_io, ClientConfig::default())
            .await
            .unwrap();
    let (client, _) = client
        .join(ClientHandshakeConfig::random(
            SocketAddr::from((Ipv4Addr::LOCALHOST, 19132)),
            "Tester",
        ))
        .await
        .unwrap();
    (server.await.unwrap().unwrap(), client)
}

async fn next_named<R: Role>(
    stream: &mut BedrockStream<Play, R, Memory>,
    name: McpePacketName,
) -> RawPacket
where
    Play: PhaseOf<R>,
{
    loop {
        let packet = stream.recv_packet_raw().await.unwrap();
        if packet.is(name) {
            return packet;
        }
    }
}

/// The server side of a streamed spawn (`start_game_streamed`), back in
/// play at the client's radius request.
async fn streamed_server(
    server_io: Memory,
    config: Arc<BedrockListenerConfig>,
) -> Result<BedrockStream<Play, Server, Memory>, JolyneError> {
    let key = SecretKey::random(&mut rand::thread_rng());
    let stream = BedrockStream::<Handshake, Server, _>::from_transport(server_io, config);
    let login = stream
        .accept_network_settings()
        .await
        .map_err(Failed::into_error)?;
    let secure = login.authenticate().await.map_err(Failed::into_error)?;
    let packs = secure
        .finish_handshake(&key)
        .await
        .map_err(Failed::into_error)?;
    let start = packs.negotiate_packs().await.map_err(Failed::into_error)?;
    start
        .start_game_streamed(WorldTemplate::default().to_join_params(5))
        .await
        .map_err(Failed::into_error)
}

/// A streamed spawn enters play before the client initializes: the
/// application sends `PlayStatus(PlayerSpawn)`, and the play receive path
/// ends the spawn on `SetLocalPlayerAsInitialized` (still delivered).
#[tokio::test]
async fn streamed_spawn_completes_on_the_play_receive_path() {
    use crate::valentine::{PlayStatusPacket, PlayStatusPacketStatus};
    let (server_io, client_io) = pipe(64);
    let server = tokio::spawn(async move {
        let mut play = streamed_server(server_io, offline()).await?;
        assert!(!play.is_initialized());
        assert_eq!(play.chunk_radius(), Some(16));
        play.send_packet(
            PlayStatusPacket {
                status: PlayStatusPacketStatus::Playerspawn,
            }
            .into(),
        )
        .await?;
        let initialized =
            next_named(&mut play, McpePacketName::SetLocalPlayerAsInitializedPacket).await;
        assert!(play.is_initialized());
        Ok::<_, JolyneError>((play, initialized))
    });
    let client =
        BedrockStream::<Handshake, Client, _>::from_transport(client_io, ClientConfig::default())
            .await
            .unwrap();
    let (client, _) = client
        .join(ClientHandshakeConfig::random(
            SocketAddr::from((Ipv4Addr::LOCALHOST, 19132)),
            "Tester",
        ))
        .await
        .unwrap();
    let (server, _) = server.await.unwrap().unwrap();
    assert_eq!(server.runtime_id(), client.runtime_id());
}

/// A streamed spawn whose client never initializes ends in
/// `Timeout(Spawn)` on the play receive path, at the spawn deadline.
#[tokio::test(start_paused = true)]
async fn streamed_spawn_times_out_on_the_play_receive_path() {
    let (server_io, client_io) = pipe(64);
    let server = tokio::spawn(async move {
        let mut play = streamed_server(server_io, offline()).await?;
        // No PlayerSpawn: the client never initializes.
        loop {
            play.recv_packet_raw().await?;
        }
    });
    let client =
        BedrockStream::<Handshake, Client, _>::from_transport(client_io, ClientConfig::default())
            .await
            .unwrap();
    let joining = tokio::spawn(client.join(ClientHandshakeConfig::random(
        SocketAddr::from((Ipv4Addr::LOCALHOST, 19132)),
        "Tester",
    )));
    let result: Result<(), JolyneError> = server.await.unwrap();
    assert!(matches!(
        result,
        Err(JolyneError::Protocol(ProtocolError::Timeout(
            HandshakePhase::Spawn
        )))
    ));
    joining.abort();
}

#[tokio::test]
async fn join_echo_and_typed_disconnect() {
    let (mut server, mut client) = joined(offline()).await;
    assert_eq!(server.runtime_id(), client.runtime_id());
    assert_eq!(server.identity().display_name(), "Tester");
    assert_eq!(
        server.client_data().device_os,
        1,
        "jolyne clients identify as Android"
    );
    assert_eq!(client.latency(), Some(Duration::from_millis(3)));

    client
        .send_packet(TextPacket::default().into())
        .await
        .unwrap();
    next_named(&mut server, McpePacketName::TextPacket).await;
    assert!(server.metrics().packets_in >= 1);

    server
        .disconnect(EnumsConnectionDisconnectFailReason::Kicked, "bye")
        .await
        .unwrap();
    loop {
        match client.recv_packet_raw().await {
            Ok(_) => continue,
            Err(JolyneError::Disconnected(disconnect)) => {
                assert_eq!(disconnect.message.as_deref(), Some("bye"));
                break;
            }
            Err(other) => panic!("expected Disconnected, got {other}"),
        }
    }
}

/// A transport whose only inbound event is one receive error.
struct FailsOnce {
    error: Option<PeerGone>,
}

#[derive(Debug, thiserror::Error)]
#[error("peer gone (normal close: {0})")]
struct PeerGone(bool);

impl Transport for FailsOnce {
    type Error = PeerGone;
    const FRAMING: Framing = Framing::RakNet;

    fn poll_send_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), PeerGone>> {
        Poll::Ready(Ok(()))
    }
    fn start_send(self: Pin<&mut Self>, _: Transmit) -> Result<(), PeerGone> {
        Ok(())
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), PeerGone>> {
        Poll::Ready(Ok(()))
    }
    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), PeerGone>> {
        Poll::Ready(Ok(()))
    }
    fn poll_recv(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<Datagram, PeerGone>>> {
        Poll::Ready(self.error.take().map(Err))
    }
    fn peer_addr(&self) -> SocketAddr {
        SocketAddr::from((Ipv4Addr::LOCALHOST, 0))
    }
    fn is_peer_close(error: &PeerGone) -> bool {
        error.0
    }
}

/// A normal remote close (e.g. RakNet's disconnection notification on Save
/// and Quit) is `ConnectionClosed`, not a transport failure.
#[tokio::test]
async fn a_normal_peer_close_is_connection_closed_not_a_transport_error() {
    for normal in [true, false] {
        let mut link = BedrockTransport::new(
            FailsOnce {
                error: Some(PeerGone(normal)),
            },
            crate::config::ConnectionOptions::default(),
        );
        let error = link.recv_packet_raw().await.unwrap_err();
        if normal {
            assert!(matches!(error, JolyneError::ConnectionClosed), "{error}");
        } else {
            assert!(matches!(error, JolyneError::Transport(_)), "{error}");
        }
    }
}

// Paused clock: the driver's `now()` and its timers share Tokio's clock, so
// the interval flush fires at exactly 20 ms of virtual time.
#[tokio::test(start_paused = true)]
async fn manual_and_interval_flush_batch_packets() {
    let (mut server, mut client) = joined(offline()).await;
    let before = server.metrics().batches_out;
    server.set_flush_mode(FlushMode::Manual).await.unwrap();
    for _ in 0..5 {
        server
            .send_packet(TextPacket::default().into())
            .await
            .unwrap();
    }
    assert_eq!(server.metrics().batches_out, before);
    server.flush().await.unwrap();
    assert_eq!(server.metrics().batches_out, before + 1);

    server
        .set_flush_mode(FlushMode::Interval(Duration::from_millis(20)))
        .await
        .unwrap();
    server
        .send_packet(TextPacket::default().into())
        .await
        .unwrap();
    assert_eq!(server.metrics().batches_out, before + 1);
    // The ticker fires while the stream is driven by a receive.
    let _ = tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            server.recv_packet_raw().await.unwrap();
        }
    })
    .await;
    assert_eq!(server.metrics().batches_out, before + 2);
    let mut texts = 0;
    while texts < 6 {
        next_named(&mut client, McpePacketName::TextPacket).await;
        texts += 1;
    }
}

#[tokio::test]
async fn close_and_drop_flush_buffered_packets() {
    let (mut server, mut client) = joined(offline()).await;
    server.set_flush_mode(FlushMode::Manual).await.unwrap();
    server
        .send_packet(TextPacket::default().into())
        .await
        .unwrap();
    server.close().await.unwrap();
    next_named(&mut client, McpePacketName::TextPacket).await;

    let (mut server, mut client) = joined(offline()).await;
    client.set_flush_mode(FlushMode::Manual).await.unwrap();
    client
        .send_packet(TextPacket::default().into())
        .await
        .unwrap();
    drop(client);
    next_named(&mut server, McpePacketName::TextPacket).await;
}

#[tokio::test]
async fn failed_step_keeps_the_connection_and_sends_a_reason() {
    let (server_io, mut client_io) = pipe(8);
    let config = Arc::new(BedrockListenerConfig {
        login_timeout: Duration::from_millis(50),
        ..Default::default()
    });
    let stream = BedrockStream::<Handshake, Server, _>::from_transport(server_io, config);
    let failed = match stream.accept_network_settings().await {
        Err(failed) => failed,
        Ok(_) => panic!("no client spoke"),
    };
    assert!(matches!(
        failed.error(),
        JolyneError::Protocol(ProtocolError::Timeout(HandshakePhase::NetworkSettings))
    ));
    let error = failed.disconnect().await;
    assert!(matches!(
        error,
        JolyneError::Protocol(ProtocolError::Timeout(_))
    ));
    let message = client_io.rx.recv().await.expect("disconnect sent");
    let mut client =
        crate::proto::conn::Connection::new(Framing::RakNet, ClientConfig::default().connection);
    client
        .handle_datagram(Datagram::Contiguous(message.payload))
        .unwrap();
    assert!(matches!(
        client.next_packet(),
        Err(JolyneError::Disconnected(disconnect))
            if disconnect.reason == EnumsConnectionDisconnectFailReason::Timeout
    ));
}

#[tokio::test]
async fn sends_wait_for_transport_capacity() {
    let (mut server, client) = joined(offline()).await;
    let blocked = tokio::time::timeout(Duration::from_millis(200), async {
        for _ in 0..1_000 {
            server
                .send_packet(TextPacket::default().into())
                .await
                .unwrap();
        }
    })
    .await;
    assert!(
        blocked.is_err(),
        "an undrained peer must apply backpressure"
    );
    drop(client);
}

/// Closing gives up on a peer that stopped reading instead of pinning the
/// task.
#[tokio::test]
async fn close_is_bounded_when_the_peer_stops_reading() {
    let (mut server, client) = joined(offline()).await;
    let filled = tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            server
                .send_packet(TextPacket::default().into())
                .await
                .unwrap();
        }
    })
    .await;
    assert!(filled.is_err(), "the pipe must apply backpressure");
    tokio::time::pause();
    assert!(matches!(
        server.close().await,
        Err(JolyneError::Protocol(ProtocolError::CloseTimeout))
    ));
    drop(client);
}

/// Output flushed while advancing between steps shares the step's deadline:
/// here `LoginSuccess` (encryption off) cannot be written because the client
/// never reads.
#[tokio::test]
async fn advancing_is_bounded_by_the_step_deadline() {
    use crate::proto::batch::BatchCompression;
    use crate::valentine::{LoginPacket, PROTOCOL_VERSION, RequestNetworkSettingsPacket};

    let (server_io, client_io) = pipe(1);
    let config = Arc::new(BedrockListenerConfig {
        online_mode: false,
        encryption_enabled: false,
        login_timeout: Duration::from_millis(300),
        ..Default::default()
    });
    let server_key = SecretKey::random(&mut rand::thread_rng());
    let server = tokio::spawn(async move {
        let stream = BedrockStream::<Handshake, Server, _>::from_transport(server_io, config);
        let login = stream.accept_network_settings().await?;
        let secure = login.authenticate().await?;
        secure.finish_handshake(&server_key).await?;
        Ok::<(), JolyneError>(())
    });

    // A raw client: settings, then a compressed Login; it never reads.
    let now = std::time::Instant::now();
    let mut client =
        crate::proto::conn::Connection::new(Framing::RakNet, ClientConfig::default().connection);
    let send = |client: &mut crate::proto::conn::Connection| {
        let mut out = Vec::new();
        while let Some(transmit) = client.poll_transmit() {
            out.push(transmit);
        }
        out
    };
    client
        .queue_packet(
            &RequestNetworkSettingsPacket {
                client_network_version: PROTOCOL_VERSION,
            }
            .into(),
            now,
        )
        .unwrap();
    client.flush().unwrap();
    let tx = client_io.tx.get_ref().unwrap().clone();
    for transmit in send(&mut client) {
        tx.send(transmit).await.unwrap();
    }
    client
        .enable_compression(BatchCompression::Deflate, 7, 1)
        .unwrap();
    let key = SecretKey::random(&mut rand::thread_rng());
    let (chain, token) =
        crate::auth::client::generate_self_signed_chain(&key, "Tester", uuid::Uuid::new_v4())
            .unwrap();
    client
        .queue_packet(
            &LoginPacket {
                client_network_version: PROTOCOL_VERSION,
                connection_request: crate::auth::client::encode_login_request(&chain, &token)
                    .unwrap()
                    .into(),
            }
            .into(),
            now,
        )
        .unwrap();
    client.flush().unwrap();
    for transmit in send(&mut client) {
        tx.send(transmit).await.unwrap();
    }

    let result = tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("the server step must not hang")
        .unwrap();
    assert!(
        matches!(
            result,
            Err(JolyneError::Protocol(ProtocolError::Timeout(
                HandshakePhase::Encryption
            )))
        ),
        "{result:?}"
    );
    drop(client_io);
}

/// A receive cancelled while output is backpressured (e.g. in `select!`)
/// must not lose a packet it already decoded.
#[tokio::test]
async fn cancelled_receive_keeps_queued_packets() {
    let (mut server, mut client) = joined(offline()).await;
    // Two packets in one datagram: the second stays queued in the server.
    client.set_flush_mode(FlushMode::Manual).await.unwrap();
    for _ in 0..2 {
        client
            .send_packet(TextPacket::default().into())
            .await
            .unwrap();
    }
    client.flush().await.unwrap();
    next_named(&mut server, McpePacketName::TextPacket).await;
    // Fill the undrained pipe so the server has output it cannot write.
    let filled = tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            server
                .send_packet(TextPacket::default().into())
                .await
                .unwrap();
        }
    })
    .await;
    assert!(filled.is_err(), "the pipe must apply backpressure");
    let cancelled = tokio::time::timeout(Duration::from_millis(50), server.recv_packet_raw()).await;
    assert!(
        cancelled.is_err(),
        "the receive waits for transport capacity"
    );
    // Drain the client; the queued packet must still be delivered.
    let drain = tokio::spawn(async move { while client.recv_packet_raw().await.is_ok() {} });
    let packet = tokio::time::timeout(Duration::from_secs(5), server.recv_packet_raw())
        .await
        .expect("queued packet delivered")
        .unwrap();
    assert!(packet.is(McpePacketName::TextPacket));
    drain.abort();
}

/// Proxy relay: downstream player ↔ proxy ↔ upstream server, encryption
/// terminated per side, unknown IDs forwarded byte-identically, Disconnect
/// forwarded and typed.
#[tokio::test]
async fn proxy_relays_unknown_packets_and_disconnects() {
    use crate::proto::raw::{PacketHeader, RawPacket};

    let passthrough = Arc::new(crate::proxy::proxy_listener_config(BedrockListenerConfig {
        online_mode: false,
        ..Default::default()
    }));
    // Upstream server.
    let (upstream_server_io, upstream_client_io) = pipe(64);
    let upstream_config = passthrough.clone();
    let upstream = tokio::spawn(async move {
        let key = SecretKey::random(&mut rand::thread_rng());
        BedrockStream::<Handshake, Server, _>::from_transport(upstream_server_io, upstream_config)
            .accept_join_sequence(&WorldTemplate::default(), &key)
            .await
    });
    // Proxy.
    let (proxy_io, player_io) = pipe(64);
    let proxy = tokio::spawn(async move {
        let key = SecretKey::random(&mut rand::thread_rng());
        let session = crate::proxy::accept(
            BedrockStream::<Handshake, Server, _>::from_transport(proxy_io, passthrough),
            &key,
            |login| {
                let name = login.identity.display_name().to_owned();
                async move {
                    let stream = BedrockStream::<Handshake, Client, _>::from_transport(
                        upstream_client_io,
                        ClientConfig::default(),
                    )
                    .await?;
                    Ok((
                        stream,
                        ClientHandshakeConfig::random(
                            SocketAddr::from((Ipv4Addr::LOCALHOST, 1)),
                            name,
                        ),
                    ))
                }
            },
        )
        .await
        .unwrap();
        session.relay(|_, _| crate::proxy::Verdict::Forward).await
    });
    // Player.
    let player =
        BedrockStream::<Handshake, Client, _>::from_transport(player_io, ClientConfig::default())
            .await
            .unwrap();
    let (mut player, _) = player
        .join(ClientHandshakeConfig::random(
            SocketAddr::from((Ipv4Addr::LOCALHOST, 19133)),
            "Proxied",
        ))
        .await
        .unwrap();
    let (mut upstream, login) = upstream.await.unwrap().unwrap();
    assert_eq!(login.identity.display_name(), "Proxied");
    assert_eq!(player.runtime_id(), upstream.runtime_id());

    let unknown = RawPacket::new(PacketHeader::new(1000), b"custom").unwrap();
    player.send_packet_raw(&unknown).await.unwrap();
    player
        .send_packet(TextPacket::default().into())
        .await
        .unwrap();
    let forwarded = loop {
        let packet = upstream.recv_packet_raw().await.unwrap();
        if packet.id() == 1000 {
            break packet;
        }
    };
    assert_eq!(forwarded.body(), b"custom");
    next_named(&mut upstream, McpePacketName::TextPacket).await;

    upstream
        .disconnect(
            EnumsConnectionDisconnectFailReason::Kicked,
            "upstream says bye",
        )
        .await
        .unwrap();
    loop {
        match player.recv_packet_raw().await {
            Ok(_) => continue,
            Err(JolyneError::Disconnected(disconnect)) => {
                assert_eq!(disconnect.message.as_deref(), Some("upstream says bye"));
                break;
            }
            Err(other) => panic!("expected forwarded Disconnect, got {other}"),
        }
    }
    let end = proxy.await.unwrap();
    assert_eq!(end.side, crate::proxy::Side::Upstream);
    assert!(matches!(end.error, JolyneError::Disconnected(_)));
}

#[tokio::test]
async fn resource_packs_download_over_the_async_layer() {
    use crate::proto::packs::{ResourcePack, ServerPacks};

    let content: bytes::Bytes = (0..300_000u32)
        .map(|i| (i % 253) as u8)
        .collect::<Vec<_>>()
        .into();
    let pack =
        Arc::new(ResourcePack::new(uuid::Uuid::from_u128(9), "1.0.0", content.clone()).unwrap());
    let config = Arc::new(BedrockListenerConfig {
        online_mode: false,
        resource_packs: Arc::new(ServerPacks {
            packs: vec![pack],
            required: true,
            chunk_interval: Duration::from_millis(1),
            ..ServerPacks::default()
        }),
        ..Default::default()
    });
    let (_server, client) = joined(config).await;
    let packs = client.resource_packs();
    assert_eq!(packs.len(), 1);
    assert_eq!(packs[0].content, content);
    assert_eq!(packs[0].offer.uuid, uuid::Uuid::from_u128(9));
}

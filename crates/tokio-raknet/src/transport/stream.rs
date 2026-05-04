use std::net::SocketAddr;
use std::time::{Duration, Instant};

use bytes::{BufMut, Bytes, BytesMut};
use futures::{Sink, SinkExt, Stream};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{self, MissedTickBehavior, timeout};
use tokio_util::sync::PollSender;

use crate::protocol::{
    constants::{
        DEFAULT_UNCONNECTED_MAGIC, MAXIMUM_MTU_SIZE, MINIMUM_MTU_SIZE, RAKNET_PROTOCOL_VERSION,
        UDP_HEADER_SIZE,
    },
    datagram::Datagram,
    packet::RaknetPacket,
    types::EoBPadding,
};
use crate::session::manager::{ConnectionState, ManagedSession, SessionConfig, SessionRole};

use super::{OutboundMsg, ReceivedMessage};
use crate::transport::Message;

use crate::protocol::constants::{self};

const TICK_INTERVAL: Duration = Duration::from_millis(20);
const HANDSHAKE_RETRIES: usize = 3;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_millis(400);

/// Configuration for a `RaknetStream`.
#[derive(Debug, Clone)]
pub struct RaknetStreamConfig {
    /// MTU size to attempt negotiation with.
    pub mtu: u16,
    /// Optional socket receive buffer size.
    pub socket_recv_buffer_size: Option<usize>,
    /// Optional socket send buffer size.
    pub socket_send_buffer_size: Option<usize>,
    /// Timeout for the initial connection handshake.
    pub connection_timeout: Duration,
    /// Timeout for an active session.
    pub session_timeout: Duration,
    /// Maximum number of ordering channels.
    pub max_ordering_channels: usize,
    /// Maximum capacity of the ACK queue.
    pub ack_queue_capacity: usize,
    /// Timeout for reassembling split packets.
    pub split_timeout: Duration,
    /// Maximum window size for reliable packets.
    pub reliable_window: u32,
    /// Maximum number of parts in a single split packet.
    pub max_split_parts: u32,
    /// Maximum number of concurrent split packets being reassembled.
    pub max_concurrent_splits: usize,
    /// Maximum number of incoming ACK/NACK ranges to queue per session.
    pub max_incoming_ack_queue: usize,
}

impl Default for RaknetStreamConfig {
    fn default() -> Self {
        Self {
            mtu: 1400,
            socket_recv_buffer_size: None,
            socket_send_buffer_size: None,
            connection_timeout: Duration::from_secs(10),
            session_timeout: Duration::from_secs(10),
            max_ordering_channels: constants::MAXIMUM_ORDERING_CHANNELS as usize,
            ack_queue_capacity: 1024,
            split_timeout: Duration::from_secs(30),
            reliable_window: constants::MAX_ACK_SEQUENCES as u32,
            max_split_parts: 8192,

            max_concurrent_splits: 32, // Bound per-session split reassembly memory
            max_incoming_ack_queue: 4096,
        }
    }
}

/// A unified RakNet connection stream.
///
/// This struct represents a connection to a remote peer, whether initiated locally (client)
/// or accepted from a listener (server). It provides methods to send and receive data.
pub struct RaknetStream {
    local: SocketAddr,
    peer: SocketAddr,
    incoming: mpsc::Receiver<Result<ReceivedMessage, crate::RaknetError>>,
    outbound_tx: PollSender<OutboundMsg>,
}

impl RaknetStream {
    /// Internal constructor for creating a stream from an established connection.
    pub(crate) fn new(
        local: SocketAddr,
        peer: SocketAddr,
        incoming: mpsc::Receiver<Result<ReceivedMessage, crate::RaknetError>>,
        outbound_tx: mpsc::Sender<OutboundMsg>,
    ) -> Self {
        Self {
            local,
            peer,
            incoming,
            outbound_tx: PollSender::new(outbound_tx),
        }
    }

    /// Connect to a RakNet server at the given address using default configuration.
    pub async fn connect(server: SocketAddr) -> Result<Self, crate::RaknetError> {
        Self::connect_with_config(server, RaknetStreamConfig::default()).await
    }

    /// Connect to a RakNet server with a custom configuration.
    pub async fn connect_with_config(
        server: SocketAddr,
        config: RaknetStreamConfig,
    ) -> Result<Self, crate::RaknetError> {
        let connection_timeout = config.connection_timeout;

        timeout(connection_timeout, async move {
            let socket = std::net::UdpSocket::bind("0.0.0.0:0")?;
            socket.set_nonblocking(true)?;
            apply_socket_buffer_config(
                &socket,
                config.socket_recv_buffer_size,
                config.socket_send_buffer_size,
            );

            let socket = UdpSocket::from_std(socket)?;
            let local = socket.local_addr()?;

            // Perform offline handshake using OpenConnectionRequest1/2.
            let client_guid = client_guid();
            let handshake =
                perform_offline_handshake(&socket, server, config.mtu as usize, client_guid)
                    .await?;

            // Use negotiated MTU
            let mut config = config;
            config.mtu = handshake.mtu;

            let (outbound_tx, outbound_rx) = mpsc::channel::<OutboundMsg>(1024);
            let (to_app_tx, to_app_rx) =
                mpsc::channel::<Result<ReceivedMessage, crate::RaknetError>>(128);
            let (ready_tx, ready_rx) = oneshot::channel();

            let context = ClientMuxerContext {
                server,
                server_guid: handshake.server_guid,
                client_guid,
                secure_connection_established: handshake.secure_connection_established,
                outbound_rx,
                to_app: to_app_tx,
                ready: ready_tx,
                config,
            };

            tokio::spawn(run_client_muxer(socket, context));

            match ready_rx.await {
                Ok(Ok(())) => Ok(Self {
                    local,
                    peer: server,
                    incoming: to_app_rx,
                    outbound_tx: PollSender::new(outbound_tx),
                }),
                Ok(Err(e)) => Err(e),
                Err(_) => Err(crate::RaknetError::ConnectionAborted),
            }
        })
        .await
        .map_err(|_| connection_timed_out(connection_timeout))?
    }

    /// Receives the next inbound RakNet user payload without rebuilding the
    /// packet ID byte into a contiguous buffer.
    pub async fn recv_message(&mut self) -> Option<Result<ReceivedMessage, crate::RaknetError>> {
        self.incoming.recv().await
    }

    /// Returns the local address this stream is bound to.
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }

    /// Returns the address of the remote peer.
    pub fn peer_addr(&self) -> SocketAddr {
        self.peer
    }

    /// Sends a message using reliability preferences.
    ///
    /// This is a convenience wrapper around the `Sink` trait.
    /// Sends a message using reliability preferences.
    ///
    /// This is a convenience wrapper around the `Sink` trait.
    pub async fn send_encoded(
        &mut self,
        msg: impl Into<super::Message>,
    ) -> Result<(), crate::RaknetError> {
        let msg = msg.into();
        self.send(msg).await
    }
}

impl Stream for RaknetStream {
    type Item = Result<Bytes, crate::RaknetError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.as_mut().poll_recv_message(cx) {
            Poll::Ready(Some(Ok(msg))) => {
                let mut full = BytesMut::with_capacity(1 + msg.payload.len());
                full.put_u8(msg.id);
                full.extend_from_slice(&msg.payload);
                Poll::Ready(Some(Ok(full.freeze())))
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl RaknetStream {
    /// Poll the next inbound RakNet user payload without rebuilding the leading
    /// ID byte into the body buffer.
    pub fn poll_recv_message(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<ReceivedMessage, crate::RaknetError>>> {
        self.incoming.poll_recv(cx)
    }
}

impl Sink<Message> for RaknetStream {
    type Error = crate::RaknetError;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.get_mut().outbound_tx)
            .poll_ready(cx)
            .map_err(|_| crate::RaknetError::ConnectionClosed)
    }

    fn start_send(self: Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
        let peer = self.peer;
        let bytes = item.buffer;
        if bytes.is_empty() {
            return Ok(());
        }
        let id = bytes[0];
        let body = bytes.slice(1..);

        let msg = OutboundMsg {
            peer,
            packet: RaknetPacket::UserData { id, payload: body },
            reliability: item.reliability,
            channel: item.channel,
            priority: item.priority,
        };

        Pin::new(&mut self.get_mut().outbound_tx)
            .start_send(msg)
            .map_err(|_| crate::RaknetError::ConnectionClosed)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.get_mut().outbound_tx)
            .poll_flush(cx)
            .map_err(|_| crate::RaknetError::ConnectionClosed)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.get_mut().outbound_tx)
            .poll_close(cx)
            .map_err(|_| crate::RaknetError::ConnectionClosed)
    }
}

struct OfflineHandshake {
    mtu: u16,
    server_guid: u64,
    secure_connection_established: bool,
}

enum ClientLoopEvent<Msg> {
    Inbound(std::io::Result<(usize, SocketAddr)>),
    Outbound(Msg),
    Tick,
    OutboundClosed,
}

struct ClientMuxerContext {
    // Connection properties
    server: SocketAddr,
    server_guid: u64,
    client_guid: u64,
    secure_connection_established: bool,

    // Communication channels
    outbound_rx: mpsc::Receiver<OutboundMsg>,
    to_app: mpsc::Sender<Result<ReceivedMessage, crate::RaknetError>>,
    ready: oneshot::Sender<Result<(), crate::RaknetError>>,
    config: RaknetStreamConfig,
}

#[tracing::instrument(skip(socket, context), fields(server = %context.server, mtu = context.config.mtu), level = "debug")]
async fn run_client_muxer(socket: UdpSocket, mut context: ClientMuxerContext) {
    let mut buf = vec![0u8; context.config.mtu as usize + UDP_HEADER_SIZE + 64];
    let mut send_buf = BytesMut::with_capacity(context.config.mtu as usize + UDP_HEADER_SIZE + 64);
    let mut managed: Option<ManagedSession> = None;
    let mut handshake_started = false;
    // We move the `ready` sender into a local Option
    let mut ready_signal = Some(context.ready);
    let mut tick = time::interval(TICK_INTERVAL);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    // Initial handshake ensure
    {
        let now = Instant::now();
        let ms = ensure_client_session(
            &mut managed,
            context.server,
            context.config.mtu as usize,
            context.client_guid,
            now,
            &context.config,
        );
        ensure_client_handshake(
            ms,
            &mut handshake_started,
            context.server_guid,
            now,
            context.secure_connection_established,
            &socket,
            context.server,
            &mut send_buf,
        )
        .await;
    }

    loop {
        let event = tokio::select! {
            res = socket.recv_from(&mut buf) => ClientLoopEvent::Inbound(res),
            msg = context.outbound_rx.recv() => match msg {
                Some(msg) => ClientLoopEvent::Outbound(msg),
                None => ClientLoopEvent::OutboundClosed,
            },
            _ = tick.tick() => ClientLoopEvent::Tick,
        };

        match event {
            ClientLoopEvent::Inbound(res) => {
                let (len, peer) = match res {
                    Ok(v) => v,
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::ConnectionReset {
                            tracing::debug!("udp connection reset (ignoring): {}", e);
                            continue;
                        }
                        tracing::error!("udp socket recv error: {}", e);
                        break;
                    }
                };

                if peer != context.server {
                    tracing::debug!("ignoring packet from unknown peer");
                    continue;
                }

                if len == 0 {
                    continue;
                }

                let mut packet = Bytes::copy_from_slice(&buf[..len]);

                // Filter out non-datagram packets (offline packets, e.g. OpenConnectionReply2)
                // Valid datagrams must have VALID (0x80), ACK (0x40), or NACK (0x20) flags.
                // Offline packets are typically ID < 0x20.
                let header_byte = packet[0];
                if header_byte < 0x80 && (header_byte & 0x60) == 0 {
                    match RaknetPacket::decode(&mut packet) {
                        Ok(pkt) => {
                            let error = match pkt {
                                RaknetPacket::ConnectionRequestFailed(_) => {
                                    Some(crate::RaknetError::ConnectionRequestFailed)
                                }
                                RaknetPacket::AlreadyConnected(_) => {
                                    Some(crate::RaknetError::AlreadyConnected)
                                }
                                RaknetPacket::IncompatibleProtocolVersion(_) => {
                                    Some(crate::RaknetError::IncompatibleProtocolVersion)
                                }
                                RaknetPacket::NoFreeIncomingConnections(_) => {
                                    Some(crate::RaknetError::ServerFull)
                                }
                                RaknetPacket::ConnectionBanned(_) => {
                                    Some(crate::RaknetError::ConnectionBanned)
                                }
                                RaknetPacket::IpRecentlyConnected(_) => {
                                    Some(crate::RaknetError::IpRecentlyConnected)
                                }
                                _ => None,
                            };

                            if let Some(e) = error {
                                tracing::debug!(error = ?e, "received connection failure packet");
                                if let Some(tx) = ready_signal.take() {
                                    let _ = tx.send(Err(e));
                                }
                                return;
                            } else {
                                tracing::debug!("ignoring non-datagram packet");
                            }
                        }
                        Err(_) => {
                            tracing::debug!("ignoring malformed non-datagram packet");
                        }
                    }
                    continue;
                }

                if let Ok(dgram) = Datagram::decode(&mut packet) {
                    let now = Instant::now();
                    let ms = ensure_client_session(
                        &mut managed,
                        context.server,
                        context.config.mtu as usize,
                        context.client_guid,
                        now,
                        &context.config,
                    );
                    ensure_client_handshake(
                        ms,
                        &mut handshake_started,
                        context.server_guid,
                        now,
                        context.secure_connection_established,
                        &socket,
                        context.server,
                        &mut send_buf,
                    )
                    .await;

                    match ms.handle_datagram(dgram, now) {
                        Ok(pkts) => {
                            if !deliver_app_packets(pkts, &context.to_app).await {
                                return;
                            }
                        }
                        Err(e) => {
                            if matches!(
                                e,
                                crate::session::manager::SessionError::Protocol(
                                    crate::protocol::packet::DecodeError::InvalidAddrVersion(_)
                                )
                            ) {
                                tracing::debug!(error = ?e, "ignoring datagram with invalid addr version");
                                continue;
                            }
                            tracing::debug!(error = ?e, "failed to handle datagram");
                        }
                    }
                    notify_client_ready(ms, &mut ready_signal);

                    if ms.state() == ConnectionState::Closed {
                        if let Some(reason) = ms.last_disconnect_reason() {
                            tracing::info!(reason = ?reason, "session disconnected");
                            let _ = context
                                .to_app
                                .send(Err(crate::RaknetError::Disconnected(reason)))
                                .await;
                        } else {
                            let _ = context
                                .to_app
                                .send(Err(crate::RaknetError::ConnectionClosed))
                                .await;
                        }
                        return;
                    }

                    flush_built_datagrams(ms, &socket, context.server, now, false, &mut send_buf)
                        .await;
                } else {
                    tracing::debug!("failed to decode datagram");
                }
            }

            ClientLoopEvent::Outbound(msg) => {
                let now = Instant::now();
                let ms = ensure_client_session(
                    &mut managed,
                    context.server,
                    context.config.mtu as usize,
                    context.client_guid,
                    now,
                    &context.config,
                );
                ensure_client_handshake(
                    ms,
                    &mut handshake_started,
                    context.server_guid,
                    now,
                    context.secure_connection_established,
                    &socket,
                    context.server,
                    &mut send_buf,
                )
                .await;
                if let Err(error) =
                    ms.queue_app_packet(msg.packet, msg.reliability, msg.channel, msg.priority)
                {
                    tracing::debug!(?error, "failed to queue outbound packet");
                    let _ = context.to_app.send(Err(error.into())).await;
                    continue;
                }
                flush_built_datagrams(ms, &socket, context.server, now, false, &mut send_buf).await;
                notify_client_ready(ms, &mut ready_signal);
            }

            ClientLoopEvent::Tick => {
                if let Some(ms) = managed.as_mut() {
                    let now = Instant::now();
                    flush_built_datagrams(ms, &socket, context.server, now, true, &mut send_buf)
                        .await;
                    if !deliver_app_packets(ms.drain_pending_incoming(now), &context.to_app).await {
                        return;
                    }
                    notify_client_ready(ms, &mut ready_signal);
                }
            }

            ClientLoopEvent::OutboundClosed => break,
        }
    }

    // Graceful shutdown check using match to avoid nesting if-lets
    match managed {
        Some(mut ms) if ms.is_connected() => {
            tracing::debug!("channel closed, sending disconnect notification");
            let _ = ms.send_disconnect(crate::protocol::state::DisconnectReason::Disconnected);
            // Flush the disconnect packet
            flush_built_datagrams(
                &mut ms,
                &socket,
                context.server,
                Instant::now(),
                true,
                &mut send_buf,
            )
            .await;
        }
        _ => {}
    }

    tracing::debug!("client muxer terminated");
}

async fn deliver_app_packets(
    pkts: Vec<crate::session::IncomingPacket>,
    to_app: &mpsc::Sender<Result<ReceivedMessage, crate::RaknetError>>,
) -> bool {
    for p in ManagedSession::filter_app_packets(pkts) {
        if let RaknetPacket::UserData { id, payload } = p.packet {
            tracing::trace!("received user packet");
            let msg = ReceivedMessage {
                id,
                payload,
                reliability: p.reliability,
                channel: p.ordering_channel.unwrap_or(0),
            };
            if to_app.send(Ok(msg)).await.is_err() {
                tracing::debug!("app channel closed");
                return false;
            }
        }
    }
    true
}

#[tracing::instrument(skip_all, level = "debug")]
async fn perform_offline_handshake(
    socket: &UdpSocket,
    server: SocketAddr,
    mtu_hint: usize,
    client_guid: u64,
) -> std::io::Result<OfflineHandshake> {
    let mut reply1 = None;
    let mut used_mtu = 0;

    for mtu in mtu_probe_candidates(mtu_hint) {
        tracing::debug!(mtu = mtu, "probing mtu");
        let req1 =
            RaknetPacket::OpenConnectionRequest1(crate::protocol::packet::OpenConnectionRequest1 {
                magic: DEFAULT_UNCONNECTED_MAGIC,
                protocol_version: RAKNET_PROTOCOL_VERSION,
                // IP (20) + UDP (8) + Packet ID (1) + Magic (16) + Protocol (1) = 46 bytes overhead
                padding: EoBPadding(mtu.saturating_sub(46).into()),
            });

        let mut buf = BytesMut::new();
        if req1.encode(&mut buf).is_err() {
            tracing::warn!(mtu = mtu, "failed to encode OpenConnectionRequest1");
            continue;
        }
        if let Err(e) = socket.send_to(&buf, server).await {
            tracing::warn!(mtu = mtu, error = ?e, "failed to send OpenConnectionRequest1");
            continue;
        }

        let mut tmp = [0u8; 2048];
        let mut attempts = 0;
        while attempts < HANDSHAKE_RETRIES {
            // Short timeout for each attempt so we don't stall the whole probe sequence.
            let res = timeout(HANDSHAKE_TIMEOUT, socket.recv_from(&mut tmp)).await;

            if let Ok(Ok((len, from))) = res {
                if from == server {
                    let mut slice = &tmp[..len];
                    // Cleaner pattern match without let_chains or nesting
                    if let Ok(RaknetPacket::OpenConnectionReply1(r)) =
                        RaknetPacket::decode(&mut slice)
                    {
                        tracing::debug!(
                            mtu = mtu,
                            server_mtu = r.mtu,
                            "received OpenConnectionReply1"
                        );
                        reply1 = Some(r);
                        used_mtu = mtu;
                        break;
                    } else {
                        tracing::debug!("ignoring non-reply1 packet during probe");
                    }
                } else {
                    tracing::debug!("ignoring reply from non-server during probe");
                }
            } else {
                tracing::debug!("timeout waiting for reply1 attempt");
            }
            attempts += 1;
        }

        if reply1.is_some() {
            break;
        }
        tracing::debug!(
            mtu = mtu,
            "no valid OpenConnectionReply1 received for probe"
        );
    }

    let reply1 = reply1.ok_or_else(|| {
        tracing::error!("failed to receive any OpenConnectionReply1");
        std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "timeout waiting for OpenConnectionReply1 after probing MTUs",
        )
    })?;

    let server_mtu = reply1.mtu;
    let cookie = reply1.cookie;

    // Negotiate final MTU: min(client_probed, server_reported)
    let mtu_final = used_mtu
        .clamp(MINIMUM_MTU_SIZE, MAXIMUM_MTU_SIZE)
        .min(server_mtu);

    tracing::debug!(negotiated_mtu = mtu_final, "sending OpenConnectionRequest2");

    let req2 =
        RaknetPacket::OpenConnectionRequest2(crate::protocol::packet::OpenConnectionRequest2 {
            magic: DEFAULT_UNCONNECTED_MAGIC,
            cookie,
            client_proof: cookie.is_some(),
            server_addr: server,
            mtu: mtu_final,
            client_guid,
        });

    let mut buf2 = BytesMut::new();
    req2.encode(&mut buf2).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "failed to encode request2")
    })?;
    socket.send_to(&buf2, server).await?;

    let mut tmp = [0u8; 2048];
    let reply2 = loop {
        let res = timeout(Duration::from_secs(2), socket.recv_from(&mut tmp)).await;
        let (len, from) = match res {
            Ok(Ok(v)) => v,
            _ => {
                tracing::error!("timeout waiting for OpenConnectionReply2");
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "timeout waiting for OpenConnectionReply2",
                ));
            }
        };
        if from != server {
            continue;
        }
        let mut slice = &tmp[..len];
        if let Ok(RaknetPacket::OpenConnectionReply2(r)) = RaknetPacket::decode(&mut slice) {
            tracing::debug!(server_guid = r.server_guid, "handshake complete");
            break r;
        }
    };

    Ok(OfflineHandshake {
        mtu: mtu_final,
        server_guid: reply2.server_guid,
        secure_connection_established: reply2.security,
    })
}

fn connection_timed_out(timeout: Duration) -> crate::RaknetError {
    crate::RaknetError::Io(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        format!("connection timed out after {:?}", timeout),
    ))
}

fn apply_socket_buffer_config(
    _socket: &std::net::UdpSocket,
    recv_size: Option<usize>,
    send_size: Option<usize>,
) {
    if recv_size.is_some() || send_size.is_some() {
        tracing::debug!(
            recv_size,
            send_size,
            "socket buffer tuning is unavailable on the std-only transport path; using OS defaults"
        );
    }
}

fn mtu_probe_candidates(mtu_hint: usize) -> Vec<u16> {
    let hinted = mtu_hint.clamp(MINIMUM_MTU_SIZE as usize, MAXIMUM_MTU_SIZE as usize) as u16;
    let mut candidates = Vec::with_capacity(crate::protocol::constants::MTU_SIZES.len() + 1);
    candidates.push(hinted);
    for &candidate in crate::protocol::constants::MTU_SIZES {
        if !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }
    candidates
}

fn client_guid() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0xdead_beef_dead_beef)
}

fn ensure_client_session<'a>(
    managed: &'a mut Option<ManagedSession>,
    server: SocketAddr,
    mtu: usize,
    client_guid: u64,
    now: Instant,
    config: &RaknetStreamConfig,
) -> &'a mut ManagedSession {
    managed.get_or_insert_with(|| {
        ManagedSession::with_config(
            server,
            mtu,
            now,
            SessionConfig {
                role: SessionRole::Client,
                guid: client_guid,
                session_timeout: config.session_timeout,
                session: crate::session::SessionTunables {
                    max_ordering_channels: config.max_ordering_channels,
                    ack_queue_capacity: config.ack_queue_capacity,
                    split_timeout: config.split_timeout,
                    reliable_window: config.reliable_window,
                    max_split_parts: config.max_split_parts,
                    max_concurrent_splits: config.max_concurrent_splits,
                    max_incoming_ack_queue: config.max_incoming_ack_queue,
                },
                ..SessionConfig::default()
            },
        )
    })
}

#[allow(clippy::too_many_arguments)]
async fn ensure_client_handshake(
    managed: &mut ManagedSession,
    handshake_started: &mut bool,
    server_guid: u64,
    now: Instant,
    secure_connection_established: bool,
    socket: &UdpSocket,
    server: SocketAddr,
    send_buf: &mut BytesMut,
) {
    if !*handshake_started
        && managed
            .start_client_handshake(server_guid, now, secure_connection_established)
            .is_ok()
    {
        *handshake_started = true;
        flush_built_datagrams(managed, socket, server, now, false, send_buf).await;
    }
}

#[tracing::instrument(skip_all, fields(peer= %peer, on_tick = %run_tick), level = "trace")]
async fn flush_built_datagrams(
    managed: &mut ManagedSession,
    socket: &UdpSocket,
    peer: SocketAddr,
    now: Instant,
    run_tick: bool,
    out: &mut BytesMut,
) {
    if run_tick {
        for d in managed.on_tick(now) {
            tracing::trace!("send_tick_datagram");
            out.clear();
            if let Err(e) = d.encode(out) {
                tracing::error!(error = ?e, "failed to encode tick datagram - dropping");
                continue;
            }
            let _ = socket.send_to(out.as_ref(), peer).await;
        }
    }

    while let Some(d) = managed.build_datagram(now) {
        tracing::trace!("send_datagram");
        out.clear();
        if let Err(e) = d.encode(out) {
            tracing::error!(error = ?e, "failed to encode datagram - dropping");
            continue;
        }
        let _ = socket.send_to(out.as_ref(), peer).await;
    }

    // Delivery of any unblocked packets happens via drain_ready_to_app() at call sites.
}

#[tracing::instrument(skip(managed, ready), level = "trace")]
fn notify_client_ready(
    managed: &ManagedSession,

    ready: &mut Option<oneshot::Sender<Result<(), crate::RaknetError>>>,
) {
    if managed.is_connected()
        && let Some(tx) = ready.take()
    {
        tracing::trace!("sending ready signal");

        let _ = tx.send(Ok(()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mtu_probe_candidates_prefers_hint_without_duplicates() {
        assert_eq!(
            mtu_probe_candidates(900),
            vec![900, 1200, MAXIMUM_MTU_SIZE, MINIMUM_MTU_SIZE]
        );
        assert_eq!(
            mtu_probe_candidates(MAXIMUM_MTU_SIZE as usize),
            vec![MAXIMUM_MTU_SIZE, 1200, MINIMUM_MTU_SIZE]
        );
    }

    #[test]
    fn mtu_probe_candidates_clamps_out_of_range_hints() {
        assert_eq!(
            mtu_probe_candidates(0).first().copied(),
            Some(MINIMUM_MTU_SIZE)
        );
        assert_eq!(
            mtu_probe_candidates(usize::MAX).first().copied(),
            Some(MAXIMUM_MTU_SIZE)
        );
    }
}

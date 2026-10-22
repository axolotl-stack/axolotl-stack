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
    constants::{MAXIMUM_MTU_SIZE, UDP_HEADER_SIZE},
    datagram::Datagram,
    packet::RaknetPacket,
};
use crate::session::TransmissionPolicy;
use crate::session::manager::{ConnectionState, ManagedSession, SessionConfig, SessionRole};
use crate::session::policy::VANILLA_TICK_INTERVAL;

use super::handshake;
use super::{OutboundMsg, ReceivedMessage};
use crate::transport::Message;

use crate::protocol::constants::{self};

/// Optimized client maintenance cadence.
const TICK_INTERVAL: Duration = Duration::from_millis(20);
/// go-raknet `conn.go:newConn`: the application queue grows to 4096 messages.
pub(crate) const CLIENT_RECEIVE_QUEUE: usize = 4096;
/// go-raknet `dial.go:clientListen` reads into a `maxMTUSize` buffer.
const VANILLA_MAX_DATAGRAM: usize = MAXIMUM_MTU_SIZE as usize;

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
    /// Transmission policy. Defaults to [`TransmissionPolicy::Vanilla`], the
    /// lunar-bedrock/go-raknet (vanilla Bedrock client) behaviour.
    pub transmission_policy: TransmissionPolicy,
    /// Transient receive errors (connection refused/reset/aborted, host or
    /// network unreachable) ignored during the offline handshake; `None` is
    /// unlimited. go-raknet `Dialer.MaxTransientErrors` defaults to 10.
    pub max_transient_errors: Option<u32>,
}

impl Default for RaknetStreamConfig {
    fn default() -> Self {
        Self {
            mtu: MAXIMUM_MTU_SIZE,
            socket_recv_buffer_size: Some(super::DEFAULT_SOCKET_RECV_BUFFER),
            socket_send_buffer_size: None,
            connection_timeout: Duration::from_secs(10),
            // Upstream `conn.go:Conn.startTicking`: base inactivity timeout5s.
            session_timeout: Duration::from_secs(5),
            max_ordering_channels: constants::MAXIMUM_ORDERING_CHANNELS as usize,
            ack_queue_capacity: 1024,
            split_timeout: Duration::from_secs(30),
            reliable_window: 2048,
            // Upstream `conn.go:Conn.receiveSplitPacket`; also enforced locally
            // for clients to bound memory from hostile servers.
            max_split_parts: 512,
            max_concurrent_splits: 16,
            max_incoming_ack_queue: 4096,
            transmission_policy: TransmissionPolicy::Vanilla,
            max_transient_errors: Some(10),
        }
    }
}

impl RaknetStreamConfig {
    pub fn validate(&self) -> Result<(), super::ConfigError> {
        super::validate_mtu(self.mtu)?;
        super::validate_duration(self.connection_timeout, "connection_timeout")?;
        super::validate_duration(self.session_timeout, "session_timeout")?;
        for (value, field) in [
            (self.socket_recv_buffer_size, "socket_recv_buffer_size"),
            (self.socket_send_buffer_size, "socket_send_buffer_size"),
        ] {
            super::require_config(value != Some(0), field, "must be positive when specified")?;
        }
        super::validate_session_limits(
            self.max_ordering_channels,
            self.ack_queue_capacity,
            self.split_timeout,
            self.reliable_window,
            self.max_split_parts,
            self.max_concurrent_splits,
            self.max_incoming_ack_queue,
        )
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
    rtt: super::RttCell,
}

impl RaknetStream {
    /// Internal constructor for creating a stream from an established connection.
    pub(crate) fn new(
        local: SocketAddr,
        peer: SocketAddr,
        incoming: mpsc::Receiver<Result<ReceivedMessage, crate::RaknetError>>,
        outbound_tx: mpsc::Sender<OutboundMsg>,
        rtt: super::RttCell,
    ) -> Self {
        Self {
            local,
            peer,
            incoming,
            outbound_tx: PollSender::new(outbound_tx),
            rtt,
        }
    }

    /// Latest round-trip time estimate of the session, `None` before the
    /// first acknowledgement sample: the smoothed ACK RTT (Optimized) or the
    /// mean of the recent resend-map samples (Vanilla, go-raknet
    /// `resend_map.go:rtt`). Readable without involving the transport task.
    pub fn rtt(&self) -> Option<Duration> {
        self.rtt.load()
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
        config.validate().map_err(std::io::Error::from)?;
        let connection_timeout = config.connection_timeout;

        timeout(connection_timeout, async move {
            let socket = std::net::UdpSocket::bind(if server.is_ipv4() {
                "0.0.0.0:0"
            } else {
                "[::]:0"
            })?;
            socket.set_nonblocking(true)?;
            super::apply_socket_buffers(
                &socket,
                config.socket_recv_buffer_size,
                config.socket_send_buffer_size,
            )?;

            let socket = UdpSocket::from_std(socket)?;
            let local = socket.local_addr()?;

            // Perform offline handshake using OpenConnectionRequest1/2.
            let client_guid = match config.transmission_policy {
                TransmissionPolicy::Vanilla => handshake::vanilla_dialer_id(),
                TransmissionPolicy::Optimized => client_guid(),
            };
            let handshake = handshake::perform(
                &socket,
                server,
                config.transmission_policy,
                config.mtu,
                client_guid,
                config.max_transient_errors,
            )
            .await?;

            // Use negotiated MTU
            let mut config = config;
            config.mtu = handshake.mtu;

            let (outbound_tx, outbound_rx) = mpsc::channel::<OutboundMsg>(1024);
            let (to_app_tx, to_app_rx) =
                mpsc::channel::<Result<ReceivedMessage, crate::RaknetError>>(CLIENT_RECEIVE_QUEUE);
            let (ready_tx, ready_rx) = oneshot::channel();

            let maintenance_interval = match config.transmission_policy {
                TransmissionPolicy::Vanilla => VANILLA_TICK_INTERVAL,
                TransmissionPolicy::Optimized => TICK_INTERVAL,
            };
            let rtt = super::RttCell::new();
            let context = ClientMuxerContext {
                server,
                server_guid: handshake.server_guid,
                client_guid,
                secure_connection_established: handshake.secure_connection_established,
                outbound_rx,
                to_app: to_app_tx,
                ready: ready_tx,
                config,
                maintenance_interval,
                rtt: rtt.clone(),
            };

            tokio::spawn(run_client_muxer(socket, context));

            match ready_rx.await {
                Ok(Ok(())) => Ok(Self::new(local, server, to_app_rx, outbound_tx, rtt)),
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

        let msg = OutboundMsg {
            peer,
            buffer: bytes,
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

enum ClientLoopEvent<Msg> {
    Inbound(std::io::Result<(usize, SocketAddr)>),
    Outbound(Msg),
    Ack,
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
    /// Maintenance cadence: 100 ms for Vanilla (go-raknet's connection
    /// ticker), `TICK_INTERVAL` for Optimized. Tests may lengthen it to prove a
    /// behaviour does not depend on maintenance.
    maintenance_interval: Duration,
    /// RTT estimate published to the `RaknetStream`.
    rtt: super::RttCell,
}

/// Windows reports a datagram longer than the receive buffer as `WSAEMSGSIZE`
/// (Unix truncates silently). go-raknet logs it and keeps reading; the
/// datagram is dropped in both policies.
pub(crate) fn is_oversized_datagram(error: &std::io::Error) -> bool {
    #[cfg(windows)]
    {
        error.raw_os_error() == Some(10040)
    }
    #[cfg(not(windows))]
    {
        let _ = error;
        false
    }
}

fn client_session(
    config: &RaknetStreamConfig,
    server: SocketAddr,
    client_guid: u64,
    now: Instant,
) -> ManagedSession {
    ManagedSession::with_config(
        server,
        config.mtu as usize,
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
                transmission: config.transmission_policy,
            },
            ..SessionConfig::default()
        },
    )
}

/// Report a closed session to the application.
fn report_closed(
    managed: &ManagedSession,
    to_app: &mpsc::Sender<Result<ReceivedMessage, crate::RaknetError>>,
) {
    let error = match managed.last_disconnect_reason() {
        Some(reason) => {
            tracing::info!(reason = ?reason, "session disconnected");
            crate::RaknetError::Disconnected(reason)
        }
        None => crate::RaknetError::ConnectionClosed,
    };
    let _ = to_app.try_send(Err(error));
}

#[tracing::instrument(skip(socket, context), fields(server = %context.server, mtu = context.config.mtu), level = "debug")]
async fn run_client_muxer(socket: UdpSocket, mut context: ClientMuxerContext) {
    let vanilla = context.config.transmission_policy.is_vanilla();
    // Vanilla reads like go-raknet's 1492-byte buffer; one extra byte detects
    // (and drops) longer datagrams on platforms that truncate.
    let buffer_len = if vanilla {
        VANILLA_MAX_DATAGRAM + 1
    } else {
        context.config.mtu as usize + UDP_HEADER_SIZE + 64
    };
    let mut buf = vec![0u8; buffer_len];
    let mut send_buf = BytesMut::with_capacity(context.config.mtu as usize + UDP_HEADER_SIZE + 64);
    let mut ready_signal = Some(context.ready);
    let server = context.server;

    let now = Instant::now();
    let mut managed = client_session(&context.config, server, context.client_guid, now);
    // go-raknet conn.go:newConn starts the 100 ms connection ticker before the
    // ConnectionRequest is written; its first tick fires one period later.
    let mut tick = if vanilla {
        time::interval_at(
            (now + context.maintenance_interval).into(),
            context.maintenance_interval,
        )
    } else {
        time::interval(context.maintenance_interval)
    };
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let ack_timer = time::sleep(Duration::ZERO);
    tokio::pin!(ack_timer);
    let mut outbound_open = true;

    if managed
        .start_client_handshake(
            context.server_guid,
            now,
            context.secure_connection_established,
        )
        .is_ok()
    {
        flush_built_datagrams(&mut managed, &socket, server, now, false, &mut send_buf).await;
    }

    loop {
        context.rtt.store(managed.rtt_micros());
        let ack_deadline = managed.ack_deadline();
        if let Some(deadline) = ack_deadline {
            ack_timer.as_mut().reset(deadline.into());
        }
        let event = tokio::select! {
            res = socket.recv_from(&mut buf) => ClientLoopEvent::Inbound(res),
            msg = context.outbound_rx.recv(), if outbound_open => match msg {
                Some(msg) => ClientLoopEvent::Outbound(msg),
                None => ClientLoopEvent::OutboundClosed,
            },
            _ = tick.tick() => ClientLoopEvent::Tick,
            _ = &mut ack_timer, if ack_deadline.is_some() => ClientLoopEvent::Ack,
        };

        match event {
            ClientLoopEvent::Inbound(res) => {
                let (len, peer) = match res {
                    Ok(v) => v,
                    Err(e)
                        if handshake::is_transient_receive_error(&e)
                            || is_oversized_datagram(&e) =>
                    {
                        tracing::debug!("ignoring transient udp receive error: {}", e);
                        continue;
                    }
                    Err(e) => {
                        tracing::error!("udp socket recv error: {}", e);
                        break;
                    }
                };

                if peer != server {
                    tracing::debug!("ignoring packet from unknown peer");
                    continue;
                }

                if len == 0 {
                    continue;
                }
                if vanilla && len > VANILLA_MAX_DATAGRAM {
                    tracing::debug!(len, "dropping datagram longer than go-raknet's buffer");
                    continue;
                }
                if vanilla {
                    // conn.go:receive refreshes activity for every datagram it
                    // is handed, before (and whatever) its first byte.
                    managed.record_activity(Instant::now());
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

                let now = Instant::now();
                let dgram = match Datagram::decode(&mut packet) {
                    Ok(dgram) => dgram,
                    Err(error) if vanilla => {
                        // dial.go:clientListen logs handler errors and keeps
                        // reading.
                        tracing::debug!(?error, "dropping malformed connected datagram");
                        continue;
                    }
                    Err(error) => {
                        tracing::debug!(?error, "failed to decode connected datagram");
                        let _ = context.to_app.try_send(Err(error.into()));
                        return;
                    }
                };

                match managed.handle_datagram(dgram, now) {
                    Ok(pkts) => {
                        if !deliver_app_packets(pkts, &context.to_app).await {
                            break;
                        }
                    }
                    Err(error) if vanilla => {
                        tracing::debug!(?error, "dropping datagram after protocol error");
                    }
                    Err(e) => {
                        tracing::debug!(error = ?e, "failed to handle datagram");
                        let _ = context.to_app.try_send(Err(e.into()));
                        return;
                    }
                }
                notify_client_ready(&managed, &mut ready_signal);

                if vanilla {
                    // Responses (NIC, pong, the echoed disconnect) are written
                    // before the connection closes, as in go-raknet.
                    flush_built_datagrams(&mut managed, &socket, server, now, false, &mut send_buf)
                        .await;
                }
                if managed.state() == ConnectionState::Closed {
                    report_closed(&managed, &context.to_app);
                    return;
                }
                if !vanilla {
                    flush_built_datagrams(&mut managed, &socket, server, now, false, &mut send_buf)
                        .await;
                }
            }

            ClientLoopEvent::Outbound(msg) => {
                let now = Instant::now();
                if let Err(error) =
                    managed.queue_app_buffer(msg.buffer, msg.reliability, msg.channel, msg.priority)
                {
                    tracing::debug!(?error, "failed to queue outbound packet");
                    let _ = context.to_app.try_send(Err(error.into()));
                    continue;
                }
                flush_built_datagrams(&mut managed, &socket, server, now, false, &mut send_buf)
                    .await;
                notify_client_ready(&managed, &mut ready_signal);
            }

            ClientLoopEvent::Ack => {
                flush_built_datagrams(
                    &mut managed,
                    &socket,
                    server,
                    Instant::now(),
                    false,
                    &mut send_buf,
                )
                .await;
            }

            ClientLoopEvent::Tick => {
                let now = Instant::now();
                flush_built_datagrams(&mut managed, &socket, server, now, true, &mut send_buf)
                    .await;
                if managed.state() == ConnectionState::Closed {
                    if vanilla {
                        report_closed(&managed, &context.to_app);
                    } else {
                        let _ = context
                            .to_app
                            .try_send(Err(crate::RaknetError::ConnectionClosed));
                    }
                    return;
                }
                if !deliver_app_packets(managed.drain_pending_incoming(now), &context.to_app).await
                {
                    break;
                }
                notify_client_ready(&managed, &mut ready_signal);
            }

            ClientLoopEvent::OutboundClosed => {
                outbound_open = false;
                if !vanilla {
                    break;
                }
                // go-raknet Conn.Close: keep ticking (ACKs, resends) until the
                // resend map drains, then send a bare DisconnectNotification.
                tracing::debug!("stream dropped, closing after drain");
                if managed
                    .close(
                        crate::protocol::state::DisconnectReason::Disconnected,
                        Instant::now(),
                    )
                    .is_err()
                {
                    break;
                }
            }
        }
    }

    // The loop only ends on a local failure (socket error, full or dropped
    // application queue, or an Optimized stream drop): notify the peer.
    if managed.state() != ConnectionState::Closed && (vanilla || managed.is_connected()) {
        tracing::debug!("sending disconnect notification");
        managed.disconnect_now(crate::protocol::state::DisconnectReason::Disconnected);
        flush_built_datagrams(
            &mut managed,
            &socket,
            server,
            Instant::now(),
            !vanilla,
            &mut send_buf,
        )
        .await;
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
            if to_app.try_send(Ok(msg)).is_err() {
                tracing::debug!("app channel closed");
                return false;
            }
        }
    }
    true
}

fn connection_timed_out(timeout: Duration) -> crate::RaknetError {
    crate::RaknetError::Io(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        format!("connection timed out after {:?}", timeout),
    ))
}

fn client_guid() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| (d.as_nanos() as u64) | (1 << 63))
        .unwrap_or(0xdead_beef_dead_beef)
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

    #[tokio::test]
    async fn client_acks_a_quiet_server_without_application_or_socket_traffic() {
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let client_address = socket.local_addr().unwrap();
        let (_outbound_tx, outbound_rx) = mpsc::channel(1);
        let (to_app, mut incoming) = mpsc::channel(8);
        let (ready, ready_rx) = oneshot::channel();
        let task = tokio::spawn(run_client_muxer(
            socket,
            ClientMuxerContext {
                server: server.local_addr().unwrap(),
                server_guid: 1,
                client_guid: u64::MAX,
                secure_connection_established: false,
                outbound_rx,
                to_app,
                ready,
                config: RaknetStreamConfig {
                    transmission_policy: TransmissionPolicy::Optimized,
                    ..RaknetStreamConfig::default()
                },
                // A maintenance tick far beyond the assertion window: an ACK
                // that arrives in time can only come from the ACK deadline path,
                // so scheduler load cannot turn a maintenance flush into a pass
                // and the window can be generous enough not to flake.
                maintenance_interval: Duration::from_secs(30),
                rtt: crate::transport::RttCell::new(),
            },
        ));
        let mut session = ManagedSession::with_config(
            client_address,
            1200,
            Instant::now(),
            SessionConfig {
                role: SessionRole::Server,
                ..SessionConfig::default()
            },
        );
        let mut buffer = [0; 2048];
        let mut encoded = BytesMut::new();
        timeout(Duration::from_secs(1), async {
            while !session.is_connected() {
                let (len, _) = server.recv_from(&mut buffer).await.unwrap();
                let datagram = Datagram::decode(&mut &buffer[..len]).unwrap();
                session.handle_datagram(datagram, Instant::now()).unwrap();
                flush_built_datagrams(
                    &mut session,
                    &server,
                    client_address,
                    Instant::now(),
                    false,
                    &mut encoded,
                )
                .await;
            }
        })
        .await
        .unwrap();
        ready_rx.await.unwrap().unwrap();

        // Five independent quiet rounds: no reply, ping or further input may
        // provide the ACK. The server does not run maintenance.
        for _ in 0..5 {
            session
                .queue_app_buffer(
                    Bytes::from_static(b"\xfequiet server"),
                    crate::protocol::reliability::Reliability::ReliableOrdered,
                    0,
                    crate::protocol::state::RakPriority::Normal,
                )
                .unwrap();
            let datagram = session.build_datagram(Instant::now()).unwrap();
            let sequence = datagram.header.sequence.value();
            encoded.clear();
            datagram.encode(&mut encoded).unwrap();
            server.send_to(&encoded, client_address).await.unwrap();
            let message = incoming.recv().await.unwrap().unwrap();
            assert_eq!(message.payload, Bytes::from_static(b"quiet server"));
            timeout(Duration::from_millis(500), async {
                loop {
                    let (len, _) = server.recv_from(&mut buffer).await.unwrap();
                    let datagram = Datagram::decode(&mut &buffer[..len]).unwrap();
                    let acknowledged = matches!(
                        &datagram.payload,
                        crate::protocol::datagram::DatagramPayload::Ack(ack)
                            if ack.ranges.iter().any(|range|
                                range.start.value() <= sequence && range.end.value() >= sequence)
                    );
                    session.handle_datagram(datagram, Instant::now()).unwrap();
                    if acknowledged {
                        // Retire feedback so the next round has an empty window.
                        while session.build_datagram(Instant::now()).is_some() {}
                        break;
                    }
                }
            })
            .await
            .expect("client ACK waited for maintenance");
        }
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn malformed_server_datagram_reports_error_and_stops_muxer() {
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let client_address = socket.local_addr().unwrap();
        let (_outbound_tx, outbound_rx) = mpsc::channel(1);
        let (to_app, mut incoming) = mpsc::channel(1);
        let (ready, _ready_rx) = oneshot::channel();
        let task = tokio::spawn(run_client_muxer(
            socket,
            ClientMuxerContext {
                server: server.local_addr().unwrap(),
                server_guid: 1,
                client_guid: u64::MAX,
                secure_connection_established: false,
                outbound_rx,
                to_app,
                ready,
                config: RaknetStreamConfig {
                    transmission_policy: TransmissionPolicy::Optimized,
                    ..RaknetStreamConfig::default()
                },
                maintenance_interval: TICK_INTERVAL,
                rtt: crate::transport::RttCell::new(),
            },
        ));
        // A connected datagram flag without its mandatory sequence number.
        server.send_to(&[0x80], client_address).await.unwrap();
        let error = timeout(Duration::from_secs(1), incoming.recv())
            .await
            .unwrap();
        assert!(matches!(error, Some(Err(crate::RaknetError::Decode(_)))));
        timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn probe_write_errors_exhaust_candidates_instead_of_aborting_discovery() {
        // An IPv4 socket cannot send to the IPv6 loopback address. This produces
        // a real local send error without relying on a particular route MTU.
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let result = timeout(
            Duration::from_secs(1),
            handshake::perform(
                &socket,
                "[::1]:19132".parse().unwrap(),
                TransmissionPolicy::Optimized,
                1492,
                u64::MAX,
                Some(10),
            ),
        )
        .await
        .expect("failed writes must advance immediately to smaller probes");
        let Err(crate::RaknetError::Io(error)) = result else {
            panic!("address-family-mismatched probe did not fail with an I/O error");
        };
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(error.to_string().contains("after probing MTUs"));
    }

    #[test]
    fn invalid_stream_config_is_rejected() {
        let mut config = RaknetStreamConfig {
            mtu: 399,
            ..Default::default()
        };
        assert!(config.validate().is_err());
        config.mtu = 1492;
        config.connection_timeout = Duration::ZERO;
        assert!(config.validate().is_err());
    }

    #[test]
    fn generated_guid_is_negative_on_wire() {
        assert!((client_guid() as i64) < 0);
    }

    /// KA-7: errors go-raknet treats as transient never end the client.
    #[test]
    fn transient_receive_errors_do_not_close_the_client() {
        use std::io::{Error, ErrorKind};
        for kind in [
            ErrorKind::ConnectionRefused,
            ErrorKind::ConnectionReset,
            ErrorKind::ConnectionAborted,
            ErrorKind::HostUnreachable,
            ErrorKind::NetworkUnreachable,
        ] {
            assert!(handshake::is_transient_receive_error(&Error::from(kind)));
        }
        assert!(!handshake::is_transient_receive_error(&Error::from(
            ErrorKind::PermissionDenied
        )));
        #[cfg(windows)]
        assert!(is_oversized_datagram(&Error::from_raw_os_error(10040)));
    }

    /// HS-10: connection failure packets end the dial with a typed error in
    /// both policies.
    #[tokio::test]
    async fn connection_failure_packet_is_reported() {
        for policy in [TransmissionPolicy::Vanilla, TransmissionPolicy::Optimized] {
            let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let client = socket.local_addr().unwrap();
            let (_outbound, outbound_rx) = mpsc::channel(1);
            let (to_app, _incoming) = mpsc::channel(1);
            let (ready, ready_rx) = oneshot::channel();
            let task = tokio::spawn(run_client_muxer(
                socket,
                ClientMuxerContext {
                    server: server.local_addr().unwrap(),
                    server_guid: 1,
                    client_guid: u64::MAX,
                    secure_connection_established: false,
                    outbound_rx,
                    to_app,
                    ready,
                    config: RaknetStreamConfig {
                        transmission_policy: policy,
                        ..RaknetStreamConfig::default()
                    },
                    maintenance_interval: Duration::from_millis(100),
                    rtt: crate::transport::RttCell::new(),
                },
            ));
            let mut buffer = [0; 2048];
            server.recv_from(&mut buffer).await.unwrap();
            server.send_to(&[0x14], client).await.unwrap();
            let result = timeout(Duration::from_secs(1), ready_rx).await.unwrap();
            assert!(
                matches!(result, Ok(Err(crate::RaknetError::ServerFull))),
                "{policy:?}"
            );
            timeout(Duration::from_secs(1), task)
                .await
                .unwrap()
                .unwrap();
        }
    }

    /// A Vanilla client muxer connected to a scripted server session.
    struct VanillaHarness {
        server: UdpSocket,
        client: SocketAddr,
        session: ManagedSession,
        incoming: mpsc::Receiver<Result<ReceivedMessage, crate::RaknetError>>,
        outbound: Option<mpsc::Sender<OutboundMsg>>,
        task: tokio::task::JoinHandle<()>,
        /// When the server received the ConnectionRequest.
        request_seen: Instant,
    }

    impl VanillaHarness {
        async fn connect() -> Self {
            Self::connect_with(RaknetStreamConfig::default()).await
        }

        async fn connect_with(config: RaknetStreamConfig) -> Self {
            let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let client = socket.local_addr().unwrap();
            let (outbound, outbound_rx) = mpsc::channel(8);
            let (to_app, incoming) = mpsc::channel(CLIENT_RECEIVE_QUEUE);
            let (ready, ready_rx) = oneshot::channel();
            let task = tokio::spawn(run_client_muxer(
                socket,
                ClientMuxerContext {
                    server: server.local_addr().unwrap(),
                    server_guid: 1,
                    client_guid: handshake::vanilla_dialer_id(),
                    secure_connection_established: false,
                    outbound_rx,
                    to_app,
                    ready,
                    config,
                    maintenance_interval: VANILLA_TICK_INTERVAL,
                    rtt: crate::transport::RttCell::new(),
                },
            ));
            let mut session = ManagedSession::with_config(
                client,
                1492,
                Instant::now(),
                SessionConfig {
                    role: SessionRole::Server,
                    ..SessionConfig::default()
                },
            );
            let mut buffer = [0; 2048];
            let mut encoded = BytesMut::new();
            let mut request_seen = None;
            timeout(Duration::from_secs(2), async {
                while !session.is_connected() {
                    let (len, _) = server.recv_from(&mut buffer).await.unwrap();
                    request_seen.get_or_insert_with(Instant::now);
                    let datagram = Datagram::decode(&mut &buffer[..len]).unwrap();
                    session.handle_datagram(datagram, Instant::now()).unwrap();
                    flush_built_datagrams(
                        &mut session,
                        &server,
                        client,
                        Instant::now(),
                        false,
                        &mut encoded,
                    )
                    .await;
                }
            })
            .await
            .unwrap();
            ready_rx.await.unwrap().unwrap();
            Self {
                server,
                client,
                session,
                incoming,
                outbound: Some(outbound),
                task,
                request_seen: request_seen.unwrap(),
            }
        }

        /// Send one user message from the server, returning its datagram sequence.
        async fn send_message(&mut self, body: &'static [u8]) -> u32 {
            self.session
                .queue_app_buffer(
                    Bytes::from_static(body),
                    crate::protocol::reliability::Reliability::ReliableOrdered,
                    0,
                    crate::protocol::state::RakPriority::Normal,
                )
                .unwrap();
            let datagram = self.session.build_datagram(Instant::now()).unwrap();
            self.send_datagram(&datagram).await;
            datagram.header.sequence.value()
        }

        async fn send_datagram(&self, datagram: &Datagram) {
            let mut encoded = BytesMut::new();
            datagram.encode(&mut encoded).unwrap();
            self.server.send_to(&encoded, self.client).await.unwrap();
        }

        /// Next datagram from the client, without feeding the server session.
        async fn recv_raw(&self) -> Datagram {
            let mut buffer = [0; 2048];
            let (len, _) = timeout(Duration::from_secs(3), self.server.recv_from(&mut buffer))
                .await
                .expect("client went silent")
                .unwrap();
            Datagram::decode(&mut &buffer[..len]).unwrap()
        }

        async fn expect_message(&mut self, body: &[u8]) {
            let message = timeout(Duration::from_secs(2), self.incoming.recv())
                .await
                .expect("message not delivered")
                .unwrap()
                .unwrap();
            assert_eq!(message.id, body[0]);
            assert_eq!(message.payload.as_ref(), &body[1..]);
        }
    }

    fn acks(datagram: &Datagram, sequence: u32) -> bool {
        matches!(&datagram.payload, crate::protocol::datagram::DatagramPayload::Ack(ack)
            if ack.ranges.iter().any(|range|
                range.start.value() <= sequence && range.end.value() >= sequence))
    }

    /// MTU-11: datagrams above go-raknet's 1492-byte read buffer are dropped
    /// without ending the connection.
    #[tokio::test]
    async fn vanilla_client_drops_datagrams_longer_than_1492() {
        let mut harness = VanillaHarness::connect().await;
        let oversized = Datagram {
            header: crate::protocol::types::DatagramHeader {
                flags: crate::protocol::constants::DatagramFlags::VALID,
                sequence: crate::protocol::types::Sequence24::new(100),
            },
            payload: crate::protocol::datagram::DatagramPayload::EncapsulatedPackets(vec![
                crate::protocol::encapsulated_packet::EncapsulatedPacket {
                    header: crate::protocol::types::EncapsulatedPacketHeader {
                        reliability: crate::protocol::reliability::Reliability::Unreliable,
                        is_split: false,
                        needs_bas: false,
                    },
                    bit_length: 1500 << 3,
                    reliable_index: None,
                    sequence_index: None,
                    ordering_index: None,
                    ordering_channel: None,
                    split: None,
                    payload: Bytes::from(vec![0xfe; 1500]),
                },
            ]),
        };
        harness.send_datagram(&oversized).await;
        harness.send_message(b"\xfeafter").await;
        harness.expect_message(b"\xfeafter").await;
        assert!(!harness.task.is_finished());
    }

    /// KA-2: go-raknet `conn.go:receive` refreshes activity for every
    /// datagram, including offline IDs it then ignores. A server sending only
    /// those keeps the client past its inactivity timeout.
    #[tokio::test]
    async fn vanilla_client_offline_packets_refresh_activity() {
        let mut harness = VanillaHarness::connect_with(RaknetStreamConfig {
            session_timeout: Duration::from_millis(300),
            ..RaknetStreamConfig::default()
        })
        .await;
        // Ping ticks at 0.5 s and 1 s check `idle > 300 ms + 2 × rtt`.
        let until = Instant::now() + Duration::from_millis(1200);
        while Instant::now() < until {
            harness
                .server
                .send_to(&[0x1c], harness.client)
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        // A closing client would ACK but no longer handle this message.
        harness.send_message(b"\xfealive").await;
        harness.expect_message(b"\xfealive").await;
    }

    /// KA-8: malformed datagrams are logged and dropped (Optimized reports
    /// the error and stops instead).
    #[tokio::test]
    async fn vanilla_client_survives_malformed_datagram() {
        let mut harness = VanillaHarness::connect().await;
        harness
            .server
            .send_to(&[0x80], harness.client)
            .await
            .unwrap();
        harness.send_message(b"\xfestill here").await;
        harness.expect_message(b"\xfestill here").await;
        assert!(!harness.task.is_finished());
    }

    /// REL-7: a datagram beyond the 2048 receive window is dropped, and the
    /// connection continues like go-raknet's logged receive errors.
    #[tokio::test]
    async fn vanilla_client_survives_receive_window_violation() {
        let mut harness = VanillaHarness::connect().await;
        harness
            .session
            .queue_app_buffer(
                Bytes::from_static(b"\xfefar"),
                crate::protocol::reliability::Reliability::Unreliable,
                0,
                crate::protocol::state::RakPriority::Normal,
            )
            .unwrap();
        let mut far = harness.session.build_datagram(Instant::now()).unwrap();
        far.header.sequence = crate::protocol::types::Sequence24::new(5000);
        harness.send_datagram(&far).await;
        harness.send_message(b"\xfenear").await;
        harness.expect_message(b"\xfenear").await;
        assert!(
            harness.incoming.try_recv().is_err(),
            "far datagram delivered"
        );
        assert!(!harness.task.is_finished());
    }

    /// CC-4/ACK-1: the client ticker starts with the session, before the
    /// ConnectionRequest, and ACKs only every 100 ms. Lower bounds only:
    /// timers never fire early, so this cannot flake under load.
    #[tokio::test]
    async fn vanilla_client_ticks_every_100ms_from_session_creation() {
        let mut harness = VanillaHarness::connect().await;
        let first = harness.send_message(b"\xfeone").await;
        let first_ack = loop {
            if acks(&harness.recv_raw().await, first) {
                break Instant::now();
            }
        };
        assert!(
            first_ack.duration_since(harness.request_seen) >= Duration::from_millis(90),
            "ACK before the first 100 ms tick"
        );
        let second = harness.send_message(b"\xfetwo").await;
        let second_ack = loop {
            if acks(&harness.recv_raw().await, second) {
                break Instant::now();
            }
        };
        assert!(
            second_ack.duration_since(first_ack) >= Duration::from_millis(90),
            "ACK between ticks"
        );
    }

    /// KA-5: dropping the stream drains unacknowledged data first; the bare
    /// DisconnectNotification follows the ACK that empties the resend map.
    #[tokio::test]
    async fn vanilla_client_drop_closes_after_drain() {
        let mut harness = VanillaHarness::connect().await;
        // Retire the handshake's resend entries so only `last` remains.
        let mut buffer = [0; 2048];
        let mut encoded = BytesMut::new();
        harness
            .outbound
            .as_ref()
            .unwrap()
            .send(OutboundMsg {
                peer: harness.server.local_addr().unwrap(),
                buffer: Bytes::from_static(b"\xfelast"),
                reliability: crate::protocol::reliability::Reliability::ReliableOrdered,
                channel: 0,
                priority: crate::protocol::state::RakPriority::Normal,
            })
            .await
            .unwrap();
        drop(harness.outbound.take());
        // Withhold every ACK: the client keeps resending and never says goodbye.
        let withheld_until = Instant::now() + Duration::from_millis(700);
        let mut pending = Vec::new();
        while Instant::now() < withheld_until {
            let Ok(Ok((len, _))) = timeout(
                withheld_until - Instant::now(),
                harness.server.recv_from(&mut buffer),
            )
            .await
            else {
                break;
            };
            let datagram = Datagram::decode(&mut &buffer[..len]).unwrap();
            if let crate::protocol::datagram::DatagramPayload::EncapsulatedPackets(frames) =
                &datagram.payload
            {
                assert!(
                    frames.iter().all(|frame| frame.payload.as_ref() != [0x15]),
                    "disconnect sent before the resend map drained"
                );
            }
            pending.push(datagram);
        }
        // Now ACK everything: the next tick sends the bare notification.
        for datagram in pending {
            harness
                .session
                .handle_datagram(datagram, Instant::now())
                .unwrap();
        }
        flush_built_datagrams(
            &mut harness.session,
            &harness.server,
            harness.client,
            // Past the Optimized server's 1 ms ACK deadline.
            Instant::now() + Duration::from_millis(5),
            false,
            &mut encoded,
        )
        .await;
        let disconnect = timeout(Duration::from_secs(2), async {
            loop {
                let datagram = harness.recv_raw().await;
                if let crate::protocol::datagram::DatagramPayload::EncapsulatedPackets(frames) =
                    &datagram.payload
                    && frames.iter().any(|frame| frame.payload.as_ref() == [0x15])
                {
                    break frames[0].header.reliability;
                }
            }
        })
        .await
        .expect("no DisconnectNotification after the drain");
        assert_eq!(
            disconnect,
            crate::protocol::reliability::Reliability::ReliableOrdered
        );
        timeout(Duration::from_secs(1), harness.task)
            .await
            .unwrap()
            .unwrap();
    }
}

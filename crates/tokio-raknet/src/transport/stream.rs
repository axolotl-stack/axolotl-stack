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
/// The Bedrock client receives into a 1600-byte buffer, so it handles
/// datagrams up to 1600 bytes, longer than its 1492-byte MTU (PARITY MTU-11).
pub(crate) const VANILLA_MAX_DATAGRAM: usize = 1600;

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
    /// Mojang RakNet behaviour of the Minecraft Bedrock client (`PARITY.md`).
    pub transmission_policy: TransmissionPolicy,
    /// Transient receive errors (connection refused/reset/aborted, host or
    /// network unreachable) ignored during the offline handshake; `None` is
    /// unlimited. go-raknet `Dialer.MaxTransientErrors` defaults to 10.
    pub max_transient_errors: Option<u32>,
    /// Local address the client socket binds. `None` (the default) binds the
    /// server family's wildcard (`0.0.0.0:0` / `[::]:0`), or its loopback
    /// address when the server is on loopback, so local dials never open a
    /// socket on a network interface (no host-firewall prompt).
    pub local_addr: Option<SocketAddr>,
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
            local_addr: None,
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
    /// sliding window's `estimatedRTT` (Vanilla, Mojang RakNet). Readable
    /// without involving the transport task.
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
            let socket =
                std::net::UdpSocket::bind(super::client_bind_addr(server, config.local_addr))?;
            socket.set_nonblocking(true)?;
            super::apply_socket_buffers(
                &socket,
                config.socket_recv_buffer_size,
                config.socket_send_buffer_size,
            )?;

            let socket = UdpSocket::from_std(socket)?;
            let local = socket.local_addr()?;

            // Perform offline handshake using OpenConnectionRequest1/2.
            // Vanilla: a fresh random GUID per dial, like the Bedrock client
            // (real-client captures, PARITY HS-1).
            let client_guid = match config.transmission_policy {
                TransmissionPolicy::Vanilla => handshake::random_guid(),
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
    /// Maintenance cadence: Mojang's 10 ms update cycle for Vanilla,
    /// `TICK_INTERVAL` for Optimized. Tests may lengthen it to prove a
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
    // Vanilla reads like Mojang's 1600-byte recvfrom buffer; one extra byte
    // detects (and drops) longer datagrams on platforms that truncate.
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
                    tracing::debug!(len, "dropping datagram longer than Mojang's receive buffer");
                    continue;
                }
                if vanilla {
                    managed.record_datagram_activity(&buf[..len], Instant::now());
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
                    // Mojang's update runs when a datagram wakes the loop,
                    // then the 10 ms wait restarts.
                    flush_built_datagrams(&mut managed, &socket, server, now, true, &mut send_buf)
                        .await;
                    tick.reset();
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
                let immediate = msg.priority == crate::protocol::state::RakPriority::Immediate;
                if let Err(error) =
                    managed.queue_app_buffer(msg.buffer, msg.reliability, msg.channel, msg.priority)
                {
                    tracing::debug!(?error, "failed to queue outbound packet");
                    let _ = context.to_app.try_send(Err(error.into()));
                    continue;
                }
                // Vanilla: only IMMEDIATE sends wake Mojang's update loop;
                // other priorities leave on the next 10 ms update.
                if !vanilla || immediate {
                    flush_built_datagrams(
                        &mut managed,
                        &socket,
                        server,
                        now,
                        vanilla,
                        &mut send_buf,
                    )
                    .await;
                    if vanilla {
                        tick.reset();
                    }
                }
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
                    client_guid: handshake::random_guid(),
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
            timeout(Duration::from_secs(2), async {
                while !session.is_connected() {
                    let (len, _) = server.recv_from(&mut buffer).await.unwrap();
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

    /// MTU-11: Mojang's 1600-byte receive buffer: datagrams up to 1600 bytes
    /// are handled, longer ones are dropped without ending the connection.
    #[tokio::test]
    async fn vanilla_client_handles_datagrams_up_to_1600_bytes() {
        fn unreliable(sequence: u32, payload: Vec<u8>) -> Datagram {
            Datagram {
                header: crate::protocol::types::DatagramHeader {
                    flags: crate::protocol::constants::DatagramFlags::VALID
                        | crate::protocol::constants::DatagramFlags::HAS_B_AND_AS,
                    sequence: crate::protocol::types::Sequence24::new(sequence),
                },
                payload: crate::protocol::datagram::DatagramPayload::EncapsulatedPackets(vec![
                    crate::protocol::encapsulated_packet::EncapsulatedPacket {
                        header: crate::protocol::types::EncapsulatedPacketHeader {
                            reliability: crate::protocol::reliability::Reliability::Unreliable,
                            is_split: false,
                            needs_bas: false,
                        },
                        bit_length: (payload.len() << 3) as u16,
                        reliable_index: None,
                        sequence_index: None,
                        ordering_index: None,
                        ordering_channel: None,
                        split: None,
                        payload: Bytes::from(payload),
                    },
                ]),
            }
        }
        // Datagram header (4 bytes) + unreliable frame header (3 bytes).
        let largest = [vec![0xfe], vec![0x01; VANILLA_MAX_DATAGRAM - 8]].concat();
        let oversized = [vec![0xfe], vec![0x02; VANILLA_MAX_DATAGRAM - 7]].concat();
        let mut harness = VanillaHarness::connect().await;
        let mut encoded = BytesMut::new();
        unreliable(100, largest.clone())
            .encode(&mut encoded)
            .unwrap();
        assert_eq!(encoded.len(), VANILLA_MAX_DATAGRAM);
        harness
            .send_datagram(&unreliable(100, largest.clone()))
            .await;
        harness.expect_message(&largest).await;
        // Dropped: the next delivered message is the one after it.
        harness.send_datagram(&unreliable(101, oversized)).await;
        let after = b"\xfeafter";
        harness
            .send_datagram(&unreliable(102, after.to_vec()))
            .await;
        harness.expect_message(after).await;
        assert!(!harness.task.is_finished());
    }

    /// DG-2: Mojang drops the slow-start bit once its window passes the
    /// threshold (BDS `baseline` capture: 0x80 and 0x88 datagrams); the
    /// receiver ignores 0x04/0x08, so every flag combination is delivered.
    #[tokio::test]
    async fn vanilla_client_delivers_datagrams_without_the_slow_start_bit() {
        let mut harness = VanillaHarness::connect().await;
        for (sequence, flags) in [(100u32, 0x80u8), (101, 0x88), (102, 0x84), (103, 0x8c)] {
            let body = [0xfe, flags];
            harness
                .send_datagram(&Datagram {
                    header: crate::protocol::types::DatagramHeader {
                        flags: crate::protocol::constants::DatagramFlags::from_bits_truncate(flags),
                        sequence: crate::protocol::types::Sequence24::new(sequence),
                    },
                    payload: crate::protocol::datagram::DatagramPayload::EncapsulatedPackets(vec![
                        crate::protocol::encapsulated_packet::EncapsulatedPacket {
                            header: crate::protocol::types::EncapsulatedPacketHeader {
                                reliability: crate::protocol::reliability::Reliability::Unreliable,
                                is_split: false,
                                needs_bas: false,
                            },
                            bit_length: (body.len() << 3) as u16,
                            reliable_index: None,
                            sequence_index: None,
                            ordering_index: None,
                            ordering_channel: None,
                            split: None,
                            payload: Bytes::copy_from_slice(&body),
                        },
                    ]),
                })
                .await;
            harness.expect_message(&body).await;
        }
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

    /// REL-7/ACK-3: Mojang accepts a datagram up to 50000 numbers ahead
    /// (NAKing at most 1000 of the gap) and drops one beyond that without
    /// ending the connection.
    #[tokio::test]
    async fn vanilla_client_accepts_gaps_up_to_50000_datagrams() {
        let mut harness = VanillaHarness::connect().await;
        for (sequence, body) in [(60_000u32, b"\xfefar".as_slice()), (5000, b"\xfenear")] {
            harness
                .session
                .queue_app_buffer(
                    Bytes::from_static(body),
                    crate::protocol::reliability::Reliability::Unreliable,
                    0,
                    crate::protocol::state::RakPriority::Normal,
                )
                .unwrap();
            let mut datagram = harness.session.build_datagram(Instant::now()).unwrap();
            datagram.header.sequence = crate::protocol::types::Sequence24::new(sequence);
            harness.send_datagram(&datagram).await;
        }
        harness.expect_message(b"\xfenear").await;
        assert!(
            harness.incoming.try_recv().is_err(),
            "datagram beyond the 50000 gap delivered"
        );
        let nak = timeout(Duration::from_secs(2), async {
            loop {
                if let crate::protocol::datagram::DatagramPayload::Nak(nak) =
                    harness.recv_raw().await.payload
                {
                    break nak;
                }
            }
        })
        .await
        .expect("gap not NAKed");
        let naked: u32 = nak.ranges.iter().map(|range| range.len()).sum();
        assert!(naked <= 1000, "{naked}");
        assert_eq!(nak.ranges.last().unwrap().end.value(), 4999);
        assert!(!harness.task.is_finished());
    }

    /// ACK-1: once the client has an RTT sample (its ConnectionRequest was
    /// ACKed), a received datagram is ACKed no earlier than 10 ms later.
    /// Lower bound only: timers never fire early, so this cannot flake.
    #[tokio::test]
    async fn vanilla_client_acks_10ms_after_the_oldest_datagram() {
        let mut harness = VanillaHarness::connect().await;
        // ACK the client's handshake datagrams so it has an RTT sample.
        let mut buffer = [0; 2048];
        while let Ok(Ok((len, _))) = timeout(
            Duration::from_millis(50),
            harness.server.recv_from(&mut buffer),
        )
        .await
        {
            let datagram = Datagram::decode(&mut &buffer[..len]).unwrap();
            harness
                .session
                .handle_datagram(datagram, Instant::now())
                .unwrap();
        }
        let mut encoded = BytesMut::new();
        flush_built_datagrams(
            &mut harness.session,
            &harness.server,
            harness.client,
            Instant::now() + Duration::from_millis(5),
            false,
            &mut encoded,
        )
        .await;
        tokio::time::sleep(Duration::from_millis(30)).await;
        let sent_at = Instant::now();
        let first = harness.send_message(b"\xfeone").await;
        let acked_at = loop {
            if acks(&harness.recv_raw().await, first) {
                break Instant::now();
            }
        };
        harness.expect_message(b"\xfeone").await;
        assert!(
            acked_at.duration_since(sent_at) >= Duration::from_millis(10),
            "ACK before Mojang's 10 ms SYN"
        );
    }

    /// KA-5: dropping the stream queues Mojang's bare ReliableOrdered
    /// `0x15` behind the pending data at once; the client stays open while
    /// it is unacknowledged and closes once the server ACKs it.
    #[tokio::test]
    async fn vanilla_client_drop_sends_disconnect_and_closes_when_acked() {
        let mut harness = VanillaHarness::connect().await;
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
        // Withhold every ACK for a while.
        let withheld_until = Instant::now() + Duration::from_millis(300);
        let mut pending = Vec::new();
        let mut disconnect = None;
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
                && let Some(frame) = frames.iter().find(|frame| frame.payload.as_ref() == [0x15])
            {
                disconnect.get_or_insert(frame.header.reliability);
            }
            pending.push(datagram);
        }
        assert_eq!(
            disconnect,
            Some(crate::protocol::reliability::Reliability::ReliableOrdered),
            "the notification does not wait for the data to drain"
        );
        assert!(!harness.task.is_finished(), "closed before the ACK");
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
        timeout(Duration::from_secs(1), harness.task)
            .await
            .expect("client still open after its notification was ACKed")
            .unwrap();
    }
}

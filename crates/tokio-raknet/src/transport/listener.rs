mod offline;
mod online;
mod rate_limiter;

pub use rate_limiter::PingRateLimiter;

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use bytes::{Bytes, BytesMut};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use crate::protocol::constants::{self, UDP_HEADER_SIZE};
use crate::transport::listener_conn::SessionState;
use crate::transport::mux::new_tick_interval;
use crate::transport::stream::RaknetStream;

use offline::PendingConnection;

use online::{dispatch_datagram, handle_outgoing_msg, tick_sessions};

/// Configuration for a `RaknetListener`.
#[derive(Debug, Clone)]

pub struct RaknetListenerConfig {
    /// Maximum number of concurrent connections allowed.
    pub max_connections: usize,

    /// Maximum number of pending connections (handshakes).
    pub max_pending_connections: usize,

    /// Maximum MTU size to support/advertise.
    pub max_mtu: u16,

    /// Optional socket receive buffer size.
    pub socket_recv_buffer_size: Option<usize>,

    /// Optional socket send buffer size.
    pub socket_send_buffer_size: Option<usize>,

    /// Timeout duration for inactive sessions.
    pub session_timeout: Duration,

    /// Duration before a session is considered stale.
    pub session_stale: Duration,

    /// Maximum bytes of reliable data to queue for a single session before disconnecting.
    pub max_queued_reliable_bytes: usize,

    /// Initial advertisement string.
    pub advertisement: Vec<u8>,

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
    /// Prevents OOM attacks from malicious peers sending excessive ACK/NACK packets.
    pub max_incoming_ack_queue: usize,

    /// Maximum pings per second per IP address for rate limiting.
    /// Set to 0 to disable rate limiting.
    pub ping_rate_limit_per_ip: u32,

    /// Maximum number of source IPs tracked by the ping/handshake rate limiter.
    pub ping_rate_limit_max_entries: usize,

    /// Transmission policy of accepted sessions. Defaults to
    /// [`TransmissionPolicy::Optimized`](crate::session::TransmissionPolicy).
    /// Vanilla sessions must be ticked every 100 ms; the listener's
    /// maintenance interval already is.
    pub transmission_policy: crate::session::TransmissionPolicy,
}

impl Default for RaknetListenerConfig {
    fn default() -> Self {
        Self {
            max_connections: 1024,
            max_pending_connections: 256, // Reduced from 1024 to prevent handshake flooding
            max_mtu: constants::MAXIMUM_MTU_SIZE,
            socket_recv_buffer_size: Some(super::DEFAULT_SOCKET_RECV_BUFFER),
            socket_send_buffer_size: None,
            // Upstream `conn.go:Conn.startTicking`: base inactivity timeout5s.
            session_timeout: Duration::from_secs(5),
            session_stale: Duration::from_secs(5),
            max_queued_reliable_bytes: 1024 * 1024, // Reduced from 4MB to 1MB per session
            advertisement: b"MCPE;Tokio-Raknet Default Advertisement;527;1.19.1;0;10;13253860892328930865;Tokio Raknet;Survival;1;19132;19133".to_vec(),
            max_ordering_channels: constants::MAXIMUM_ORDERING_CHANNELS as usize,
            ack_queue_capacity: 1024,
            split_timeout: Duration::from_secs(30),
            reliable_window: 2048,
            // Upstream `conn.go:Conn.receiveSplitPacket`.
            max_split_parts: 512,
            max_concurrent_splits: 16,
            max_incoming_ack_queue: 4096,
            ping_rate_limit_per_ip: 10, // 10 pings per second per IP
            ping_rate_limit_max_entries: 16_384,
            transmission_policy: crate::session::TransmissionPolicy::Optimized,
        }
    }
}

impl RaknetListenerConfig {
    /// Validates explicit resource limits before binding. Existing bounded admission
    /// limits are a local deviation: upstream `listener.go:Listen` has no global cap.
    pub fn validate(&self) -> Result<(), super::ConfigError> {
        use super::{require_config, validate_duration, validate_mtu, validate_session_limits};
        validate_mtu(self.max_mtu)?;
        require_config(
            self.max_connections > 0,
            "max_connections",
            "must be positive",
        )?;
        require_config(
            self.max_pending_connections > 0,
            "max_pending_connections",
            "must be positive",
        )?;
        require_config(
            self.max_queued_reliable_bytes > 0,
            "max_queued_reliable_bytes",
            "must be positive",
        )?;
        require_config(
            self.ping_rate_limit_per_ip == 0 || self.ping_rate_limit_max_entries > 0,
            "ping_rate_limit_max_entries",
            "must be positive when rate limiting is enabled",
        )?;
        require_config(
            self.advertisement.len() <= i16::MAX as usize,
            "advertisement",
            "must not exceed 32767 bytes",
        )?;
        for (value, field) in [
            (self.socket_recv_buffer_size, "socket_recv_buffer_size"),
            (self.socket_send_buffer_size, "socket_send_buffer_size"),
        ] {
            require_config(value != Some(0), field, "must be positive when specified")?;
        }
        validate_duration(self.session_timeout, "session_timeout")?;
        validate_duration(self.session_stale, "session_stale")?;
        require_config(
            self.session_stale <= self.session_timeout,
            "session_stale",
            "must not exceed session_timeout",
        )?;
        validate_session_limits(
            self.max_ordering_channels,
            self.ack_queue_capacity,
            self.split_timeout,
            self.reliable_window,
            self.max_split_parts,
            self.max_concurrent_splits,
            self.max_incoming_ack_queue,
        )
    }

    /// Returns a configuration optimized for high-performance servers with ample memory.
    ///
    /// - `max_pending_connections`: 1024
    /// - `max_queued_reliable_bytes`: 4MB
    pub fn high_performance() -> Self {
        Self {
            max_pending_connections: 1024,
            max_queued_reliable_bytes: 4 * 1024 * 1024,
            ..Default::default()
        }
    }
}

/// Server-side RakNet listener that accepts new connections.
pub struct RaknetListener {
    local_addr: SocketAddr,
    new_connections: mpsc::Receiver<super::listener_conn::Accepted>,
    outbound_tx: mpsc::Sender<super::OutboundMsg>,
    advertisement: Arc<RwLock<Bytes>>,
}

impl RaknetListener {
    /// Binds a new listener to the specified address using default configuration.
    pub async fn bind(addr: SocketAddr) -> std::io::Result<Self> {
        Self::bind_with_config(addr, RaknetListenerConfig::default()).await
    }

    /// Binds a new listener to the specified address using the provided configuration.
    pub async fn bind_with_config(
        addr: SocketAddr,
        config: RaknetListenerConfig,
    ) -> std::io::Result<Self> {
        config.validate().map_err(std::io::Error::from)?;
        let socket = std::net::UdpSocket::bind(addr)?;
        socket.set_nonblocking(true)?;
        super::apply_socket_buffers(
            &socket,
            config.socket_recv_buffer_size,
            config.socket_send_buffer_size,
        )?;

        let socket = UdpSocket::from_std(socket)?;
        let local_addr = socket.local_addr()?;
        let (new_conn_tx, new_conn_rx) = mpsc::channel(32);
        let (outbound_tx, outbound_rx) = mpsc::channel(1024);
        let advertisement = Arc::new(RwLock::new(Bytes::from(config.advertisement.clone())));

        tokio::spawn(run_listener_muxer(
            socket,
            config,
            new_conn_tx,
            outbound_rx,
            advertisement.clone(),
        ));

        Ok(Self {
            local_addr,
            new_connections: new_conn_rx,
            outbound_tx,
            advertisement,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// The server GUID advertised in `UnconnectedPong`, `OpenConnectionReply1`
    /// and `OpenConnectionReply2`: one random value per process.
    pub fn guid(&self) -> u64 {
        offline::server_guid()
    }

    fn stream(&self, accepted: super::listener_conn::Accepted) -> RaknetStream {
        RaknetStream::new(
            self.local_addr,
            accepted.peer,
            accepted.incoming,
            self.outbound_tx.clone(),
            accepted.rtt,
        )
    }

    /// Accepts the next incoming connection.
    pub async fn accept(&mut self) -> Option<RaknetStream> {
        let accepted = self.new_connections.recv().await?;
        Some(self.stream(accepted))
    }

    /// Poll-based accept for use with manual polling or futures combinators.
    pub fn poll_accept(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<RaknetStream>> {
        match self.new_connections.poll_recv(cx) {
            std::task::Poll::Ready(Some(accepted)) => {
                std::task::Poll::Ready(Some(self.stream(accepted)))
            }
            std::task::Poll::Ready(None) => std::task::Poll::Ready(None),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }

    /// Set the unconnected-pong advertisement.
    ///
    /// `go-raknet/listener.go:PongData` caps this at signed 16-bit length.
    /// Return a typed error rather than panicking or silently discarding input.
    pub fn set_advertisement(&self, data: Vec<u8>) -> Result<(), super::ConfigError> {
        super::require_config(
            data.len() <= i16::MAX as usize,
            "advertisement",
            "must not exceed 32767 bytes",
        )?;
        let mut guard = self
            .advertisement
            .write()
            .unwrap_or_else(|error| error.into_inner());
        *guard = Bytes::from(data);
        Ok(())
    }

    /// Gets a copy of the current advertisement data.
    pub fn get_advertisement(&self) -> Vec<u8> {
        self.advertisement
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .to_vec()
    }
}

enum LoopEvent<Msg> {
    Inbound(std::io::Result<(usize, std::net::SocketAddr)>),
    Outbound(Msg),
    Tick,
    Ack,
}

/// One entry per armed peer, including peers flushed inline and re-armed before
/// their old timer fires. New receive transitions have monotonic deadlines.
#[derive(Default)]
struct AckSchedule {
    deadlines: VecDeque<(Instant, SocketAddr)>,
    armed: HashMap<SocketAddr, Instant>,
}

impl AckSchedule {
    fn arm(&mut self, deadline: Instant, peer: SocketAddr) {
        match self.armed.entry(peer) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                if *entry.get() == deadline {
                    return;
                }
                entry.insert(deadline);
                // Inline feedback can clear and then re-arm a peer before its
                // old timer fires. Cancel that stale entry on receive, keeping
                // one slot per peer and ACK wakes strictly O(due entries).
                self.deadlines.retain(|&(_, queued)| queued != peer);
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(deadline);
            }
        }
        // Only receive-side None->Some transitions enqueue deadlines. Immediate
        // feedback was already flushed inline; delayed deadlines are now + 1 ms.
        debug_assert!(
            self.deadlines
                .back()
                .is_none_or(|&(last, _)| last <= deadline)
        );
        self.deadlines.push_back((deadline, peer));
    }

    fn deadline(&self) -> Option<Instant> {
        self.deadlines.front().map(|&(deadline, _)| deadline)
    }

    fn pop(&mut self) -> Option<(Instant, SocketAddr)> {
        let entry = self.deadlines.pop_front()?;
        self.armed.remove(&entry.1);
        Some(entry)
    }

    fn remove(&mut self, peer: SocketAddr) {
        if self.armed.remove(&peer).is_some() {
            self.deadlines.retain(|&(_, queued)| queued != peer);
        }
    }
}

async fn flush_scheduled_acks(
    schedule: &mut AckSchedule,
    sessions: &mut HashMap<SocketAddr, SessionState>,
    socket: &UdpSocket,
    now: Instant,
    send_buf: &mut BytesMut,
) {
    while schedule.deadline().is_some_and(|deadline| deadline <= now) {
        let (_, peer) = schedule.pop().unwrap();
        let Some(state) = sessions.get_mut(&peer) else {
            continue;
        };
        if state
            .managed
            .ack_deadline()
            .is_some_and(|deadline| deadline <= now)
        {
            super::mux::flush_managed(&mut state.managed, socket, peer, now, false, send_buf).await;
            state.publish_rtt();
        }
    }
}

async fn run_listener_muxer(
    socket: UdpSocket,

    config: RaknetListenerConfig,

    new_conn_tx: super::listener_conn::AcceptSender,

    mut outbound_rx: mpsc::Receiver<super::OutboundMsg>,

    advertisement: Arc<RwLock<Bytes>>,
) {
    // Allocate a receive buffer large enough to avoid OS "message too long" errors even if a peer
    // sends a slightly larger probe than our configured MTU.
    let mut buf = vec![0u8; (config.max_mtu as usize + UDP_HEADER_SIZE + 64).max(2048)];
    let mut send_buf = BytesMut::with_capacity(config.max_mtu as usize + UDP_HEADER_SIZE + 64);
    let mut sessions: HashMap<SocketAddr, SessionState> = HashMap::new();
    let mut pending: HashMap<SocketAddr, PendingConnection> = HashMap::new();
    let mut tick = new_tick_interval();
    let mut ack_schedule = AckSchedule::default();
    let ack_timer = tokio::time::sleep(Duration::ZERO);
    tokio::pin!(ack_timer);
    let mut rate_limiter = rate_limiter::PingRateLimiter::with_max_entries(
        config.ping_rate_limit_per_ip,
        config.ping_rate_limit_max_entries,
    );

    loop {
        let next_ack = ack_schedule.deadline();
        if let Some(deadline) = next_ack {
            ack_timer.as_mut().reset(deadline.into());
        }
        let event = tokio::select! {
            res = socket.recv_from(&mut buf) => LoopEvent::Inbound(res),
            msg = outbound_rx.recv() => match msg {
                Some(msg) => LoopEvent::Outbound(msg),
                None => break,
            },
            _ = tick.tick() => LoopEvent::Tick,
            _ = &mut ack_timer, if next_ack.is_some() => LoopEvent::Ack,
        };

        match event {
            LoopEvent::Inbound(res) => match res {
                Ok((len, peer)) => {
                    dispatch_datagram(
                        &socket,
                        &config,
                        &buf[..len],
                        peer,
                        &mut sessions,
                        &mut pending,
                        &new_conn_tx,
                        &advertisement,
                        &mut rate_limiter,
                        &mut send_buf,
                    )
                    .await;
                    if let Some(state) = sessions.get(&peer) {
                        if let Some(deadline) = state.managed.ack_deadline() {
                            ack_schedule.arm(deadline, peer);
                        }
                    } else {
                        ack_schedule.remove(peer);
                    }
                }
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::ConnectionReset {
                        continue;
                    }
                    tracing::error!("UDP socket error: {}", e);
                }
            },
            LoopEvent::Outbound(msg) => {
                handle_outgoing_msg(
                    &socket,
                    config.max_mtu as usize,
                    msg,
                    &mut sessions,
                    &config,
                    &mut send_buf,
                )
                .await;
            }
            LoopEvent::Ack => {
                flush_scheduled_acks(
                    &mut ack_schedule,
                    &mut sessions,
                    &socket,
                    Instant::now(),
                    &mut send_buf,
                )
                .await;
            }
            LoopEvent::Tick => {
                pending.retain(|peer, connection| {
                    if sessions
                        .get(peer)
                        .is_some_and(|state| state.handshake_confirmed)
                    {
                        return false;
                    }
                    if connection.expires_at <= std::time::Instant::now() {
                        sessions.remove(peer);
                        return false;
                    }
                    sessions.contains_key(peer)
                });
                tick_sessions(&socket, &mut sessions, &new_conn_tx, &mut send_buf).await;
                // Session expiry is maintenance work, not an ACK wake scan.
                ack_schedule
                    .deadlines
                    .retain(|(_, peer)| sessions.contains_key(peer));
                ack_schedule
                    .armed
                    .retain(|peer, _| sessions.contains_key(peer));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle_session(peer: SocketAddr) -> SessionState {
        let (to_app, _rx) = mpsc::channel(1);
        SessionState {
            managed: crate::session::manager::ManagedSession::new(peer, 1200, Instant::now()),
            to_app,
            pending_rx: None,
            announced: true,
            handshake_confirmed: true,
            rtt: crate::transport::RttCell::new(),
        }
    }

    #[test]
    fn ack_rearms_keep_one_entry_per_peer_until_the_previous_entry_is_consumed() {
        let now = Instant::now();
        let peer = "127.0.0.1:19132".parse().unwrap();
        let mut schedule = AckSchedule::default();
        for _ in 0..2048 {
            schedule.arm(now, peer);
        }
        assert_eq!(schedule.pop(), Some((now, peer)));
        assert_eq!(schedule.pop(), None);
        let later = now + Duration::from_millis(1);
        schedule.arm(later, peer);
        assert_eq!(schedule.pop(), Some((later, peer)));
        // Removing a session must also free its membership slot before reuse.
        schedule.arm(later, peer);
        schedule.remove(peer);
        assert_eq!(schedule.pop(), None);
        schedule.arm(later, peer);
        assert_eq!(schedule.pop(), Some((later, peer)));
    }

    #[test]
    fn rearming_a_peer_replaces_its_stale_entry_before_later_receives() {
        let now = Instant::now();
        let first = "127.0.0.1:19132".parse().unwrap();
        let second = "127.0.0.1:19133".parse().unwrap();
        let mut schedule = AckSchedule::default();
        schedule.arm(now, first);
        let rearmed = now + Duration::from_millis(2);
        schedule.arm(rearmed, first);
        schedule.arm(now + Duration::from_millis(3), second);
        assert_eq!(schedule.pop(), Some((rearmed, first)));
        assert_eq!(
            schedule.pop(),
            Some((now + Duration::from_millis(3), second))
        );
    }

    #[tokio::test]
    async fn ack_wake_flushes_due_peers_and_skips_removed_peers_among_idle_sessions() {
        use crate::protocol::constants::DatagramFlags;
        use crate::protocol::datagram::{Datagram, DatagramPayload};
        use crate::protocol::types::{DatagramHeader, Sequence24};

        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let receiver = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let active = receiver.local_addr().unwrap();
        let removed = "127.0.0.1:1".parse().unwrap();
        let mut sessions = HashMap::new();
        for port in 20000..21024 {
            let peer = SocketAddr::from(([127, 0, 0, 1], port));
            sessions.insert(peer, idle_session(peer));
        }
        let now = Instant::now();
        let mut state = idle_session(active);
        state
            .managed
            .handle_datagram(
                Datagram {
                    header: DatagramHeader {
                        flags: DatagramFlags::VALID,
                        sequence: Sequence24::new(0),
                    },
                    payload: DatagramPayload::EncapsulatedPackets(Vec::new()),
                },
                now,
            )
            .unwrap();
        sessions.insert(active, state);
        let mut schedule = AckSchedule::default();
        schedule.arm(now, removed);
        schedule.arm(now, active);
        let mut out = BytesMut::new();
        flush_scheduled_acks(&mut schedule, &mut sessions, &socket, now, &mut out).await;
        assert!(schedule.deadline().is_none());
        assert!(
            sessions
                .get(&active)
                .unwrap()
                .managed
                .ack_deadline()
                .is_none()
        );
        let mut buffer = [0; 64];
        let (len, _) =
            tokio::time::timeout(Duration::from_millis(20), receiver.recv_from(&mut buffer))
                .await
                .unwrap()
                .unwrap();
        let ack = Datagram::decode(&mut &buffer[..len]).unwrap();
        let DatagramPayload::Ack(ack) = ack.payload else {
            panic!("expected ACK")
        };
        assert_eq!(ack.ranges[0].start.value(), 0);
        assert_eq!(ack.ranges[0].end.value(), 0);
    }

    /// Explicit timing harness: same stale ACK wake against increasingly many
    /// idle sessions; compares the previous scan with the production queue path.
    #[tokio::test]
    #[ignore = "run release, pinned to a reserved CPU, outside acceptance builds"]
    async fn ack_wake_idle_session_microbench() {
        use std::hint::black_box;
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let active = "127.0.0.1:19132".parse().unwrap();
        let mut out = BytesMut::new();
        for idle in [0, 100, 1000, 10000] {
            let mut sessions = HashMap::new();
            sessions.insert(active, idle_session(active));
            for port in 20000..20000 + idle {
                let peer = SocketAddr::from(([127, 0, 0, 1], port as u16));
                sessions.insert(peer, idle_session(peer));
            }
            let mut schedule = AckSchedule::default();
            // Warm queue capacity and the map lookup, outside timing.
            schedule.arm(Instant::now(), active);
            schedule.pop();
            for round in 0..5 {
                for queued in if round % 2 == 0 {
                    [false, true]
                } else {
                    [true, false]
                } {
                    let now = Instant::now();
                    let started = Instant::now();
                    for _ in 0..1000 {
                        if queued {
                            schedule.arm(now, active);
                            flush_scheduled_acks(
                                &mut schedule,
                                black_box(&mut sessions),
                                &socket,
                                now,
                                &mut out,
                            )
                            .await;
                        } else {
                            let mut next: Option<Instant> = None;
                            for state in black_box(&sessions).values() {
                                if let Some(deadline) = black_box(state.managed.ack_deadline()) {
                                    next = Some(next.map_or(deadline, |old| old.min(deadline)));
                                }
                            }
                            black_box(next);
                        }
                    }
                    println!(
                        "ACK_WAKE idle={idle} round={round} queued={queued} ns={:.1}",
                        started.elapsed().as_nanos() as f64 / 1000.0
                    );
                }
            }
        }
    }

    #[test]
    fn invalid_listener_config_is_rejected() {
        let mut config = RaknetListenerConfig {
            max_mtu: 399,
            ..Default::default()
        };
        assert!(config.validate().is_err());
        config.max_mtu = 1492;
        assert!(config.validate().is_ok());
        config.max_ordering_channels = 0;
        assert!(config.validate().is_err());
    }

    #[tokio::test]
    async fn invalid_config_returns_typed_error_before_bind() {
        let config = RaknetListenerConfig {
            max_mtu: 1,
            ..Default::default()
        };
        let error =
            match RaknetListener::bind_with_config("127.0.0.1:0".parse().unwrap(), config).await {
                Ok(_) => panic!("invalid config accepted"),
                Err(error) => error,
            };
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(error.get_ref().unwrap().is::<crate::error::ConfigError>());
    }
}

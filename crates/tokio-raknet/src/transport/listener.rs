mod offline;
mod online;
mod rate_limiter;

pub use rate_limiter::PingRateLimiter;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::Duration;

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
}

impl Default for RaknetListenerConfig {
    fn default() -> Self {
        Self {
            max_connections: 1024,
            max_pending_connections: 256, // Reduced from 1024 to prevent handshake flooding
            max_mtu: constants::MAXIMUM_MTU_SIZE,
            socket_recv_buffer_size: None,
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
    new_connections: mpsc::Receiver<(
        SocketAddr,
        mpsc::Receiver<Result<super::ReceivedMessage, crate::RaknetError>>,
    )>,
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
        report_socket_buffer_config(
            config.socket_recv_buffer_size,
            config.socket_send_buffer_size,
        );

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

    /// Accepts the next incoming connection.
    pub async fn accept(&mut self) -> Option<RaknetStream> {
        let (peer, incoming) = self.new_connections.recv().await?;

        Some(RaknetStream::new(
            self.local_addr,
            peer,
            incoming,
            self.outbound_tx.clone(),
        ))
    }

    /// Poll-based accept for use with manual polling or futures combinators.
    pub fn poll_accept(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<RaknetStream>> {
        match self.new_connections.poll_recv(cx) {
            std::task::Poll::Ready(Some((peer, incoming))) => {
                let stream =
                    RaknetStream::new(self.local_addr, peer, incoming, self.outbound_tx.clone());
                std::task::Poll::Ready(Some(stream))
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

fn report_socket_buffer_config(recv_size: Option<usize>, send_size: Option<usize>) {
    if recv_size.is_some() || send_size.is_some() {
        tracing::debug!(
            recv_size,
            send_size,
            "socket buffer tuning is unavailable on the std-only listener path; using OS defaults"
        );
    }
}

async fn run_listener_muxer(
    socket: UdpSocket,

    config: RaknetListenerConfig,

    new_conn_tx: mpsc::Sender<(
        SocketAddr,
        mpsc::Receiver<Result<super::ReceivedMessage, crate::RaknetError>>,
    )>,

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
    let mut next_ack: Option<std::time::Instant> = None;
    let ack_timer = tokio::time::sleep(Duration::ZERO);
    tokio::pin!(ack_timer);
    let mut rate_limiter = rate_limiter::PingRateLimiter::with_max_entries(
        config.ping_rate_limit_per_ip,
        config.ping_rate_limit_max_entries,
    );

    loop {
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
                    if let Some(deadline) = sessions
                        .get(&peer)
                        .and_then(|state| state.managed.ack_deadline())
                    {
                        next_ack = Some(next_ack.map_or(deadline, |next| next.min(deadline)));
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
                let now = std::time::Instant::now();
                next_ack = None;
                for (&peer, state) in &mut sessions {
                    if state
                        .managed
                        .ack_deadline()
                        .is_some_and(|deadline| deadline <= now)
                    {
                        super::mux::flush_managed(
                            &mut state.managed,
                            &socket,
                            peer,
                            now,
                            false,
                            &mut send_buf,
                        )
                        .await;
                    }
                    if let Some(deadline) = state.managed.ack_deadline() {
                        next_ack = Some(next_ack.map_or(deadline, |next| next.min(deadline)));
                    }
                }
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
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

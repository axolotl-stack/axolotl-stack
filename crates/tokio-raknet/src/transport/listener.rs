mod offline;
mod online;

use std::collections::HashMap;
use std::future::poll_fn;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::task::Poll;
use std::time::Duration;

use tokio::io::ReadBuf;
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
}

impl Default for RaknetListenerConfig {
    fn default() -> Self {
        Self {
            max_connections: 1024,
            max_pending_connections: 256, // Reduced from 1024 to prevent handshake flooding
            max_mtu: 1400,
            socket_recv_buffer_size: None,
            socket_send_buffer_size: None,
            session_timeout: Duration::from_secs(10),
            session_stale: Duration::from_secs(5),
            max_queued_reliable_bytes: 1024 * 1024, // Reduced from 4MB to 1MB per session
            advertisement: b"MCPE;Tokio-Raknet Default Advertisement;527;1.19.1;0;10;13253860892328930865;Tokio Raknet;Survival;1;19132;19133".to_vec(),
            max_ordering_channels: constants::MAXIMUM_ORDERING_CHANNELS as usize,
            ack_queue_capacity: 1024,
            split_timeout: Duration::from_secs(30),
            reliable_window: constants::MAX_ACK_SEQUENCES as u32,
            max_split_parts: 8192,
            max_concurrent_splits: 32, // Reduced from 4096 to prevent OOM DOS
        }
    }
}

impl RaknetListenerConfig {
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
    advertisement: Arc<RwLock<Vec<u8>>>,
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
        let socket = std::net::UdpSocket::bind(addr)?;
        socket.set_nonblocking(true)?;

        // if let Some(size) = config.socket_recv_buffer_size {
        //     let _ = socket.set_recv_buffer_size(size);
        // }
        // if let Some(size) = config.socket_send_buffer_size {
        //     let _ = socket.set_send_buffer_size(size);
        // }

        let socket = UdpSocket::from_std(socket)?;
        let local_addr = socket.local_addr()?;
        let (new_conn_tx, new_conn_rx) = mpsc::channel(32);
        let (outbound_tx, outbound_rx) = mpsc::channel(1024);
        let advertisement = Arc::new(RwLock::new(config.advertisement.clone()));

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

    /// Sets the advertisement data (Pong payload) sent in response to UnconnectedPing (0x01) and OpenConnections (0x02).
    pub fn set_advertisement(&self, data: Vec<u8>) {
        if let Ok(mut guard) = self.advertisement.write() {
            *guard = data;
        }
    }

    /// Gets a copy of the current advertisement data.
    pub fn get_advertisement(&self) -> Vec<u8> {
        self.advertisement
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

enum LoopEvent<Msg> {
    Inbound(std::io::Result<(usize, std::net::SocketAddr)>),
    Outbound(Msg),
    Tick,
}

async fn run_listener_muxer(
    socket: UdpSocket,

    config: RaknetListenerConfig,

    new_conn_tx: mpsc::Sender<(
        SocketAddr,
        mpsc::Receiver<Result<super::ReceivedMessage, crate::RaknetError>>,
    )>,

    mut outbound_rx: mpsc::Receiver<super::OutboundMsg>,

    advertisement: Arc<RwLock<Vec<u8>>>,
) {
    // Allocate a receive buffer large enough to avoid OS "message too long" errors even if a peer
    // sends a slightly larger probe than our configured MTU.
    let mut buf = vec![0u8; (config.max_mtu as usize + UDP_HEADER_SIZE + 64).max(2048)];
    let mut sessions: HashMap<SocketAddr, SessionState> = HashMap::new();
    let mut pending: HashMap<SocketAddr, PendingConnection> = HashMap::new();
    let mut tick = new_tick_interval();

    loop {
        // 2. The Polling Block (Replaces select!)
        let event = poll_fn(|cx| {
            // A. Poll Outbound (Channel)
            // prioritize outbound or inbound? select! is random, this is ordered.
            // Putting outbound first gives it priority.
            if let Poll::Ready(Some(msg)) = outbound_rx.poll_recv(cx) {
                return Poll::Ready(LoopEvent::Outbound(msg));
            }

            // B. Poll Inbound (UDP)
            // We need a ReadBuf for poll_recv_from
            let mut read_buf = ReadBuf::new(&mut buf);
            if let Poll::Ready(res) = socket.poll_recv_from(cx, &mut read_buf) {
                let len = read_buf.filled().len();
                return Poll::Ready(LoopEvent::Inbound(res.map(|addr| (len, addr))));
            }

            // C. Poll Tick (Timer)
            if let Poll::Ready(_) = tick.poll_tick(cx) {
                return Poll::Ready(LoopEvent::Tick);
            }

            Poll::Pending
        })
        .await;

        // 3. The Logic Block (Now just a simple match)
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
                    )
                    .await;
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
                )
                .await;
            }
            LoopEvent::Tick => {
                tick_sessions(&socket, &mut sessions).await;
            }
        }
    }
}

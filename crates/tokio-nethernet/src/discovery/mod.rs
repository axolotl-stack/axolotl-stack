//! LAN Discovery for NetherNet.
//!
//! This module implements the encrypted UDP-based LAN discovery protocol
//! used by Minecraft: Bedrock Edition for local network games.
//!
//! # Overview
//!
//! LAN discovery uses UDP port 7551 with encrypted packets:
//! - **RequestPacket** - Broadcast by clients to find servers
//! - **ResponsePacket** - Sent by servers with game info
//! - **MessagePacket** - Used to exchange NetherNet signals over LAN

mod crypto;
mod packet;

pub use packet::{ServerData, ServerDataBuilder, TransportLayer};

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::net::UdpSocket;
use tokio::sync::{RwLock, mpsc};
use tracing::{debug, trace, warn};

use crate::signaling::{Signal, Signaling, SignalingChannel};

/// Default port for NetherNet LAN discovery.
pub const DEFAULT_PORT: u16 = 7551;

/// Configuration for discovery listener.
#[derive(Clone)]
pub struct DiscoveryListenerConfig {
    /// Network ID for this listener. Randomly generated if zero.
    pub network_id: u64,
    /// Broadcast address for sending discovery requests.
    pub broadcast_addr: SocketAddr,
    /// Interval for sending discovery broadcasts.
    pub broadcast_interval: Duration,
    /// Timeout for expiring inactive addresses.
    pub address_timeout: Duration,
}

impl Default for DiscoveryListenerConfig {
    fn default() -> Self {
        Self {
            network_id: rand::random(),
            broadcast_addr: "255.255.255.255:7551".parse().unwrap(),
            broadcast_interval: Duration::from_secs(2),
            address_timeout: Duration::from_secs(15),
        }
    }
}

/// Address mapping with timestamp for expiration.
struct AddressEntry {
    addr: SocketAddr,
    last_seen: Instant,
}

/// LAN Discovery listener implementing the `Signaling` trait.
pub struct DiscoveryListener {
    socket: Arc<UdpSocket>,
    config: DiscoveryListenerConfig,
    addresses: RwLock<HashMap<u64, AddressEntry>>,
    responses: RwLock<HashMap<u64, Vec<u8>>>,
    pong_data: RwLock<Option<Vec<u8>>>,
    signal_tx: mpsc::Sender<Signal>,
    signal_rx: RwLock<Option<mpsc::Receiver<Signal>>>,
}

impl DiscoveryListener {
    /// Bind to the specified address for LAN discovery.
    pub async fn bind(addr: &str, config: DiscoveryListenerConfig) -> std::io::Result<Arc<Self>> {
        let socket = Arc::new(UdpSocket::bind(addr).await?);
        socket.set_broadcast(true)?;

        let (signal_tx, signal_rx) = mpsc::channel(128);

        let listener = Arc::new(Self {
            socket,
            config,
            addresses: RwLock::new(HashMap::new()),
            responses: RwLock::new(HashMap::new()),
            pong_data: RwLock::new(None),
            signal_tx,
            signal_rx: RwLock::new(Some(signal_rx)),
        });

        let l = listener.clone();
        tokio::spawn(async move { l.listen_loop().await });

        let l = listener.clone();
        tokio::spawn(async move { l.background_loop().await });

        Ok(listener)
    }

    /// Take the signal receiver for routing to nethernet.
    pub async fn take_signal_receiver(&self) -> Option<mpsc::Receiver<Signal>> {
        self.signal_rx.write().await.take()
    }

    /// Set server data for responding to discovery requests.
    pub async fn set_server_data(&self, data: ServerData) {
        let bytes = data.encode();
        *self.pong_data.write().await = Some(bytes);
    }

    /// Get discovered servers and their response data.
    pub async fn responses(&self) -> HashMap<u64, Vec<u8>> {
        self.responses.read().await.clone()
    }

    async fn listen_loop(&self) {
        let mut buf = [0u8; 1024];
        debug!("Discovery listener started, waiting for packets...");
        loop {
            match self.socket.recv_from(&mut buf).await {
                Ok((len, addr)) => {
                    trace!(
                        from = %addr,
                        len = len,
                        "Received UDP packet"
                    );

                    if let Err(e) = self.handle_packet(&buf[..len], addr).await {
                        debug!(error = %e, from = %addr, "Error handling discovery packet");
                    }
                }
                Err(e) => {
                    // On Windows, error 10054 (WSAECONNRESET) happens when we
                    // send to a port with no listener. This is normal for UDP
                    // broadcast/discovery - just ignore and continue.
                    #[cfg(windows)]
                    if e.raw_os_error() == Some(10054) {
                        trace!(error = %e, "Ignoring WSAECONNRESET (no listener on broadcast target)");
                        continue;
                    }

                    warn!(error = %e, "Discovery socket error");
                    break;
                }
            }
        }
    }

    async fn background_loop(&self) {
        let mut interval = tokio::time::interval(self.config.broadcast_interval);
        loop {
            interval.tick().await;

            let timeout = self.config.address_timeout;
            let mut addrs = self.addresses.write().await;
            addrs.retain(|_, entry| entry.last_seen.elapsed() < timeout);
            drop(addrs);

            let request = packet::encode_request(self.config.network_id);
            if let Err(e) = self
                .socket
                .send_to(&request, self.config.broadcast_addr)
                .await
            {
                trace!(error = %e, "Failed to send discovery broadcast");
            }
        }
    }

    async fn handle_packet(&self, data: &[u8], addr: SocketAddr) -> anyhow::Result<()> {
        let (pkt, sender_id) = packet::decode(data).map_err(|e| anyhow::anyhow!("{}", e))?;

        if sender_id == self.config.network_id {
            return Ok(());
        }

        {
            let mut addrs = self.addresses.write().await;
            addrs.insert(
                sender_id,
                AddressEntry {
                    addr,
                    last_seen: Instant::now(),
                },
            );
        }

        match pkt {
            packet::Packet::Request => {
                if let Some(data) = self.pong_data.read().await.as_ref() {
                    trace!(to = %addr, len = data.len(), "Sending discovery response");
                    let response = packet::encode_response(self.config.network_id, data);
                    self.socket.send_to(&response, addr).await?;
                }
            }
            packet::Packet::Response(data) => {
                self.responses.write().await.insert(sender_id, data);
            }
            packet::Packet::Message(msg) => {
                if msg.data == "Ping" {
                    return Ok(());
                }
                let signal = Signal::parse(&msg.data, sender_id.to_string())?;
                let _ = self.signal_tx.send(signal).await;
            }
        }

        Ok(())
    }

    async fn send_to_network(&self, network_id: u64, packet: &[u8]) -> anyhow::Result<()> {
        let addrs = self.addresses.read().await;
        let entry = addrs
            .get(&network_id)
            .ok_or_else(|| anyhow::anyhow!("No address for network ID {}", network_id))?;
        self.socket.send_to(packet, entry.addr).await?;
        Ok(())
    }
}

#[async_trait]
impl Signaling for DiscoveryListener {
    async fn signal(&self, signal: Signal) -> anyhow::Result<()> {
        let network_id: u64 = signal
            .network_id
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid network ID"))?;

        let msg_packet =
            packet::encode_message(self.config.network_id, network_id, &signal.to_string());

        self.send_to_network(network_id, &msg_packet).await
    }

    fn network_id(&self) -> String {
        self.config.network_id.to_string()
    }

    fn set_pong_data(&self, data: &[u8]) {
        let s = String::from_utf8_lossy(data);
        let parts: Vec<&str> = s.split(';').collect();
        if parts.len() >= 9 {
            let server_data = ServerData {
                server_name: parts.get(1).unwrap_or(&"").to_string(),
                level_name: parts.get(7).unwrap_or(&"").to_string(),
                player_count: parts.get(4).and_then(|s| s.parse().ok()).unwrap_or(1),
                max_player_count: parts.get(5).and_then(|s| s.parse().ok()).unwrap_or(10),
                game_type: 0,
                editor_world: false,
                hardcore: false,
                transport_layer: TransportLayer::NetherNet,
                connection_type: 4,
            };
            let bytes = server_data.encode();
            if let Ok(mut guard) = self.pong_data.try_write() {
                *guard = Some(bytes);
            }
        }
    }
}

#[async_trait]
impl SignalingChannel for DiscoveryListener {
    async fn take_signal_receiver(&self) -> Option<mpsc::Receiver<Signal>> {
        self.signal_rx.write().await.take()
    }
}

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
//!
//! Wire behavior follows upstream `discovery/packet.go:Marshal/Unmarshal`,
//! `discovery/crypto.go:encrypt/decrypt`, and the version 7
//! `discovery/server_data.go:ServerData.MarshalBinary` layout. Timing and
//! addressing follow the real client's LAN discovery (see
//! `PARITY.md`, rows L9-L13): requests every 2 s to every interface's directed
//! broadcast, only while not announcing; peer addresses are learned from
//! responses and messages and expire after 60 s; discovered servers after 5 s.
//! Locally, at most 1024 peer identities and their responses are retained.
//! A full 128-signal queue drops new signals, and a full UDP send queue drops
//! discovery replies, so spam cannot suspend reception while awaiting a consumer.

mod crypto;
/// Public encrypted/HMAC packet codec (`encode_*` / `decode`), usable without
/// a socket, e.g. by custom transports and benchmarks.
pub mod packet;

pub use packet::{ServerData, ServerDataBuilder, TransportLayer};

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::net::UdpSocket;
use tokio::sync::{RwLock, mpsc};
use tracing::{debug, trace, warn};

use crate::signaling::{Signal, Signaling, SignalingChannel};

/// Upstream `discovery/listener.go:ListenConfig.Listen` (`DefaultPort`).
pub const DEFAULT_PORT: u16 = 7551;

/// Receive capacity from upstream `discovery/listener.go:Listener.listen` (`maxUDPPacketSize`).
const MAX_UDP_PACKET_SIZE: usize = 65_535;
/// Local resource policy, not a wire constant. Responses share the admitted address set.
const MAX_DISCOVERY_PEERS: usize = 1024;

/// Configuration for discovery listener.
#[derive(Clone)]
pub struct DiscoveryListenerConfig {
    /// Network ID for this listener. Randomly generated if zero.
    pub network_id: u64,
    /// Destination of discovery requests. The default limited broadcast
    /// `255.255.255.255:7551` is expanded like the client's network
    /// discovery: the directed broadcast of every non-loopback,
    /// non-link-local IPv4 interface plus `ff02::1` from an IPv6 socket, all
    /// on this port, falling back to the limited broadcast when no interface
    /// qualifies (Windows sends a limited broadcast on one interface only).
    /// Any other address, e.g. `127.0.0.1:7551`, is used as given.
    pub broadcast_addr: SocketAddr,
    /// Broadcast interval; default 2 s (the client's 2000 ms, upstream
    /// `discovery/listener.go:Listener.background`).
    pub broadcast_interval: Duration,
    /// Peer address expiry; default 60 s, the client's LAN peer database
    /// (go-nethernet uses 15 s).
    pub address_timeout: Duration,
    /// Discovered-server expiry; default 5 s, the client's server list.
    pub response_timeout: Duration,
}

impl Default for DiscoveryListenerConfig {
    fn default() -> Self {
        Self {
            network_id: rand::random(),
            broadcast_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), DEFAULT_PORT),
            broadcast_interval: Duration::from_secs(2),
            address_timeout: Duration::from_secs(60),
            response_timeout: Duration::from_secs(5),
        }
    }
}

/// Address mapping with timestamp for expiration.
struct AddressEntry {
    addr: SocketAddr,
    last_seen: Instant,
}

/// Decoded server data with the time it was last received.
struct ResponseEntry {
    data: Vec<u8>,
    last_seen: Instant,
}

/// Request destinations for a configured limited broadcast on `port`, as the
/// client computes them: `ip | !netmask` for every IPv4 network that is not
/// loopback, link-local (169.254/16) or a single host, deduplicated; `ff02::1`
/// when sending from an IPv6 socket; the limited broadcast if no IPv4 network
/// qualifies.
pub(crate) fn request_destinations(
    networks: impl IntoIterator<Item = (Ipv4Addr, Ipv4Addr)>,
    port: u16,
    ipv6_socket: bool,
) -> Vec<SocketAddr> {
    let mut destinations: Vec<SocketAddr> = Vec::new();
    for (ip, netmask) in networks {
        if ip.is_loopback()
            || ip.is_link_local()
            || ip.is_unspecified()
            || netmask == Ipv4Addr::BROADCAST
        {
            continue;
        }
        let broadcast = Ipv4Addr::from(ip.to_bits() | !netmask.to_bits());
        let destination = SocketAddr::new(IpAddr::V4(broadcast), port);
        if !destinations.contains(&destination) {
            destinations.push(destination);
        }
    }
    if destinations.is_empty() {
        destinations.push(SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), port));
    }
    if ipv6_socket {
        // IPv4 destinations travel as v4-mapped addresses on a dual-stack socket.
        for destination in &mut destinations {
            if let IpAddr::V4(v4) = destination.ip() {
                destination.set_ip(IpAddr::V6(v4.to_ipv6_mapped()));
            }
        }
        destinations.push(SocketAddr::new(
            IpAddr::V6(Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 1)),
            port,
        ));
    }
    destinations
}

/// IPv4 networks of the local interfaces, `(address, netmask)`.
fn local_ipv4_networks() -> Vec<(Ipv4Addr, Ipv4Addr)> {
    match if_addrs::get_if_addrs() {
        Ok(interfaces) => interfaces
            .into_iter()
            .filter_map(|interface| match interface.addr {
                if_addrs::IfAddr::V4(v4) => Some((v4.ip, v4.netmask)),
                if_addrs::IfAddr::V6(_) => None,
            })
            .collect(),
        Err(error) => {
            trace!(%error, "Interface enumeration failed; using the limited broadcast");
            Vec::new()
        }
    }
}

/// LAN Discovery listener implementing the `Signaling` trait.
pub struct DiscoveryListener {
    socket: Arc<UdpSocket>,
    config: DiscoveryListenerConfig,
    addresses: RwLock<HashMap<u64, AddressEntry>>,
    responses: RwLock<HashMap<u64, ResponseEntry>>,
    pong_data: RwLock<Option<Vec<u8>>>,
    signal_tx: mpsc::Sender<Signal>,
    signal_rx: RwLock<Option<mpsc::Receiver<Signal>>>,
    /// Reused wire-text buffer for outgoing signals; held only while encoding.
    signal_text: parking_lot::Mutex<String>,
}

impl DiscoveryListener {
    /// Bind to the specified address for LAN discovery.
    pub async fn bind(
        addr: &str,
        mut config: DiscoveryListenerConfig,
    ) -> std::io::Result<Arc<Self>> {
        if config.network_id == 0 {
            config.network_id = rand::random();
        }
        if config.broadcast_interval.is_zero() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "broadcast interval must be nonzero",
            ));
        }
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
            signal_text: parking_lot::Mutex::new(String::new()),
        });

        let l = listener.clone();
        tokio::spawn(async move { l.listen_loop().await });

        let l = listener.clone();
        tokio::spawn(async move { l.background_loop().await });

        Ok(listener)
    }

    /// Actual UDP bind address, including an OS-assigned ephemeral port.
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    /// Take the signal receiver for routing to nethernet.
    pub async fn take_signal_receiver(&self) -> Option<mpsc::Receiver<Signal>> {
        self.signal_rx.write().await.take()
    }

    /// Set server data for responding to discovery requests. While server
    /// data is set, this listener announces and stops sending requests (the
    /// client only broadcasts while browsing).
    pub async fn set_server_data(&self, data: ServerData) {
        let bytes = data.encode();
        *self.pong_data.write().await = Some(bytes);
    }

    /// Send the current server data to `addr` without a request. The client
    /// lists any response it receives while browsing (it keeps no pending
    /// request state), so this makes a server visible to a client whose
    /// broadcasts cannot reach this socket, e.g. a loopback-only one.
    pub async fn send_server_data_to(&self, addr: SocketAddr) -> std::io::Result<()> {
        let response = match self.pong_data.read().await.as_ref() {
            Some(data) => packet::encode_response(self.config.network_id, data),
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "no server data set",
                ));
            }
        };
        self.socket.send_to(&response, addr).await.map(|_| ())
    }

    /// Get discovered servers and their decoded application data (not ASCII
    /// hex), excluding servers silent for longer than `response_timeout`.
    pub async fn responses(&self) -> HashMap<u64, Vec<u8>> {
        let timeout = self.config.response_timeout;
        self.responses
            .read()
            .await
            .iter()
            .filter(|(_, entry)| entry.last_seen.elapsed() <= timeout)
            .map(|(id, entry)| (*id, entry.data.clone()))
            .collect()
    }

    async fn listen_loop(&self) {
        // Full UDP-sized storage prevents OS-level truncation of ordinary datagrams.
        let mut buf = vec![0u8; MAX_UDP_PACKET_SIZE];
        debug!("Discovery listener started, waiting for packets...");
        loop {
            match self.socket.recv_from(&mut buf).await {
                Ok((len, addr)) => {
                    // A capacity-sized read may be truncated (e.g. a jumbogram).
                    if len == buf.len() {
                        continue;
                    }
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
                    #[cfg(windows)]
                    if e.raw_os_error() == Some(10040) {
                        // WSAEMSGSIZE: discard a truncated datagram, keep receiving.
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

            self.expire_peers(Instant::now()).await;

            if self.pong_data.read().await.is_some() {
                continue;
            }
            let request = packet::encode_request(self.config.network_id);
            for destination in self.request_destinations() {
                if let Err(e) = self.socket.send_to(&request, destination).await {
                    trace!(error = %e, to = %destination, "Failed to send discovery request");
                }
            }
        }
    }

    fn request_destinations(&self) -> Vec<SocketAddr> {
        let configured = self.config.broadcast_addr;
        if configured.ip() != IpAddr::V4(Ipv4Addr::BROADCAST) {
            return vec![configured];
        }
        let ipv6_socket = self.socket.local_addr().is_ok_and(|local| local.is_ipv6());
        request_destinations(local_ipv4_networks(), configured.port(), ipv6_socket)
    }

    async fn expire_peers(&self, now: Instant) {
        let mut addrs = self.addresses.write().await;
        addrs.retain(|_, entry| {
            now.saturating_duration_since(entry.last_seen) <= self.config.address_timeout
        });
        self.responses.write().await.retain(|id, entry| {
            addrs.contains_key(id)
                && now.saturating_duration_since(entry.last_seen) <= self.config.response_timeout
        });
    }

    /// Record `sender_id` at `addr`; false when a new identity would exceed
    /// the peer budget (existing peers stay usable during a flood).
    fn learn_address(
        addrs: &mut HashMap<u64, AddressEntry>,
        sender_id: u64,
        addr: SocketAddr,
        now: Instant,
    ) -> bool {
        if !addrs.contains_key(&sender_id) && addrs.len() >= MAX_DISCOVERY_PEERS {
            return false;
        }
        addrs.insert(
            sender_id,
            AddressEntry {
                addr,
                last_seen: now,
            },
        );
        true
    }

    async fn handle_packet(&self, data: &[u8], addr: SocketAddr) -> anyhow::Result<()> {
        let (pkt, sender_id) = packet::decode(data).map_err(|e| anyhow::anyhow!("{}", e))?;

        if sender_id == self.config.network_id {
            return Ok(());
        }
        let now = Instant::now();

        match pkt {
            // Requests are answered at their source but teach no address
            // (the client's peer database learns from responses and messages).
            packet::Packet::Request => {
                if let Some(data) = self.pong_data.read().await.as_ref() {
                    trace!(to = %addr, len = data.len(), "Sending discovery response");
                    let response = packet::encode_response(self.config.network_id, data);
                    // A saturated UDP send queue must not stall inbound signaling.
                    match self.socket.try_send_to(&response, addr) {
                        Ok(_) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                        Err(e) => return Err(e.into()),
                    }
                }
            }
            packet::Packet::Response(data) => {
                // Same lock ordering as expiry: never retain responses for expired peers.
                let mut addrs = self.addresses.write().await;
                if Self::learn_address(&mut addrs, sender_id, addr, now) {
                    self.responses.write().await.insert(
                        sender_id,
                        ResponseEntry {
                            data,
                            last_seen: now,
                        },
                    );
                }
            }
            packet::Packet::Message(msg) => {
                let learned =
                    Self::learn_address(&mut *self.addresses.write().await, sender_id, addr, now);
                if !learned
                    || msg.recipient_id != self.config.network_id
                    || msg.data == "Ping"
                    || msg.data.is_empty()
                {
                    return Ok(());
                }
                let signal = Signal::parse_owned(msg.data, sender_id.to_string())?;
                // Bounded overload policy: drop newest signal rather than block UDP reception.
                let _ = self.signal_tx.try_send(signal);
            }
        }

        Ok(())
    }

    async fn send_to_network(&self, network_id: u64, packet: &[u8]) -> anyhow::Result<()> {
        let addr = self
            .addresses
            .read()
            .await
            .get(&network_id)
            .map(|entry| entry.addr)
            .ok_or_else(|| anyhow::anyhow!("No address for network ID {}", network_id))?;
        self.socket.send_to(packet, addr).await?;
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

        let msg_packet = {
            let mut text = self.signal_text.lock();
            text.clear();
            signal.write_wire(&mut text);
            packet::encode_message(self.config.network_id, network_id, &text)
        };

        self.send_to_network(network_id, &msg_packet).await
    }

    fn network_id(&self) -> String {
        self.config.network_id.to_string()
    }

    /// LAN negotiation is non-trickle in both directions: the client's LAN
    /// host answers LAN offers without trickle, and the 1.26.52 client's LAN
    /// offer carries all its
    /// host candidates inline and is never followed by `CANDIDATEADD`
    /// (real-client capture, PARITY.md N1/N7).
    fn disable_trickle_ice(&self) -> bool {
        true
    }

    /// Vanilla LAN peers need no `a=identity`: BDS 1.26.51 answers LAN offers
    /// without one and accepts identityless LAN offers (its HTTP signaling
    /// rejects them with 37). Present identities are still verified.
    fn requires_identity(&self) -> bool {
        false
    }

    fn set_pong_data(&self, data: &[u8]) {
        let s = String::from_utf8_lossy(data);
        let parts: Vec<&str> = s.split(';').collect();
        if parts.len() >= 9 {
            let server_data = ServerData {
                server_name: parts.get(1).unwrap_or(&"").to_string(),
                level_name: parts.get(7).unwrap_or(&"").to_string(),
                player_count: parts.get(4).and_then(|s| s.parse().ok()).unwrap_or(0),
                max_player_count: parts.get(5).and_then(|s| s.parse().ok()).unwrap_or(0),
                game_type: match parts[8].trim().to_ascii_lowercase().as_str() {
                    "creative" => 1,
                    "adventure" => 2,
                    _ => 0,
                },
                editor_world: false,
                hardcore: false,
                transport_layer: TransportLayer::NetherNet,
                connection_type: 4,
                ..Default::default()
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

#[cfg(test)]
mod tests {
    use super::*;

    async fn listener() -> DiscoveryListener {
        let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let (signal_tx, signal_rx) = mpsc::channel(128);
        DiscoveryListener {
            socket,
            config: DiscoveryListenerConfig {
                network_id: 42,
                ..Default::default()
            },
            addresses: RwLock::new(HashMap::new()),
            responses: RwLock::new(HashMap::new()),
            pong_data: RwLock::new(None),
            signal_tx,
            signal_rx: RwLock::new(Some(signal_rx)),
            signal_text: parking_lot::Mutex::new(String::new()),
        }
    }

    #[tokio::test]
    async fn large_sdp_survives_udp_receive() {
        let listener = Arc::new(listener().await);
        let mut signals = listener.take_signal_receiver().await.unwrap();
        let receiver = listener.clone();
        let task = tokio::spawn(async move { receiver.listen_loop().await });
        let sender = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let sdp = format!("v=0\r\n{}", "a=candidate:test\r\n".repeat(256));
        let wire = packet::encode_message(
            u64::MAX,
            42,
            &format!("CONNECTREQUEST 18446744073709551615 {sdp}"),
        );
        for malformed in [&wire[..16], &wire[..1024], &wire[..wire.len() - 1]] {
            sender
                .send_to(malformed, listener.socket.local_addr().unwrap())
                .await
                .unwrap();
        }
        sender
            .send_to(&wire, listener.socket.local_addr().unwrap())
            .await
            .unwrap();
        let result = tokio::time::timeout(Duration::from_secs(2), signals.recv()).await;
        task.abort();
        let signal = result.unwrap().unwrap();
        assert_eq!(
            signal.to_string(),
            format!("CONNECTREQUEST 18446744073709551615 {sdp}")
        );
        assert_eq!(signal.network_id, u64::MAX.to_string());
    }

    #[tokio::test]
    async fn bounded_state_and_full_signal_queue_do_not_block() {
        let listener = listener().await;
        let addr = "127.0.0.1:12345".parse().unwrap();
        for id in 100..1300 {
            listener
                .handle_packet(&packet::encode_response(id, b"data"), addr)
                .await
                .unwrap();
        }
        assert!(listener.addresses.read().await.len() <= 1024);
        assert!(listener.responses.read().await.len() <= 1024);
        let message = packet::encode_message(100, 42, "CONNECTREQUEST 1 v=0");
        tokio::time::timeout(Duration::from_secs(2), async {
            for _ in 0..256 {
                listener.handle_packet(&message, addr).await.unwrap();
            }
        })
        .await
        .expect("full signal queue blocked receive processing");
        listener
            .handle_packet(&packet::encode_response(100, b"updated"), addr)
            .await
            .unwrap();
        assert_eq!(listener.responses().await.get(&100).unwrap(), b"updated");
    }

    #[tokio::test]
    async fn recipient_filter_and_response_expiry() {
        let listener = listener().await;
        let mut signals = listener.take_signal_receiver().await.unwrap();
        let addr = "127.0.0.1:12345".parse().unwrap();
        listener
            .handle_packet(&packet::encode_message(1, 99, "CONNECTREQUEST 1 v=0"), addr)
            .await
            .unwrap();
        assert!(signals.try_recv().is_err());
        listener
            .handle_packet(&packet::encode_message(1, 42, ""), addr)
            .await
            .unwrap();
        listener
            .handle_packet(&packet::encode_response(1, b"data"), addr)
            .await
            .unwrap();
        listener
            .expire_peers(Instant::now() + Duration::from_secs(61))
            .await;
        assert!(listener.addresses.read().await.is_empty());
        assert!(listener.responses().await.is_empty());
    }

    /// The client keeps a peer's address 60 s after its last response or
    /// message but lists a server only 5 s (PARITY.md L12/L13); both used to
    /// share 15 s.
    #[tokio::test]
    async fn expiry_keeps_addresses_60s_and_servers_5s() {
        let listener = listener().await;
        let addr = "127.0.0.1:12345".parse().unwrap();
        listener
            .handle_packet(&packet::encode_response(1, b"data"), addr)
            .await
            .unwrap();
        let start = Instant::now();
        listener.expire_peers(start + Duration::from_secs(4)).await;
        assert!(listener.responses.read().await.contains_key(&1));
        listener.expire_peers(start + Duration::from_secs(20)).await;
        assert!(!listener.responses.read().await.contains_key(&1));
        assert!(listener.addresses.read().await.contains_key(&1));
        listener.expire_peers(start + Duration::from_secs(59)).await;
        assert!(listener.addresses.read().await.contains_key(&1));
        listener.expire_peers(start + Duration::from_secs(61)).await;
        assert!(listener.addresses.read().await.is_empty());
    }

    /// Only responses and messages teach the client a peer's address; a
    /// request (which anyone can send) is answered but leaves no route.
    #[tokio::test]
    async fn requests_do_not_make_a_peer_addressable() {
        let listener = listener().await;
        listener.set_server_data(ServerData::default()).await;
        let requester = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        // Replies use `try_send_to`; let the reactor report the fresh socket writable.
        listener.socket.writable().await.unwrap();
        listener
            .handle_packet(&packet::encode_request(7), requester.local_addr().unwrap())
            .await
            .unwrap();
        let mut reply = vec![0; MAX_UDP_PACKET_SIZE];
        let len = tokio::time::timeout(Duration::from_secs(2), requester.recv(&mut reply))
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            packet::decode(&reply[..len]).unwrap(),
            (packet::Packet::Response(_), 42)
        ));
        assert!(listener.addresses.read().await.is_empty());
        assert!(
            listener
                .signal(Signal::error(
                    1,
                    "7".into(),
                    crate::SignalErrorCode::InactivityTimeout
                ))
                .await
                .is_err()
        );
        listener
            .handle_packet(
                &packet::encode_message(7, 42, "CANDIDATEADD 1 candidate:x"),
                requester.local_addr().unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            listener.addresses.read().await.get(&7).unwrap().addr,
            requester.local_addr().unwrap()
        );
    }

    /// Requests go to every interface's directed broadcast, as the client's
    /// network discovery computes them; the limited broadcast only when no
    /// interface qualifies; IPv6 sockets add `ff02::1`.
    #[test]
    fn directed_broadcasts_follow_interfaces() {
        let net = |ip: &str, mask: &str| (ip.parse().unwrap(), mask.parse().unwrap());
        let addr = |text: &str| -> SocketAddr { text.parse().unwrap() };
        assert_eq!(
            request_destinations(
                [
                    net("192.168.1.20", "255.255.255.0"),
                    net("10.4.0.7", "255.255.0.0"),
                    net("192.168.1.21", "255.255.255.0"),
                    net("127.0.0.1", "255.0.0.0"),
                    net("169.254.3.4", "255.255.0.0"),
                    net("172.16.0.9", "255.255.255.255"),
                ],
                7551,
                false,
            ),
            [addr("192.168.1.255:7551"), addr("10.4.255.255:7551")]
        );
        assert_eq!(
            request_destinations([net("127.0.0.1", "255.0.0.0")], 7551, false),
            [addr("255.255.255.255:7551")]
        );
        assert_eq!(
            request_destinations([net("192.168.1.20", "255.255.255.0")], 7551, true),
            [addr("[::ffff:192.168.1.255]:7551"), addr("[ff02::1]:7551")]
        );
    }
}

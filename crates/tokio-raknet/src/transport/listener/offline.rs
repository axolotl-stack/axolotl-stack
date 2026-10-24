use std::{
    net::SocketAddr,
    sync::{Arc, LazyLock, RwLock},
    time::{Duration, Instant},
};

use super::rate_limiter::PingRateLimiter;

use bytes::{BufMut, Bytes, BytesMut};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use super::online::maybe_announce_connection;
use crate::protocol::{
    constants::{
        DEFAULT_UNCONNECTED_MAGIC, MAXIMUM_MTU_SIZE, MINIMUM_MTU_SIZE, RAKNET_PROTOCOL_VERSION,
        UDP_HEADER_SIZE,
    },
    packet::{
        AlreadyConnected, IncompatibleProtocolVersion, IpRecentlyConnected,
        NoFreeIncomingConnections, OpenConnectionReply1, OpenConnectionReply2,
        OpenConnectionRequest2, Packet, RaknetPacket, UnconnectedPong,
    },
};
use crate::session::manager::{ConnectionState, ManagedSession, SessionConfig};
use crate::transport::listener_conn::SessionState;

/// Handshakes expire absolutely ten seconds after `OpenConnectionRequest2`,
/// not after inactivity. Optimized: go-raknet
/// `handler.go:handleOpenConnectionRequest2`. Vanilla: Mojang RakNet drops
/// remote systems still unverified or handshaking 10000 ms after creation
/// (PARITY MTU-14).
#[derive(Clone, Copy)]
pub(super) struct PendingConnection {
    pub expires_at: Instant,
}

pub(super) fn is_offline_packet_id(id: u8) -> bool {
    let x = matches!(id, 0x01 | 0x02 | 0x05 | 0x07);
    x
}

use crate::transport::listener::RaknetListenerConfig;

pub(super) fn server_session_config(config: &RaknetListenerConfig) -> SessionConfig {
    SessionConfig {
        role: crate::session::manager::SessionRole::Server,
        guid: server_guid(),
        session_timeout: config.session_timeout,
        session_stale: config.session_stale,
        max_queued_reliable_bytes: Some(config.max_queued_reliable_bytes),
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
        ..Default::default()
    }
}

/// The largest MTU Bedrock Dedicated Server grants (capture `probe`: Reply1
/// MTU capped at 1400, PARITY LS-1); client builds use 1492.
const BDS_MAXIMUM_MTU_SIZE: u16 = 1400;

/// RakNet `defaultMTUSize` (Jenkins `RakPeer.cpp:206`), the floor of every
/// negotiated MTU.
const MOJANG_DEFAULT_MTU_SIZE: u16 = 576;

/// RakNet's `UDP_HEADER_SIZE` (IPv4 + UDP headers), added to the Request1
/// payload length for every address family (capture `probe`, PARITY LS-1).
const RAKNET_UDP_HEADER_SIZE: usize = 28;

/// Mojang RakNet's same-IP reconnect window (PARITY LS-6; Jenkins
/// `limitConnectionFrequencyFromTheSameIP` uses 100 ms and is off by default).
const MOJANG_SAME_IP_WINDOW: Duration = Duration::from_millis(50);

/// RakNet `UNASSIGNED_RAKNET_GUID`; Request2 GUID lookups skip it.
const UNASSIGNED_RAKNET_GUID: u64 = u64::MAX;

/// An offline reply of a Vanilla (Bedrock Dedicated Server) listener.
#[derive(Debug)]
enum VanillaReply {
    Packet(RaknetPacket),
    /// `id, magic, server GUID`: Mojang RakNet's layout of 0x12
    /// AlreadyConnected (capture `probe`, PARITY LS-5), 0x14
    /// NoFreeIncomingConnections and 0x1a IpRecentlyConnected.
    Refusal(u8),
}

impl VanillaReply {
    fn encode(&self, buf: &mut BytesMut) -> bool {
        buf.clear();
        match self {
            Self::Packet(pkt) => pkt.encode(buf).is_ok(),
            Self::Refusal(id) => {
                buf.put_u8(*id);
                buf.put_slice(&DEFAULT_UNCONNECTED_MAGIC);
                buf.put_u64(server_guid());
                true
            }
        }
    }
}

/// The largest MTU a Vanilla listener negotiates: BDS's 1400, or the
/// configured `max_mtu` if lower.
fn vanilla_max_mtu(config: &RaknetListenerConfig) -> u16 {
    config.max_mtu.min(BDS_MAXIMUM_MTU_SIZE)
}

/// Vanilla OpenConnectionRequest1 (capture `probe`, PARITY LS-1/LS-2; Jenkins
/// `RakPeer.cpp:5127-5196`): a foreign protocol gets 0x19 carrying protocol
/// 11; otherwise Reply1 with no security
/// cookie and MTU `min(UDP payload length + 28, MAXIMUM_MTU_SIZE)`, with no
/// lower bound. There is no capacity or connection check at this stage.
fn vanilla_request1(
    config: &RaknetListenerConfig,
    protocol_version: u8,
    payload_len: usize,
) -> VanillaReply {
    if protocol_version != RAKNET_PROTOCOL_VERSION {
        return VanillaReply::Packet(RaknetPacket::IncompatibleProtocolVersion(
            IncompatibleProtocolVersion {
                protocol: RAKNET_PROTOCOL_VERSION,
                magic: DEFAULT_UNCONNECTED_MAGIC,
                server_guid: server_guid(),
            },
        ));
    }
    let mtu = (payload_len + RAKNET_UDP_HEADER_SIZE).min(usize::from(vanilla_max_mtu(config)));
    VanillaReply::Packet(RaknetPacket::OpenConnectionReply1(OpenConnectionReply1 {
        magic: DEFAULT_UNCONNECTED_MAGIC,
        server_guid: server_guid(),
        cookie: None,
        mtu: mtu as u16,
    }))
}

/// Mojang RakNet's session MTU (PARITY LS-4): `max(defaultMTUSize 576,
/// clamp(requested, 400, MAXIMUM_MTU_SIZE))`. A configured `max_mtu` below
/// 576 also caps the floor.
fn vanilla_session_mtu(requested: u16, max_mtu: u16) -> u16 {
    requested
        .clamp(MINIMUM_MTU_SIZE, max_mtu)
        .max(MOJANG_DEFAULT_MTU_SIZE.min(max_mtu))
}

/// Mojang RakNet's same-IP reconnect limit (PARITY LS-6; Jenkins
/// `RakPeer.cpp:3604-3624` with 100 ms and off by default): a live connection
/// from the same IP, any port, created less than 50 ms ago refuses the new one.
///
/// Jenkins exempts addresses `IsLoopbackAddress` matches: the
/// host's own interface addresses and first external address. tokio does not
/// enumerate interfaces and exempts loopback IPs. The state read is the
/// sessions map itself, so it is bounded by `max_connections`.
fn ip_recently_connected(
    sessions: &std::collections::HashMap<SocketAddr, SessionState>,
    peer: SocketAddr,
    now: Instant,
) -> bool {
    !peer.ip().to_canonical().is_loopback()
        && sessions.iter().any(|(addr, state)| {
            addr.ip() == peer.ip()
                && now
                    .checked_duration_since(state.created_at)
                    .is_some_and(|age| age < MOJANG_SAME_IP_WINDOW)
        })
}

/// Vanilla OpenConnectionRequest2 (capture `probe`, PARITY LS-3..LS-7;
/// Jenkins `RakPeer.cpp:5198-5418`). No cookie, preceding Request1 or GUID/MTU
/// validation is required. Outcomes, in order:
/// - same address and GUID, connection still unverified (no
///   ConnectionRequest yet) → Reply2 again, echoing this request's MTU;
/// - any other address or GUID in use → 0x12 AlreadyConnected;
/// - full → 0x14 NoFreeIncomingConnections (evicting an existing peer is
///   not reproduced);
/// - same IP connected < 50 ms ago → 0x1a IpRecentlyConnected;
/// - otherwise a session at [`vanilla_session_mtu`] and Reply2 echoing the
///   requested MTU verbatim.
///
/// The local pending-handshake caps stay as a safety bound: excess requests
/// are dropped unanswered.
fn vanilla_request2(
    config: &RaknetListenerConfig,
    req: &OpenConnectionRequest2,
    peer: SocketAddr,
    now: Instant,
    sessions: &mut std::collections::HashMap<SocketAddr, SessionState>,
    pending: &mut std::collections::HashMap<SocketAddr, PendingConnection>,
) -> Option<VanillaReply> {
    let reply2 = || {
        VanillaReply::Packet(RaknetPacket::OpenConnectionReply2(OpenConnectionReply2 {
            magic: DEFAULT_UNCONNECTED_MAGIC,
            server_guid: server_guid(),
            server_addr: peer,
            mtu: req.mtu,
            security: false,
        }))
    };
    let guid_owner = if req.client_guid == UNASSIGNED_RAKNET_GUID {
        None
    } else {
        sessions
            .iter()
            .find(|(_, state)| state.client_guid == req.client_guid)
            .map(|(addr, _)| *addr)
    };
    match (sessions.get(&peer), guid_owner) {
        (None, None) => {}
        (Some(state), Some(owner))
            if owner == peer && state.managed.state() == ConnectionState::Unconnected =>
        {
            return Some(reply2());
        }
        _ => return Some(VanillaReply::Refusal(AlreadyConnected::ID)),
    }
    if sessions.len() >= config.max_connections {
        return Some(VanillaReply::Refusal(NoFreeIncomingConnections::ID));
    }
    if ip_recently_connected(sessions, peer, now) {
        return Some(VanillaReply::Refusal(IpRecentlyConnected::ID));
    }
    if exceeds_pending_budget(config, peer, sessions, pending) {
        return None;
    }
    let mtu = vanilla_session_mtu(req.mtu, vanilla_max_mtu(config));
    insert_session(config, peer, mtu, req.client_guid, now, sessions, pending);
    Some(reply2())
}

/// Upstream has no per-IP/global admission caps. Retain the configured
/// bounded pending budget, also applying it per IP across all sessions.
fn exceeds_pending_budget(
    config: &RaknetListenerConfig,
    peer: SocketAddr,
    sessions: &std::collections::HashMap<SocketAddr, SessionState>,
    pending: &std::collections::HashMap<SocketAddr, PendingConnection>,
) -> bool {
    pending.len() >= config.max_pending_connections
        || sessions
            .keys()
            .filter(|addr| addr.ip() == peer.ip())
            .count()
            >= config.max_pending_connections
}

fn insert_session(
    config: &RaknetListenerConfig,
    peer: SocketAddr,
    mtu: u16,
    client_guid: u64,
    now: Instant,
    sessions: &mut std::collections::HashMap<SocketAddr, SessionState>,
    pending: &mut std::collections::HashMap<SocketAddr, PendingConnection>,
) {
    pending.insert(
        peer,
        PendingConnection {
            expires_at: now + Duration::from_secs(10),
        },
    );

    let (tx, rx) =
        mpsc::channel::<Result<crate::transport::ReceivedMessage, crate::RaknetError>>(128);
    let sess_config = server_session_config(config);
    let managed = ManagedSession::with_config(peer, mtu as usize, now, sess_config);
    sessions.insert(
        peer,
        SessionState {
            managed,
            to_app: tx,
            pending_rx: Some(rx),
            announced: false,
            handshake_confirmed: false,
            rtt: crate::transport::RttCell::new(),
            client_guid,
            created_at: now,
        },
    );
}

async fn send_vanilla_reply(
    socket: &UdpSocket,
    peer: SocketAddr,
    reply: VanillaReply,
    buf: &mut BytesMut,
) {
    if reply.encode(buf) {
        let _ = socket.send_to(buf.as_ref(), peer).await;
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_offline(
    socket: &UdpSocket,
    config: &RaknetListenerConfig,
    bytes: &[u8],
    peer: SocketAddr,
    sessions: &mut std::collections::HashMap<SocketAddr, SessionState>,
    pending: &mut std::collections::HashMap<SocketAddr, PendingConnection>,
    new_conn_tx: &crate::transport::listener_conn::AcceptSender,
    advertisement: &Arc<RwLock<Bytes>>,
    rate_limiter: &mut PingRateLimiter,
    send_buf: &mut BytesMut,
) {
    let now = Instant::now();
    let vanilla = config.transmission_policy.is_vanilla();

    let mut slice = bytes;
    let decoded = if bytes.first() == Some(&0x07) {
        slice = &bytes[1..];
        // Optimized issues a cookie in every Reply1, so its Request2 carries
        // one. BDS answers Request2 without a cookie (capture `probe`).
        OpenConnectionRequest2::decode_body_with_security(&mut slice, !vanilla)
            .map(RaknetPacket::OpenConnectionRequest2)
    } else {
        RaknetPacket::decode(&mut slice)
    };
    let pkt = match decoded {
        Ok(p) => p,
        Err(_) => {
            // go-raknet `listener.go:security.blockFor`; Mojang RakNet just
            // ignores a malformed offline packet.
            if !vanilla {
                rate_limiter.block(peer.ip(), now);
            }
            return;
        }
    };

    match pkt {
        RaknetPacket::UnconnectedPing(req) => {
            if req.magic != DEFAULT_UNCONNECTED_MAGIC {
                return;
            }

            // Rate limit ping responses to prevent amplification attacks
            if !rate_limiter.check_and_record(peer.ip(), now) {
                return;
            }

            let ad_data = advertisement
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let ad_bytes = if ad_data.is_empty() {
                None
            } else {
                Some(ad_data)
            };

            let reply = RaknetPacket::UnconnectedPong(UnconnectedPong {
                ping_time: req.ping_time,
                server_guid: server_guid(),
                magic: DEFAULT_UNCONNECTED_MAGIC,
                advertisement: crate::protocol::types::Advertisement(ad_bytes),
            });

            send_unconnected_packet(socket, peer, reply, send_buf).await;
        }
        RaknetPacket::UnconnectedPingOpenConnections(req) => {
            if req.magic != DEFAULT_UNCONNECTED_MAGIC {
                return;
            }

            // Rate limit ping responses to prevent amplification attacks
            if !rate_limiter.check_and_record(peer.ip(), now) {
                return;
            }

            let ad_data = advertisement
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let ad_bytes = if ad_data.is_empty() {
                None
            } else {
                Some(ad_data)
            };

            let reply = RaknetPacket::UnconnectedPong(UnconnectedPong {
                ping_time: req.ping_time,
                server_guid: server_guid(),
                magic: DEFAULT_UNCONNECTED_MAGIC,
                advertisement: crate::protocol::types::Advertisement(ad_bytes),
            });

            send_unconnected_packet(socket, peer, reply, send_buf).await;
        }
        RaknetPacket::OpenConnectionRequest1(req) => {
            if req.magic != DEFAULT_UNCONNECTED_MAGIC {
                return;
            }

            if !rate_limiter.allow_request1(peer.ip(), now) {
                return;
            }
            if vanilla {
                let reply = vanilla_request1(config, req.protocol_version, bytes.len());
                send_vanilla_reply(socket, peer, reply, send_buf).await;
                return;
            }
            if req.protocol_version != RAKNET_PROTOCOL_VERSION {
                let reply =
                    RaknetPacket::IncompatibleProtocolVersion(IncompatibleProtocolVersion {
                        protocol: RAKNET_PROTOCOL_VERSION,
                        magic: DEFAULT_UNCONNECTED_MAGIC,
                        server_guid: server_guid(),
                    });
                send_unconnected_packet(socket, peer, reply, send_buf).await;
                rate_limiter.block(peer.ip(), now);
                return;
            }

            let ip_header = if peer.is_ipv4() { 20 } else { 40 };
            let padding_len = req.padding.0;
            let mtu_guess =
                padding_len + 1 + DEFAULT_UNCONNECTED_MAGIC.len() + 1 + ip_header + UDP_HEADER_SIZE;
            let mtu_clamped = (mtu_guess as u16).clamp(MINIMUM_MTU_SIZE, config.max_mtu);
            let cookie = rate_limiter.cookie(peer, now);

            if sessions.len() >= config.max_connections {
                let reply = RaknetPacket::NoFreeIncomingConnections(NoFreeIncomingConnections);
                send_unconnected_packet(socket, peer, reply, send_buf).await;
                return;
            }

            let reply = RaknetPacket::OpenConnectionReply1(OpenConnectionReply1 {
                magic: DEFAULT_UNCONNECTED_MAGIC,
                server_guid: server_guid(),
                cookie: Some(cookie),
                mtu: mtu_clamped,
            });

            send_unconnected_packet(socket, peer, reply, send_buf).await;
        }
        RaknetPacket::OpenConnectionRequest2(req) => {
            if req.magic != DEFAULT_UNCONNECTED_MAGIC {
                return;
            }
            if vanilla {
                if let Some(reply) = vanilla_request2(config, &req, peer, now, sessions, pending) {
                    send_vanilla_reply(socket, peer, reply, send_buf).await;
                }
                return;
            }

            if !req
                .cookie
                .is_some_and(|cookie| rate_limiter.valid_cookie(peer, cookie, now))
                || (req.client_guid as i64) >= 0
            {
                rate_limiter.block(peer.ip(), now);
                return;
            }
            if let Some(state) = sessions.get(&peer) {
                let reply = RaknetPacket::OpenConnectionReply2(OpenConnectionReply2 {
                    magic: DEFAULT_UNCONNECTED_MAGIC,
                    server_guid: server_guid(),
                    server_addr: peer,
                    mtu: state.managed.mtu() as u16,
                    security: false,
                });
                send_unconnected_packet(socket, peer, reply, send_buf).await;
                return;
            }

            if req.mtu < MINIMUM_MTU_SIZE || req.mtu > MAXIMUM_MTU_SIZE {
                rate_limiter.block(peer.ip(), now);
                return;
            }

            if sessions.len() >= config.max_connections {
                let reply = RaknetPacket::NoFreeIncomingConnections(NoFreeIncomingConnections);
                send_unconnected_packet(socket, peer, reply, send_buf).await;
                return;
            }

            if exceeds_pending_budget(config, peer, sessions, pending) {
                return;
            }
            let mtu_final = config.max_mtu.min(req.mtu);
            insert_session(
                config,
                peer,
                mtu_final,
                req.client_guid,
                now,
                sessions,
                pending,
            );
            if let Some(state) = sessions.get_mut(&peer) {
                maybe_announce_connection(peer, state, new_conn_tx).await;
            }

            // Upstream `handler.go:handleOpenConnectionRequest2` returns the
            // client's observed address, not the listener's local address.
            let server_addr = peer;

            let reply = RaknetPacket::OpenConnectionReply2(OpenConnectionReply2 {
                magic: DEFAULT_UNCONNECTED_MAGIC,
                server_guid: server_guid(),
                server_addr,
                mtu: mtu_final,
                security: false,
            });

            send_unconnected_packet(socket, peer, reply, send_buf).await;
        }
        _ => {}
    }
}

/// One random server GUID per process, advertised in every offline reply.
static SERVER_GUID: LazyLock<u64> = LazyLock::new(|| {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x1234_5678_9abc_def0)
});

pub(super) fn server_guid() -> u64 {
    *SERVER_GUID
}

async fn send_unconnected_packet(
    socket: &UdpSocket,
    peer: SocketAddr,
    pkt: RaknetPacket,
    buf: &mut BytesMut,
) {
    buf.clear();
    if pkt.encode(buf).is_ok() {
        let _ = socket.send_to(buf.as_ref(), peer).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::constants::DatagramFlags;
    use crate::protocol::datagram::{Datagram, DatagramPayload};
    use crate::protocol::encapsulated_packet::EncapsulatedPacket;
    use crate::protocol::packet::{ConnectionRequest, OpenConnectionRequest1};
    use crate::protocol::reliability::Reliability;
    use crate::protocol::types::{
        DatagramHeader, EncapsulatedPacketHeader, EoBPadding, RaknetTime, Sequence24,
    };
    use crate::session::TransmissionPolicy;
    use std::collections::HashMap;

    const POLICIES: [TransmissionPolicy; 2] =
        [TransmissionPolicy::Vanilla, TransmissionPolicy::Optimized];

    fn listener_config(policy: TransmissionPolicy) -> RaknetListenerConfig {
        RaknetListenerConfig {
            transmission_policy: policy,
            ..Default::default()
        }
    }

    /// A RakNet client's Request1 probe for `probe_mtu`: padded to
    /// `probe_mtu - 28` bytes of UDP payload (PARITY MTU-3).
    fn request1(protocol_version: u8, probe_mtu: usize) -> BytesMut {
        let mut wire = BytesMut::new();
        RaknetPacket::OpenConnectionRequest1(OpenConnectionRequest1 {
            magic: DEFAULT_UNCONNECTED_MAGIC,
            protocol_version,
            padding: EoBPadding(probe_mtu - RAKNET_UDP_HEADER_SIZE - 18),
        })
        .encode(&mut wire)
        .unwrap();
        assert_eq!(wire.len(), probe_mtu - RAKNET_UDP_HEADER_SIZE);
        wire
    }

    fn request2_packet(cookie: Option<u32>, mtu: u16, client_guid: u64) -> OpenConnectionRequest2 {
        OpenConnectionRequest2 {
            magic: DEFAULT_UNCONNECTED_MAGIC,
            cookie,
            client_proof: false,
            server_addr: "127.0.0.1:19132".parse().unwrap(),
            mtu,
            client_guid,
        }
    }

    fn request2(cookie: Option<u32>, mtu: u16, client_guid: u64) -> BytesMut {
        let mut wire = BytesMut::new();
        RaknetPacket::OpenConnectionRequest2(request2_packet(cookie, mtu, client_guid))
            .encode(&mut wire)
            .unwrap();
        wire
    }

    /// `id, magic, server GUID` as Mojang RakNet writes its refusals.
    fn refusal_bytes(id: u8) -> Vec<u8> {
        let mut out = vec![id];
        out.extend_from_slice(&DEFAULT_UNCONNECTED_MAGIC);
        out.extend_from_slice(&server_guid().to_be_bytes());
        out
    }

    fn decode(bytes: &[u8]) -> RaknetPacket {
        RaknetPacket::decode(&mut &bytes[..]).unwrap()
    }

    /// Drives a server session past its ConnectionRequest (Mojang
    /// HANDLING_CONNECTION_REQUEST; it is no longer UNVERIFIED_SENDER).
    fn receive_connection_request(state: &mut SessionState, client_guid: u64, now: Instant) {
        let mut payload = BytesMut::new();
        RaknetPacket::ConnectionRequest(ConnectionRequest {
            client_guid,
            timestamp: RaknetTime(1),
            secure: false,
        })
        .encode(&mut payload)
        .unwrap();
        let datagram = Datagram {
            header: DatagramHeader {
                flags: DatagramFlags::VALID,
                sequence: Sequence24::new(0),
            },
            payload: DatagramPayload::EncapsulatedPackets(vec![EncapsulatedPacket {
                header: EncapsulatedPacketHeader::with_reliability(Reliability::Reliable),
                bit_length: (payload.len() as u16) * 8,
                reliable_index: Some(Sequence24::new(0)),
                sequence_index: None,
                ordering_index: None,
                ordering_channel: None,
                split: None,
                payload: payload.freeze(),
            }]),
        };
        state.managed.handle_datagram(datagram, now).unwrap();
        assert_eq!(state.managed.state(), ConnectionState::OnlineHandshake);
    }

    /// Admission state for calling `vanilla_request2` with arbitrary peers and
    /// clocks.
    struct Admission {
        config: RaknetListenerConfig,
        sessions: HashMap<SocketAddr, SessionState>,
        pending: HashMap<SocketAddr, PendingConnection>,
    }

    impl Admission {
        fn vanilla() -> Self {
            Self {
                config: listener_config(TransmissionPolicy::Vanilla),
                sessions: HashMap::new(),
                pending: HashMap::new(),
            }
        }

        fn request2(
            &mut self,
            peer: &str,
            mtu: u16,
            client_guid: u64,
            now: Instant,
        ) -> Option<VanillaReply> {
            vanilla_request2(
                &self.config,
                &request2_packet(None, mtu, client_guid),
                peer.parse().unwrap(),
                now,
                &mut self.sessions,
                &mut self.pending,
            )
        }

        fn session(&self, peer: &str) -> Option<&SessionState> {
            self.sessions.get(&peer.parse().unwrap())
        }
    }

    fn reply2_mtu(reply: Option<VanillaReply>, peer: &str) -> u16 {
        match reply {
            Some(VanillaReply::Packet(RaknetPacket::OpenConnectionReply2(reply))) => {
                assert_eq!(reply.server_addr, peer.parse().unwrap());
                assert_eq!(reply.server_guid, server_guid());
                assert!(!reply.security);
                reply.mtu
            }
            other => panic!("expected Reply2, got {other:?}"),
        }
    }

    fn refusal(reply: Option<VanillaReply>) -> u8 {
        match reply {
            Some(VanillaReply::Refusal(id)) => id,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    /// A listener's offline state, fed through `dispatch_datagram` with
    /// replies read back on real loopback sockets.
    struct Wire {
        server: UdpSocket,
        clients: [UdpSocket; 2],
        config: RaknetListenerConfig,
        sessions: HashMap<SocketAddr, SessionState>,
        pending: HashMap<SocketAddr, PendingConnection>,
        accept: crate::transport::listener_conn::AcceptSender,
        _accepted: mpsc::Receiver<crate::transport::listener_conn::Accepted>,
        advertisement: Arc<RwLock<Bytes>>,
        limiter: PingRateLimiter,
        send: BytesMut,
    }

    impl Wire {
        async fn new(config: RaknetListenerConfig) -> Self {
            let (accept, accepted) = mpsc::channel(1);
            Self {
                server: UdpSocket::bind("127.0.0.1:0").await.unwrap(),
                clients: [
                    UdpSocket::bind("127.0.0.1:0").await.unwrap(),
                    UdpSocket::bind("127.0.0.1:0").await.unwrap(),
                ],
                config,
                sessions: HashMap::new(),
                pending: HashMap::new(),
                accept,
                _accepted: accepted,
                advertisement: Arc::new(RwLock::new(Bytes::new())),
                limiter: PingRateLimiter::new(10),
                send: BytesMut::new(),
            }
        }

        fn peer(&self, client: usize) -> SocketAddr {
            self.clients[client].local_addr().unwrap()
        }

        fn cookie(&mut self, client: usize) -> u32 {
            let peer = self.peer(client);
            self.limiter.cookie(peer, Instant::now())
        }

        async fn deliver(&mut self, client: usize, wire: &[u8]) {
            let peer = self.peer(client);
            super::super::online::dispatch_datagram(
                &self.server,
                &self.config,
                wire,
                peer,
                &mut self.sessions,
                &mut self.pending,
                &self.accept,
                &self.advertisement,
                &mut self.limiter,
                &mut self.send,
            )
            .await;
        }

        async fn reply(&self, client: usize) -> Vec<u8> {
            let mut buf = [0u8; 2048];
            let len =
                tokio::time::timeout(Duration::from_secs(1), self.clients[client].recv(&mut buf))
                    .await
                    .expect("no reply")
                    .unwrap();
            buf[..len].to_vec()
        }

        async fn assert_silent(&self, client: usize) {
            let mut buf = [0u8; 2048];
            let received = tokio::time::timeout(
                Duration::from_millis(100),
                self.clients[client].recv(&mut buf),
            )
            .await;
            assert!(received.is_err(), "unexpected reply {:?}", received);
        }
    }

    #[tokio::test]
    async fn request1_is_stateless_and_request2_requires_address_cookie() {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let peer = client.local_addr().unwrap();
        let config = RaknetListenerConfig::default();
        let mut sessions = HashMap::new();
        let mut pending = HashMap::new();
        let (tx, _rx) = mpsc::channel(1);
        let ad = Arc::new(RwLock::new(Bytes::new()));
        let mut limiter = PingRateLimiter::new(10);
        let mut send = BytesMut::new();
        let mut wire = BytesMut::new();
        RaknetPacket::OpenConnectionRequest1(crate::protocol::packet::OpenConnectionRequest1 {
            magic: DEFAULT_UNCONNECTED_MAGIC,
            protocol_version: RAKNET_PROTOCOL_VERSION,
            padding: crate::protocol::types::EoBPadding(1200 - 46),
        })
        .encode(&mut wire)
        .unwrap();
        handle_offline(
            &socket,
            &config,
            &wire,
            peer,
            &mut sessions,
            &mut pending,
            &tx,
            &ad,
            &mut limiter,
            &mut send,
        )
        .await;
        assert!(pending.is_empty());
        assert!(sessions.is_empty());

        // Authenticate directly with a valid cookie; no per-peer request1 state
        // is required, even when the cookie's previous packet was lost.
        let cookie = limiter.cookie(peer, Instant::now());
        wire.clear();
        RaknetPacket::OpenConnectionRequest2(crate::protocol::packet::OpenConnectionRequest2 {
            magic: DEFAULT_UNCONNECTED_MAGIC,
            cookie: Some(cookie),
            client_proof: false,
            server_addr: socket.local_addr().unwrap(),
            mtu: 1200,
            client_guid: u64::MAX,
        })
        .encode(&mut wire)
        .unwrap();
        handle_offline(
            &socket,
            &config,
            &wire,
            peer,
            &mut sessions,
            &mut pending,
            &tx,
            &ad,
            &mut limiter,
            &mut send,
        )
        .await;
        assert_eq!(sessions.len(), 1);
        assert_eq!(pending.len(), 1);
        let expires = pending[&peer].expires_at;
        handle_offline(
            &socket,
            &config,
            &wire,
            peer,
            &mut sessions,
            &mut pending,
            &tx,
            &ad,
            &mut limiter,
            &mut send,
        )
        .await;
        assert_eq!(sessions.len(), 1);
        assert_eq!(pending[&peer].expires_at, expires);
    }

    /// BDS 1.26.51 capture: Reply1 MTU = min(probe MTU, 1400) with no lower
    /// bound.
    #[test]
    fn vanilla_reply1_mtu_is_payload_plus_28_capped_at_1400() {
        let mut config = listener_config(TransmissionPolicy::Vanilla);
        let reply1 = |config: &RaknetListenerConfig, probe_mtu: usize| match vanilla_request1(
            config,
            RAKNET_PROTOCOL_VERSION,
            probe_mtu - RAKNET_UDP_HEADER_SIZE,
        ) {
            VanillaReply::Packet(RaknetPacket::OpenConnectionReply1(reply)) => {
                assert_eq!(reply.cookie, None);
                assert_eq!(reply.server_guid, server_guid());
                reply.mtu
            }
            other => panic!("expected Reply1, got {other:?}"),
        };
        for (probe, expected) in [
            (1500, 1400),
            (1492, 1400),
            (1464, 1400),
            (1401, 1400),
            (1300, 1300),
            (1000, 1000),
            (576, 576),
            (400, 400),
            (300, 300),
            (100, 100),
            (46, 46),
        ] {
            assert_eq!(reply1(&config, probe), expected, "probe {probe}");
        }
        // A lower configured cap lowers the maximum, never the floor.
        config.max_mtu = 1200;
        assert_eq!(reply1(&config, 1492), 1200);
        assert_eq!(reply1(&config, 300), 300);
    }

    /// Reply1 on the wire: Vanilla writes security 0 and no cookie (28 bytes)
    /// and no 400 floor; Optimized keeps its cookie, the go-raknet IPv4
    /// header estimate and the [400, max_mtu] clamp.
    #[tokio::test]
    async fn reply1_wire_per_policy() {
        for (policy, probe, mtu, cookie) in [
            (TransmissionPolicy::Vanilla, 1492, 1400, false),
            (TransmissionPolicy::Vanilla, 300, 300, false),
            (TransmissionPolicy::Optimized, 1492, 1492, true),
            (TransmissionPolicy::Optimized, 300, 400, true),
        ] {
            let mut wire = Wire::new(listener_config(policy)).await;
            wire.deliver(0, &request1(RAKNET_PROTOCOL_VERSION, probe))
                .await;
            let bytes = wire.reply(0).await;
            assert_eq!(bytes[0], OpenConnectionReply1::ID);
            assert_eq!(bytes[25], cookie as u8, "{policy:?} security byte");
            assert_eq!(bytes.len(), if cookie { 32 } else { 28 });
            let RaknetPacket::OpenConnectionReply1(reply) = decode(&bytes) else {
                panic!("expected Reply1")
            };
            assert_eq!(reply.mtu, mtu, "{policy:?} probe {probe}");
            assert_eq!(reply.cookie.is_some(), cookie);
        }
    }

    /// BDS answers protocol 10 or 12 with 0x19 carrying protocol 11 and keeps
    /// answering (capture `probe`); Optimized blocks the IP
    /// after its 0x19 (go-raknet `listener.go:security.blockFor`).
    #[tokio::test]
    async fn incompatible_protocol_reply_per_policy() {
        let mut expected = vec![IncompatibleProtocolVersion::ID, RAKNET_PROTOCOL_VERSION];
        expected.extend_from_slice(&DEFAULT_UNCONNECTED_MAGIC);
        expected.extend_from_slice(&server_guid().to_be_bytes());
        for policy in POLICIES {
            let mut wire = Wire::new(listener_config(policy)).await;
            for protocol in [10, 12] {
                wire.deliver(0, &request1(protocol, 1492)).await;
                if protocol == 12 && !policy.is_vanilla() {
                    wire.assert_silent(0).await;
                } else {
                    assert_eq!(wire.reply(0).await, expected, "{policy:?}");
                }
            }
            wire.deliver(0, &request1(RAKNET_PROTOCOL_VERSION, 1492))
                .await;
            if policy.is_vanilla() {
                assert_eq!(wire.reply(0).await[0], OpenConnectionReply1::ID);
            } else {
                wire.assert_silent(0).await;
            }
        }
    }

    /// BDS answers a Request2 with no preceding Request1 (and no cookie, any
    /// GUID, MTU 2000) with Reply2 echoing the MTU; the session uses 1400.
    /// Optimized reads a cookie, rejects it and blocks the address.
    #[tokio::test]
    async fn request2_without_request1_per_policy() {
        let request = request2(None, 2000, 7);
        let mut vanilla = Wire::new(listener_config(TransmissionPolicy::Vanilla)).await;
        vanilla.deliver(0, &request).await;
        let RaknetPacket::OpenConnectionReply2(reply) = decode(&vanilla.reply(0).await) else {
            panic!("expected Reply2")
        };
        assert_eq!(reply.mtu, 2000);
        assert_eq!(reply.server_addr, vanilla.peer(0));
        assert_eq!(reply.server_guid, server_guid());
        assert!(!reply.security);
        let state = &vanilla.sessions[&vanilla.peer(0)];
        assert_eq!(state.managed.mtu(), 1400);
        assert_eq!(state.client_guid, 7);
        assert_eq!(vanilla.pending.len(), 1);

        let mut optimized = Wire::new(listener_config(TransmissionPolicy::Optimized)).await;
        optimized.deliver(0, &request).await;
        optimized.assert_silent(0).await;
        assert!(optimized.sessions.is_empty());
        let ip = optimized.peer(0).ip();
        assert!(optimized.limiter.blocked(ip, Instant::now()));
    }

    /// BDS 1.26.51 capture: Reply2 echoes 1492, 1400, 1200, 576, 400, 399,
    /// 300, 1500, 1600 and 2000; the session MTU follows Mojang RakNet's
    /// clamp (PARITY LS-4).
    #[test]
    fn vanilla_reply2_echoes_requested_mtu_and_session_mtu_is_clamped() {
        let now = Instant::now();
        for (requested, session) in [
            (1492u16, 1400u16),
            (1400, 1400),
            (1200, 1200),
            (576, 576),
            (400, 576),
            (399, 576),
            (300, 576),
            (1500, 1400),
            (1600, 1400),
            (2000, 1400),
        ] {
            let mut admission = Admission::vanilla();
            let reply = admission.request2("10.0.0.1:1000", requested, 1, now);
            assert_eq!(reply2_mtu(reply, "10.0.0.1:1000"), requested);
            let state = admission.session("10.0.0.1:1000").unwrap();
            assert_eq!(state.managed.mtu(), usize::from(session), "{requested}");
        }
        assert_eq!(vanilla_session_mtu(1492, 1200), 1200);
        assert_eq!(vanilla_session_mtu(300, 500), 500);
    }

    /// BDS (capture `probe`, PARITY LS-5): the same address and GUID get Reply2
    /// again (echoing the new MTU, no second session) while the connection is
    /// UNVERIFIED_SENDER, and 0x12 once its ConnectionRequest arrived.
    #[test]
    fn vanilla_repeated_request2_replies_again_until_connection_request() {
        let now = Instant::now();
        let peer = "10.0.0.1:1000";
        let mut admission = Admission::vanilla();
        assert_eq!(
            reply2_mtu(admission.request2(peer, 1492, 5, now), peer),
            1492
        );
        let expires = admission.pending[&peer.parse().unwrap()].expires_at;
        let later = now + Duration::from_millis(5);
        assert_eq!(
            reply2_mtu(admission.request2(peer, 1200, 5, later), peer),
            1200
        );
        assert_eq!(admission.sessions.len(), 1);
        assert_eq!(admission.session(peer).unwrap().managed.mtu(), 1400);
        assert_eq!(
            admission.pending[&peer.parse().unwrap()].expires_at,
            expires
        );

        let state = admission.sessions.get_mut(&peer.parse().unwrap()).unwrap();
        receive_connection_request(state, 5, later);
        assert_eq!(
            refusal(admission.request2(peer, 1492, 5, later)),
            AlreadyConnected::ID
        );
        assert_eq!(admission.sessions.len(), 1);
    }

    /// A repeated Request2 on the wire: Vanilla echoes the new MTU, Optimized
    /// repeats the session MTU.
    #[tokio::test]
    async fn repeated_request2_reply_mtu_per_policy() {
        for (policy, second_reply) in [
            (TransmissionPolicy::Vanilla, 1200),
            (TransmissionPolicy::Optimized, 1492),
        ] {
            let mut wire = Wire::new(listener_config(policy)).await;
            let cookie = (!policy.is_vanilla()).then(|| wire.cookie(0));
            let guid = (-5i64) as u64;
            for (mtu, expected) in [(1492, 1492), (1200, second_reply)] {
                wire.deliver(0, &request2(cookie, mtu, guid)).await;
                let RaknetPacket::OpenConnectionReply2(reply) = decode(&wire.reply(0).await) else {
                    panic!("expected Reply2")
                };
                assert_eq!(reply.mtu, expected, "{policy:?}");
            }
            assert_eq!(wire.sessions.len(), 1);
        }
    }

    /// BDS (capture `probe`, PARITY LS-5; Jenkins outcomes 2-4): a GUID in use
    /// at another address, or an address in use with another GUID, gets 0x12;
    /// the unassigned GUID never matches.
    #[test]
    fn vanilla_guid_or_address_conflict_is_already_connected() {
        let now = Instant::now();
        let mut admission = Admission::vanilla();
        reply2_mtu(
            admission.request2("10.0.0.1:1000", 1400, 9, now),
            "10.0.0.1:1000",
        );
        assert_eq!(
            refusal(admission.request2("10.0.0.2:1000", 1400, 9, now)),
            AlreadyConnected::ID
        );
        assert_eq!(
            refusal(admission.request2("10.0.0.1:1000", 1400, 10, now)),
            AlreadyConnected::ID
        );
        assert_eq!(admission.sessions.len(), 1);
        for peer in ["10.0.0.3:1000", "10.0.0.4:1000"] {
            let reply = admission.request2(peer, 1400, UNASSIGNED_RAKNET_GUID, now);
            reply2_mtu(reply, peer);
        }
        assert_eq!(admission.sessions.len(), 3);
    }

    /// BDS 1.26.51 capture: the same GUID from a different address gets 0x12
    /// with magic and server GUID. Optimized has no GUID check.
    #[tokio::test]
    async fn same_guid_from_another_address_per_policy() {
        for policy in POLICIES {
            let mut wire = Wire::new(listener_config(policy)).await;
            let guid = (-9i64) as u64;
            for client in 0..2 {
                let cookie = (!policy.is_vanilla()).then(|| wire.cookie(client));
                wire.deliver(client, &request2(cookie, 1400, guid)).await;
            }
            assert_eq!(wire.reply(0).await[0], OpenConnectionReply2::ID);
            let second = wire.reply(1).await;
            if policy.is_vanilla() {
                assert_eq!(second, refusal_bytes(AlreadyConnected::ID));
                assert_eq!(wire.sessions.len(), 1);
            } else {
                assert_eq!(second[0], OpenConnectionReply2::ID);
                assert_eq!(wire.sessions.len(), 2);
            }
        }
    }

    /// BDS answers Request1 from a connected address with Reply1 (capture
    /// `probe`), and has no capacity check before Request2; a
    /// full Vanilla listener then refuses Request2 with 0x14, magic and GUID
    /// (without evicting an existing peer). Optimized
    /// refuses Request1 with an ID-only 0x14 once full.
    #[tokio::test]
    async fn request1_from_connected_peer_and_when_full_per_policy() {
        for policy in POLICIES {
            let mut wire = Wire::new(RaknetListenerConfig {
                max_connections: 1,
                ..listener_config(policy)
            })
            .await;
            let guid = (-3i64) as u64;
            let cookie = (!policy.is_vanilla()).then(|| wire.cookie(0));
            wire.deliver(0, &request2(cookie, 1400, guid)).await;
            assert_eq!(wire.reply(0).await[0], OpenConnectionReply2::ID);

            let probe = request1(RAKNET_PROTOCOL_VERSION, 1492);
            wire.deliver(0, &probe).await;
            wire.deliver(1, &probe).await;
            if policy.is_vanilla() {
                assert_eq!(wire.reply(0).await[0], OpenConnectionReply1::ID);
                assert_eq!(wire.reply(1).await[0], OpenConnectionReply1::ID);
                wire.deliver(1, &request2(None, 1400, guid - 1)).await;
                assert_eq!(
                    wire.reply(1).await,
                    refusal_bytes(NoFreeIncomingConnections::ID)
                );
            } else {
                assert_eq!(wire.reply(0).await, [NoFreeIncomingConnections::ID]);
                assert_eq!(wire.reply(1).await, [NoFreeIncomingConnections::ID]);
            }
            assert_eq!(wire.sessions.len(), 1);
        }
    }

    /// Mojang RakNet's same-IP reconnect limit (PARITY LS-6):
    /// a new connection from an IP (any port) whose live connection was
    /// created under 50 ms earlier gets 0x1a; loopback is exempt.
    #[test]
    fn vanilla_same_ip_within_50ms_is_ip_recently_connected() {
        let start = Instant::now();
        let mut admission = Admission::vanilla();
        reply2_mtu(
            admission.request2("10.0.0.1:1000", 1400, 1, start),
            "10.0.0.1:1000",
        );
        let at = |ms| start + Duration::from_millis(ms);
        assert_eq!(
            refusal(admission.request2("10.0.0.1:2000", 1400, 2, at(49))),
            IpRecentlyConnected::ID
        );
        assert!(admission.session("10.0.0.1:2000").is_none());
        reply2_mtu(
            admission.request2("10.0.0.9:2000", 1400, 3, at(49)),
            "10.0.0.9:2000",
        );
        reply2_mtu(
            admission.request2("10.0.0.1:2000", 1400, 2, at(50)),
            "10.0.0.1:2000",
        );

        // Only live connections count.
        admission.sessions.clear();
        reply2_mtu(
            admission.request2("10.0.0.1:3000", 1400, 4, at(51)),
            "10.0.0.1:3000",
        );

        for (first, second) in [
            ("127.0.0.1:1000", "127.0.0.1:2000"),
            ("[::1]:1000", "[::1]:2000"),
        ] {
            reply2_mtu(admission.request2(first, 1400, 5, start), first);
            reply2_mtu(admission.request2(second, 1400, 6, start), second);
            admission.sessions.clear();
        }
    }

    /// Optimized keeps accepting a second address of one IP immediately.
    #[tokio::test]
    async fn optimized_has_no_same_ip_reconnect_limit() {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let config = listener_config(TransmissionPolicy::Optimized);
        let mut sessions = HashMap::new();
        let mut pending = HashMap::new();
        let (tx, _rx) = mpsc::channel(1);
        let ad = Arc::new(RwLock::new(Bytes::new()));
        let mut limiter = PingRateLimiter::new(10);
        let mut send = BytesMut::new();
        for peer in ["10.0.0.1:1000", "10.0.0.1:2000"] {
            let peer: SocketAddr = peer.parse().unwrap();
            let cookie = limiter.cookie(peer, Instant::now());
            let request = request2(Some(cookie), 1400, (-1i64) as u64 - u64::from(peer.port()));
            handle_offline(
                &socket,
                &config,
                &request,
                peer,
                &mut sessions,
                &mut pending,
                &tx,
                &ad,
                &mut limiter,
                &mut send,
            )
            .await;
        }
        assert_eq!(sessions.len(), 2);
    }

    /// The refusal layout agrees with the 0x12 codec and carries magic and
    /// the server GUID for 0x14 and 0x1a as well.
    #[test]
    fn vanilla_refusals_carry_magic_and_server_guid() {
        let mut buf = BytesMut::new();
        for id in [
            AlreadyConnected::ID,
            NoFreeIncomingConnections::ID,
            IpRecentlyConnected::ID,
        ] {
            assert!(VanillaReply::Refusal(id).encode(&mut buf));
            assert_eq!(&buf[..], &refusal_bytes(id)[..]);
        }
        assert!(VanillaReply::Refusal(AlreadyConnected::ID).encode(&mut buf));
        let RaknetPacket::AlreadyConnected(packet) = decode(&buf) else {
            panic!("expected AlreadyConnected")
        };
        assert_eq!(packet.server_guid, server_guid());
    }
}

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    net::SocketAddr,
    sync::{Arc, OnceLock, RwLock},
    time::{Duration, Instant},
};

use super::rate_limiter::PingRateLimiter;

use bytes::{Bytes, BytesMut};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use super::online::maybe_announce_connection;
use crate::protocol::{
    constants::{
        DEFAULT_UNCONNECTED_MAGIC, MAXIMUM_MTU_SIZE, MINIMUM_MTU_SIZE, RAKNET_PROTOCOL_VERSION,
        UDP_HEADER_SIZE,
    },
    packet::{
        AlreadyConnected, IncompatibleProtocolVersion, OpenConnectionReply1, OpenConnectionReply2,
        RaknetPacket, UnconnectedPong,
    },
};
use crate::session::manager::{ManagedSession, SessionConfig};
use crate::transport::listener_conn::SessionState;

pub(super) struct PendingConnection {
    pub mtu: u16,
    pub expires_at: Instant,
    pub cookie: u32,
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
        },
        ..Default::default()
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
    new_conn_tx: &mpsc::Sender<(
        SocketAddr,
        mpsc::Receiver<Result<crate::transport::ReceivedMessage, crate::RaknetError>>,
    )>,
    advertisement: &Arc<RwLock<Vec<u8>>>,
    rate_limiter: &mut PingRateLimiter,
) {
    let now = Instant::now();
    pending.retain(|_, p| p.expires_at > now);

    let mut slice = bytes;
    let pkt = match RaknetPacket::decode(&mut slice) {
        Ok(p) => p,
        Err(_) => return,
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

            let ad_data = advertisement.read().unwrap().clone();
            let ad_bytes = if ad_data.is_empty() {
                None
            } else {
                Some(Bytes::from(ad_data))
            };

            let reply = RaknetPacket::UnconnectedPong(UnconnectedPong {
                ping_time: req.ping_time,
                server_guid: server_guid(),
                magic: DEFAULT_UNCONNECTED_MAGIC,
                advertisement: crate::protocol::types::Advertisement(ad_bytes),
            });

            send_unconnected_packet(socket, peer, reply).await;
        }
        RaknetPacket::UnconnectedPingOpenConnections(req) => {
            if req.magic != DEFAULT_UNCONNECTED_MAGIC {
                return;
            }

            // Rate limit ping responses to prevent amplification attacks
            if !rate_limiter.check_and_record(peer.ip(), now) {
                return;
            }

            let ad_data = advertisement.read().unwrap().clone();
            let ad_bytes = if ad_data.is_empty() {
                None
            } else {
                Some(Bytes::from(ad_data))
            };

            let reply = RaknetPacket::UnconnectedPong(UnconnectedPong {
                ping_time: req.ping_time,
                server_guid: server_guid(),
                magic: DEFAULT_UNCONNECTED_MAGIC,
                advertisement: crate::protocol::types::Advertisement(ad_bytes),
            });

            send_unconnected_packet(socket, peer, reply).await;
        }
        RaknetPacket::OpenConnectionRequest1(req) => {
            if req.magic != DEFAULT_UNCONNECTED_MAGIC {
                return;
            }

            if req.protocol_version != RAKNET_PROTOCOL_VERSION {
                let reply =
                    RaknetPacket::IncompatibleProtocolVersion(IncompatibleProtocolVersion {
                        protocol: RAKNET_PROTOCOL_VERSION,
                        magic: DEFAULT_UNCONNECTED_MAGIC,
                        server_guid: server_guid(),
                    });
                send_unconnected_packet(socket, peer, reply).await;
                return;
            }

            let ip_header = if peer.is_ipv4() { 20 } else { 40 };
            let padding_len = req.padding.0;
            let mtu_guess =
                padding_len + 1 + DEFAULT_UNCONNECTED_MAGIC.len() + 1 + ip_header + UDP_HEADER_SIZE;
            let mtu_clamped = clamp_mtu(mtu_guess as u16, MINIMUM_MTU_SIZE, config.max_mtu);
            let cookie = generate_cookie(peer);

            if sessions.len() >= config.max_connections {
                let reply = RaknetPacket::NoFreeIncomingConnections(
                    crate::protocol::packet::NoFreeIncomingConnections,
                );
                send_unconnected_packet(socket, peer, reply).await;
                return;
            }

            if pending.len() >= config.max_pending_connections {
                return;
            }

            pending.insert(
                peer,
                PendingConnection {
                    mtu: mtu_clamped,
                    expires_at: now + Duration::from_secs(10),
                    cookie,
                },
            );

            let reply = RaknetPacket::OpenConnectionReply1(OpenConnectionReply1 {
                magic: DEFAULT_UNCONNECTED_MAGIC,
                server_guid: server_guid(),
                cookie: Some(cookie),
                mtu: mtu_clamped,
            });

            send_unconnected_packet(socket, peer, reply).await;
        }
        RaknetPacket::OpenConnectionRequest2(req) => {
            if req.magic != DEFAULT_UNCONNECTED_MAGIC {
                return;
            }

            let pc = match pending.remove(&peer) {
                Some(pc) => pc,
                None => {
                    // If the peer is not pending but we have a session, it means the client
                    // missed the OpenConnectionReply2 and is retrying. We should resend it.
                    if let Some(state) = sessions.get(&peer) {
                        let server_addr = socket.local_addr().unwrap_or(peer);
                        let reply = RaknetPacket::OpenConnectionReply2(OpenConnectionReply2 {
                            magic: DEFAULT_UNCONNECTED_MAGIC,
                            server_guid: server_guid(),
                            server_addr,
                            mtu: state.managed.mtu() as u16,
                            // NO ENCRYPTION.
                            security: false,
                        });
                        send_unconnected_packet(socket, peer, reply).await;
                    }
                    return;
                }
            };

            if req.cookie != Some(pc.cookie) {
                return;
            }

            if req.mtu < MINIMUM_MTU_SIZE || req.mtu > MAXIMUM_MTU_SIZE {
                send_already_connected(socket, peer).await;
                return;
            }

            let mtu_final = pc.mtu.min(req.mtu);

            let (tx, rx) =
                mpsc::channel::<Result<crate::transport::ReceivedMessage, crate::RaknetError>>(128);
            let sess_config = server_session_config(config);
            let managed = ManagedSession::with_config(peer, mtu_final as usize, now, sess_config);
            sessions.insert(
                peer,
                SessionState {
                    managed,
                    to_app: tx,
                    pending_rx: Some(rx),
                    announced: false,
                },
            );
            if let Some(state) = sessions.get_mut(&peer) {
                maybe_announce_connection(peer, state, new_conn_tx).await;
            }

            // Fallback to peer address if local address cannot be determined.
            // This is a best-effort approach to avoid crashing.
            let server_addr = socket.local_addr().unwrap_or(peer);

            let reply = RaknetPacket::OpenConnectionReply2(OpenConnectionReply2 {
                magic: DEFAULT_UNCONNECTED_MAGIC,
                server_guid: server_guid(),
                server_addr,
                mtu: mtu_final,
                security: false,
            });

            send_unconnected_packet(socket, peer, reply).await;
        }
        _ => {}
    }
}

fn clamp_mtu(v: u16, min: u16, max: u16) -> u16 {
    v.clamp(min, max)
}

fn generate_cookie(peer: SocketAddr) -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};

    let mut hasher = DefaultHasher::new();
    peer.hash(&mut hasher);
    if let Ok(duration) = SystemTime::now().duration_since(UNIX_EPOCH) {
        hasher.write_u64(duration.as_nanos() as u64);
    }
    let mut cookie = (hasher.finish() & 0xffff_ffff) as u32;
    // Ensure cookie doesn't start with 4 or 6 to avoid ambiguity with IP versions
    // in OpenConnectionRequest2 decoding logic.
    let first_byte = (cookie >> 24) as u8;
    if first_byte == 4 || first_byte == 6 {
        cookie ^= 0x0100_0000;
    }
    cookie
}

fn server_guid() -> u64 {
    static GUID: OnceLock<u64> = OnceLock::new();
    *GUID.get_or_init(|| {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x1234_5678_9abc_def0)
    })
}

async fn send_unconnected_packet(socket: &UdpSocket, peer: SocketAddr, pkt: RaknetPacket) {
    let mut buf = BytesMut::new();
    if pkt.encode(&mut buf).is_ok() {
        let _ = socket.send_to(&buf, peer).await;
    }
}

async fn send_already_connected(socket: &UdpSocket, peer: SocketAddr) {
    let pkt = RaknetPacket::AlreadyConnected(AlreadyConnected {
        magic: DEFAULT_UNCONNECTED_MAGIC,
        server_guid: server_guid(),
    });
    send_unconnected_packet(socket, peer, pkt).await;
}

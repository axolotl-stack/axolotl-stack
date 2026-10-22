use std::{
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
        IncompatibleProtocolVersion, OpenConnectionReply1, OpenConnectionReply2, RaknetPacket,
        UnconnectedPong,
    },
};
use crate::session::manager::{ManagedSession, SessionConfig};
use crate::transport::listener_conn::SessionState;

/// Authenticated handshakes expire absolutely after ten seconds, matching
/// upstream `handler.go:handleOpenConnectionRequest2`, not an idle timeout.
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
    advertisement: &Arc<RwLock<Bytes>>,
    rate_limiter: &mut PingRateLimiter,
    send_buf: &mut BytesMut,
) {
    let now = Instant::now();

    let mut slice = bytes;
    let decoded = if bytes.first() == Some(&0x07) {
        slice = &bytes[1..];
        crate::protocol::packet::OpenConnectionRequest2::decode_body_with_security(&mut slice, true)
            .map(RaknetPacket::OpenConnectionRequest2)
    } else {
        RaknetPacket::decode(&mut slice)
    };
    let pkt = match decoded {
        Ok(p) => p,
        Err(_) => {
            rate_limiter.block(peer.ip(), now);
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
                let reply = RaknetPacket::NoFreeIncomingConnections(
                    crate::protocol::packet::NoFreeIncomingConnections,
                );
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
                let reply = RaknetPacket::NoFreeIncomingConnections(
                    crate::protocol::packet::NoFreeIncomingConnections,
                );
                send_unconnected_packet(socket, peer, reply, send_buf).await;
                return;
            }

            // Upstream has no per-IP/global admission caps. Retain the configured
            // bounded pending budget, also applying it per IP across all sessions.
            if pending.len() >= config.max_pending_connections
                || sessions
                    .keys()
                    .filter(|addr| addr.ip() == peer.ip())
                    .count()
                    >= config.max_pending_connections
            {
                return;
            }
            let mtu_final = config.max_mtu.min(req.mtu);
            pending.insert(
                peer,
                PendingConnection {
                    expires_at: now + Duration::from_secs(10),
                },
            );

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
                    handshake_confirmed: false,
                },
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
    use std::collections::HashMap;

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
}

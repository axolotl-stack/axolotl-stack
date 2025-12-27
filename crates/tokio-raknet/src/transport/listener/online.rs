use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Instant;

use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use crate::protocol::{datagram::Datagram, packet::RaknetPacket};
use crate::session::manager::{ConnectionState, ManagedSession};
use crate::transport::listener_conn::SessionState;
use crate::transport::mux::flush_managed;
use bytes::BufMut;

use super::offline::{
    PendingConnection, handle_offline, is_offline_packet_id, server_session_config,
};
use super::rate_limiter::PingRateLimiter;

use std::sync::{Arc, RwLock};

use crate::transport::listener::RaknetListenerConfig;

#[allow(clippy::too_many_arguments)]
pub(super) async fn dispatch_datagram(
    socket: &UdpSocket,
    config: &RaknetListenerConfig,
    bytes: &[u8],
    peer: SocketAddr,
    sessions: &mut HashMap<SocketAddr, SessionState>,
    pending: &mut HashMap<SocketAddr, PendingConnection>,
    new_conn_tx: &mpsc::Sender<(
        SocketAddr,
        mpsc::Receiver<Result<crate::transport::ReceivedMessage, crate::RaknetError>>,
    )>,
    advertisement: &Arc<RwLock<Vec<u8>>>,
    rate_limiter: &mut PingRateLimiter,
) {
    if sessions.contains_key(&peer) {
        if !handle_incoming_udp(socket, config, bytes, peer, sessions, pending, new_conn_tx).await {
            // If decoding failed, check if it is an offline packet (e.g. handshake retry).
            // If so, don't kill the session; let handle_offline deal with it.
            if is_offline_packet_id(bytes[0]) {
                handle_offline(
                    socket,
                    config,
                    bytes,
                    peer,
                    sessions,
                    pending,
                    new_conn_tx,
                    advertisement,
                    rate_limiter,
                )
                .await;
            } else {
                // Garbage or unexpected packet; drop session.
                sessions.remove(&peer);
                handle_offline(
                    socket,
                    config,
                    bytes,
                    peer,
                    sessions,
                    pending,
                    new_conn_tx,
                    advertisement,
                    rate_limiter,
                )
                .await;
            }
        }
        return;
    }

    if bytes.is_empty() {
        return;
    }

    if is_offline_packet_id(bytes[0]) {
        handle_offline(
            socket,
            config,
            bytes,
            peer,
            sessions,
            pending,
            new_conn_tx,
            advertisement,
            rate_limiter,
        )
        .await;
    } else {
        // Unexpected packet from unknown peer; ignore.
    }
}

#[tracing::instrument(skip(socket, sessions), level = "trace")]
pub(super) async fn handle_outgoing_msg(
    socket: &UdpSocket,
    mtu: usize,
    msg: crate::transport::OutboundMsg,
    sessions: &mut HashMap<SocketAddr, SessionState>,
    config: &RaknetListenerConfig,
) {
    let now = Instant::now();
    let state = sessions.entry(msg.peer).or_insert_with(|| {
        let (tx, rx) = mpsc::channel(128);
        let sess_config = server_session_config(config);
        SessionState {
            managed: ManagedSession::with_config(msg.peer, mtu, now, sess_config),
            to_app: tx,
            pending_rx: Some(rx),
            announced: false,
        }
    });

    let _ = state
        .managed
        .queue_app_packet(msg.packet, msg.reliability, msg.channel, msg.priority);

    tracing::trace!("outbound queued");
    flush_managed(&mut state.managed, socket, msg.peer, now, false).await;
}

#[tracing::instrument(skip(socket, sessions), level = "trace")]
pub(super) async fn tick_sessions(
    socket: &UdpSocket,
    sessions: &mut HashMap<SocketAddr, SessionState>,
) {
    let now = Instant::now();
    let mut dead = Vec::new();

    for (&peer, state) in sessions.iter_mut() {
        flush_managed(&mut state.managed, socket, peer, now, true).await;

        if matches!(state.managed.state(), ConnectionState::Closed) {
            // Inform app of disconnection if it was connected/announced
            if state.announced {
                if let Some(reason) = state.managed.last_disconnect_reason() {
                    let _ = state
                        .to_app
                        .send(Err(crate::RaknetError::Disconnected(reason)))
                        .await;
                } else {
                    let _ = state
                        .to_app
                        .send(Err(crate::RaknetError::ConnectionClosed))
                        .await;
                }
            }
            dead.push(peer);
        }
    }

    for peer in dead {
        sessions.remove(&peer);
    }
}

#[tracing::instrument(skip(socket, sessions, _pending, new_conn_tx), level = "trace")]
async fn handle_incoming_udp(
    socket: &UdpSocket,
    config: &RaknetListenerConfig,
    bytes: &[u8],
    peer: SocketAddr,
    sessions: &mut HashMap<SocketAddr, SessionState>,
    _pending: &mut HashMap<SocketAddr, PendingConnection>,
    new_conn_tx: &mpsc::Sender<(
        SocketAddr,
        mpsc::Receiver<Result<crate::transport::ReceivedMessage, crate::RaknetError>>,
    )>,
) -> bool {
    let mut slice = bytes;
    let dgram = match Datagram::decode(&mut slice) {
        Ok(d) => d,
        Err(e) => {
            tracing::debug!(error = ?e, "failed to decode datagram");
            return false;
        }
    };
    let now = Instant::now();
    let state = sessions.entry(peer).or_insert_with(|| {
        tracing::debug!(mtu = config.max_mtu, "create_session");
        let (tx, rx) = mpsc::channel(128);
        let sess_config = server_session_config(config);
        let sess = ManagedSession::with_config(peer, config.max_mtu as usize, now, sess_config);
        SessionState {
            managed: sess,
            to_app: tx,
            pending_rx: Some(rx),
            announced: false,
        }
    });

    let closed_after = if let Ok(pkts) = state.managed.handle_datagram(dgram, now) {
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!("handle_datagram");
            for _pkt in &pkts {
                tracing::trace!("pkt");
            }
        }
        for pkt in ManagedSession::filter_app_packets(pkts) {
            if let RaknetPacket::UserData { id, payload } = pkt.packet {
                // Reassemble original app bytes as go-raknet does: id byte + payload bytes.
                let mut buf = bytes::BytesMut::with_capacity(1 + payload.len());
                buf.put_u8(id);
                buf.extend_from_slice(&payload);
                let msg = crate::transport::ReceivedMessage {
                    buffer: buf.freeze(),
                    reliability: pkt.reliability,
                    channel: pkt.ordering_channel.unwrap_or(0),
                };
                let _ = state.to_app.send(Ok(msg)).await;
            }
        }
        false
    } else {
        false
    };

    maybe_announce_connection(peer, state, new_conn_tx).await;
    flush_managed(&mut state.managed, socket, peer, now, false).await;

    if closed_after || matches!(state.managed.state(), ConnectionState::Closed) {
        if state.announced {
            if let Some(reason) = state.managed.last_disconnect_reason() {
                let _ = state
                    .to_app
                    .send(Err(crate::RaknetError::Disconnected(reason)))
                    .await;
            } else {
                let _ = state
                    .to_app
                    .send(Err(crate::RaknetError::ConnectionClosed))
                    .await;
            }
        }
        sessions.remove(&peer);
    }
    true
}

#[tracing::instrument(skip(state, new_conn_tx), level = "trace")]
pub(super) async fn maybe_announce_connection(
    peer: SocketAddr,
    state: &mut SessionState,
    new_conn_tx: &mpsc::Sender<(
        SocketAddr,
        mpsc::Receiver<Result<crate::transport::ReceivedMessage, crate::RaknetError>>,
    )>,
) {
    if state.announced || !state.managed.is_connected() {
        tracing::trace!("maybe_announce");
        return;
    }

    if let Some(rx) = state.pending_rx.take() {
        state.announced = true;
        tracing::info!("announce_connection");
        if new_conn_tx.send((peer, rx)).await.is_err() {
            state.announced = false;
        }
    }
}

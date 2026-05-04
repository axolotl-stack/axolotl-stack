use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Instant;

use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use crate::protocol::state::DisconnectReason;
use crate::protocol::{datagram::Datagram, packet::RaknetPacket};
use crate::session::manager::{ConnectionState, ManagedSession};
use crate::transport::listener_conn::SessionState;
use crate::transport::mux::flush_managed;
use bytes::Bytes;
use bytes::BytesMut;

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
    advertisement: &Arc<RwLock<Bytes>>,
    rate_limiter: &mut PingRateLimiter,
    send_buf: &mut BytesMut,
) {
    if bytes.is_empty() {
        return;
    }

    if sessions.contains_key(&peer) {
        if !handle_incoming_udp(
            socket,
            config,
            bytes,
            peer,
            sessions,
            pending,
            new_conn_tx,
            send_buf,
        )
        .await
        {
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
                    send_buf,
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
                    send_buf,
                )
                .await;
            }
        }
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
            send_buf,
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
    send_buf: &mut BytesMut,
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
            handshake_confirmed: false,
        }
    });

    if let Err(error) =
        state
            .managed
            .queue_app_packet(msg.packet, msg.reliability, msg.channel, msg.priority)
    {
        let _ = state.to_app.send(Err(error.into())).await;
        return;
    }

    tracing::trace!("outbound queued");
    flush_managed(&mut state.managed, socket, msg.peer, now, false, send_buf).await;
}

#[tracing::instrument(skip(socket, sessions), level = "trace")]
pub(super) async fn tick_sessions(
    socket: &UdpSocket,
    sessions: &mut HashMap<SocketAddr, SessionState>,
    new_conn_tx: &mpsc::Sender<(
        SocketAddr,
        mpsc::Receiver<Result<crate::transport::ReceivedMessage, crate::RaknetError>>,
    )>,
    send_buf: &mut BytesMut,
) {
    let now = Instant::now();
    let mut dead = Vec::new();

    for (&peer, state) in sessions.iter_mut() {
        maybe_announce_connection(peer, state, new_conn_tx).await;
        flush_managed(&mut state.managed, socket, peer, now, true, send_buf).await;
        let pending = state.managed.drain_pending_incoming(now);
        update_handshake_confirmation(&pending, state);
        if state.handshake_confirmed {
            deliver_packets_to_app(pending, state);
        }

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
#[allow(clippy::too_many_arguments)]
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
    send_buf: &mut BytesMut,
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
            handshake_confirmed: false,
        }
    });

    let closed_after = if let Ok(pkts) = state.managed.handle_datagram(dgram, now) {
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!("handle_datagram");
            for _pkt in &pkts {
                tracing::trace!("pkt");
            }
        }
        update_handshake_confirmation(&pkts, state);
        if state.handshake_confirmed {
            deliver_packets_to_app(pkts, state);
        }
        false
    } else {
        false
    };

    maybe_announce_connection(peer, state, new_conn_tx).await;
    flush_managed(&mut state.managed, socket, peer, now, false, send_buf).await;

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
    if state.announced || !state.handshake_confirmed || !state.managed.is_connected() {
        return;
    }

    if let Some(rx) = state.pending_rx.take() {
        tracing::info!("announce_connection");
        match new_conn_tx.try_send((peer, rx)) {
            Ok(()) => {
                state.announced = true;
            }
            Err(mpsc::error::TrySendError::Full((_, rx))) => {
                state.pending_rx = Some(rx);
                state.announced = false;
            }
            Err(mpsc::error::TrySendError::Closed((_, rx))) => {
                state.pending_rx = Some(rx);
                state.announced = false;
            }
        }
    }
}

fn update_handshake_confirmation(
    pkts: &[crate::session::IncomingPacket],
    state: &mut SessionState,
) {
    if pkts
        .iter()
        .any(|pkt| matches!(pkt.packet, RaknetPacket::NewIncomingConnection(_)))
    {
        state.handshake_confirmed = true;
    }
}

fn deliver_packets_to_app(pkts: Vec<crate::session::IncomingPacket>, state: &mut SessionState) {
    for pkt in ManagedSession::filter_app_packets(pkts) {
        if let RaknetPacket::UserData { id, payload } = pkt.packet {
            let msg = crate::transport::ReceivedMessage {
                id,
                payload,
                reliability: pkt.reliability,
                channel: pkt.ordering_channel.unwrap_or(0),
            };
            match state.to_app.try_send(Ok(msg)) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(_)) => {
                    let _ = state
                        .managed
                        .send_disconnect(DisconnectReason::QueueTooLong);
                    break;
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    let _ = state
                        .managed
                        .send_disconnect(DisconnectReason::Disconnected);
                    break;
                }
            }
        }
    }
}

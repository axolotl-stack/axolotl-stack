use std::net::SocketAddr;
use std::time::Instant;

use crate::protocol::constants::{
    DEFAULT_UNCONNECTED_MAGIC, LOCAL_IP_ADDRESSES_V4, LOCAL_IP_ADDRESSES_V6,
};
use crate::protocol::packet::{
    ConnectedPing, ConnectedPong, ConnectionRequest, ConnectionRequestAccepted,
    ConnectionRequestFailed, DisconnectionNotification, NewIncomingConnection, RaknetPacket,
};
use crate::protocol::reliability::Reliability;
use crate::protocol::state::{DisconnectReason, RakPriority};
use crate::protocol::types::RaknetTime;

use super::{ConnectionState, ManagedSession, SessionError, SessionRole};

impl ManagedSession {
    /// Begin the client-side online handshake by emitting a `ConnectionRequest`.
    pub fn start_client_handshake(
        &mut self,
        server_guid: u64,
        now: Instant,
        secure: bool,
    ) -> Result<(), SessionError> {
        if self.state == ConnectionState::Closed {
            return Err(SessionError::Closed);
        }
        if self.config.role != SessionRole::Client {
            return Err(SessionError::InvalidState {
                state: self.state,
                msg: "cannot start client handshake when not acting as a client",
            });
        }

        self.remote_guid = Some(server_guid);
        self.state = ConnectionState::OnlineHandshake;

        self.last_activity = now;
        self.last_pong_received = now;

        let timestamp = Self::current_raknet_time(now);

        let pkt = RaknetPacket::ConnectionRequest(ConnectionRequest {
            client_guid: self.config.guid,
            timestamp,

            secure,
        });

        self.queue_control_packet(pkt, Reliability::Reliable, 0, RakPriority::Immediate);

        Ok(())
    }

    /// Send a graceful `DisconnectionNotification` and transition to closing.
    pub fn send_disconnect(&mut self, reason: DisconnectReason) -> Result<(), SessionError> {
        if matches!(self.state, ConnectionState::Closed) {
            return Err(SessionError::Closed);
        }

        let pkt = RaknetPacket::DisconnectionNotification(DisconnectionNotification { reason });

        self.queue_control_packet(pkt, Reliability::Reliable, 0, RakPriority::Immediate);
        self.state = ConnectionState::Closing;
        self.last_disconnect_reason = Some(reason);

        Ok(())
    }

    pub(super) fn handle_control_packet(&mut self, pkt: &RaknetPacket, now: Instant) {
        match pkt {
            RaknetPacket::ConnectionRequest(req) => self.handle_connection_request(req, now),
            RaknetPacket::ConnectionRequestAccepted(acc) => {
                self.handle_connection_request_accepted(acc, now)
            }
            RaknetPacket::ConnectionRequestFailed(_) => self.handle_connection_request_failed(),
            RaknetPacket::NewIncomingConnection(conn) => {
                self.handle_new_incoming_connection(conn, now)
            }
            RaknetPacket::ConnectedPing(ping) => self.handle_connected_ping(ping, now),
            RaknetPacket::ConnectedPong(pong) => self.handle_connected_pong(pong, now),
            RaknetPacket::DisconnectionNotification(notification) => {
                self.handle_disconnection_notification(notification)
            }
            _ => self.handle_generic_control_states(pkt),
        }
    }

    fn handle_connection_request(&mut self, req: &ConnectionRequest, now: Instant) {
        if self.config.role != SessionRole::Server {
            return;
        }

        self.remote_guid = Some(req.client_guid);

        self.state = ConnectionState::OnlineHandshake;
        self.last_activity = now;
        self.last_pong_received = now;

        tracing::debug!(
            server_guid = ?self.remote_guid,
            peer = %self.peer,
            ts = ?req.timestamp,
            "accept_conn_req"
        );

        let accepted_ts = Self::current_raknet_time(now);
        let packet = RaknetPacket::ConnectionRequestAccepted(ConnectionRequestAccepted {
            address: self.peer,

            system_index: 47,
            system_addresses: Self::default_system_addresses(self.peer),
            request_timestamp: req.timestamp,

            accepted_timestamp: accepted_ts,
        });

        self.queue_control_packet(packet, Reliability::Unreliable, 0, RakPriority::Immediate);
        self.trace_control("send_conn_request_accepted");
    }

    fn handle_connection_request_accepted(
        &mut self,

        pkt: &ConnectionRequestAccepted,
        now: Instant,
    ) {
        if self.config.role != SessionRole::Client {
            return;
        }

        self.state = ConnectionState::OnlineHandshake;

        self.last_activity = now;
        self.last_pong_received = now;

        let packet = RaknetPacket::NewIncomingConnection(NewIncomingConnection {
            server_address: self.peer,
            system_addresses: Self::default_system_addresses(self.peer),
            request_timestamp: Self::current_raknet_time(now),

            accepted_timestamp: pkt.request_timestamp,
        });

        self.queue_control_packet(
            packet,
            Reliability::ReliableOrdered,
            0,
            RakPriority::Immediate,
        );
        self.state = ConnectionState::Connected;
    }

    fn handle_connection_request_failed(&mut self) {
        self.state = ConnectionState::Closed;

        self.last_disconnect_reason = Some(DisconnectReason::ConnectionRequestFailed);
    }

    fn handle_new_incoming_connection(&mut self, _pkt: &NewIncomingConnection, now: Instant) {
        self.state = ConnectionState::Connected;
        self.last_activity = now;
        self.last_pong_received = now;
    }

    fn handle_connected_ping(&mut self, pkt: &ConnectedPing, now: Instant) {
        self.last_activity = now;

        let pong = RaknetPacket::ConnectedPong(ConnectedPong {
            ping_time: pkt.ping_time,
            pong_time: Self::current_raknet_time(now),
        });
        self.queue_control_packet(pong, Reliability::Unreliable, 0, RakPriority::Immediate);
        self.trace_control("send_connected_pong");
    }

    fn handle_connected_pong(&mut self, pkt: &ConnectedPong, now: Instant) {
        self.last_activity = now;
        self.last_pong_received = now;
        if self
            .current_ping_nonce
            .map(|nonce| nonce == pkt.ping_time.0)
            .unwrap_or(false)
        {
            self.current_ping_nonce = None;
        }
    }

    fn handle_disconnection_notification(&mut self, pkt: &DisconnectionNotification) {
        self.state = ConnectionState::Closed;
        self.last_disconnect_reason = Some(pkt.reason);
    }

    fn handle_generic_control_states(&mut self, pkt: &RaknetPacket) {
        match self.state {
            ConnectionState::Unconnected | ConnectionState::OnlineHandshake => {
                if is_connection_established(pkt) {
                    self.state = ConnectionState::Connected;
                }
            }
            ConnectionState::Connected | ConnectionState::Stale => {
                if is_connection_closed(pkt) {
                    self.state = ConnectionState::Closed;
                }
            }
            ConnectionState::Closing | ConnectionState::Closed => {}
        }
    }

    pub(super) fn queue_control_packet(
        &mut self,

        pkt: RaknetPacket,
        rel: Reliability,
        channel: u8,

        priority: RakPriority,
    ) {
        if channel as usize >= self.config.session.max_ordering_channels {
            return;
        }
        let added = self.inner.queue_packet(pkt, rel, channel, priority);
        self.queued_reliable_bytes = self.queued_reliable_bytes.saturating_add(added);
    }

    #[allow(dead_code)]
    fn queue_connection_request_failed(&mut self) {
        let packet = RaknetPacket::ConnectionRequestFailed(ConnectionRequestFailed {
            magic: DEFAULT_UNCONNECTED_MAGIC,
            server_guid: self.config.guid,
        });
        self.queue_control_packet(
            packet,
            Reliability::ReliableOrdered,
            0,
            RakPriority::Immediate,
        );
    }

    pub(super) fn default_system_addresses(peer: SocketAddr) -> [SocketAddr; 10] {
        if peer.is_ipv4() {
            LOCAL_IP_ADDRESSES_V4.map(SocketAddr::V4)
        } else {
            LOCAL_IP_ADDRESSES_V6.map(SocketAddr::V6)
        }
    }

    pub(super) fn current_raknet_time(now: Instant) -> RaknetTime {
        let start = crate::protocol::types::raknet_start_time();
        let millis = now.saturating_duration_since(start).as_millis() as u64;
        RaknetTime(millis)
    }

    fn trace_control(&self, event: &str) {
        tracing::trace!(
            event = event,
            state = ?self.state,
            peer = %self.peer,
            role = ?self.config.role,
            "control_trace"
        );
    }
}

fn is_connection_established(pkt: &RaknetPacket) -> bool {
    matches!(
        pkt,
        RaknetPacket::NewIncomingConnection(_)
            | RaknetPacket::ConnectionRequestAccepted(_)
            | RaknetPacket::ConnectionRequest(_)
    )
}

fn is_connection_closed(pkt: &RaknetPacket) -> bool {
    matches!(pkt, RaknetPacket::DisconnectionNotification(_))
}

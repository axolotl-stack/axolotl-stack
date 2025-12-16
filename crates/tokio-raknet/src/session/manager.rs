mod control;
mod tick;

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use thiserror::Error;

use crate::protocol::{
    constants::{SESSION_STALE, SESSION_TIMEOUT},
    datagram::{Datagram, DatagramPayload},
    packet::{DecodeError, RaknetPacket},
    reliability::Reliability,
    state::{DisconnectReason, RakPriority},
};

use super::{Session, SessionTunables};

/// High-level connection state for a managed RakNet session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Unconnected,
    OnlineHandshake,
    Connected,
    Stale,
    Closing,
    Closed,
}

/// Errors that can occur when using a managed session at a high level.
#[derive(Error, Debug)]
pub enum SessionError {
    #[error("session is closed")]
    Closed,

    #[error("invalid operation in state {state:?}: {msg}")]
    InvalidState {
        state: ConnectionState,
        msg: &'static str,
    },

    #[error(transparent)]
    Protocol(#[from] DecodeError),
}

/// Role the managed session is acting in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionRole {
    Client,
    Server,
}

/// Configuration for the high-level session manager.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub role: SessionRole,
    pub guid: u64,
    pub session_stale: Duration,
    pub session_timeout: Duration,
    pub ping_interval: Duration,
    pub max_queued_reliable_bytes: Option<usize>,
    pub session: SessionTunables,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            role: SessionRole::Client,
            guid: 0,
            session_stale: SESSION_STALE,
            session_timeout: SESSION_TIMEOUT,
            ping_interval: Duration::from_millis(500),
            max_queued_reliable_bytes: None,
            session: SessionTunables::default(),
        }
    }
}

/// Higher-level wrapper around `Session` that tracks connection state,
/// last activity and enforces a few simple state rules.
pub struct ManagedSession {
    inner: Session,
    peer: SocketAddr,
    state: ConnectionState,
    last_activity: Instant,
    config: SessionConfig,
    last_pong_received: Instant,
    last_ping_sent: Option<Instant>,
    current_ping_nonce: Option<u64>,
    queued_reliable_bytes: usize,
    remote_guid: Option<u64>,
    last_disconnect_reason: Option<DisconnectReason>,
}

impl ManagedSession {
    pub fn new(peer: SocketAddr, mtu: usize, now: Instant) -> Self {
        Self::with_config(peer, mtu, now, SessionConfig::default())
    }

    pub fn with_config(peer: SocketAddr, mtu: usize, now: Instant, config: SessionConfig) -> Self {
        Self {
            inner: Session::with_tunables(mtu, config.session.clone()),
            peer,

            state: ConnectionState::Unconnected,
            last_activity: now,
            config,

            last_pong_received: now,
            last_ping_sent: None,
            current_ping_nonce: None,

            queued_reliable_bytes: 0,
            remote_guid: None,
            last_disconnect_reason: None,
        }
    }

    pub fn peer(&self) -> SocketAddr {
        self.peer
    }

    pub fn mtu(&self) -> usize {
        self.inner.mtu()
    }

    pub fn state(&self) -> ConnectionState {
        self.state
    }

    pub fn config(&self) -> &SessionConfig {
        &self.config
    }

    pub fn last_disconnect_reason(&self) -> Option<DisconnectReason> {
        self.last_disconnect_reason
    }

    pub fn is_connected(&self) -> bool {
        matches!(
            self.state,
            ConnectionState::Connected | ConnectionState::Stale
        )
    }

    /// Queue a high-level RakNet packet for sending.
    ///
    /// This applies basic state checks (e.g. prevent unconnected packets when
    /// already connected, or any send on a closed session) and delegates to
    /// the underlying `Session`.
    pub fn queue_app_packet(
        &mut self,
        pkt: RaknetPacket,
        rel: Reliability,
        channel: u8,
        priority: RakPriority,
    ) -> Result<(), SessionError> {
        match self.state {
            ConnectionState::Closed | ConnectionState::Closing => {
                return Err(SessionError::Closed);
            }
            ConnectionState::Unconnected | ConnectionState::OnlineHandshake => {
                if matches!(pkt, RaknetPacket::UserData { .. }) {
                    return Err(SessionError::InvalidState {
                        state: self.state,
                        msg: "cannot send user data before connection is established",
                    });
                }
            }
            ConnectionState::Connected | ConnectionState::Stale => {
                if is_unconnected_packet(&pkt) {
                    return Err(SessionError::InvalidState {
                        state: self.state,
                        msg: "cannot send unconnected packet on a connected session",
                    });
                }
            }
        }

        if channel as usize >= self.config.session.max_ordering_channels {
            return Err(SessionError::InvalidState {
                state: self.state,
                msg: "ordering channel out of range",
            });
        }

        let added = self.inner.queue_packet(pkt, rel, channel, priority);
        self.queued_reliable_bytes = self.queued_reliable_bytes.saturating_add(added);
        // Treat an outbound enqueue as activity to avoid stale self timeouts
        self.last_activity = Instant::now();

        Ok(())
    }

    /// Handle an incoming datagram, updating state and returning any
    /// high-level packets for the application.
    pub fn handle_datagram(
        &mut self,
        dgram: Datagram,
        now: Instant,
    ) -> Result<Vec<crate::session::IncomingPacket>, SessionError> {
        if self.state == ConnectionState::Closed {
            return Ok(Vec::new());
        }

        self.last_activity = now;
        if self.state == ConnectionState::Stale {
            self.state = ConnectionState::Connected;
        }

        match dgram.payload {
            DatagramPayload::EncapsulatedPackets(packets) => {
                let pkts = self.inner.handle_data_payload(packets, now)?;

                // Only DATA datagrams participate in sequence/NACK tracking.
                // We process sequence AFTER handling payload so that if handling fails
                // (e.g. split buffer full), we don't ACK the datagram, forcing a resend.
                self.inner.process_datagram_sequence(dgram.header.sequence);

                for pkt in &pkts {
                    self.handle_control_packet(&pkt.packet, now);
                }
                Ok(pkts)
            }
            DatagramPayload::Ack(payload) => {
                self.inner.handle_ack_payload(payload);
                Ok(Vec::new())
            }
            DatagramPayload::Nak(payload) => {
                self.inner.handle_nack_payload(payload);
                Ok(Vec::new())
            }
        }
    }

    /// Build the next outgoing datagram, if any.
    /// ACKs/NACKs/Resends are built in `on_tick`.
    pub fn build_datagram(&mut self, now: Instant) -> Option<Datagram> {
        let dgram = self.inner.build_data_datagram(now)?;

        if let DatagramPayload::EncapsulatedPackets(packets) = &dgram.payload {
            self.debit_reliable_bytes(packets);
        }

        Some(dgram)
    }

    /// Filter a batch of decoded packets down to game-level packets that
    /// should be delivered to the application. Currently this keeps only
    /// user-data packets (IDs >= 0x80).
    pub fn filter_app_packets(
        pkts: Vec<crate::session::IncomingPacket>,
    ) -> Vec<crate::session::IncomingPacket> {
        pkts.into_iter()
            .filter(|p| is_app_packet(&p.packet))
            .collect()
    }
}

fn is_unconnected_packet(pkt: &RaknetPacket) -> bool {
    matches!(
        pkt,
        RaknetPacket::UnconnectedPing(_)
            | RaknetPacket::UnconnectedPong(_)
            | RaknetPacket::UnconnectedPingOpenConnections(_)
    )
}

fn is_app_packet(pkt: &RaknetPacket) -> bool {
    matches!(pkt, RaknetPacket::UserData { .. })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{
        datagram::DatagramPayload,
        packet::{ConnectionRequest, ConnectionRequestAccepted, DisconnectionNotification},
        state::DisconnectReason,
        types::RaknetTime,
    };
    use bytes::Bytes;

    #[test]
    fn state_transitions_on_control_packets() {
        let peer: SocketAddr = "127.0.0.1:19132".parse().unwrap();

        let mut ms = ManagedSession::new(peer, 1200, Instant::now());
        assert_eq!(ms.state(), ConnectionState::Unconnected);

        let ctrl = RaknetPacket::ConnectionRequestAccepted(ConnectionRequestAccepted {
            address: "127.0.0.1:19132".parse().unwrap(),
            system_index: 0,
            system_addresses: [peer; 10],
            request_timestamp: RaknetTime(0),
            accepted_timestamp: RaknetTime(0),
        });
        ms.handle_control_packet(&ctrl, Instant::now());
        assert!(ms.is_connected());

        let disc = RaknetPacket::DisconnectionNotification(DisconnectionNotification {
            reason: DisconnectReason::Disconnected,
        });
        ms.handle_control_packet(&disc, Instant::now());
        assert_eq!(ms.state(), ConnectionState::Closed);
    }

    #[test]
    fn client_handshake_emits_connection_request() {
        let peer: SocketAddr = "127.0.0.1:19133".parse().unwrap();
        let now = Instant::now();
        let config = SessionConfig {
            role: SessionRole::Client,
            guid: 0x01,
            ..Default::default()
        };
        let mut ms = ManagedSession::with_config(peer, 1200, now, config);

        ms.start_client_handshake(0x02, now, false).unwrap();

        let dgram = ms
            .build_datagram(now)
            .expect("expected datagram with connection request");

        let pkt = decode_first_packet(&dgram);
        assert!(matches!(pkt, RaknetPacket::ConnectionRequest(_)));
    }

    #[test]
    fn server_handles_connection_request_and_sends_accept() {
        let peer: SocketAddr = "127.0.0.1:19134".parse().unwrap();
        let now = Instant::now();

        let config = SessionConfig {
            role: SessionRole::Server,
            guid: 0xaa,
            ..Default::default()
        };
        let mut ms = ManagedSession::with_config(peer, 1200, now, config);

        let request = RaknetPacket::ConnectionRequest(ConnectionRequest {
            client_guid: 0xaa,
            timestamp: RaknetTime(42),
            secure: false,
        });
        ms.handle_control_packet(&request, now);

        let dgram = ms
            .build_datagram(now)
            .expect("expected datagram with connection request accepted");
        let pkt = decode_first_packet(&dgram);

        assert!(matches!(pkt, RaknetPacket::ConnectionRequestAccepted(_)));
    }

    #[test]
    fn ping_is_scheduled_during_tick() {
        let peer: SocketAddr = "127.0.0.1:19135".parse().unwrap();
        let now = Instant::now();
        let config = SessionConfig {
            ping_interval: Duration::from_millis(0),
            ..Default::default()
        };

        let mut ms = ManagedSession::with_config(peer, 1200, now, config);
        ms.state = ConnectionState::Connected;

        let _ = ms.on_tick(now);

        let dgram = ms
            .build_datagram(now)
            .expect("expected datagram containing connected ping");

        let pkt = decode_first_packet(&dgram);
        assert!(matches!(pkt, RaknetPacket::ConnectedPing(_)));
    }

    #[test]
    fn rejects_user_data_before_handshake() {
        let peer: SocketAddr = "127.0.0.1:19136".parse().unwrap();
        let now = Instant::now();
        let mut ms = ManagedSession::new(peer, 1200, now);

        let pkt = RaknetPacket::UserData {
            id: 0x80,
            payload: Bytes::from_static(b"test"),
        };

        let res = ms.queue_app_packet(pkt, Reliability::Reliable, 0, RakPriority::High);
        assert!(matches!(res, Err(SessionError::InvalidState { .. })));
    }

    #[test]
    fn rejects_out_of_range_channel() {
        let peer: SocketAddr = "127.0.0.1:19137".parse().unwrap();
        let now = Instant::now();
        let mut ms = ManagedSession::new(peer, 1200, now);
        ms.state = ConnectionState::Connected;

        let pkt = RaknetPacket::UserData {
            id: 0x80,
            payload: Bytes::from_static(b"test"),
        };

        let res = ms.queue_app_packet(pkt, Reliability::Reliable, 99, RakPriority::High);
        assert!(matches!(res, Err(SessionError::InvalidState { .. })));
    }

    fn decode_first_packet(dgram: &crate::protocol::datagram::Datagram) -> RaknetPacket {
        if let DatagramPayload::EncapsulatedPackets(packets) = &dgram.payload {
            let encap = packets
                .first()
                .expect("datagram should contain at least one packet");
            let mut payload = encap.payload.clone();
            RaknetPacket::decode(&mut payload).expect("packet should decode")
        } else {
            panic!("Expected a datagram with encapsulated packets for this test")
        }
    }
}

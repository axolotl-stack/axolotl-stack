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

use super::{QueuePacketError, Session, SessionTunables};

/// Requested ACK timer backstop, independent of the 100 ms maintenance tick.
/// Tokio rounds timers
/// up to millisecond ticks (typically 1-2 ms); Windows IOCP may delay them by
/// roughly 15.6 ms. Runtime/OS scheduling adds latency: this is not a wall-clock
/// upper bound. Handshake, split, continuous-send and half-MTU pressure bypass it.
pub(crate) const MAX_ACK_DELAY: Duration = Duration::from_millis(1);

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

    #[error(transparent)]
    Queue(#[from] QueuePacketError),
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
    created_at: Instant,
    closing_since: Option<Instant>,
    config: SessionConfig,
    last_pong_received: Instant,
    last_ping_sent: Option<Instant>,
    current_ping_nonce: Option<u64>,
    queued_reliable_bytes: usize,
    remote_guid: Option<u64>,
    last_disconnect_reason: Option<DisconnectReason>,
    ack_deadline: Option<Instant>,
    pending_ack_bytes: usize,
    /// go-raknet `conn.go:startTicking` `acksLeft`, sampled while closing.
    vanilla_acks_left: usize,
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
            created_at: now,
            closing_since: None,

            last_pong_received: now,
            last_ping_sent: None,
            current_ping_nonce: None,

            queued_reliable_bytes: 0,
            remote_guid: None,
            last_disconnect_reason: None,
            ack_deadline: None,
            pending_ack_bytes: 0,
            vanilla_acks_left: 0,
        }
    }

    /// Transmission policy of the underlying session.
    pub fn transmission_policy(&self) -> super::TransmissionPolicy {
        self.inner.transmission_policy()
    }

    /// Latest RTT estimate, `None` before the first acknowledgement sample.
    pub fn rtt(&self) -> Option<Duration> {
        match self.inner.rtt_micros() {
            u64::MAX => None,
            micros => Some(Duration::from_micros(micros)),
        }
    }

    pub(crate) fn rtt_micros(&self) -> u64 {
        self.inner.rtt_micros()
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

        let added = self.inner.queue_packet(pkt, rel, channel, priority)?;
        self.queued_reliable_bytes = self.queued_reliable_bytes.saturating_add(added);

        Ok(())
    }
    /// Transport messages already include their ID and can be fragmented by
    /// slicing the original Bytes rather than serializing them a second time.
    pub(crate) fn queue_app_buffer(
        &mut self,
        buffer: bytes::Bytes,
        rel: Reliability,
        channel: u8,
        priority: RakPriority,
    ) -> Result<(), SessionError> {
        match self.state {
            ConnectionState::Closed | ConnectionState::Closing => return Err(SessionError::Closed),
            ConnectionState::Unconnected | ConnectionState::OnlineHandshake => {
                return Err(SessionError::InvalidState {
                    state: self.state,
                    msg: "cannot send user data before connection is established",
                });
            }
            ConnectionState::Connected | ConnectionState::Stale => {}
        }
        let added = self
            .inner
            .queue_encoded_packet(buffer, rel, channel, priority)?;
        self.queued_reliable_bytes = self.queued_reliable_bytes.saturating_add(added);
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
                // go-raknet conn.go:receiveDatagram validates and deduplicates
                // the receive window before touching any packet payload.
                let accepted = self
                    .inner
                    .accept_datagram_sequence(dgram.header.sequence, || now)?;
                if self.inner.transmission_policy().is_vanilla() {
                    // go-raknet conn.go:flushACKs: ACKs wait for the 100 ms
                    // tick. conn.go:handlePacket: a closing connection still
                    // ACKs datagrams but no longer handles their frames.
                    if !accepted || self.state == ConnectionState::Closing {
                        return Ok(Vec::new());
                    }
                } else {
                    self.pending_ack_bytes = self.pending_ack_bytes.saturating_add(
                        4 + packets.iter().map(|packet| packet.size()).sum::<usize>(),
                    );
                    if self.state != ConnectionState::Connected
                        || dgram
                            .header
                            .flags
                            .contains(crate::protocol::constants::DatagramFlags::CONTINUOUS_SEND)
                        || packets.iter().any(|packet| packet.header.is_split)
                        || self.pending_ack_bytes >= self.inner.mtu() / 2
                    {
                        self.ack_deadline = Some(now);
                    } else {
                        self.ack_deadline.get_or_insert(now + MAX_ACK_DELAY);
                    }
                    if !accepted {
                        return Ok(Vec::new());
                    }
                }
                let mut pkts = self.inner.handle_data_payload(packets, now)?;
                pkts.extend(self.inner.drain_pending_incoming());

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

    /// Flush due ACKs and immediate NAK/data; retire incoming ACKs immediately
    /// so application delivery and congestion progress never wait for maintenance.
    /// Transports must service `ack_deadline` even when no further traffic arrives.
    pub fn build_datagram(&mut self, now: Instant) -> Option<Datagram> {
        if self.inner.transmission_policy().is_vanilla() {
            let dgram = self.inner.next_vanilla_datagram(now)?;
            if let DatagramPayload::EncapsulatedPackets(packets) = &dgram.payload {
                self.debit_reliable_bytes(packets);
            }
            return Some(dgram);
        }
        self.inner.process_incoming_acks_naks(now);
        let ack = if self.ack_deadline.is_some_and(|deadline| now >= deadline) {
            let ack = self.inner.build_ack_datagram(now);
            if ack.is_none() {
                self.ack_deadline = None;
                self.pending_ack_bytes = 0;
            }
            ack
        } else {
            None
        };
        let dgram = ack
            .or_else(|| self.inner.build_nak_datagram())
            .or_else(|| self.inner.build_data_datagram(now))?;

        if let DatagramPayload::EncapsulatedPackets(packets) = &dgram.payload {
            self.debit_reliable_bytes(packets);
        }

        Some(dgram)
    }

    /// Earliest pending feedback deadline. A custom transport must call
    /// `build_datagram` at this time even if its socket/application queues are idle.
    pub fn ack_deadline(&self) -> Option<Instant> {
        self.ack_deadline
    }

    pub fn drain_pending_incoming(&mut self, now: Instant) -> Vec<crate::session::IncomingPacket> {
        let pkts = self.inner.drain_pending_incoming();
        for pkt in &pkts {
            self.handle_control_packet(&pkt.packet, now);
        }
        pkts
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
    fn encoded_application_send_respects_connection_state_and_channel_bounds() {
        let peer = "127.0.0.1:19132".parse().unwrap();
        let mut session = ManagedSession::new(peer, 1200, Instant::now());
        for state in [
            ConnectionState::Unconnected,
            ConnectionState::OnlineHandshake,
            ConnectionState::Closing,
            ConnectionState::Closed,
        ] {
            session.state = state;
            assert!(
                session
                    .queue_app_buffer(
                        Bytes::from_static(b"\xfehello"),
                        Reliability::ReliableOrdered,
                        0,
                        RakPriority::Normal,
                    )
                    .is_err()
            );
            assert!(session.inner.outgoing_heap.is_empty());
        }
        session.state = ConnectionState::Connected;
        assert!(
            session
                .queue_app_buffer(
                    Bytes::from_static(b"\xfehello"),
                    Reliability::ReliableOrdered,
                    255,
                    RakPriority::Normal,
                )
                .is_err()
        );
        assert!(session.inner.outgoing_heap.is_empty());
    }

    #[test]
    fn small_datagram_acks_coalesce_without_postponing_the_first_deadline() {
        let now = Instant::now();
        let peer = "127.0.0.1:19132".parse().unwrap();
        let mut sender = ManagedSession::new(peer, 1200, now);
        let mut receiver = ManagedSession::new(peer, 1200, now);
        sender.state = ConnectionState::Connected;
        receiver.state = ConnectionState::Connected;
        for delay in [0, 500] {
            let time = now + Duration::from_micros(delay);
            sender
                .queue_app_buffer(
                    Bytes::from_static(b"\xfefirst"),
                    Reliability::ReliableOrdered,
                    0,
                    RakPriority::Normal,
                )
                .unwrap();
            let datagram = sender.build_datagram(time).unwrap();
            let delivered = receiver.handle_datagram(datagram, time).unwrap();
            assert_eq!(delivered.len(), 1, "delivery must not wait for ACK timing");
            assert!(receiver.build_datagram(time).is_none());
        }
        assert!(
            receiver
                .build_datagram(now + Duration::from_micros(999))
                .is_none()
        );
        let ack = receiver
            .build_datagram(now + Duration::from_millis(1))
            .unwrap();
        let DatagramPayload::Ack(payload) = ack.payload else {
            panic!("expected ACK");
        };
        assert_eq!(payload.ranges.len(), 1);
        assert_eq!(payload.ranges[0].start.value(), 0);
        assert_eq!(payload.ranges[0].end.value(), 1);
        assert!(
            receiver
                .build_datagram(now + Duration::from_millis(1))
                .is_none()
        );
    }

    #[test]
    fn continuous_send_acks_a_small_datagram_without_waiting_for_the_deadline() {
        let now = Instant::now();
        let peer = "127.0.0.1:19132".parse().unwrap();
        let mut sender = ManagedSession::new(peer, 1200, now);
        let mut receiver = ManagedSession::new(peer, 1200, now);
        sender.state = ConnectionState::Connected;
        receiver.state = ConnectionState::Connected;
        sender
            .queue_app_buffer(
                Bytes::from_static(b"\xfemore queued"),
                Reliability::ReliableOrdered,
                0,
                RakPriority::Normal,
            )
            .unwrap();
        let mut datagram = sender.build_datagram(now).unwrap();
        datagram
            .header
            .flags
            .insert(crate::protocol::constants::DatagramFlags::CONTINUOUS_SEND);
        assert_eq!(receiver.handle_datagram(datagram, now).unwrap().len(), 1);
        assert!(matches!(
            receiver
                .build_datagram(now)
                .map(|datagram| datagram.payload),
            Some(DatagramPayload::Ack(_))
        ));
    }

    #[test]
    fn half_mtu_of_received_data_flushes_feedback_immediately() {
        let now = Instant::now();
        let peer = "127.0.0.1:19132".parse().unwrap();
        let mut sender = ManagedSession::new(peer, 1200, now);
        let mut receiver = ManagedSession::new(peer, 1200, now);
        sender.state = ConnectionState::Connected;
        receiver.state = ConnectionState::Connected;
        sender
            .queue_app_buffer(
                Bytes::from(vec![0xfe; 700]),
                Reliability::ReliableOrdered,
                0,
                RakPriority::Normal,
            )
            .unwrap();
        let datagram = sender.build_datagram(now).unwrap();
        assert_eq!(receiver.handle_datagram(datagram, now).unwrap().len(), 1);
        assert!(matches!(
            receiver.build_datagram(now).unwrap().payload,
            DatagramPayload::Ack(_)
        ));
    }

    #[test]
    fn uneven_datagrams_can_stall_the_window_only_until_the_ack_deadline() {
        let now = Instant::now();
        let peer = "127.0.0.1:19132".parse().unwrap();
        let mut sender = ManagedSession::new(peer, 1200, now);
        let mut receiver = ManagedSession::new(peer, 1200, now);
        sender.state = ConnectionState::Connected;
        receiver.state = ConnectionState::Connected;
        sender
            .queue_app_buffer(
                Bytes::from(vec![0xfe; 480]),
                Reliability::ReliableOrdered,
                0,
                RakPriority::Normal,
            )
            .unwrap();
        let first = sender.build_datagram(now).unwrap();
        assert_eq!(first.size(), 494);
        receiver.handle_datagram(first, now).unwrap();
        sender
            .queue_app_buffer(
                Bytes::from(vec![0xfe; 700]),
                Reliability::ReliableOrdered,
                0,
                RakPriority::Normal,
            )
            .unwrap();
        assert!(sender.build_datagram(now).is_none());
        assert!(receiver.build_datagram(now).is_none());
        let deadline = now + MAX_ACK_DELAY;
        let ack = receiver.build_datagram(deadline).unwrap();
        assert!(matches!(&ack.payload, DatagramPayload::Ack(_)));
        sender.handle_datagram(ack, deadline).unwrap();
        assert!(matches!(
            sender.build_datagram(deadline).unwrap().payload,
            DatagramPayload::EncapsulatedPackets(_)
        ));
    }

    // Fixed-clock session model: advance only to the requested backstop, never maintenance.
    fn deliver_without_maintenance(size: usize) {
        let mut now = Instant::now();
        let peer = "127.0.0.1:19132".parse().unwrap();
        let mut sender = ManagedSession::new(peer, 1200, now);
        let mut receiver = ManagedSession::new(peer, 1200, now);
        sender.state = ConnectionState::Connected;
        receiver.state = ConnectionState::Connected;
        let payload = Bytes::from(vec![0x5a; size - 1]);
        sender
            .queue_app_packet(
                RaknetPacket::UserData {
                    id: 0xfe,
                    payload: payload.clone(),
                },
                Reliability::ReliableOrdered,
                0,
                RakPriority::Normal,
            )
            .unwrap();
        let mut delivered = Vec::new();
        let mut acked = 0;
        for _ in 0..1024 {
            while let Some(datagram) = sender.build_datagram(now) {
                delivered.extend(receiver.handle_datagram(datagram, now).unwrap());
            }
            if size < 4096 {
                now += MAX_ACK_DELAY;
            }
            let mut feedback = 0;
            while let Some(datagram) = receiver.build_datagram(now) {
                assert!(matches!(datagram.payload, DatagramPayload::Ack(_)));
                sender.handle_datagram(datagram, now).unwrap();
                feedback += 1;
                acked += 1;
            }
            if sender.inner.sent_datagrams.is_empty() && sender.queued_reliable_bytes == 0 {
                break;
            }
            assert!(
                feedback > 0,
                "ACK/window progress waited for maintenance: queued={} outstanding={} window={}",
                sender.queued_reliable_bytes,
                sender.inner.sent_datagrams.len(),
                sender.inner.sliding.get_transmission_bandwidth()
            );
        }
        assert!(acked > 0);
        assert!(
            sender.inner.sent_datagrams.is_empty(),
            "all reliable datagrams must be acknowledged"
        );
        assert_eq!(sender.queued_reliable_bytes, 0);
        assert_eq!(delivered.len(), 1);
        match &delivered[0].packet {
            RaknetPacket::UserData {
                id,
                payload: actual,
            } => {
                assert_eq!(*id, 0xfe);
                assert_eq!(actual, &payload);
            }
            _ => panic!("expected application payload"),
        }
    }

    #[test]
    fn reliable_delivery_and_ack_do_not_wait_for_maintenance() {
        deliver_without_maintenance(32);
    }

    #[test]
    fn split_delivery_and_ack_do_not_wait_for_maintenance() {
        deliver_without_maintenance(65536);
    }

    #[test]
    fn state_transitions_on_control_packets() {
        let peer: SocketAddr = "127.0.0.1:19132".parse().unwrap();

        let mut ms = ManagedSession::new(peer, 1200, Instant::now());
        assert_eq!(ms.state(), ConnectionState::Unconnected);

        let ctrl = RaknetPacket::ConnectionRequestAccepted(ConnectionRequestAccepted {
            address: "127.0.0.1:19132".parse().unwrap(),
            system_index: 0,
            system_addresses: vec![peer; 10],
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

    #[test]
    fn handshake_send_failure_is_reported_without_state_transition() {
        let peer: SocketAddr = "127.0.0.1:19138".parse().unwrap();
        let now = Instant::now();
        let config = SessionConfig {
            role: SessionRole::Client,
            session: SessionTunables {
                max_ordering_channels: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut ms = ManagedSession::with_config(peer, 1200, now, config);

        let err = ms.start_client_handshake(0x1234, now, false).unwrap_err();
        assert!(matches!(err, SessionError::Queue(_)));
        assert_eq!(ms.state(), ConnectionState::Unconnected);
    }

    #[test]
    fn disconnect_send_failure_does_not_close_session_early() {
        let peer: SocketAddr = "127.0.0.1:19139".parse().unwrap();
        let now = Instant::now();
        let config = SessionConfig {
            session: SessionTunables {
                max_ordering_channels: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut ms = ManagedSession::with_config(peer, 1200, now, config);
        ms.state = ConnectionState::Connected;

        let err = ms
            .send_disconnect(DisconnectReason::Disconnected)
            .unwrap_err();
        assert!(matches!(err, SessionError::Queue(_)));
        assert_eq!(ms.state(), ConnectionState::Connected);
        assert!(ms.last_disconnect_reason().is_none());
    }

    #[test]
    fn ping_state_is_not_updated_when_ping_cannot_be_queued() {
        let peer: SocketAddr = "127.0.0.1:19140".parse().unwrap();
        let now = Instant::now();
        let config = SessionConfig {
            session: SessionTunables {
                max_ordering_channels: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut ms = ManagedSession::with_config(peer, 1200, now, config);
        ms.state = ConnectionState::Connected;

        ms.send_connected_ping(now);

        assert!(ms.last_ping_sent.is_none());
        assert!(ms.current_ping_nonce.is_none());
    }

    #[test]
    fn half_open_sessions_time_out() {
        let peer: SocketAddr = "127.0.0.1:19141".parse().unwrap();
        let now = Instant::now();
        let config = SessionConfig {
            session_stale: Duration::from_millis(10),
            session_timeout: Duration::from_secs(10),
            ..Default::default()
        };
        let mut ms = ManagedSession::with_config(peer, 1200, now, config);

        let _ = ms.on_tick(now + Duration::from_secs(10));

        assert_eq!(ms.state(), ConnectionState::Closed);
        assert!(matches!(
            ms.last_disconnect_reason(),
            Some(DisconnectReason::TimedOut)
        ));
    }

    #[test]
    fn outgoing_keepalives_do_not_refresh_receive_timeout() {
        let now = Instant::now();
        let mut session = ManagedSession::new("127.0.0.1:19142".parse().unwrap(), 1200, now);
        session.state = ConnectionState::Connected;
        session.on_tick(now + Duration::from_secs(1));
        assert_eq!(session.last_activity, now);
    }

    #[test]
    fn handshake_activity_does_not_extend_completion_deadline() {
        let now = Instant::now();
        let mut session = ManagedSession::new("127.0.0.1:19143".parse().unwrap(), 1200, now);
        session.last_activity = now + Duration::from_secs(9);
        session.on_tick(now + Duration::from_secs(11));
        assert_eq!(session.state(), ConnectionState::Closed);
    }

    #[test]
    fn keepalive_is_retried_without_waiting_for_pong() {
        let now = Instant::now();
        let mut session = ManagedSession::new("127.0.0.1:19144".parse().unwrap(), 1200, now);
        session.state = ConnectionState::Connected;
        session.send_connected_ping(now);
        assert!(session.should_send_ping(now + Duration::from_millis(500)));
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

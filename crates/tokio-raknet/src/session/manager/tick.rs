use std::time::{Duration, Instant};

use bytes::Bytes;

use crate::protocol::packet::Packet;
use crate::protocol::{
    datagram::Datagram,
    encapsulated_packet::EncapsulatedPacket,
    packet::{ConnectedPing, RaknetPacket},
    reliability::Reliability,
    state::{DisconnectReason, RakPriority},
};
use crate::session::policy::VANILLA_PING_TICKS;

use super::{ConnectionState, ManagedSession, SessionError};

/// go-raknet `conn.go:startTicking`: an empty resend map permits closing after
/// one second; closing is forced after five.
const VANILLA_CLOSE_DRAINED: Duration = Duration::from_secs(1);
const VANILLA_CLOSE_LIMIT: Duration = Duration::from_secs(5);

/// `conn.go:Close` stores `time.Now().Unix()`: the closing start is truncated
/// to the wall-clock second, so `since` is overestimated by this fraction.
fn unix_subsecond() -> Duration {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| Duration::from_nanos(u64::from(elapsed.subsec_nanos())))
        .unwrap_or_default()
}

impl ManagedSession {
    /// Close the session. Optimized sends `DisconnectionNotification`
    /// immediately and drains; Vanilla follows go-raknet `Conn.Close`: drain
    /// the resend map on later ticks, then send a bare notification.
    pub fn close(&mut self, reason: DisconnectReason, now: Instant) -> Result<(), SessionError> {
        if !self.inner.transmission_policy().is_vanilla() {
            return self.send_disconnect(reason);
        }
        match self.state {
            ConnectionState::Closed => Err(SessionError::Closed),
            ConnectionState::Closing => Ok(()),
            _ => {
                self.last_disconnect_reason = Some(reason);
                self.begin_vanilla_close_at(now, unix_subsecond());
                Ok(())
            }
        }
    }

    /// Notify the peer and stop: the transport is ending because of a local
    /// failure. Vanilla writes go-raknet's bare notification immediately.
    pub fn disconnect_now(&mut self, reason: DisconnectReason) {
        if self.inner.transmission_policy().is_vanilla() {
            self.last_disconnect_reason.get_or_insert(reason);
            self.vanilla_close_immediately();
        } else {
            let _ = self.send_disconnect(reason);
        }
    }

    /// go-raknet `conn.go:receive` refreshes activity for every received
    /// datagram, including ones it fails to parse.
    pub(crate) fn record_activity(&mut self, now: Instant) {
        self.last_activity = now;
    }

    /// `conn.go:Close` with an explicit Unix sub-second offset (tests).
    pub(crate) fn begin_vanilla_close_at(&mut self, now: Instant, unix_subsecond: Duration) {
        if matches!(
            self.state,
            ConnectionState::Closing | ConnectionState::Closed
        ) {
            return;
        }
        self.state = ConnectionState::Closing;
        self.closing_since = Some(now.checked_sub(unix_subsecond).unwrap_or(now));
    }

    /// `conn.go:closeImmediately`: write a bare `DisconnectNotification`
    /// (ReliableOrdered, never retransmitted) and close.
    pub(crate) fn vanilla_close_immediately(&mut self) {
        if self.state == ConnectionState::Closed {
            return;
        }
        if let Ok(added) = self.inner.queue_encoded_packet(
            Bytes::from_static(&[crate::protocol::packet::DisconnectionNotification::ID]),
            Reliability::ReliableOrdered,
            0,
            RakPriority::Immediate,
        ) {
            self.queued_reliable_bytes = self.queued_reliable_bytes.saturating_add(added);
        }
        self.inner.clear_unacknowledged();
        self.state = ConnectionState::Closed;
        self.last_disconnect_reason
            .get_or_insert(DisconnectReason::Disconnected);
    }

    /// go-raknet `conn.go:startTicking`, one 100 ms tick.
    fn on_vanilla_tick(&mut self, now: Instant) -> Vec<Datagram> {
        // flushACKs, then checkResend on every third tick.
        let out = self.inner.on_tick(now);
        let tick = self.inner.vanilla_ticks();
        match self.state {
            ConnectionState::Closed => return out,
            ConnectionState::Closing => {
                let before = self.vanilla_acks_left;
                self.vanilla_acks_left = self.inner.unacknowledged();
                let since = now.saturating_duration_since(self.closing_since.unwrap_or(now));
                // Closes as soon as the resend map drains, once empty for more
                // than a second, and after five seconds regardless.
                let drained = self.vanilla_acks_left == 0;
                if (drained && (before != 0 || since > VANILLA_CLOSE_DRAINED))
                    || since > VANILLA_CLOSE_LIMIT
                {
                    self.vanilla_close_immediately();
                }
                return out;
            }
            _ => {}
        }
        if tick.is_multiple_of(VANILLA_PING_TICKS) {
            // Pinged in every state, including the online handshake.
            let ping = RaknetPacket::ConnectedPing(ConnectedPing {
                ping_time: Self::current_raknet_time(now),
            });
            if self
                .queue_control_packet(
                    ping,
                    Reliability::ReliableOrdered,
                    0,
                    RakPriority::Immediate,
                )
                .is_ok()
            {
                self.last_ping_sent = Some(now);
            }
            let rtt = self.inner.vanilla_rtt(now);
            let idle = now.saturating_duration_since(self.last_activity);
            if idle > self.config.session_timeout.saturating_add(rtt * 2) {
                self.last_disconnect_reason = Some(DisconnectReason::TimedOut);
                self.begin_vanilla_close_at(now, unix_subsecond());
            }
        }
        self.enforce_queue_limit();
        out
    }

    /// Run periodic maintenance and return any datagrams that should be sent.
    pub fn on_tick(&mut self, now: Instant) -> Vec<Datagram> {
        if self.inner.transmission_policy().is_vanilla() {
            return self.on_vanilla_tick(now);
        }
        let idle = now.saturating_duration_since(self.last_activity);
        if self.is_connected() {
            // go-raknet conn.go:Conn.startTicking: five seconds plus two RTTs.
            let timeout = self
                .config
                .session_timeout
                .saturating_add(self.inner.sliding.measured_rtt().saturating_mul(2));
            if idle >= timeout {
                let _ = self.send_disconnect(DisconnectReason::TimedOut);
                self.closing_since = Some(now);
                self.last_disconnect_reason = Some(DisconnectReason::TimedOut);
            } else if idle >= self.config.session_stale {
                self.state = ConnectionState::Stale;
            }
        } else if matches!(
            self.state,
            ConnectionState::Unconnected | ConnectionState::OnlineHandshake
        ) && now.saturating_duration_since(self.created_at)
            >= std::time::Duration::from_secs(10)
        {
            self.state = ConnectionState::Closed;
            self.last_disconnect_reason = Some(DisconnectReason::TimedOut);
        }

        // go-raknet conn.go:Conn.startTicking drains ACKs for up to five seconds;
        // an empty recovery queue permits closure after one second.
        if self.state == ConnectionState::Closing {
            let since = now.saturating_duration_since(*self.closing_since.get_or_insert(now));
            if since >= std::time::Duration::from_secs(5)
                || (since >= std::time::Duration::from_secs(1)
                    && self.inner.sent_datagrams.is_empty()
                    && self.inner.outgoing_heap.is_empty())
            {
                self.state = ConnectionState::Closed;
            }
        }

        if self.should_send_ping(now) {
            self.send_connected_ping(now);
        }

        self.enforce_queue_limit();

        self.inner.on_tick(now)
    }

    pub(crate) fn should_send_ping(&self, now: Instant) -> bool {
        if !self.is_connected() {
            return false;
        }
        match self.last_ping_sent {
            Some(last) => now.saturating_duration_since(last) >= self.config.ping_interval,
            None => true,
        }
    }

    pub(crate) fn send_connected_ping(&mut self, now: Instant) {
        let timestamp = Self::current_raknet_time(now);

        let pkt = RaknetPacket::ConnectedPing(ConnectedPing {
            ping_time: timestamp,
        });

        if self
            .queue_control_packet(pkt, Reliability::Unreliable, 0, RakPriority::Immediate)
            .is_ok()
        {
            self.last_ping_sent = Some(now);
            self.current_ping_nonce = Some(timestamp.0);
        }
    }

    pub(crate) fn enforce_queue_limit(&mut self) {
        if let Some(limit) = self.config.max_queued_reliable_bytes
            && self.queued_reliable_bytes > limit
        {
            let _ = self.send_disconnect(DisconnectReason::QueueTooLong);

            self.state = ConnectionState::Closed;
            self.last_disconnect_reason = Some(DisconnectReason::QueueTooLong);
        }
    }

    pub(crate) fn debit_reliable_bytes(&mut self, packets: &[EncapsulatedPacket]) {
        for pkt in packets {
            if pkt.header.reliability.is_reliable() {
                self.queued_reliable_bytes = self.queued_reliable_bytes.saturating_sub(pkt.size());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{
        ack::{AckNackPayload, SequenceRange},
        datagram::DatagramPayload,
        packet::{
            ConnectedPong, ConnectionRequest, ConnectionRequestAccepted, DetectLostConnection,
            DisconnectionNotification,
        },
        types::{RaknetTime, Sequence24},
    };
    use crate::session::{
        SessionTunables, TransmissionPolicy,
        manager::{SessionConfig, SessionRole},
    };

    const PEER: &str = "127.0.0.1:19132";

    fn managed(
        policy: TransmissionPolicy,
        role: SessionRole,
        peer: &str,
        now: Instant,
    ) -> ManagedSession {
        ManagedSession::with_config(
            peer.parse().unwrap(),
            1492,
            now,
            SessionConfig {
                role,
                guid: (-7i64) as u64,
                session: SessionTunables {
                    transmission: policy,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
    }

    /// Every data frame the session would write now.
    fn sent_frames(ms: &mut ManagedSession, now: Instant) -> Vec<EncapsulatedPacket> {
        std::iter::from_fn(|| ms.build_datagram(now))
            .filter_map(|datagram| match datagram.payload {
                DatagramPayload::EncapsulatedPackets(frames) => Some(frames),
                _ => None,
            })
            .flatten()
            .collect()
    }

    fn decode(frame: &EncapsulatedPacket) -> RaknetPacket {
        RaknetPacket::decode(&mut frame.payload.clone()).unwrap()
    }

    fn cra(accepted: u64) -> RaknetPacket {
        RaknetPacket::ConnectionRequestAccepted(ConnectionRequestAccepted {
            address: "127.0.0.1:50000".parse().unwrap(),
            system_index: 0,
            system_addresses: vec!["0.0.0.0:0".parse().unwrap(); 20],
            request_timestamp: RaknetTime(1),
            accepted_timestamp: RaknetTime(accepted),
        })
    }

    fn connected_client(policy: TransmissionPolicy, now: Instant) -> ManagedSession {
        let mut ms = managed(policy, SessionRole::Client, PEER, now);
        ms.start_client_handshake(9, now, false).unwrap();
        ms.handle_control_packet(&cra(5), now);
        sent_frames(&mut ms, now);
        assert!(ms.is_connected());
        ms
    }

    fn ack_all(ms: &mut ManagedSession, now: Instant) {
        ms.inner.handle_ack_payload(AckNackPayload {
            ranges: vec![SequenceRange {
                start: Sequence24::new(0),
                end: Sequence24::new(1000),
            }],
        });
        ms.inner.process_incoming_acks_naks(now);
    }

    /// HS-4: go-raknet writes `ConnectionRequest{Secure: false}` ReliableOrdered.
    #[test]
    fn vanilla_connection_request_is_reliable_ordered_and_insecure() {
        let now = Instant::now();
        for (policy, reliability) in [
            (TransmissionPolicy::Vanilla, Reliability::ReliableOrdered),
            (TransmissionPolicy::Optimized, Reliability::Reliable),
        ] {
            let mut ms = managed(policy, SessionRole::Client, PEER, now);
            ms.start_client_handshake(9, now, true).unwrap();
            let frames = sent_frames(&mut ms, now);
            assert_eq!(frames[0].header.reliability, reliability);
            let RaknetPacket::ConnectionRequest(request) = decode(&frames[0]) else {
                panic!("expected ConnectionRequest");
            };
            assert_eq!(request.secure, !policy.is_vanilla());
            assert_eq!(request.client_guid, (-7i64) as u64);
        }
    }

    /// HS-7/HS-3: go-raknet `NewIncomingConnection` bytes: the server address,
    /// 20 zero addresses, `PingTime = CRA.PongTime`, `PongTime = now`.
    #[test]
    fn vanilla_new_incoming_connection_matches_go_bytes() {
        let now = Instant::now();
        for (peer, address) in [
            (PEER, vec![0x04, 0x80, 0xff, 0xff, 0xfe, 0x4a, 0xbc]),
            (
                "[::ffff:127.0.0.1]:19132",
                vec![0x04, 0x80, 0xff, 0xff, 0xfe, 0x4a, 0xbc],
            ),
        ] {
            let mut ms = managed(TransmissionPolicy::Vanilla, SessionRole::Client, peer, now);
            ms.start_client_handshake(9, now, false).unwrap();
            sent_frames(&mut ms, now);
            ms.handle_control_packet(&cra(0x1234), now);
            let frames = sent_frames(&mut ms, now);
            assert_eq!(frames.len(), 1);
            assert_eq!(frames[0].header.reliability, Reliability::ReliableOrdered);
            let bytes = &frames[0].payload;
            let mut expected = vec![0x13];
            expected.extend_from_slice(&address);
            for _ in 0..20 {
                expected.extend_from_slice(&[0x04, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00]);
            }
            expected.extend_from_slice(&0x1234u64.to_be_bytes());
            let pong = ManagedSession::current_raknet_time(now).0;
            expected.extend_from_slice(&pong.to_be_bytes());
            assert_eq!(&bytes[..], &expected[..]);
        }
        // IPv6 servers use lunar's address layout.
        let mut ms = managed(
            TransmissionPolicy::Vanilla,
            SessionRole::Client,
            "[2001:db8::1]:19132",
            now,
        );
        ms.start_client_handshake(9, now, false).unwrap();
        sent_frames(&mut ms, now);
        ms.handle_control_packet(&cra(1), now);
        let frames = sent_frames(&mut ms, now);
        assert_eq!(&frames[0].payload[1..8], &[6, 0, 0, 0, 23, 0x4a, 0xbc]);
    }

    /// HS-6 (bug, both policies): a second CRA neither sends another NIC nor
    /// moves a closing session back to `Connected`.
    #[test]
    fn duplicate_cra_does_not_resend_nic_or_reopen_closing_session() {
        let now = Instant::now();
        for policy in [TransmissionPolicy::Vanilla, TransmissionPolicy::Optimized] {
            let mut ms = connected_client(policy, now);
            ms.handle_control_packet(&cra(6), now);
            assert!(sent_frames(&mut ms, now).is_empty(), "{policy:?}");
            ms.close(DisconnectReason::Disconnected, now).unwrap();
            ms.handle_control_packet(&cra(7), now);
            assert_eq!(ms.state(), ConnectionState::Closing, "{policy:?}");
        }
    }

    /// HS-11 (bug, both policies): the CRA is ReliableOrdered and a repeated
    /// request keeps an established session established.
    #[test]
    fn connection_request_accepted_is_reliable_and_state_preserving() {
        let now = Instant::now();
        let request = RaknetPacket::ConnectionRequest(ConnectionRequest {
            client_guid: (-3i64) as u64,
            timestamp: RaknetTime(11),
            secure: false,
        });
        for (policy, addresses, index) in [
            (TransmissionPolicy::Vanilla, 20, 0),
            (TransmissionPolicy::Optimized, 10, 47),
        ] {
            let mut ms = managed(policy, SessionRole::Server, PEER, now);
            ms.handle_control_packet(&request, now);
            let frames = sent_frames(&mut ms, now);
            assert_eq!(frames[0].header.reliability, Reliability::ReliableOrdered);
            assert!(ms.inner.unacknowledged() > 0, "a lost CRA is resent");
            let RaknetPacket::ConnectionRequestAccepted(accepted) = decode(&frames[0]) else {
                panic!("expected ConnectionRequestAccepted");
            };
            assert_eq!(accepted.system_addresses.len(), addresses);
            assert_eq!(accepted.system_index, index);
            assert_eq!(accepted.request_timestamp.0, 11);
            assert_eq!(ms.state(), ConnectionState::OnlineHandshake);

            ms.state = ConnectionState::Connected;
            ms.handle_control_packet(&request, now);
            assert_eq!(ms.state(), ConnectionState::Connected, "{policy:?}");
        }
    }

    /// HS-8/KA-1/REL-1: pings every fifth 100 ms tick from session creation,
    /// ReliableOrdered, also during the online handshake.
    #[test]
    fn vanilla_pings_every_fifth_tick_from_session_creation() {
        let start = Instant::now();
        let mut ms = managed(
            TransmissionPolicy::Vanilla,
            SessionRole::Client,
            PEER,
            start,
        );
        ms.start_client_handshake(9, start, false).unwrap();
        sent_frames(&mut ms, start);
        let mut ping_ticks = Vec::new();
        for n in 1..=10u32 {
            let now = start + VANILLA_TICK * n;
            ms.on_tick(now);
            for frame in sent_frames(&mut ms, now) {
                if let RaknetPacket::ConnectedPing(ping) = decode(&frame) {
                    assert_eq!(frame.header.reliability, Reliability::ReliableOrdered);
                    assert_eq!(ping.ping_time.0, ManagedSession::current_raknet_time(now).0);
                    ping_ticks.push(n);
                }
            }
            ack_all(&mut ms, now);
        }
        assert_eq!(ms.state(), ConnectionState::OnlineHandshake);
        assert_eq!(ping_ticks, [5, 10]);
    }

    const VANILLA_TICK: Duration = Duration::from_millis(100);

    /// KA-2: on ping ticks, `idle > 5 s + 2 × rtt` (strict) starts closing.
    #[test]
    fn vanilla_inactivity_is_checked_on_ping_ticks_with_strict_bound() {
        let start = Instant::now();
        let mut ms = connected_client(TransmissionPolicy::Vanilla, start);
        ack_all(&mut ms, start);
        // No RTT samples remain in the window by then: 5 s + 2 × 50 ms.
        let samples_expire = start + Duration::from_secs(6);
        assert_eq!(
            ms.inner.vanilla_rtt(samples_expire),
            Duration::from_millis(50)
        );
        let mut closed_at = None;
        for n in 1..=60u32 {
            ms.on_tick(start + VANILLA_TICK * n);
            if ms.state() == ConnectionState::Closing {
                closed_at = Some(n);
                break;
            }
        }
        // Tick 51 (5.1 s) is not a ping tick; tick 55 is the first check
        // with idle > 5.1 s.
        assert_eq!(closed_at, Some(55));
        assert!(matches!(
            ms.last_disconnect_reason(),
            Some(DisconnectReason::TimedOut)
        ));
    }

    /// KA-5/KA-7: close waits for the resend map (≤ 5 s), then writes a bare
    /// ReliableOrdered `DisconnectNotification` and closes.
    #[test]
    fn vanilla_close_drains_then_sends_bare_disconnect() {
        let start = Instant::now();
        let bare_disconnect = |ms: &mut ManagedSession, now: Instant| {
            let frames = sent_frames(ms, now);
            frames.len() == 1
                && frames[0].payload.as_ref() == [0x15]
                && frames[0].header.reliability == Reliability::ReliableOrdered
        };

        // Unacked data: closes on the tick after its ACK drains the map.
        let mut ms = connected_client(TransmissionPolicy::Vanilla, start);
        ms.queue_app_buffer(
            bytes::Bytes::from_static(b"\xfedata"),
            Reliability::ReliableOrdered,
            0,
            RakPriority::Normal,
        )
        .unwrap();
        sent_frames(&mut ms, start);
        ms.begin_vanilla_close_at(start, Duration::ZERO);
        ms.on_tick(start + VANILLA_TICK);
        assert_eq!(ms.state(), ConnectionState::Closing);
        assert!(sent_frames(&mut ms, start + VANILLA_TICK).is_empty());
        ack_all(&mut ms, start + VANILLA_TICK);
        ms.on_tick(start + VANILLA_TICK * 2);
        assert_eq!(ms.state(), ConnectionState::Closed);
        assert!(bare_disconnect(&mut ms, start + VANILLA_TICK * 2));

        // Empty map: strictly more than one second, measured from the Unix
        // second the close started in.
        let mut ms = connected_client(TransmissionPolicy::Vanilla, start);
        ack_all(&mut ms, start);
        ms.begin_vanilla_close_at(start, Duration::from_millis(950));
        ms.on_tick(start + Duration::from_millis(50));
        assert_eq!(ms.state(), ConnectionState::Closing);
        ms.on_tick(start + Duration::from_millis(51));
        assert_eq!(ms.state(), ConnectionState::Closed);
        assert!(bare_disconnect(&mut ms, start + Duration::from_millis(51)));

        // Never acknowledged: forced after five seconds.
        let mut ms = connected_client(TransmissionPolicy::Vanilla, start);
        ms.begin_vanilla_close_at(start, Duration::ZERO);
        let mut closed_at = None;
        for n in 1..=60u32 {
            ms.on_tick(start + VANILLA_TICK * n);
            if ms.state() == ConnectionState::Closed {
                closed_at = Some(n);
                break;
            }
        }
        assert_eq!(closed_at, Some(51));
    }

    /// KA-6: a received notification is echoed before closing (Vanilla).
    #[test]
    fn vanilla_remote_disconnect_is_echoed() {
        let now = Instant::now();
        let notification = RaknetPacket::DisconnectionNotification(DisconnectionNotification {
            reason: DisconnectReason::ClosedByRemotePeer,
        });
        let mut ms = connected_client(TransmissionPolicy::Vanilla, now);
        ms.handle_control_packet(&notification, now);
        assert_eq!(ms.state(), ConnectionState::Closed);
        let frames = sent_frames(&mut ms, now);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].payload.as_ref(), [0x15]);

        let mut ms = connected_client(TransmissionPolicy::Optimized, now);
        ms.handle_control_packet(&notification, now);
        assert_eq!(ms.state(), ConnectionState::Closed);
        assert!(sent_frames(&mut ms, now).is_empty());
    }

    /// HS-9: `DetectLostConnections` is answered with a ReliableOrdered ping.
    #[test]
    fn vanilla_detect_lost_connections_is_answered_with_ping() {
        let now = Instant::now();
        let detect = RaknetPacket::DetectLostConnection(DetectLostConnection);
        let mut ms = connected_client(TransmissionPolicy::Vanilla, now);
        ms.handle_control_packet(&detect, now);
        let frames = sent_frames(&mut ms, now);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].header.reliability, Reliability::ReliableOrdered);
        assert!(matches!(decode(&frames[0]), RaknetPacket::ConnectedPing(_)));

        let mut ms = connected_client(TransmissionPolicy::Optimized, now);
        ms.handle_control_packet(&detect, now);
        assert!(sent_frames(&mut ms, now).is_empty());
    }

    /// KA-4: pings are answered with Unreliable pongs echoing the ping time.
    #[test]
    fn connected_ping_is_answered_with_unreliable_pong() {
        let now = Instant::now();
        for policy in [TransmissionPolicy::Vanilla, TransmissionPolicy::Optimized] {
            let mut ms = connected_client(policy, now);
            ms.handle_control_packet(
                &RaknetPacket::ConnectedPing(ConnectedPing {
                    ping_time: RaknetTime(77),
                }),
                now,
            );
            let frames = sent_frames(&mut ms, now);
            assert_eq!(frames[0].header.reliability, Reliability::Unreliable);
            let RaknetPacket::ConnectedPong(pong) = decode(&frames[0]) else {
                panic!("expected ConnectedPong");
            };
            assert_eq!(pong.ping_time.0, 77);
            assert_eq!(pong.pong_time.0, ManagedSession::current_raknet_time(now).0);
        }
    }

    /// KA-3: pongs (even with a future ping time) never feed the RTT.
    #[test]
    fn pong_never_affects_rtt() {
        let now = Instant::now();
        let mut ms = connected_client(TransmissionPolicy::Vanilla, now);
        ack_all(&mut ms, now);
        let before = ms.inner.vanilla_rtt(now);
        ms.handle_control_packet(
            &RaknetPacket::ConnectedPong(ConnectedPong {
                ping_time: RaknetTime(u64::MAX),
                pong_time: RaknetTime(0),
            }),
            now,
        );
        assert_eq!(ms.inner.vanilla_rtt(now), before);
        assert!(ms.is_connected());
    }
}

use std::time::Instant;

use bytes::Bytes;

use crate::protocol::packet::Packet;
use crate::protocol::{
    datagram::Datagram,
    encapsulated_packet::EncapsulatedPacket,
    packet::{ConnectedPing, DisconnectionNotification, RaknetPacket},
    reliability::Reliability,
    state::{DisconnectReason, RakPriority},
};
use crate::session::policy::{
    VANILLA_HANDSHAKE_TIMEOUT, VANILLA_PING_INTERVAL, VANILLA_TIMEOUT,
};

use super::{ConnectionState, ManagedSession, SessionError, VanillaClose};

impl ManagedSession {
    /// Close the session. Optimized sends `DisconnectionNotification`
    /// immediately and drains. Vanilla follows Mojang's
    /// `CloseConnection(addr, true, 0, LOW_PRIORITY)`: a bare ReliableOrdered
    /// notification, then the connection closes once nothing is queued or
    /// unacknowledged (`DISCONNECT_ASAP`) or the dead-connection timeout hits.
    pub fn close(&mut self, reason: DisconnectReason, now: Instant) -> Result<(), SessionError> {
        if !self.inner.transmission_policy().is_vanilla() {
            return self.send_disconnect(reason);
        }
        match self.state {
            ConnectionState::Closed => Err(SessionError::Closed),
            ConnectionState::Closing => Ok(()),
            _ => {
                self.queue_vanilla_disconnect(RakPriority::Low)?;
                self.state = ConnectionState::Closing;
                self.closing_since = Some(now);
                self.last_disconnect_reason = Some(reason);
                self.vanilla_close = Some(VanillaClose::Asap);
                Ok(())
            }
        }
    }

    /// Notify the peer and stop: the transport is ending because of a local
    /// failure. Vanilla queues Mojang's bare notification with `IMMEDIATE`
    /// priority for the caller's final flush.
    pub fn disconnect_now(&mut self, reason: DisconnectReason) {
        if self.inner.transmission_policy().is_vanilla() {
            if self.state == ConnectionState::Closed {
                return;
            }
            self.last_disconnect_reason.get_or_insert(reason);
            let _ = self.queue_vanilla_disconnect(RakPriority::Immediate);
            self.state = ConnectionState::Closed;
        } else {
            let _ = self.send_disconnect(reason);
        }
    }

    /// Mojang `NotifyAndFlagForShutdown`: the one-byte `0x15`, ReliableOrdered
    /// on channel 0.
    fn queue_vanilla_disconnect(&mut self, priority: RakPriority) -> Result<(), SessionError> {
        let added = self.inner.queue_encoded_packet(
            Bytes::from_static(&[DisconnectionNotification::ID]),
            Reliability::ReliableOrdered,
            0,
            priority,
        )?;
        self.queued_reliable_bytes = self.queued_reliable_bytes.saturating_add(added);
        Ok(())
    }

    /// Test hook: a datagram arrived from the peer at `now`.
    #[cfg(test)]
    pub(crate) fn record_activity(&mut self, now: Instant) {
        self.last_activity = now;
    }

    /// Mojang `HandleSocketReceiveFromConnectedPlayer` refreshes
    /// `timeLastDatagramArrived` for every datagram of 3+ bytes handed to
    /// the reliability layer (the `0x80` bit set), before validating it;
    /// offline messages from the peer never reach it (lens RL_006:143-152).
    pub(crate) fn record_datagram_activity(&mut self, datagram: &[u8], now: Instant) {
        if datagram.len() >= 3 && datagram[0] & 0x80 != 0 {
            self.last_activity = now;
        }
    }

    #[cfg(test)]
    pub(crate) fn last_activity(&self) -> Instant {
        self.last_activity
    }

    /// One Mojang `RakPeer::RunUpdateCycle` pass for this connection: the
    /// reliable keepalive, the occasional ping, the dead-connection and
    /// handshake timeouts, one `ReliabilityLayer::Update`, then the close
    /// conditions of the disconnect modes (lens RP_113:1528-1597, RL_010).
    fn on_vanilla_tick(&mut self, now: Instant) -> Vec<Datagram> {
        if self.state == ConnectionState::Closed {
            return Vec::new();
        }
        // RL Update: a connection with unacknowledged reliable messages and
        // no datagram for `timeoutTime` is dead: closed silently (no
        // notification; the application sees a timeout).
        if self.inner.vanilla_messages_in_flight() > 0
            && now.saturating_duration_since(self.last_activity) > VANILLA_TIMEOUT
        {
            self.state = ConnectionState::Closed;
            self.last_disconnect_reason
                .get_or_insert(DisconnectReason::TimedOut);
            return Vec::new();
        }
        if matches!(
            self.state,
            ConnectionState::Unconnected | ConnectionState::OnlineHandshake
        ) && now.saturating_duration_since(self.created_at) > VANILLA_HANDSHAKE_TIMEOUT
        {
            self.state = ConnectionState::Closed;
            self.last_disconnect_reason = Some(DisconnectReason::TimedOut);
            return Vec::new();
        }
        if self.state == ConnectionState::Connected {
            // Reliable keepalive: nothing unacknowledged and no keepalive for
            // half the timeout (`lastReliableSend` is set only here and at
            // connection creation).
            if now.saturating_duration_since(self.vanilla_last_reliable_send) > VANILLA_TIMEOUT / 2
                && self.inner.vanilla_messages_in_flight() == 0
            {
                self.send_vanilla_ping(now, Reliability::Reliable);
                self.vanilla_last_reliable_send = now;
            }
            // Occasional ping (Bedrock enables it): unreliable, every 5 s,
            // the first on the first update after connecting.
            if self.vanilla_next_ping.is_none_or(|next| next < now) {
                self.vanilla_next_ping = Some(now + VANILLA_PING_INTERVAL);
                self.send_vanilla_ping(now, Reliability::Unreliable);
            }
        }
        let out = self.inner.on_tick(now);
        self.credit_vanilla_acks();
        match self.vanilla_close {
            Some(VanillaClose::Asap)
                if self.inner.outgoing_heap.is_empty()
                    && self.inner.vanilla_messages_in_flight() == 0 =>
            {
                self.state = ConnectionState::Closed;
            }
            Some(VanillaClose::OnNoAck) if !self.inner.vanilla_acks_pending() => {
                self.state = ConnectionState::Closed;
            }
            _ => {}
        }
        self.enforce_queue_limit();
        out
    }

    /// `PingInternal`: `ConnectedPing` with the current time, `IMMEDIATE`.
    pub(super) fn send_vanilla_ping(&mut self, now: Instant, reliability: Reliability) {
        let ping = RaknetPacket::ConnectedPing(ConnectedPing {
            ping_time: Self::current_raknet_time(now),
        });
        if self
            .queue_control_packet(ping, reliability, 0, RakPriority::Immediate)
            .is_ok()
        {
            self.last_ping_sent = Some(now);
        }
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

    /// Vanilla: release the reliable bytes the peer has ACKed. Sends never
    /// debit, so `queued_reliable_bytes` covers queued and unACKed data.
    pub(crate) fn credit_vanilla_acks(&mut self) {
        let acked = self.inner.take_vanilla_acked_bytes();
        self.queued_reliable_bytes = self.queued_reliable_bytes.saturating_sub(acked);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::protocol::{
        ack::{AckNackPayload, SequenceRange},
        datagram::DatagramPayload,
        packet::{
            ConnectedPong, ConnectionRequest, ConnectionRequestAccepted, DetectLostConnection,
            NewIncomingConnection,
        },
        types::{RaknetTime, Sequence24},
    };
    use crate::session::{
        SessionTunables, TransmissionPolicy,
        manager::{SessionConfig, SessionRole},
        policy::VANILLA_TICK_INTERVAL,
    };

    const PEER: &str = "127.0.0.1:19132";
    const TICK: Duration = VANILLA_TICK_INTERVAL;
    /// A later instant than the previous update's (one update runs per
    /// instant, as in a transport where every wake reads the clock anew).
    const EPSILON: Duration = Duration::from_micros(1);

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

    /// Frames written by a maintenance tick and the flush after it.
    fn tick_frames(ms: &mut ManagedSession, now: Instant) -> Vec<EncapsulatedPacket> {
        let mut frames: Vec<EncapsulatedPacket> = ms
            .on_tick(now)
            .into_iter()
            .filter_map(|datagram| match datagram.payload {
                DatagramPayload::EncapsulatedPackets(frames) => Some(frames),
                _ => None,
            })
            .flatten()
            .collect();
        frames.extend(sent_frames(ms, now));
        frames
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

    fn in_flight(ms: &ManagedSession) -> usize {
        if ms.transmission_policy().is_vanilla() {
            ms.inner.vanilla_messages_in_flight()
        } else {
            ms.inner.sent_datagrams.len()
        }
    }

    fn pings(frames: &[EncapsulatedPacket]) -> Vec<Reliability> {
        frames
            .iter()
            .filter(|frame| matches!(decode(frame), RaknetPacket::ConnectedPing(_)))
            .map(|frame| frame.header.reliability)
            .collect()
    }

    /// HS-4: Mojang sends `ConnectionRequest` RELIABLE with the security
    /// byte 0 (lens RN_006:2647); Optimized keeps Reliable and Reply2's flag.
    #[test]
    fn connection_request_is_reliable_and_vanilla_is_insecure() {
        let now = Instant::now();
        for policy in [TransmissionPolicy::Vanilla, TransmissionPolicy::Optimized] {
            let mut ms = managed(policy, SessionRole::Client, PEER, now);
            ms.start_client_handshake(9, now, true).unwrap();
            let frames = sent_frames(&mut ms, now);
            assert_eq!(frames[0].header.reliability, Reliability::Reliable);
            let RaknetPacket::ConnectionRequest(request) = decode(&frames[0]) else {
                panic!("expected ConnectionRequest");
            };
            assert_eq!(request.secure, !policy.is_vanilla());
            assert_eq!(request.client_guid, (-7i64) as u64);
        }
    }

    /// HS-7/HS-8/KA-1: the Mojang client answers the CRA with a
    /// NewIncomingConnection (server address as dialed, 20 unassigned
    /// addresses in the standard layout, `CRA.PongTime`, now), then pings
    /// unreliably; the first update pings again (lens RP_113:2633-2737).
    #[test]
    fn vanilla_new_incoming_connection_bytes_and_connect_pings() {
        let now = Instant::now();
        for (peer, address) in [
            (PEER, vec![0x04, 0x80, 0xff, 0xff, 0xfe, 0x4a, 0xbc]),
            (
                "[2001:db8::1]:19132",
                [
                    vec![6, 23, 0, 0x4a, 0xbc, 0, 0, 0, 0],
                    vec![0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
                    vec![0, 0, 0, 0],
                ]
                .concat(),
            ),
        ] {
            let mut ms = managed(TransmissionPolicy::Vanilla, SessionRole::Client, peer, now);
            ms.start_client_handshake(9, now, false).unwrap();
            sent_frames(&mut ms, now);
            // The CRA arrives later than the request left: its update runs at
            // a new instant and writes the NIC, the NIC's ping and the
            // update's first occasional ping together.
            let arrived = now + EPSILON;
            ms.handle_control_packet(&cra(0x1234), arrived);
            let frames = tick_frames(&mut ms, arrived);
            assert_eq!(frames[0].header.reliability, Reliability::ReliableOrdered);
            let mut expected = vec![NewIncomingConnection::ID];
            expected.extend_from_slice(&address);
            for _ in 0..20 {
                expected.extend_from_slice(&[0x04, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00]);
            }
            expected.extend_from_slice(&0x1234u64.to_be_bytes());
            let pong = ManagedSession::current_raknet_time(arrived).0;
            expected.extend_from_slice(&pong.to_be_bytes());
            assert_eq!(&frames[0].payload[..], &expected[..], "{peer}");
            assert_eq!(
                pings(&frames[1..]),
                vec![Reliability::Unreliable, Reliability::Unreliable]
            );
            assert!(pings(&tick_frames(&mut ms, arrived + TICK)).is_empty());
        }
    }

    /// HS-6 (bug, both policies): a second CRA neither sends another NIC nor
    /// moves a closing session back to `Connected`.
    #[test]
    fn duplicate_cra_does_not_resend_nic_or_reopen_closing_session() {
        let now = Instant::now();
        for policy in [TransmissionPolicy::Vanilla, TransmissionPolicy::Optimized] {
            let mut ms = connected_client(policy, now);
            ms.handle_control_packet(&cra(6), now);
            assert!(
                !sent_frames(&mut ms, now)
                    .iter()
                    .any(|f| matches!(decode(f), RaknetPacket::NewIncomingConnection(_))),
                "{policy:?}"
            );
            ms.close(DisconnectReason::Disconnected, now).unwrap();
            ms.handle_control_packet(&cra(7), now);
            assert_eq!(ms.state(), ConnectionState::Closing, "{policy:?}");
        }
    }

    /// HS-11: the CRA is ReliableOrdered and a repeated request keeps an
    /// established session established. Vanilla writes BDS's 20 addresses
    /// and system index 0; Optimized 10 addresses and index 47.
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
            assert!(in_flight(&ms) > 0, "a lost CRA is resent");
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

    /// HS-11/HS-3: BDS's CRA bytes (capture `baseline.jsonl`): client
    /// address in the standard `sockaddr` layout, index 0, 20 addresses,
    /// request time echoed, then the server time.
    #[test]
    fn vanilla_connection_request_accepted_matches_bds_layout() {
        let now = Instant::now();
        let request = RaknetPacket::ConnectionRequest(ConnectionRequest {
            client_guid: (-3i64) as u64,
            timestamp: RaknetTime(11),
            secure: false,
        });
        let mut ms = managed(TransmissionPolicy::Vanilla, SessionRole::Server, PEER, now);
        ms.handle_control_packet(&request, now);
        let frames = sent_frames(&mut ms, now);
        let mut expected = vec![0x10, 0x04, 0x80, 0xff, 0xff, 0xfe, 0x4a, 0xbc, 0, 0];
        for _ in 0..20 {
            expected.extend_from_slice(&[0x04, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00]);
        }
        expected.extend_from_slice(&11u64.to_be_bytes());
        let pong = ManagedSession::current_raknet_time(now).0;
        expected.extend_from_slice(&pong.to_be_bytes());
        assert_eq!(&frames[0].payload[..], &expected[..]);
    }

    /// KA-1/HS-8: the BDS server pings unreliably when the NIC arrives and
    /// on its next update (two pings in the capture), then every 5 s.
    #[test]
    fn vanilla_server_pings_twice_on_connect_then_every_five_seconds() {
        let start = Instant::now();
        let mut ms = managed(TransmissionPolicy::Vanilla, SessionRole::Server, PEER, start);
        ms.handle_control_packet(
            &RaknetPacket::ConnectionRequest(ConnectionRequest {
                client_guid: 3,
                timestamp: RaknetTime(1),
                secure: false,
            }),
            start,
        );
        sent_frames(&mut ms, start);
        let arrived = start + EPSILON;
        ms.handle_control_packet(
            &RaknetPacket::NewIncomingConnection(NewIncomingConnection {
                server_address: PEER.parse().unwrap(),
                system_addresses: vec!["0.0.0.0:0".parse().unwrap(); 20],
                request_timestamp: RaknetTime(1),
                accepted_timestamp: RaknetTime(2),
            }),
            arrived,
        );
        assert!(ms.is_connected());
        let mut times = Vec::new();
        let mut now = arrived;
        for _ in 0..1200 {
            for reliability in pings(&tick_frames(&mut ms, now)) {
                if reliability == Reliability::Unreliable {
                    times.push(now.duration_since(arrived).as_millis());
                }
            }
            ms.record_activity(now);
            ack_all(&mut ms, now);
            now += TICK;
        }
        // The second ping at 0 ms is the first update's occasional ping;
        // `nextPingTime < now` is strict, so the next one is at 5010 ms.
        assert_eq!(times, vec![0, 0, 5010, 10020]);
    }

    /// KA-2: with nothing unacknowledged for half the timeout, a reliable
    /// ping keeps the connection alive (lens RP_113:1528-1554).
    #[test]
    fn vanilla_reliable_keepalive_after_five_idle_seconds() {
        let start = Instant::now();
        let mut ms = connected_client(TransmissionPolicy::Vanilla, start);
        ack_all(&mut ms, start);
        let mut reliable = Vec::new();
        let mut now = start;
        while now < start + Duration::from_secs(11) {
            now += TICK;
            if pings(&tick_frames(&mut ms, now)).contains(&Reliability::Reliable) {
                reliable.push(now.duration_since(start).as_millis());
            }
            ms.record_activity(now);
            ack_all(&mut ms, now);
        }
        assert_eq!(reliable, vec![5010, 10020]);
    }

    /// KA-2: unacknowledged data and no datagram for more than 10 s: the
    /// connection is dropped silently (BDS capture `client-silence`).
    #[test]
    fn vanilla_dead_connection_closes_silently_after_ten_seconds() {
        let start = Instant::now();
        let mut ms = connected_client(TransmissionPolicy::Vanilla, start);
        assert!(ms.inner.vanilla_messages_in_flight() > 0);
        let mut now = start;
        let mut last_sent = start;
        while ms.state() != ConnectionState::Closed {
            now += TICK;
            if !tick_frames(&mut ms, now).is_empty() {
                last_sent = now;
            }
        }
        assert_eq!(now.duration_since(start), Duration::from_millis(10_010));
        assert!(last_sent < now, "nothing is sent when the connection dies");
        assert!(matches!(
            ms.last_disconnect_reason(),
            Some(DisconnectReason::TimedOut)
        ));
    }

    /// KA-2: only connected datagrams (0x80 bit, 3+ bytes, even malformed)
    /// refresh `timeLastDatagramArrived`; offline messages from the peer do
    /// not keep a dead connection alive.
    #[test]
    fn vanilla_activity_counts_connected_datagrams_only() {
        let start = Instant::now();
        let mut ms = connected_client(TransmissionPolicy::Vanilla, start);
        let later = start + Duration::from_secs(1);
        for offline in [&[0x1c, 0, 0, 0][..], &[0x84, 0][..]] {
            ms.record_datagram_activity(offline, later);
            assert_eq!(ms.last_activity(), start);
        }
        ms.record_datagram_activity(&[0x84, 0xff, 0xff], later);
        assert_eq!(ms.last_activity(), later);
    }

    /// Mojang drops connections still in the online handshake 10 s after
    /// creation.
    #[test]
    fn vanilla_handshake_times_out_after_ten_seconds() {
        let start = Instant::now();
        let mut ms = managed(TransmissionPolicy::Vanilla, SessionRole::Client, PEER, start);
        ms.start_client_handshake(9, start, false).unwrap();
        ms.on_tick(start + Duration::from_secs(10));
        assert_eq!(ms.state(), ConnectionState::OnlineHandshake);
        ms.on_tick(start + Duration::from_millis(10_001));
        assert_eq!(ms.state(), ConnectionState::Closed);
    }

    /// REL-12/RAKNET-01: reliable bytes stay charged to
    /// `max_queued_reliable_bytes` until ACKed: a peer that stays active but
    /// never ACKs trips the cap; regular ACKs keep traffic open.
    #[test]
    fn vanilla_queue_limit_counts_unacked_reliable_bytes() {
        const LIMIT: usize = 16 * 1024;
        let start = Instant::now();
        let write = |ms: &mut ManagedSession| {
            for _ in 0..4 {
                ms.queue_app_buffer(
                    Bytes::from(vec![0xfe; 1000]),
                    Reliability::ReliableOrdered,
                    0,
                    RakPriority::Normal,
                )
                .unwrap();
            }
        };

        let mut silent = connected_client(TransmissionPolicy::Vanilla, start);
        silent.config.max_queued_reliable_bytes = Some(LIMIT);
        let mut closed_at = None;
        for n in 1..=20u32 {
            let now = start + TICK * n;
            write(&mut silent);
            silent.record_activity(now);
            tick_frames(&mut silent, now);
            if silent.state() == ConnectionState::Closed {
                closed_at = Some(n);
                break;
            }
        }
        assert_eq!(closed_at, Some(5));
        assert!(matches!(
            silent.last_disconnect_reason(),
            Some(DisconnectReason::QueueTooLong)
        ));

        let mut acked = connected_client(TransmissionPolicy::Vanilla, start);
        acked.config.max_queued_reliable_bytes = Some(LIMIT);
        for n in 1..=50u32 {
            let now = start + TICK * n;
            write(&mut acked);
            tick_frames(&mut acked, now);
            acked.record_activity(now);
            ack_all(&mut acked, now);
            assert!(acked.is_connected(), "tick {n}");
        }
    }

    /// KA-5: close queues the bare ReliableOrdered `0x15` and closes once it
    /// (and everything before it) is acknowledged.
    #[test]
    fn vanilla_close_sends_bare_disconnect_and_closes_when_acked() {
        let start = Instant::now();
        let mut ms = connected_client(TransmissionPolicy::Vanilla, start);
        ack_all(&mut ms, start);
        ms.close(DisconnectReason::Disconnected, start).unwrap();
        let now = start + TICK;
        let frames = tick_frames(&mut ms, now);
        let notification: Vec<&EncapsulatedPacket> = frames
            .iter()
            .filter(|frame| frame.payload.as_ref() == [0x15])
            .collect();
        assert_eq!(notification.len(), 1);
        assert_eq!(
            notification[0].header.reliability,
            Reliability::ReliableOrdered
        );
        assert_eq!(ms.state(), ConnectionState::Closing);
        ms.record_activity(now);
        tick_frames(&mut ms, now + TICK);
        assert_eq!(ms.state(), ConnectionState::Closing, "not acknowledged yet");
        ack_all(&mut ms, now + TICK);
        tick_frames(&mut ms, now + TICK * 2);
        assert_eq!(ms.state(), ConnectionState::Closed);
    }

    /// KA-6: a received notification is ACKed, never echoed, and the
    /// connection closes once that ACK is out (BDS capture `baseline`).
    #[test]
    fn vanilla_remote_disconnect_is_acked_not_echoed() {
        let start = Instant::now();
        let mut ms = connected_client(TransmissionPolicy::Vanilla, start);
        ack_all(&mut ms, start);
        // An RTT sample, so the ACK waits for the 10 ms SYN.
        ms.handle_datagram(
            Datagram {
                header: crate::protocol::types::DatagramHeader {
                    flags: crate::protocol::constants::DatagramFlags::VALID,
                    sequence: Sequence24::new(0),
                },
                payload: DatagramPayload::EncapsulatedPackets(vec![EncapsulatedPacket {
                    header: crate::protocol::types::EncapsulatedPacketHeader {
                        reliability: Reliability::ReliableOrdered,
                        is_split: false,
                        needs_bas: false,
                    },
                    bit_length: 8,
                    reliable_index: Some(Sequence24::new(0)),
                    sequence_index: None,
                    ordering_index: Some(Sequence24::new(0)),
                    ordering_channel: Some(0),
                    split: None,
                    payload: Bytes::from_static(&[0x15]),
                }]),
            },
            start,
        )
        .unwrap();
        assert_eq!(ms.state(), ConnectionState::Closing);
        let mut acked = false;
        let mut now = start;
        while ms.state() != ConnectionState::Closed {
            for datagram in ms.on_tick(now) {
                match datagram.payload {
                    DatagramPayload::Ack(_) => acked = true,
                    DatagramPayload::EncapsulatedPackets(frames) => assert!(
                        frames.iter().all(|f| f.payload.as_ref() != [0x15]),
                        "no echo"
                    ),
                    DatagramPayload::Nak(_) => {}
                }
            }
            now += TICK;
            assert!(now < start + Duration::from_secs(1));
        }
        assert!(acked);

        let mut ms = connected_client(TransmissionPolicy::Optimized, start);
        ms.handle_control_packet(
            &RaknetPacket::DisconnectionNotification(DisconnectionNotification {
                reason: DisconnectReason::ClosedByRemotePeer,
            }),
            start,
        );
        assert_eq!(ms.state(), ConnectionState::Closed);
        assert!(sent_frames(&mut ms, start).is_empty());
    }

    /// HS-9: `DetectLostConnections` is ignored on receive (Mojang
    /// RP_113:2020-2026).
    #[test]
    fn detect_lost_connections_is_ignored() {
        let now = Instant::now();
        let detect = RaknetPacket::DetectLostConnection(DetectLostConnection);
        for policy in [TransmissionPolicy::Vanilla, TransmissionPolicy::Optimized] {
            let mut ms = connected_client(policy, now);
            sent_frames(&mut ms, now + TICK);
            ms.handle_control_packet(&detect, now + TICK);
            assert!(pings(&sent_frames(&mut ms, now + TICK * 2)).is_empty());
        }
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
            let later = now + TICK;
            let frames = sent_frames(&mut ms, later);
            let pong = frames
                .iter()
                .find_map(|frame| match decode(frame) {
                    RaknetPacket::ConnectedPong(pong) => Some((frame.header.reliability, pong)),
                    _ => None,
                })
                .expect("pong");
            assert_eq!(pong.0, Reliability::Unreliable);
            assert_eq!(pong.1.ping_time.0, 77);
            assert_eq!(pong.1.pong_time.0, ManagedSession::current_raknet_time(now).0);
        }
    }

    /// KA-3: pongs (even with a future ping time) never feed the RTT.
    #[test]
    fn pong_never_affects_rtt() {
        let now = Instant::now();
        let mut ms = connected_client(TransmissionPolicy::Vanilla, now);
        ack_all(&mut ms, now);
        let before = ms.rtt_micros();
        ms.handle_control_packet(
            &RaknetPacket::ConnectedPong(ConnectedPong {
                ping_time: RaknetTime(u64::MAX),
                pong_time: RaknetTime(0),
            }),
            now,
        );
        assert_eq!(ms.rtt_micros(), before);
        assert!(ms.is_connected());
    }
}

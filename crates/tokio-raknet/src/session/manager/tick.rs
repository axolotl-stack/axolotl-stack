use std::time::Instant;

use crate::protocol::{
    datagram::Datagram,
    encapsulated_packet::EncapsulatedPacket,
    packet::{ConnectedPing, RaknetPacket},
    reliability::Reliability,
    state::{DisconnectReason, RakPriority},
};

use super::{ConnectionState, ManagedSession};

impl ManagedSession {
    /// Run periodic maintenance and return any datagrams that should be sent.
    pub fn on_tick(&mut self, now: Instant) -> Vec<Datagram> {
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

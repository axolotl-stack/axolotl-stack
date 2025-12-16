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
        if self.is_connected() {
            let idle = now.saturating_duration_since(self.last_activity);
            if idle >= self.config.session_timeout {
                let _ = self.send_disconnect(DisconnectReason::TimedOut);
                self.state = ConnectionState::Closed;
                self.last_disconnect_reason = Some(DisconnectReason::TimedOut);
            } else if idle >= self.config.session_stale {
                self.state = ConnectionState::Stale;
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
        if self.current_ping_nonce.is_some() {
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

        self.queue_control_packet(pkt, Reliability::Unreliable, 0, RakPriority::Immediate);
        self.last_ping_sent = Some(now);
        self.current_ping_nonce = Some(timestamp.0);
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

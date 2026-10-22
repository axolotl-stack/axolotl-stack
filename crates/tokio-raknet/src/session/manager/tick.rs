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

//! Transmission policy selection and the go-raknet (Vanilla) timing state.
//!
//! The policy is a plain enum matched on the hot path (no trait objects).
//! `Optimized` keeps tokio-raknet's packing, congestion window, smoothed RTO
//! and bounded ACK coalescing. `Vanilla` reproduces lunar-bedrock/go-raknet
//! `87a6b7e` (see `PARITY.md`): one frame per datagram, no congestion window,
//! ACKs only on the 100 ms connection tick, a resend scan on every third tick
//! and go's mean-of-recent-samples RTT.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::protocol::datagram::Datagram;

/// Congestion control, resend/RTO, ACK/flush scheduling and pacing policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransmissionPolicy {
    /// Bit-for-bit lunar-bedrock/go-raknet timing, windowing, resend and
    /// framing semantics, matching the vanilla Bedrock client RakNet. Default
    /// for `RaknetStream::connect*`.
    Vanilla,
    /// tokio-raknet's latency/throughput-optimized behaviour. Default for
    /// `RaknetListener` and the low-level `Session` constructors.
    #[default]
    Optimized,
}

impl TransmissionPolicy {
    #[inline]
    pub fn is_vanilla(self) -> bool {
        matches!(self, TransmissionPolicy::Vanilla)
    }
}

/// go-raknet `conn.go:startTicking` interval.
pub const VANILLA_TICK_INTERVAL: Duration = Duration::from_millis(100);
/// go-raknet `conn.go:startTicking`: `checkResend` runs on every third tick.
pub(crate) const VANILLA_RESEND_TICKS: u64 = 3;
/// go-raknet `conn.go:startTicking`: ping and inactivity check every fifth tick.
pub(crate) const VANILLA_PING_TICKS: u64 = 5;
/// go-raknet `resend_map.go:rtt` sample window.
const RTT_WINDOW: Duration = Duration::from_secs(5);
/// go-raknet `resend_map.go:rtt` value when no samples are recorded.
const DEFAULT_RTT: Duration = Duration::from_millis(50);

/// Per-session state of the Vanilla policy.
#[derive(Debug, Default)]
pub(crate) struct VanillaState {
    /// `resendMap.delays`: (recorded at, sample). Samples are appended with a
    /// monotonically increasing clock, so expiry pops from the front.
    samples: VecDeque<(Instant, Duration)>,
    /// `Conn.rtt`: the value stored by the last `checkResend`; zero until the
    /// first resend check. Drives the NAK delay (`conn.go:451-452`).
    stored_rtt: Duration,
    /// Transmission order counter: go writes synchronously, so priorities
    /// never reorder frames.
    pub(crate) fifo: u64,
    /// `conn.go:startTicking` tick counter `i`.
    pub(crate) ticks: u64,
    /// Datagrams resent in response to a NAK, waiting for the transport.
    pub(crate) resend_ready: VecDeque<Datagram>,
    /// Pending NAK datagrams split by the go ACK writer.
    pub(crate) feedback_ready: VecDeque<Datagram>,
}

impl VanillaState {
    /// `resend_map.go:remove`: `mul` is 1 for ACKs and 2 for retransmissions.
    pub(crate) fn record(&mut self, now: Instant, sent: Instant, mul: u32) {
        self.samples
            .push_back((now, now.saturating_duration_since(sent) * mul));
    }

    /// `resend_map.go:rtt`: prune samples strictly older than five seconds,
    /// then return their integer-nanosecond mean (50 ms when empty).
    pub(crate) fn rtt(&mut self, now: Instant) -> Duration {
        while self
            .samples
            .front()
            .is_some_and(|&(at, _)| now.saturating_duration_since(at) > RTT_WINDOW)
        {
            self.samples.pop_front();
        }
        if self.samples.is_empty() {
            return DEFAULT_RTT;
        }
        let total: u128 = self.samples.iter().map(|(_, rtt)| rtt.as_nanos()).sum();
        Duration::from_nanos((total / self.samples.len() as u128) as u64)
    }

    /// `conn.go:checkResend` stores the RTT it computed.
    pub(crate) fn store_rtt(&mut self, rtt: Duration) {
        self.stored_rtt = rtt;
    }

    /// `conn.go:receiveDatagram`: `rtt + rtt/2` of the stored RTT.
    pub(crate) fn nak_delay(&self) -> Duration {
        self.stored_rtt + self.stored_rtt / 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtt_is_the_integer_mean_of_samples_within_five_seconds() {
        let start = Instant::now();
        let mut state = VanillaState::default();
        assert_eq!(state.rtt(start), DEFAULT_RTT);
        state.record(start, start - Duration::from_nanos(10), 1);
        state.record(start, start - Duration::from_nanos(15), 2);
        // (10 + 30) / 2
        assert_eq!(state.rtt(start), Duration::from_nanos(20));
        let later = start + Duration::from_millis(1);
        state.record(later, later - Duration::from_nanos(1), 1);
        // Exactly five seconds old samples are kept (go prunes `> 5s`).
        assert_eq!(state.rtt(start + RTT_WINDOW), Duration::from_nanos(13));
        assert_eq!(
            state.rtt(start + RTT_WINDOW + Duration::from_nanos(1)),
            Duration::from_nanos(1)
        );
        assert_eq!(
            state.rtt(later + RTT_WINDOW + Duration::from_nanos(1)),
            DEFAULT_RTT
        );
    }

    #[test]
    fn nak_delay_is_one_and_a_half_stored_rtt() {
        let mut state = VanillaState::default();
        assert_eq!(state.nak_delay(), Duration::ZERO);
        state.store_rtt(Duration::from_nanos(101));
        assert_eq!(state.nak_delay(), Duration::from_nanos(151));
    }
}

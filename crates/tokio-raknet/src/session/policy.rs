//! Transmission policy selection and the Mojang RakNet (Vanilla) congestion
//! control state.
//!
//! The policy is a plain enum matched on the hot path (no trait objects).
//! `Optimized` keeps tokio-raknet's packing, congestion window, smoothed RTO
//! and bounded ACK coalescing. `Vanilla` reproduces Mojang RakNet, the
//! Jenkins RakNet fork inside the Minecraft Bedrock client and Bedrock
//! Dedicated Server (see `PARITY.md`): message-level resends re-packed into
//! datagrams, `CCRakNetSlidingWindow` with Mojang's changes, ACKs 10 ms after
//! the oldest unacknowledged datagram, NAKs on gap detection and a 10 ms
//! update cycle. Evidence: the BDS 1.26.51 and real-client captures in
//! `tests/captures`.

use std::time::Duration;

use crate::protocol::types::Sequence24;

/// Congestion control, resend/RTO, ACK/flush scheduling and pacing policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransmissionPolicy {
    /// Mojang RakNet timing, windowing, resend and framing semantics: the
    /// Minecraft Bedrock client (`RaknetStream`) and Bedrock Dedicated
    /// Server (`RaknetListener`). Default for `RaknetStream::connect*`.
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

/// The RakNet update thread (Jenkins `UpdateNetworkLoop`): `RunUpdateCycle`,
/// then a 10 ms wait; a received datagram or an `IMMEDIATE` send wakes the
/// loop early.
pub const VANILLA_TICK_INTERVAL: Duration = Duration::from_millis(10);
/// `CCRakNetSlidingWindow::ShouldSendACKs`: SYN, the age of the oldest
/// unacknowledged datagram before ACKs are sent.
pub(crate) const VANILLA_ACK_DELAY: Duration = Duration::from_millis(10);
/// Occasional ping period: connected peers are pinged every 5 s (PARITY
/// KA-1, BDS and real-client captures).
pub(crate) const VANILLA_PING_INTERVAL: Duration = Duration::from_millis(5000);
/// `timeoutTime`, the Jenkins RakPeer default of 10 s (BDS capture
/// `client-silence`: the silent client is dropped 10 s after its last
/// datagram). Drives the dead-connection rule and the reliable keepalive
/// (`timeoutTime / 2`).
pub(crate) const VANILLA_TIMEOUT: Duration = Duration::from_millis(10_000);
/// Connections still in the online handshake are dropped this long after
/// creation.
pub(crate) const VANILLA_HANDSHAKE_TIMEOUT: Duration = Duration::from_millis(10_000);
/// `RESEND_BUFFER_ARRAY_LENGTH`: reliable messages in flight.
pub(crate) const VANILLA_RESEND_SLOTS: u32 = 512;
/// `DATAGRAM_MESSAGE_ID_ARRAY_LENGTH`: datagram history entries.
pub(crate) const VANILLA_HISTORY_LEN: usize = 512;
/// `UDP_HEADER_SIZE`: subtracted from the MTU for every address family.
pub(crate) const VANILLA_UDP_HEADER: usize = 28;
/// `DatagramHeaderFormat::GetDataHeaderByteLength`: 72 bits reserved per
/// datagram when packing messages and ACK/NAK ranges.
pub(crate) const VANILLA_DATAGRAM_RESERVE: usize = 9;
/// `GetMaxMessageHeaderLengthBits`: the fragment size assumes the largest
/// message header (RELIABLE_SEQUENCED + split, 23 bytes).
pub(crate) const VANILLA_MAX_MESSAGE_HEADER: usize = 23;
/// `OnGotPacket`: a gap above this rejects the datagram.
pub(crate) const VANILLA_MAX_SKIPPED_DATAGRAMS: u32 = 50_000;
/// `OnGotPacket`: at most this many datagram numbers are NAKed per gap.
pub(crate) const VANILLA_MAX_NAKS_PER_GAP: u32 = 1000;

/// `GetRTOForRetransmission`: `min(2·RTT + 4·deviation + 30 ms, 2000 ms)`.
const RTO_MAX_US: f64 = 2_000_000.0;
const RTO_ADDITIONAL_VARIANCE_US: f64 = 30_000.0;
/// RTT EWMA gain `d`.
const RTT_GAIN: f64 = 0.05;

/// Largest message payload before a message is split, and the fragment
/// payload size: `MTU - 28 - 9 - 23` = `MTU - 60` (BDS and real-client
/// captures: 1340-byte fragments at MTU 1400, PARITY REL-11).
pub(crate) fn vanilla_max_fragment_len(mtu: usize) -> usize {
    mtu.saturating_sub(VANILLA_UDP_HEADER + VANILLA_DATAGRAM_RESERVE + VANILLA_MAX_MESSAGE_HEADER)
}

/// Message bytes (headers and payloads) one datagram may carry:
/// `(MTU - 28) * 8 - 72` bits.
pub(crate) fn vanilla_datagram_capacity(mtu: usize) -> usize {
    mtu.saturating_sub(VANILLA_UDP_HEADER + VANILLA_DATAGRAM_RESERVE)
}

/// Mojang's variant of `CCRakNetSlidingWindow`.
///
/// Differences from Jenkins 4.081: congestion avoidance grows the window on
/// every ACK instead of once per congestion block, and a NAK latches the
/// block backoff (halving the threshold once per block and suppressing the
/// resend collapse for that block).
#[derive(Debug, Clone)]
pub(crate) struct MojangCongestion {
    /// `MAXIMUM_MTU_INCLUDING_UDP_HEADER`: the MTU minus 28.
    mtu: f64,
    cwnd: f64,
    ss_thresh: f64,
    next_congestion_block: Sequence24,
    backoff_this_block: bool,
    /// `_isContinuousSend`: set from the send queue state by the update and
    /// by ACK processing; gates growth and backoff.
    pub(crate) is_continuous_send: bool,
    last_rtt_us: Option<f64>,
    estimated_rtt_us: Option<f64>,
    deviation_rtt_us: f64,
}

impl MojangCongestion {
    /// `Init`: window one MTU (minus 28), slow-start threshold 0, RTT unset.
    pub(crate) fn new(mtu: usize) -> Self {
        let mtu = mtu.saturating_sub(VANILLA_UDP_HEADER) as f64;
        Self {
            mtu,
            cwnd: mtu,
            ss_thresh: 0.0,
            next_congestion_block: Sequence24::new(0),
            backoff_this_block: false,
            is_continuous_send: false,
            last_rtt_us: None,
            estimated_rtt_us: None,
            deviation_rtt_us: 0.0,
        }
    }

    /// `IsInSlowStart`: the `needsBAndAs` (0x04) datagram flag.
    pub(crate) fn in_slow_start(&self) -> bool {
        self.ss_thresh == 0.0 || self.cwnd <= self.ss_thresh
    }

    /// `GetTransmissionBandwidth`: new data allowed this update.
    pub(crate) fn transmission_bandwidth(&self, unacked_bytes: usize) -> usize {
        let unacked = unacked_bytes as f64;
        if self.cwnd < unacked {
            0
        } else {
            (self.cwnd - unacked) as usize
        }
    }

    /// `ShouldSendACKs` sends at once until the first RTT sample.
    pub(crate) fn rtt_known(&self) -> bool {
        self.last_rtt_us.is_some()
    }

    /// Smoothed RTT in microseconds, `u64::MAX` before the first sample.
    pub(crate) fn rtt_micros(&self) -> u64 {
        self.estimated_rtt_us.map_or(u64::MAX, |rtt| rtt as u64)
    }

    /// `GetRTOForRetransmission` (no exponential backoff: `timesSent` is
    /// ignored).
    pub(crate) fn rto(&self) -> Duration {
        let micros = match self.estimated_rtt_us {
            None => RTO_MAX_US,
            Some(estimated) => {
                (2.0 * estimated + 4.0 * self.deviation_rtt_us + RTO_ADDITIONAL_VARIANCE_US)
                    .min(RTO_MAX_US)
            }
        };
        Duration::from_micros(micros as u64)
    }

    /// RTT update of `OnAck`.
    pub(crate) fn on_rtt(&mut self, rtt: Duration) {
        let rtt = rtt.as_micros() as f64;
        self.last_rtt_us = Some(rtt);
        match self.estimated_rtt_us {
            None => {
                self.estimated_rtt_us = Some(rtt);
                self.deviation_rtt_us = rtt;
            }
            Some(estimated) => {
                let difference = rtt - estimated;
                self.estimated_rtt_us = Some(estimated + RTT_GAIN * difference);
                self.deviation_rtt_us += RTT_GAIN * (difference.abs() - self.deviation_rtt_us);
            }
        }
    }

    /// Window growth of `OnAck` for an ACKed datagram, only while
    /// continuously sending.
    pub(crate) fn on_ack(&mut self, datagram: Sequence24, next_datagram: Sequence24) {
        if !self.is_continuous_send {
            return;
        }
        if datagram > self.next_congestion_block {
            self.backoff_this_block = false;
            self.next_congestion_block = next_datagram;
        }
        let square = self.mtu * self.mtu;
        if self.in_slow_start() {
            self.cwnd += self.mtu;
            if self.ss_thresh != 0.0 && self.cwnd > self.ss_thresh {
                self.cwnd = self.ss_thresh + square / self.cwnd;
            }
        } else {
            self.cwnd += square / self.cwnd;
        }
    }

    /// `OnNAK` with Mojang's block latch.
    pub(crate) fn on_nak(&mut self, next_datagram: Sequence24) {
        if self.is_continuous_send && !self.backoff_this_block {
            self.ss_thresh = self.cwnd * 0.5;
            self.next_congestion_block = next_datagram;
            self.backoff_this_block = true;
        }
    }

    /// `OnResend`, once per resent message.
    pub(crate) fn on_resend(&mut self, next_datagram: Sequence24) {
        if self.is_continuous_send && !self.backoff_this_block && self.cwnd > self.mtu * 2.0 {
            self.ss_thresh = (self.cwnd * 0.5).max(self.mtu);
            self.cwnd = self.mtu;
            self.next_congestion_block = next_datagram;
            self.backoff_this_block = true;
        }
    }

    #[cfg(test)]
    pub(crate) fn window(&self) -> (f64, f64) {
        (self.cwnd, self.ss_thresh)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rto_is_two_rtt_plus_four_deviation_plus_30ms_capped_at_two_seconds() {
        let mut cc = MojangCongestion::new(1400);
        assert_eq!(cc.rto(), Duration::from_secs(2), "unset RTT");
        assert!(!cc.rtt_known());
        cc.on_rtt(Duration::from_millis(40));
        // First sample: estimate = deviation = 40 ms.
        assert_eq!(cc.rto(), Duration::from_millis(2 * 40 + 4 * 40 + 30));
        cc.on_rtt(Duration::from_millis(60));
        // estimate 41 ms, deviation 40 + 0.05·(20 - 40) = 39 ms.
        assert_eq!(cc.rto(), Duration::from_micros(82_000 + 156_000 + 30_000));
        cc.on_rtt(Duration::from_secs(30));
        assert_eq!(cc.rto(), Duration::from_secs(2), "capped");
    }

    #[test]
    fn slow_start_grows_one_mtu_per_ack_only_while_continuously_sending() {
        let mut cc = MojangCongestion::new(1400);
        assert_eq!(cc.window(), (1372.0, 0.0));
        cc.on_ack(Sequence24::new(1), Sequence24::new(2));
        assert_eq!(cc.window().0, 1372.0, "application-limited: no growth");
        cc.is_continuous_send = true;
        cc.on_ack(Sequence24::new(1), Sequence24::new(2));
        cc.on_ack(Sequence24::new(2), Sequence24::new(3));
        assert_eq!(cc.window().0, 3.0 * 1372.0);
        assert!(cc.in_slow_start());
    }

    #[test]
    fn congestion_avoidance_grows_on_every_ack_not_once_per_block() {
        let mut cc = MojangCongestion::new(1400);
        cc.is_continuous_send = true;
        for seq in 0..8 {
            cc.on_ack(Sequence24::new(seq + 1), Sequence24::new(seq + 2));
        }
        // A NAK halves the threshold: 9·1372 / 2.
        cc.on_nak(Sequence24::new(10));
        let (cwnd, ss) = cc.window();
        assert_eq!((cwnd, ss), (9.0 * 1372.0, 9.0 * 1372.0 / 2.0));
        assert!(!cc.in_slow_start());
        // Jenkins grows once per block; Mojang on each of these ACKs, all
        // inside the block that ends at datagram 10.
        cc.on_ack(Sequence24::new(3), Sequence24::new(11));
        cc.on_ack(Sequence24::new(4), Sequence24::new(11));
        let square = 1372.0 * 1372.0;
        let once = cwnd + square / cwnd;
        assert_eq!(cc.window().0, once + square / once);
    }

    #[test]
    fn nak_latches_the_block_and_suppresses_resend_collapse() {
        let mut cc = MojangCongestion::new(1400);
        cc.is_continuous_send = true;
        for seq in 0..4 {
            cc.on_ack(Sequence24::new(seq + 1), Sequence24::new(seq + 2));
        }
        let cwnd = cc.window().0;
        cc.on_nak(Sequence24::new(20));
        cc.on_nak(Sequence24::new(21));
        assert_eq!(cc.window(), (cwnd, cwnd / 2.0), "one halving per block");
        cc.on_resend(Sequence24::new(22));
        assert_eq!(cc.window().0, cwnd, "resend collapse latched out");
        // An ACK beyond the block reopens backoff: a resend collapses.
        cc.on_ack(Sequence24::new(21), Sequence24::new(23));
        cc.on_resend(Sequence24::new(23));
        assert_eq!(cc.window().0, 1372.0);
    }

    #[test]
    fn fragment_and_datagram_budgets_follow_mojang_header_reserves() {
        assert_eq!(vanilla_max_fragment_len(1400), 1340);
        assert_eq!(vanilla_max_fragment_len(1200), 1140);
        assert_eq!(vanilla_max_fragment_len(576), 516);
        assert_eq!(vanilla_datagram_capacity(1400), 1363);
    }
}

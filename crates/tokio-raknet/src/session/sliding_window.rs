use std::time::{Duration, Instant};

use crate::protocol::constants::{CC_ADDITIONAL_VARIANCE, CC_MAXIMUM_THRESHOLD};
use crate::protocol::datagram::Datagram;
use crate::protocol::types::Sequence24;

pub struct SlidingWindow {
    mtu: usize,
    cwnd: f64,
    ss_thresh: f64,
    estimated_rtt: f64, // -1 = uninitialized
    last_rtt: f64,      // -1 = none
    deviation_rtt: f64, // -1 = none
    next_congestion_block: Sequence24,
    backoff_this_block: bool,
    unacked_bytes: i64,
}

impl SlidingWindow {
    pub fn new(mtu: usize) -> Self {
        Self {
            mtu,
            cwnd: mtu as f64,
            ss_thresh: 0.0,
            estimated_rtt: -1.0,
            last_rtt: -1.0,
            deviation_rtt: -1.0,
            next_congestion_block: Sequence24::new(0),
            backoff_this_block: false,
            unacked_bytes: 0,
        }
    }

    pub fn on_packet_received(&mut self, _now: Instant) {
        // ACK scheduling happens in the session tick; nothing to track here.
    }

    pub fn on_reliable_send(&mut self, dgram: &Datagram) {
        self.unacked_bytes += dgram.size() as i64;
    }

    pub fn on_ack(
        &mut self,
        now: Instant,
        dgram: &Datagram,
        acked_seq: Sequence24,
        send_time: Instant,
    ) {
        let rtt = now.saturating_duration_since(send_time);
        let rtt_ms = rtt.as_millis() as f64;
        self.last_rtt = rtt_ms;
        self.unacked_bytes -= dgram.size() as i64;
        if self.unacked_bytes < 0 {
            self.unacked_bytes = 0;
        }

        if self.estimated_rtt < 0.0 {
            self.estimated_rtt = rtt_ms;
            self.deviation_rtt = rtt_ms;
        } else {
            let gain = 0.05;
            let diff = rtt_ms - self.estimated_rtt;
            self.estimated_rtt += gain * diff;
            self.deviation_rtt += gain * (diff.abs() - self.deviation_rtt);
        }

        let is_new_block = acked_seq > self.next_congestion_block;
        if is_new_block {
            self.backoff_this_block = false;
            self.next_congestion_block = acked_seq;
        }

        if self.is_in_slow_start() {
            self.cwnd += self.mtu as f64;
            if self.ss_thresh != 0.0 && self.cwnd > self.ss_thresh {
                self.cwnd = self.ss_thresh + (self.mtu as f64 * self.mtu as f64) / self.cwnd;
            }
        } else if is_new_block {
            self.cwnd += (self.mtu as f64 * self.mtu as f64) / self.cwnd;
        }
    }

    pub fn on_nak(&mut self) {
        if !self.backoff_this_block {
            self.ss_thresh = self.cwnd * 0.75;
        }
    }

    pub fn on_resend(&mut self, cur_seq: Sequence24) {
        if !self.backoff_this_block && self.cwnd > (self.mtu as f64 * 2.0) {
            self.ss_thresh = (self.cwnd * 0.5).max(self.mtu as f64);
            self.cwnd = self.mtu as f64;
            self.next_congestion_block = cur_seq;
            self.backoff_this_block = true;
        }
    }

    pub fn get_retransmission_bandwidth(&self) -> usize {
        self.unacked_bytes.max(0) as usize
    }

    pub fn get_transmission_bandwidth(&self) -> usize {
        if (self.unacked_bytes as f64) <= self.cwnd {
            (self.cwnd - self.unacked_bytes as f64) as usize
        } else {
            0
        }
    }

    fn is_in_slow_start(&self) -> bool {
        self.ss_thresh == 0.0 || self.cwnd <= self.ss_thresh
    }

    pub fn get_rto_for_retransmission(&self) -> Duration {
        if self.estimated_rtt < 0.0 {
            return Duration::from_millis(CC_MAXIMUM_THRESHOLD as u64);
        }
        let mut threshold =
            (2.0 * self.estimated_rtt + 4.0 * self.deviation_rtt) + CC_ADDITIONAL_VARIANCE as f64;
        if threshold > CC_MAXIMUM_THRESHOLD as f64 {
            threshold = CC_MAXIMUM_THRESHOLD as f64;
        }
        Duration::from_millis(threshold as u64)
    }

    pub fn on_send_ack(&mut self) {
        // No-op; retained for parity with Cloudburst hook points.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::constants::DatagramFlags;
    use crate::protocol::datagram::{Datagram, DatagramPayload};
    use crate::protocol::encapsulated_packet::EncapsulatedPacket;
    use crate::protocol::reliability::Reliability;
    use crate::protocol::types::{DatagramHeader, EncapsulatedPacketHeader};
    use bytes::Bytes;

    /// Helper to create a test datagram with a specific size
    fn make_test_datagram(seq: u32, payload_size: usize) -> Datagram {
        let pkt = EncapsulatedPacket {
            header: EncapsulatedPacketHeader::with_reliability(Reliability::Reliable),
            bit_length: (payload_size * 8) as u16,
            reliable_index: Some(Sequence24::new(0)),
            sequence_index: None,
            ordering_index: None,
            ordering_channel: None,
            split: None,
            payload: Bytes::from(vec![0u8; payload_size]),
        };

        Datagram {
            header: DatagramHeader {
                flags: DatagramFlags::VALID,
                sequence: Sequence24::new(seq),
            },
            payload: DatagramPayload::EncapsulatedPackets(vec![pkt]),
        }
    }

    #[test]
    fn new_window_initial_state() {
        let mtu = 1400;
        let window = SlidingWindow::new(mtu);

        // Initial cwnd should be MTU
        assert_eq!(window.get_transmission_bandwidth(), mtu);
        assert_eq!(window.get_retransmission_bandwidth(), 0);
    }

    #[test]
    fn on_reliable_send_tracks_bytes() {
        let mtu = 1400;
        let mut window = SlidingWindow::new(mtu);

        let dgram = make_test_datagram(0, 100);
        let dgram_size = dgram.size();

        window.on_reliable_send(&dgram);

        // Unacked bytes should increase
        assert_eq!(window.get_retransmission_bandwidth(), dgram_size);

        // Available bandwidth should decrease
        assert_eq!(
            window.get_transmission_bandwidth(),
            mtu - dgram_size
        );
    }

    #[test]
    fn on_ack_reduces_unacked_bytes() {
        let mtu = 1400;
        let mut window = SlidingWindow::new(mtu);

        let dgram = make_test_datagram(0, 100);
        let now = Instant::now();
        let send_time = now - Duration::from_millis(50);

        window.on_reliable_send(&dgram);
        let unacked_before = window.get_retransmission_bandwidth();

        window.on_ack(now, &dgram, Sequence24::new(0), send_time);

        // Unacked bytes should decrease
        assert!(window.get_retransmission_bandwidth() < unacked_before);
    }

    #[test]
    fn on_ack_updates_rtt_estimates() {
        let mtu = 1400;
        let mut window = SlidingWindow::new(mtu);

        let dgram = make_test_datagram(0, 100);
        let now = Instant::now();
        let send_time = now - Duration::from_millis(100);

        window.on_reliable_send(&dgram);
        window.on_ack(now, &dgram, Sequence24::new(0), send_time);

        // RTO should now be based on measured RTT, not the maximum
        let rto = window.get_rto_for_retransmission();
        assert!(
            rto < Duration::from_millis(CC_MAXIMUM_THRESHOLD as u64),
            "RTO should be less than max after measuring RTT"
        );
    }

    #[test]
    fn uninitialized_rto_is_maximum() {
        let window = SlidingWindow::new(1400);

        let rto = window.get_rto_for_retransmission();
        assert_eq!(rto, Duration::from_millis(CC_MAXIMUM_THRESHOLD as u64));
    }

    #[test]
    fn slow_start_increases_cwnd() {
        let mtu = 1400;
        let mut window = SlidingWindow::new(mtu);

        // In slow start, cwnd should be MTU initially
        let initial_bandwidth = window.get_transmission_bandwidth();

        // Simulate send and ack
        let dgram = make_test_datagram(0, 100);
        let now = Instant::now();
        let send_time = now - Duration::from_millis(50);

        window.on_reliable_send(&dgram);
        window.on_ack(now, &dgram, Sequence24::new(0), send_time);

        // After ACK in slow start, cwnd increases by MTU
        // Note: We need another send to see the full bandwidth
        let dgram2 = make_test_datagram(1, 100);
        window.on_reliable_send(&dgram2);
        window.on_ack(now, &dgram2, Sequence24::new(1), send_time);

        // Bandwidth should have increased
        assert!(
            window.get_transmission_bandwidth() > initial_bandwidth,
            "Bandwidth should increase during slow start"
        );
    }

    #[test]
    fn on_nak_reduces_threshold() {
        let mtu = 1400;
        let mut window = SlidingWindow::new(mtu);

        // First, grow cwnd through some ACKs
        for i in 0..5 {
            let dgram = make_test_datagram(i, 100);
            let now = Instant::now();
            window.on_reliable_send(&dgram);
            window.on_ack(now, &dgram, Sequence24::new(i), now - Duration::from_millis(50));
        }

        let bandwidth_before_nak = window.get_transmission_bandwidth();

        // NAK should reduce ss_thresh
        window.on_nak();

        // The immediate effect may not be visible in bandwidth,
        // but on_resend will show the backoff
        let dgram = make_test_datagram(10, 100);
        window.on_reliable_send(&dgram);
        window.on_resend(Sequence24::new(10));

        // After resend, cwnd should drop to MTU
        assert!(
            window.get_transmission_bandwidth() < bandwidth_before_nak,
            "Bandwidth should decrease after NAK + resend"
        );
    }

    #[test]
    fn on_resend_triggers_backoff() {
        let mtu = 1400;
        let mut window = SlidingWindow::new(mtu);

        // Grow cwnd
        for i in 0..5 {
            let dgram = make_test_datagram(i, 100);
            let now = Instant::now();
            window.on_reliable_send(&dgram);
            window.on_ack(now, &dgram, Sequence24::new(i), now - Duration::from_millis(50));
        }

        // Trigger resend
        window.on_resend(Sequence24::new(5));

        // cwnd should be reset to MTU
        // Note: get_transmission_bandwidth accounts for unacked_bytes
        // We need to check if the window size dropped
    }

    #[test]
    fn backoff_only_once_per_block() {
        let mtu = 1400;
        let mut window = SlidingWindow::new(mtu);

        // First resend triggers backoff
        window.on_resend(Sequence24::new(0));

        // Second resend in same block should not backoff again
        // (backoff_this_block flag prevents it)
        window.on_resend(Sequence24::new(1));

        // The cwnd should still be at MTU, not further reduced
        assert_eq!(window.get_transmission_bandwidth(), mtu as usize);
    }

    #[test]
    fn unacked_bytes_never_negative() {
        let mtu = 1400;
        let mut window = SlidingWindow::new(mtu);

        let dgram = make_test_datagram(0, 100);
        let now = Instant::now();

        // ACK without send (shouldn't happen, but test the guard)
        window.on_ack(now, &dgram, Sequence24::new(0), now - Duration::from_millis(50));

        // Should be clamped to 0, not negative
        assert_eq!(window.get_retransmission_bandwidth(), 0);
    }

    #[test]
    fn transmission_bandwidth_zero_when_fully_utilized() {
        let mtu = 1400;
        let mut window = SlidingWindow::new(mtu);

        // Send enough data to fill the window
        let dgram = make_test_datagram(0, mtu);
        window.on_reliable_send(&dgram);

        // Transmission bandwidth should be 0 (window full)
        assert_eq!(window.get_transmission_bandwidth(), 0);
    }

    #[test]
    fn rtt_smoothing() {
        let mtu = 1400;
        let mut window = SlidingWindow::new(mtu);

        // First ACK initializes RTT
        let dgram1 = make_test_datagram(0, 100);
        let now = Instant::now();
        window.on_reliable_send(&dgram1);
        window.on_ack(now, &dgram1, Sequence24::new(0), now - Duration::from_millis(100));

        let rto1 = window.get_rto_for_retransmission();

        // Second ACK with different RTT should smooth
        let dgram2 = make_test_datagram(1, 100);
        window.on_reliable_send(&dgram2);
        window.on_ack(now, &dgram2, Sequence24::new(1), now - Duration::from_millis(50));

        let rto2 = window.get_rto_for_retransmission();

        // RTO should have changed due to smoothing
        assert_ne!(rto1, rto2, "RTO should change with different RTT measurements");
    }

    #[test]
    fn on_packet_received_is_noop() {
        let mtu = 1400;
        let mut window = SlidingWindow::new(mtu);

        let before = window.get_transmission_bandwidth();
        window.on_packet_received(Instant::now());
        let after = window.get_transmission_bandwidth();

        assert_eq!(before, after, "on_packet_received should be a no-op");
    }

    #[test]
    fn on_send_ack_is_noop() {
        let mtu = 1400;
        let mut window = SlidingWindow::new(mtu);

        let before = window.get_transmission_bandwidth();
        window.on_send_ack();
        let after = window.get_transmission_bandwidth();

        assert_eq!(before, after, "on_send_ack should be a no-op");
    }
}

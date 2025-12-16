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

use std::time::Instant;

use crate::protocol::datagram::Datagram;

use super::Session;

impl Session {
    /// Periodic maintenance: prune splits, schedule resends, and emit ACK/NACK datagrams.
    pub fn on_tick(&mut self, now: Instant) -> Vec<Datagram> {
        let mut out = Vec::new();

        self.process_incoming_acks_naks(now);

        let dropped = self.split_assembler.prune(now);
        for (ch, idx) in dropped {
            if let (Some(ch), Some(idx)) = (ch, idx) {
                self.ordering.skip_index(ch, idx);
            }
        }

        let mut bw = self.sliding.get_retransmission_bandwidth();
        if bw > 0 {
            let to_resend = self.collect_resendable_datagrams(now, &mut bw);
            self.resend_datagrams(to_resend, now, &mut out);
        }

        if let Some(d) = self.build_ack_datagram(now) {
            out.push(d);
        }

        if let Some(d) = self.build_nak_datagram() {
            out.push(d);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use crate::protocol::{constants::DatagramFlags, datagram::DatagramPayload, types::Sequence24};

    use super::*;
    use std::time::Instant;

    #[test]
    fn ack_and_nak_tick_match_expected_ranges() {
        let mut s = Session::new(1200);
        let now = Instant::now();

        // Receive sequence 0 and 2, leaving a gap at 1.
        s.process_datagram_sequence(Sequence24::new(0));
        s.process_datagram_sequence(Sequence24::new(2));

        let out = s.on_tick(now);
        assert_eq!(out.len(), 2);

        let mut ack = None;
        let mut nak = None;
        for d in &out {
            if d.header.flags.contains(DatagramFlags::ACK) {
                ack = Some(d);
            }
            if d.header.flags.contains(DatagramFlags::NACK) {
                nak = Some(d);
            }
        }

        let ack = ack.expect("ack datagram");
        let nak = nak.expect("nak datagram");

        if let DatagramPayload::Ack(payload) = &ack.payload {
            assert_eq!(payload.ranges.len(), 2);
            assert_eq!(payload.ranges[0].start.value(), 0);
            assert_eq!(payload.ranges[0].end.value(), 0);
            assert_eq!(payload.ranges[1].start.value(), 2);
            assert_eq!(payload.ranges[1].end.value(), 2);
        } else {
            panic!("expected ack payload");
        }

        if let DatagramPayload::Nak(payload) = &nak.payload {
            assert_eq!(payload.ranges.len(), 1);
            assert_eq!(payload.ranges[0].start.value(), 1);
            assert_eq!(payload.ranges[0].end.value(), 1);
        } else {
            panic!("expected nak payload");
        }
    }
}

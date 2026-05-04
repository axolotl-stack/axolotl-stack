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
            if let (Some(ch), Some(idx)) = (ch, idx)
                && let Some(released) = self.ordering.skip_index(ch, idx)
            {
                let mut ready = Vec::new();
                for pkt in released {
                    if let Err(error) = self.decode_and_push(pkt, &mut ready) {
                        tracing::debug!(?error, "failed_to_decode_released_ordered_packet");
                    }
                }
                self.pending_incoming.extend(ready);
            }
        }

        let mut bw = self.sliding.get_retransmission_bandwidth();
        if bw > 0 {
            self.resend_due_datagrams(now, &mut bw, &mut out);
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
    use crate::protocol::{
        constants::DatagramFlags,
        datagram::DatagramPayload,
        encapsulated_packet::{EncapsulatedPacket, SplitInfo},
        packet::RaknetPacket,
        reliability::Reliability,
        types::{EncapsulatedPacketHeader, Sequence24},
    };

    use super::*;
    use bytes::Bytes;
    use std::time::{Duration, Instant};

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

    #[test]
    fn split_timeout_releases_buffered_ordered_packet() {
        let now = Instant::now();
        let mut session = Session::with_tunables(
            1200,
            crate::session::SessionTunables {
                split_timeout: Duration::from_millis(1),
                ..Default::default()
            },
        );

        let first_split_part = ordered_packet(0, Some((7, 2, 0)), b"\xfepartial");
        let ready = session
            .handle_data_payload(vec![first_split_part], now)
            .expect("partial split should buffer");
        assert!(ready.is_empty());

        let next_ordered = ordered_packet(1, None, b"\xfenext");
        let ready = session
            .handle_data_payload(vec![next_ordered], now)
            .expect("later ordered packet should buffer behind missing index");
        assert!(ready.is_empty());

        let _ = session.on_tick(now + Duration::from_millis(2));
        let released = session.drain_pending_incoming();

        assert_eq!(released.len(), 1);
        assert!(matches!(
            released[0].packet,
            RaknetPacket::UserData { id: 0xfe, .. }
        ));
    }

    fn ordered_packet(
        ordering_index: u32,
        split: Option<(u16, u32, u32)>,
        payload: &'static [u8],
    ) -> EncapsulatedPacket {
        let is_split = split.is_some();
        EncapsulatedPacket {
            header: EncapsulatedPacketHeader::new(Reliability::ReliableOrdered, is_split, false),
            bit_length: (payload.len() as u16) << 3,
            reliable_index: Some(Sequence24::new(ordering_index)),
            sequence_index: None,
            ordering_index: Some(Sequence24::new(ordering_index)),
            ordering_channel: Some(0),
            split: split.map(|(id, count, index)| SplitInfo { count, id, index }),
            payload: Bytes::from_static(payload),
        }
    }
}

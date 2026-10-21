use std::{collections::VecDeque, time::Instant};

use crate::protocol::{
    constants::MAX_ACK_SEQUENCES,
    encapsulated_packet::EncapsulatedPacket,
    packet::{DecodeError, RaknetPacket},
};
use bytes::Bytes;

use crate::protocol::ack::{AckNackPayload, SequenceRange};

use super::{IncomingPacket, Session};

impl Session {
    /// Handle an incoming data payload (a list of encapsulated packets).
    pub fn handle_data_payload(
        &mut self,
        packets: Vec<EncapsulatedPacket>,
        now: Instant,
    ) -> Result<Vec<IncomingPacket>, DecodeError> {
        let mut out = Vec::new();

        self.sliding.on_packet_received(now);

        for enc in packets.into_iter() {
            self.handle_encapsulated(enc, now, &mut out)?;
        }

        Ok(out)
    }

    /// Handle an incoming dedicated ACK payload.
    /// Respects `max_incoming_ack_queue` limit, dropping oldest entries if exceeded.
    pub fn handle_ack_payload(&mut self, payload: AckNackPayload) {
        Self::queue_incoming_ack_ranges(
            &mut self.incoming_acks,
            payload.ranges,
            self.max_incoming_ack_queue,
        );
    }

    /// Handle an incoming dedicated NACK payload.
    /// Respects `max_incoming_ack_queue` limit, dropping oldest entries if exceeded.
    pub fn handle_nack_payload(&mut self, payload: AckNackPayload) {
        Self::queue_incoming_ack_ranges(
            &mut self.incoming_naks,
            payload.ranges,
            self.max_incoming_ack_queue,
        );
    }

    fn inbound_ack_range_is_safe(range: SequenceRange) -> bool {
        !range.wraps() && range.len() <= MAX_ACK_SEQUENCES as u32
    }

    fn queue_incoming_ack_ranges(
        queue: &mut VecDeque<SequenceRange>,
        ranges: Vec<SequenceRange>,
        max_ranges: usize,
    ) {
        if max_ranges == 0 {
            return;
        }
        let mut total_sequences = 0u32;
        for range in ranges {
            if !Self::inbound_ack_range_is_safe(range) {
                continue;
            }

            total_sequences = total_sequences.saturating_add(range.len());
            if total_sequences > MAX_ACK_SEQUENCES as u32 {
                break;
            }

            if queue.len() >= max_ranges {
                queue.pop_front();
            }
            queue.push_back(range);
        }
    }

    fn handle_encapsulated(
        &mut self,
        enc: EncapsulatedPacket,
        now: Instant,
        out: &mut Vec<IncomingPacket>,
    ) -> Result<(), DecodeError> {
        // Every accepted reliable fragment occupies an index, not only complete
        // messages. Otherwise split traffic leaves permanent holes in this window.
        let ridx = if enc.header.reliability.is_reliable() {
            enc.reliable_index
        } else {
            None
        };

        if let Some(idx) = ridx
            && self.reliable_tracker.has_seen(idx)
        {
            // Duplicate or stale reliable packet/fragment.
            return Ok(());
        }
        if enc.header.reliability.is_ordered() {
            self.ordering.validate_ordered(&enc)?;
        }

        // Attempt to add to split assembler (or pass through if not split)
        // Note: add() consumes the packet.
        let assembled_opt = self.split_assembler.add(enc, now)?;

        // Commit only after the assembler accepted the fragment. Expired split
        // messages are deliberately abandoned by the configured split TTL.
        if let Some(idx) = ridx {
            self.reliable_tracker.see(idx);
        }

        let enc = match assembled_opt {
            Some(pkt) => pkt,
            None => return Ok(()), // Buffered partial split
        };

        if enc.header.reliability.is_ordered() {
            self.handle_ordered(enc, out)?;
        } else {
            self.decode_and_push(enc, out)?;
        }

        Ok(())
    }

    pub(crate) fn decode_and_push(
        &mut self,
        enc: EncapsulatedPacket,
        out: &mut Vec<IncomingPacket>,
    ) -> Result<(), DecodeError> {
        let mut buf = enc.payload.clone();
        let reliability = enc.header.reliability;
        let ordering_channel = enc.ordering_channel;

        let pkt = match RaknetPacket::decode(&mut buf) {
            Ok(pkt) => pkt,
            Err(DecodeError::UnknownId(id)) => {
                let body = if !enc.payload.is_empty() {
                    enc.payload.slice(1..)
                } else {
                    Bytes::new()
                };
                RaknetPacket::UserData { id, payload: body }
            }
            Err(e) => return Err(e),
        };

        if let RaknetPacket::EncapsulatedAck(payload) = pkt {
            Self::queue_incoming_ack_ranges(
                &mut self.incoming_acks,
                payload.0.ranges,
                self.max_incoming_ack_queue,
            );
            return Ok(());
        }
        if let RaknetPacket::EncapsulatedNak(payload) = pkt {
            Self::queue_incoming_ack_ranges(
                &mut self.incoming_naks,
                payload.0.ranges,
                self.max_incoming_ack_queue,
            );
            return Ok(());
        }

        out.push(IncomingPacket {
            packet: pkt,
            reliability,
            ordering_channel,
        });
        Ok(())
    }

    pub(crate) fn process_incoming_acks_naks(&mut self, now: Instant) {
        self.process_incoming_acks(now);
        self.process_incoming_naks(now);
    }

    fn process_incoming_acks(&mut self, now: Instant) {
        while let Some(range) = self.incoming_acks.pop_front() {
            while let Some((&seq, _)) = self.sent_datagrams.range(range.start..=range.end).next() {
                if let Some(tracked) = self.sent_datagrams.remove(&seq) {
                    self.sliding
                        .on_ack(now, &tracked.datagram, seq, tracked.send_time);
                }
            }
        }
    }

    fn process_incoming_naks(&mut self, now: Instant) {
        while let Some(range) = self.incoming_naks.pop_front() {
            for tracked in self
                .sent_datagrams
                .range_mut(range.start..=range.end)
                .map(|(_, value)| value)
            {
                self.sliding.on_nak();
                tracked.next_send = now;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::types::Sequence24;
    use crate::protocol::{packet::RaknetPacket, reliability::Reliability, state::RakPriority};
    use bytes::Bytes;

    #[test]
    fn drops_oversized_ack_ranges_before_queueing() {
        let mut session = Session::new(1200);
        session.handle_ack_payload(AckNackPayload {
            ranges: vec![SequenceRange {
                start: Sequence24::new(0),
                end: Sequence24::new(MAX_ACK_SEQUENCES as u32),
            }],
        });

        assert!(session.incoming_acks.is_empty());
    }

    #[test]
    fn large_ack_range_only_touches_tracked_datagrams() {
        let mut session = Session::new(1500);
        let now = Instant::now();

        session
            .queue_packet(
                RaknetPacket::UserData {
                    id: 0x80,
                    payload: Bytes::from_static(b"one"),
                },
                Reliability::Reliable,
                0,
                RakPriority::High,
            )
            .unwrap();
        session.build_data_datagram(now).expect("tracked datagram");
        assert_eq!(session.sent_datagrams.len(), 1);

        let tracked_seq = *session.sent_datagrams.keys().next().unwrap();
        session.handle_ack_payload(AckNackPayload {
            ranges: vec![SequenceRange {
                start: tracked_seq,
                end: tracked_seq,
            }],
        });
        session.process_incoming_acks_naks(now);

        assert!(session.sent_datagrams.is_empty());
    }
}

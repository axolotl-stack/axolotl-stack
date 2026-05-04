use std::{collections::VecDeque, time::Instant};

use crate::protocol::{
    constants::MAX_ACK_SEQUENCES,
    encapsulated_packet::EncapsulatedPacket,
    packet::{DecodeError, RaknetPacket},
    types::Sequence24,
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
        // Reliability Logic:
        // - For non-split reliable packets:
        //   1) Deduplicate by reliable index; drop duplicates early.
        //   2) Decode/deliver; mark reliable index as seen on success.
        //
        // - For split packets:
        //   Do NOT mark reliable-indexes as seen until the split is fully assembled.
        //   Each split part has its own reliable index, and marking parts as seen
        //   would prevent retransmission if we later drop the split due to timeout.
        //   We rely on split_assembler to filter duplicate parts per (id,index).

        let is_split = enc.header.is_split;
        let ridx = if enc.header.reliability.is_reliable() && !is_split {
            enc.reliable_index
        } else {
            None
        };

        if let Some(idx) = ridx
            && self.reliable_tracker.has_seen(idx)
        {
            // Duplicate non-split reliable; drop silently.
            return Ok(());
        }

        // Attempt to add to split assembler (or pass through if not split)
        // Note: add() consumes the packet.
        let assembled_opt = match self.split_assembler.add(enc, now) {
            Ok(v) => v,
            Err(e) => {
                // If buffer is full, we return Error.
                // We have NOT marked the reliable index as seen.
                // Sender will timeout and retransmit. Ideally buffer clears by then.
                return Err(e);
            }
        };

        // If we reached here, the packet was either buffered or reassembled successfully.
        // For non-split reliable packets, commit the reliable index now.
        // For split packets, we avoid marking per-part indexes as seen; duplicates
        // are handled by split_assembler itself.
        if !is_split && let Some(idx) = ridx {
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
            Self::for_each_sequence_in_range(range, |seq| {
                if let Some(tracked) = self.sent_datagrams.remove(&seq)
                    && let crate::protocol::datagram::DatagramPayload::EncapsulatedPackets(_) =
                        &tracked.datagram.payload
                {
                    self.sliding
                        .on_ack(now, &tracked.datagram, seq, tracked.send_time);
                }
            });
        }
    }

    fn process_incoming_naks(&mut self, now: Instant) {
        while let Some(range) = self.incoming_naks.pop_front() {
            Self::for_each_sequence_in_range(range, |seq| {
                if let Some(mut tracked) = self.sent_datagrams.remove(&seq)
                    && let crate::protocol::datagram::DatagramPayload::EncapsulatedPackets(_) =
                        &tracked.datagram.payload
                {
                    self.sliding.on_nak();
                    tracked.next_send = now;
                    self.sent_datagrams.insert(seq, tracked);
                }
            });
        }
    }

    fn for_each_sequence_in_range<F>(range: SequenceRange, mut f: F)
    where
        F: FnMut(Sequence24),
    {
        let mut seq = range.start;
        loop {
            f(seq);
            if seq == range.end {
                break;
            }
            seq = seq.next();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

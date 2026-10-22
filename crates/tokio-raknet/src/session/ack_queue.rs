use std::collections::VecDeque;

use crate::protocol::{ack::SequenceRange, types::Sequence24};

/// Maintains a bounded, merged queue of ACK/NACK ranges.
/// Keeps payload size within MTU by merging adjacent ranges
/// and slicing on demand.
#[derive(Clone, Debug)]
pub struct AckQueue {
    max_ranges: usize,
    queue: VecDeque<SequenceRange>,
}

impl AckQueue {
    pub fn new(max_ranges: usize) -> Self {
        Self {
            max_ranges: max_ranges.min(crate::protocol::constants::MAX_ACK_SEQUENCES as usize),
            queue: VecDeque::new(),
        }
    }

    #[inline]
    pub fn push(&mut self, range: SequenceRange) {
        // Normalize wrapping ranges before comparing raw endpoints.
        if let Some((tail, head)) = range.split_wrapping() {
            self.push_nonwrapping(tail);
            self.push_nonwrapping(head);
        } else {
            self.push_nonwrapping(range);
        }
    }

    #[inline]
    fn push_nonwrapping(&mut self, range: SequenceRange) {
        if range.len() > crate::protocol::constants::MAX_ACK_SEQUENCES as u32 {
            self.push_chunks(range);
        } else {
            self.push_bounded(range);
        }
    }

    // Splitting is rare. Do not make bounded pushes recursive (and force the
    // common merge path out of line) just to support oversized local ranges.
    #[cold]
    fn push_chunks(&mut self, range: SequenceRange) {
        let max_sequences = crate::protocol::constants::MAX_ACK_SEQUENCES as u32;
        let mut start = range.start.value();
        while start <= range.end.value() && self.queue.len() < self.max_ranges {
            let end = range.end.value().min(start + max_sequences - 1);
            self.push_bounded(SequenceRange {
                start: crate::protocol::types::Sequence24::new(start),
                end: crate::protocol::types::Sequence24::new(end),
            });
            start = end + 1;
        }
    }

    #[inline]
    fn push_bounded(&mut self, range: SequenceRange) {
        let max_sequences = crate::protocol::constants::MAX_ACK_SEQUENCES as u32;
        if let Some(last) = self.queue.back_mut() {
            let start = last.start.value().min(range.start.value());
            let end = last.end.value().max(range.end.value());
            if range.start.value() <= last.end.value() + 1
                && last.start.value() <= range.end.value() + 1
                && end - start < max_sequences
            {
                last.start = crate::protocol::types::Sequence24::new(start);
                last.end = crate::protocol::types::Sequence24::new(end);
                return;
            }
        }
        if self.queue.len() >= self.max_ranges {
            return;
        }

        self.queue.push_back(range);
    }

    /// Pop a set of ranges whose encoded size (plus base_overhead bytes)
    /// fits within the provided MTU.
    pub fn pop_for_mtu(
        &mut self,
        mtu: usize,
        base_overhead: usize,
        ranges: &mut Vec<SequenceRange>,
    ) {
        let mut used = base_overhead;
        // go-raknet acknowledge.go:acknowledgement.read accepts 8192 sequences.
        let mut sequences: u32 = ranges.iter().map(SequenceRange::len).sum();

        while let Some(front) = self.queue.front() {
            let size = front.encoded_size();
            if used.saturating_add(size) > mtu
                || sequences.saturating_add(front.len())
                    > crate::protocol::constants::MAX_ACK_SEQUENCES as u32
            {
                break;
            }
            sequences += front.len();
            used += size;
            ranges.push(self.queue.pop_front().unwrap());
        }
    }

    /// Move every queued range into `out`, in queue order.
    pub(crate) fn drain_into(&mut self, out: &mut Vec<SequenceRange>) {
        out.extend(self.queue.drain(..));
    }
}

/// go-raknet `acknowledge.go:acknowledgement.write`, applied repeatedly by
/// `conn.go:sendAcknowledgement` until every sequence number is written.
///
/// Sequence numbers are sorted numerically and consecutive ones merged into
/// single/range records. Before each sequence number is added, a datagram is
/// closed when its length — one flag byte, two count bytes and the records
/// written so far, excluding the in-progress record — is at least
/// `effectiveMTU - 7` (`mtu - 35`). The 8192-sequence decoder limit of
/// RAKNET-01 also closes a datagram (unreachable at go's 100 ms ACK cadence).
pub(crate) fn go_ack_chunks(mut ranges: Vec<SequenceRange>, mtu: usize) -> Vec<Vec<SequenceRange>> {
    ranges.sort_unstable_by_key(|range| range.start.value());
    let mut merged: Vec<(u32, u32)> = Vec::with_capacity(ranges.len());
    for range in ranges {
        let (start, end) = (range.start.value(), range.end.value());
        match merged.last_mut() {
            Some(last) if start <= last.1.saturating_add(1) => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }

    let limit = mtu.saturating_sub(28 + 7);
    let max_sequences = crate::protocol::constants::MAX_ACK_SEQUENCES as u32;
    let record_size = |(first, last): (u32, u32)| if first == last { 4 } else { 7 };
    let to_range = |(first, last): (u32, u32)| SequenceRange {
        start: Sequence24::new(first),
        end: Sequence24::new(last),
    };

    let mut chunks = Vec::new();
    let mut index = 0;
    let mut next = merged.first().map_or(0, |range| range.0);
    while index < merged.len() {
        let mut len = 3;
        let mut count = 0u32;
        let mut records = Vec::new();
        let mut current: Option<(u32, u32)> = None;
        while index < merged.len() {
            let (first, end) = (next, merged[index].1);
            if len >= limit || count >= max_sequences {
                break;
            }
            if let Some(record) = current.replace((first, first)) {
                len += record_size(record);
                records.push(to_range(record));
            }
            count += 1;
            if first < end {
                // Later packets of this range see the same buffer length.
                if len >= limit {
                    next = first + 1;
                    break;
                }
                let take = (end - first).min(max_sequences - count);
                current = Some((first, first + take));
                count += take;
                if take < end - first {
                    next = first + take + 1;
                    break;
                }
            }
            index += 1;
            if let Some(range) = merged.get(index) {
                next = range.0;
            }
        }
        if let Some(record) = current {
            records.push(to_range(record));
        }
        chunks.push(records);
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::types::Sequence24;

    /// Literal port of go-raknet `acknowledgement.write` (per packet, bytes).
    fn go_writer(mut packets: Vec<u32>, effective_mtu: usize) -> Vec<Vec<u8>> {
        packets.sort_unstable();
        let mut out = Vec::new();
        while !packets.is_empty() {
            let mut buf = vec![0xc0, 0, 0];
            let mut records = 0u16;
            let (mut first, mut last) = (0u32, 0u32);
            let mut n = 0;
            let write = |buf: &mut Vec<u8>, first: u32, last: u32, records: &mut u16| {
                if first == last {
                    buf.push(1);
                    buf.extend_from_slice(&first.to_le_bytes()[..3]);
                } else {
                    buf.push(0);
                    buf.extend_from_slice(&first.to_le_bytes()[..3]);
                    buf.extend_from_slice(&last.to_le_bytes()[..3]);
                }
                *records += 1;
            };
            for (index, &pk) in packets.iter().enumerate() {
                if buf.len() >= effective_mtu - 7 {
                    break;
                }
                n += 1;
                if index == 0 {
                    (first, last) = (pk, pk);
                    continue;
                }
                if pk == last + 1 {
                    last = pk;
                    continue;
                }
                write(&mut buf, first, last, &mut records);
                (first, last) = (pk, pk);
            }
            write(&mut buf, first, last, &mut records);
            buf[1..3].copy_from_slice(&records.to_be_bytes());
            out.push(buf);
            packets.drain(..n);
        }
        out
    }

    fn encode_chunks(chunks: Vec<Vec<SequenceRange>>) -> Vec<Vec<u8>> {
        use crate::protocol::{ack::AckNackPayload, packet::RaknetEncodable};
        chunks
            .into_iter()
            .map(|ranges| {
                let mut buf = bytes::BytesMut::new();
                bytes::BufMut::put_u8(&mut buf, 0xc0);
                AckNackPayload { ranges }.encode_raknet(&mut buf).unwrap();
                buf.to_vec()
            })
            .collect()
    }

    fn ranges_of(packets: &[u32]) -> Vec<SequenceRange> {
        // Push in arrival order through the bounded queue, like the session.
        let mut queue = AckQueue::new(2048);
        for &seq in packets {
            queue.push(SequenceRange {
                start: Sequence24::new(seq),
                end: Sequence24::new(seq),
            });
        }
        let mut ranges = Vec::new();
        queue.drain_into(&mut ranges);
        ranges
    }

    #[test]
    fn vanilla_ack_chunks_match_go_writer() {
        // Out-of-order arrival: 1, 3, 2 merges into one record in go.
        let packets = [1, 3, 2, 7, 9, 10, 11, 20];
        assert_eq!(
            encode_chunks(go_ack_chunks(ranges_of(&packets), 1492)),
            go_writer(packets.to_vec(), 1492 - 28)
        );

        // Chunk boundaries, including a break on the second packet of a range
        // and on the first packet of a new range, at several MTUs.
        let mut state = 0x2545_f491u32;
        for mtu in [400usize, 401, 576, 1200, 1492] {
            for round in 0..40 {
                let mut packets = Vec::new();
                let mut seq = round * 7;
                for _ in 0..400 {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    seq += 1 + (state % 3);
                    packets.push(seq);
                    if state.is_multiple_of(4) {
                        packets.push(seq + 1);
                        seq += 1;
                    }
                }
                let mut shuffled = packets.clone();
                shuffled.rotate_left(round as usize % 17);
                assert_eq!(
                    encode_chunks(go_ack_chunks(ranges_of(&shuffled), mtu)),
                    go_writer(packets, mtu - 28),
                    "mtu {mtu} round {round}"
                );
            }
        }
    }

    #[test]
    fn merges_adjacent() {
        let mut q = AckQueue::new(16);
        q.push(SequenceRange {
            start: Sequence24::new(1),
            end: Sequence24::new(1),
        });
        q.push(SequenceRange {
            start: Sequence24::new(2),
            end: Sequence24::new(2),
        });

        let mut out = Vec::new();
        q.pop_for_mtu(1024, 0, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].start.value(), 1);
        assert_eq!(out[0].end.value(), 2);
    }

    #[test]
    fn respects_mtu() {
        let mut q = AckQueue::new(16);
        for i in 0..4 {
            q.push(SequenceRange {
                start: Sequence24::new(i * 2),
                end: Sequence24::new(i * 2),
            });
        }

        // Each single range encodes to 4 bytes, so with base_overhead 2,
        // mtu 6 only allows one.
        let mut out = Vec::new();
        q.pop_for_mtu(6, 2, &mut out);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn sequential_feedback_never_emits_an_oversized_range() {
        let mut queue = AckQueue::new(2);
        for index in 0..16385 {
            let sequence = Sequence24::new(index);
            queue.push(SequenceRange {
                start: sequence,
                end: sequence,
            });
        }
        assert_eq!(queue.queue.len(), 2);
        let mut ranges = Vec::new();
        queue.pop_for_mtu(400, 3, &mut ranges);
        assert_eq!(
            ranges.iter().map(SequenceRange::len).collect::<Vec<_>>(),
            vec![8192]
        );
        ranges.clear();
        queue.pop_for_mtu(400, 3, &mut ranges);
        assert_eq!(
            ranges.iter().map(SequenceRange::len).collect::<Vec<_>>(),
            vec![8192]
        );
    }

    #[test]
    fn cannot_emit_feedback_beyond_mtu() {
        let mut queue = AckQueue::new(1);
        queue.push(SequenceRange {
            start: Sequence24::new(1),
            end: Sequence24::new(1),
        });
        let mut ranges = Vec::new();
        queue.pop_for_mtu(6, 3, &mut ranges);
        assert!(ranges.is_empty());
    }

    #[test]
    fn wrapping_oversized_feedback_preserves_coverage_and_sequence_budget() {
        let mut queue = AckQueue::new(4);
        queue.push(SequenceRange {
            start: Sequence24::new(0xffffff),
            end: Sequence24::new(8192),
        });
        for (start, end) in [(0xffffff, 0xffffff), (0, 8191), (8192, 8192)] {
            let mut ranges = Vec::new();
            queue.pop_for_mtu(400, 3, &mut ranges);
            assert_eq!(
                ranges,
                [SequenceRange {
                    start: Sequence24::new(start),
                    end: Sequence24::new(end),
                }]
            );
        }
        let mut remaining = Vec::new();
        queue.pop_for_mtu(400, 3, &mut remaining);
        assert!(remaining.is_empty());
    }
}

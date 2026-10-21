use std::collections::VecDeque;

use crate::protocol::ack::SequenceRange;

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

    pub fn push(&mut self, range: SequenceRange) {
        // Split wrapping ranges before comparing raw endpoints.
        if let Some((tail, head)) = range.split_wrapping() {
            self.push(tail);
            self.push(head);
            return;
        }
        let max_sequences = crate::protocol::constants::MAX_ACK_SEQUENCES as u32;
        if range.len() > max_sequences {
            let mut start = range.start.value();
            while start <= range.end.value() && self.queue.len() < self.max_ranges {
                let end = range.end.value().min(start + max_sequences - 1);
                self.push(SequenceRange {
                    start: crate::protocol::types::Sequence24::new(start),
                    end: crate::protocol::types::Sequence24::new(end),
                });
                start = end + 1;
            }
            return;
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::types::Sequence24;

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
}

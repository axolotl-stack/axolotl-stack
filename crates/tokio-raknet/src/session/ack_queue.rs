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
            max_ranges,
            queue: VecDeque::new(),
        }
    }

    pub fn push(&mut self, range: SequenceRange) {
        if self.queue.len() >= self.max_ranges {
            return;
        }

        if let Some(last) = self.queue.back_mut() {
            if last.end.next() == range.start {
                last.end = range.end;
                return;
            }

            // Merge overlapping or contiguous
            if last.end >= range.start && range.end >= last.start {
                last.end = std::cmp::max(last.end, range.end);
                last.start = std::cmp::min(last.start, range.start);
                return;
            }
        }

        // Normalize wrapping ranges by splitting if needed
        if let Some((tail, head)) = range.split_wrapping() {
            self.push(tail);
            self.push(head);
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

        while let Some(front) = self.queue.front() {
            let size = front.encoded_size();
            // Check if adding this range would exceed MTU
            // But if ranges is empty, we MUST add at least one if possible (though base_overhead might be large)
            // The original logic allowed breaking early.
            if !ranges.is_empty() && used + size > mtu {
                break;
            }
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
                start: Sequence24::new(i),
                end: Sequence24::new(i),
            });
        }

        // Each single range encodes to 4 bytes, so with base_overhead 2,
        // mtu 6 only allows one.
        let mut out = Vec::new();
        q.pop_for_mtu(6, 2, &mut out);
        assert_eq!(out.len(), 1);
    }
}

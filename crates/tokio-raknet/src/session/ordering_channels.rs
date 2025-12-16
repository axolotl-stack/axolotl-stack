use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::protocol::encapsulated_packet::EncapsulatedPacket;
use crate::protocol::types::Sequence24;

#[derive(Eq, PartialEq)]
struct OrderedEncap {
    index: Sequence24,
    pkt: EncapsulatedPacket,
}

impl Ord for OrderedEncap {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.index.cmp(&other.index)
    }
}

impl PartialOrd for OrderedEncap {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Manages per-channel ordered delivery heaps.
pub struct OrderingChannels {
    order_read: Vec<Sequence24>,
    order_write: Vec<Sequence24>,
    heaps: Vec<BinaryHeap<Reverse<OrderedEncap>>>,
}

impl OrderingChannels {
    pub fn new(max_channels: usize) -> Self {
        Self {
            order_read: vec![Sequence24::new(0); max_channels],
            order_write: vec![Sequence24::new(0); max_channels],
            heaps: (0..max_channels).map(|_| BinaryHeap::new()).collect(),
        }
    }

    pub fn max_channels(&self) -> usize {
        self.order_read.len()
    }

    pub fn next_order_index(&mut self, channel: u8) -> Option<Sequence24> {
        let ch = channel as usize;
        if ch >= self.order_write.len() {
            return None;
        }
        let idx = self.order_write[ch];
        self.order_write[ch] = self.order_write[ch].next();
        Some(idx)
    }

    /// Force advance the read index if it matches the given index and
    /// return any buffered packets that become ready as a result.
    ///
    /// Used when an ordered packet is dropped (e.g. split timeout) so that
    /// downstream code can immediately deliver any newly unblocked packets.
    pub fn skip_index(
        &mut self,
        channel: u8,
        index: Sequence24,
    ) -> Option<Vec<EncapsulatedPacket>> {
        let ch = channel as usize;
        if ch >= self.order_read.len() {
            return None;
        }
        if self.order_read[ch] != index {
            return None;
        }

        self.order_read[ch] = self.order_read[ch].next();

        let mut ready = Vec::new();
        while let Some(top) = self.heaps[ch].peek() {
            if top.0.index == self.order_read[ch] {
                let Reverse(OrderedEncap { index: _, pkt }) = self.heaps[ch].pop().unwrap();
                self.order_read[ch] = self.order_read[ch].next();
                ready.push(pkt);
            } else {
                break;
            }
        }

        tracing::trace!(
            channel = ch,
            skipped = index.value(),
            released = ready.len(),
            "ordering_skip_release"
        );
        Some(ready)
    }

    /// Handle an ordered packet; returns a list of packets ready for decode in-order.
    pub fn handle_ordered(&mut self, enc: EncapsulatedPacket) -> Option<Vec<EncapsulatedPacket>> {
        let ch = enc.ordering_channel? as usize;
        if ch >= self.heaps.len() {
            return None;
        }
        let idx = enc.ordering_index?;

        if self.order_read[ch] < idx {
            // Prevent unbounded growth if a client skips sequences or floods.
            if self.heaps[ch].len() >= 2048 {
                tracing::warn!(
                    channel = ch,
                    "dropping ordered packet, buffer full (len=2048)"
                );
                return Some(Vec::new());
            }

            self.heaps[ch].push(Reverse(OrderedEncap {
                index: idx,
                pkt: enc,
            }));
            return Some(Vec::new());
        } else if self.order_read[ch] > idx {
            return Some(Vec::new());
        }

        self.order_read[ch] = self.order_read[ch].next();
        let mut ready = vec![enc];

        while let Some(top) = self.heaps[ch].peek() {
            if top.0.index == self.order_read[ch] {
                let Reverse(OrderedEncap { index: _, pkt }) = self.heaps[ch].pop().unwrap();
                self.order_read[ch] = self.order_read[ch].next();
                ready.push(pkt);
            } else {
                break;
            }
        }
        Some(ready)
    }
}

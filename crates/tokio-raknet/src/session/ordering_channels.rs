use std::collections::BTreeMap;

use super::receive_window::RECEIVE_WINDOW_SIZE;
use crate::protocol::encapsulated_packet::EncapsulatedPacket;
use crate::protocol::packet::DecodeError;
use crate::protocol::types::Sequence24;

/// Per-channel queues bounded by go-raknet conn.go:Conn.receivePacket's window.
pub struct OrderingChannels {
    order_read: Vec<Sequence24>,
    order_write: Vec<Sequence24>,
    buffered: Vec<BTreeMap<Sequence24, EncapsulatedPacket>>,
}

impl OrderingChannels {
    pub fn new(max_channels: usize) -> Self {
        Self {
            order_read: vec![Sequence24::new(0); max_channels],
            order_write: vec![Sequence24::new(0); max_channels],
            buffered: (0..max_channels).map(|_| BTreeMap::new()).collect(),
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
        while let Some(pkt) = self.buffered[ch].remove(&self.order_read[ch]) {
            self.order_read[ch] = self.order_read[ch].next();
            ready.push(pkt);
        }

        tracing::trace!(
            channel = ch,
            skipped = index.value(),
            released = ready.len(),
            "ordering_skip_release"
        );
        Some(ready)
    }

    /// Validate before split assembly so invalid order indexes cannot occupy slots.
    pub fn validate_ordered(&self, enc: &EncapsulatedPacket) -> Result<(), DecodeError> {
        let ch = enc
            .ordering_channel
            .ok_or(DecodeError::OrderingWindowExceeded)? as usize;
        let idx = enc
            .ordering_index
            .ok_or(DecodeError::OrderingWindowExceeded)?;
        let Some(&read) = self.order_read.get(ch) else {
            return Err(DecodeError::OrderingWindowExceeded);
        };
        let distance = read.distance_to(idx);
        if (RECEIVE_WINDOW_SIZE..=(1 << 23)).contains(&distance) {
            return Err(DecodeError::OrderingWindowExceeded);
        }
        Ok(())
    }

    /// Handle an ordered packet; returns a list of packets ready for decode in-order.
    pub fn handle_ordered(&mut self, enc: EncapsulatedPacket) -> Option<Vec<EncapsulatedPacket>> {
        self.validate_ordered(&enc).ok()?;
        let ch = enc.ordering_channel? as usize;
        if ch >= self.buffered.len() {
            return None;
        }
        let idx = enc.ordering_index?;

        if self.order_read[ch] < idx {
            // go-raknet packet_queue.go:packetQueue.put keeps one value per index.
            self.buffered[ch].entry(idx).or_insert(enc);
            return Some(Vec::new());
        } else if self.order_read[ch] > idx {
            return Some(Vec::new());
        }

        self.order_read[ch] = self.order_read[ch].next();
        let mut ready = vec![enc];

        while let Some(pkt) = self.buffered[ch].remove(&self.order_read[ch]) {
            self.order_read[ch] = self.order_read[ch].next();
            ready.push(pkt);
        }
        Some(ready)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::reliability::Reliability;
    use crate::protocol::types::EncapsulatedPacketHeader;
    use bytes::Bytes;

    /// Helper to create an ordered packet for testing
    fn make_ordered_packet(channel: u8, index: u32, payload: &[u8]) -> EncapsulatedPacket {
        EncapsulatedPacket {
            header: EncapsulatedPacketHeader::with_reliability(Reliability::ReliableOrdered),
            bit_length: (payload.len() * 8) as u16,
            reliable_index: Some(Sequence24::new(0)),
            sequence_index: None,
            ordering_index: Some(Sequence24::new(index)),
            ordering_channel: Some(channel),
            split: None,
            payload: Bytes::copy_from_slice(payload),
        }
    }

    #[test]
    fn hostile_ordering_index_does_not_buffer() {
        let mut channels = OrderingChannels::new(1);
        channels.handle_ordered(make_ordered_packet(0, 4096, b"hostile"));
        assert!(channels.buffered[0].is_empty());
    }

    #[test]
    fn buffered_duplicates_do_not_block_delivery() {
        let mut channels = OrderingChannels::new(1);
        channels.handle_ordered(make_ordered_packet(0, 1, b"first"));
        channels.handle_ordered(make_ordered_packet(0, 1, b"duplicate"));
        channels.handle_ordered(make_ordered_packet(0, 2, b"second"));
        let ready = channels
            .handle_ordered(make_ordered_packet(0, 0, b"zero"))
            .unwrap();
        assert_eq!(
            ready.iter().map(|p| p.payload.as_ref()).collect::<Vec<_>>(),
            vec![
                b"zero".as_slice(),
                b"first".as_slice(),
                b"second".as_slice()
            ]
        );
        assert!(channels.buffered[0].is_empty());
    }

    #[test]
    fn in_order_delivery() {
        let mut channels = OrderingChannels::new(16);

        // Send packets 0, 1, 2 in order
        let pkt0 = make_ordered_packet(0, 0, b"packet0");
        let pkt1 = make_ordered_packet(0, 1, b"packet1");
        let pkt2 = make_ordered_packet(0, 2, b"packet2");

        let ready0 = channels.handle_ordered(pkt0).unwrap();
        assert_eq!(ready0.len(), 1);
        assert_eq!(ready0[0].payload.as_ref(), b"packet0");

        let ready1 = channels.handle_ordered(pkt1).unwrap();
        assert_eq!(ready1.len(), 1);
        assert_eq!(ready1[0].payload.as_ref(), b"packet1");

        let ready2 = channels.handle_ordered(pkt2).unwrap();
        assert_eq!(ready2.len(), 1);
        assert_eq!(ready2[0].payload.as_ref(), b"packet2");
    }

    #[test]
    fn out_of_order_buffering() {
        let mut channels = OrderingChannels::new(16);

        // Send packets out of order: 2, 0, 1
        let pkt0 = make_ordered_packet(0, 0, b"packet0");
        let pkt1 = make_ordered_packet(0, 1, b"packet1");
        let pkt2 = make_ordered_packet(0, 2, b"packet2");

        // Packet 2 arrives first - should be buffered
        let ready2 = channels.handle_ordered(pkt2).unwrap();
        assert!(ready2.is_empty(), "Packet 2 should be buffered");

        // Packet 0 arrives - should release packet 0 only
        let ready0 = channels.handle_ordered(pkt0).unwrap();
        assert_eq!(ready0.len(), 1);
        assert_eq!(ready0[0].payload.as_ref(), b"packet0");

        // Packet 1 arrives - should release packets 1 and 2
        let ready1 = channels.handle_ordered(pkt1).unwrap();
        assert_eq!(ready1.len(), 2);
        assert_eq!(ready1[0].payload.as_ref(), b"packet1");
        assert_eq!(ready1[1].payload.as_ref(), b"packet2");
    }

    #[test]
    fn duplicate_packet_ignored() {
        let mut channels = OrderingChannels::new(16);

        let pkt0 = make_ordered_packet(0, 0, b"packet0");
        let pkt0_dup = make_ordered_packet(0, 0, b"packet0_dup");

        // First packet should be delivered
        let ready = channels.handle_ordered(pkt0).unwrap();
        assert_eq!(ready.len(), 1);

        // Duplicate should be ignored (empty result)
        let ready_dup = channels.handle_ordered(pkt0_dup).unwrap();
        assert!(ready_dup.is_empty());
    }

    #[test]
    fn multiple_channels_independent() {
        let mut channels = OrderingChannels::new(16);

        // Send to channel 0: index 1 (out of order)
        let pkt0_1 = make_ordered_packet(0, 1, b"ch0_pkt1");
        let ready = channels.handle_ordered(pkt0_1).unwrap();
        assert!(ready.is_empty());

        // Send to channel 1: index 0 (in order for channel 1)
        let pkt1_0 = make_ordered_packet(1, 0, b"ch1_pkt0");
        let ready = channels.handle_ordered(pkt1_0).unwrap();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].payload.as_ref(), b"ch1_pkt0");

        // Send to channel 0: index 0 (releases both 0 and 1)
        let pkt0_0 = make_ordered_packet(0, 0, b"ch0_pkt0");
        let ready = channels.handle_ordered(pkt0_0).unwrap();
        assert_eq!(ready.len(), 2);
        assert_eq!(ready[0].payload.as_ref(), b"ch0_pkt0");
        assert_eq!(ready[1].payload.as_ref(), b"ch0_pkt1");
    }

    #[test]
    fn invalid_channel_returns_none() {
        let mut channels = OrderingChannels::new(4); // Only 4 channels

        let pkt = make_ordered_packet(10, 0, b"invalid"); // Channel 10 > max
        let result = channels.handle_ordered(pkt);
        assert!(result.is_none());
    }

    #[test]
    fn missing_ordering_info_returns_none() {
        let mut channels = OrderingChannels::new(16);

        // Packet without ordering_channel
        let mut pkt = make_ordered_packet(0, 0, b"test");
        pkt.ordering_channel = None;
        let result = channels.handle_ordered(pkt);
        assert!(result.is_none());

        // Packet without ordering_index
        let mut pkt = make_ordered_packet(0, 0, b"test");
        pkt.ordering_index = None;
        let result = channels.handle_ordered(pkt);
        assert!(result.is_none());
    }

    #[test]
    fn next_order_index_increments() {
        let mut channels = OrderingChannels::new(16);

        assert_eq!(channels.next_order_index(0).unwrap().value(), 0);
        assert_eq!(channels.next_order_index(0).unwrap().value(), 1);
        assert_eq!(channels.next_order_index(0).unwrap().value(), 2);

        // Different channel starts at 0
        assert_eq!(channels.next_order_index(1).unwrap().value(), 0);
    }

    #[test]
    fn next_order_index_invalid_channel() {
        let mut channels = OrderingChannels::new(4);

        assert!(channels.next_order_index(10).is_none());
    }

    #[test]
    fn skip_index_releases_buffered() {
        let mut channels = OrderingChannels::new(16);

        // Buffer packets 1 and 2
        let pkt1 = make_ordered_packet(0, 1, b"packet1");
        let pkt2 = make_ordered_packet(0, 2, b"packet2");

        channels.handle_ordered(pkt1).unwrap();
        channels.handle_ordered(pkt2).unwrap();

        // Skip index 0 (as if it was lost/dropped)
        let released = channels.skip_index(0, Sequence24::new(0)).unwrap();

        // Both buffered packets should be released
        assert_eq!(released.len(), 2);
        assert_eq!(released[0].payload.as_ref(), b"packet1");
        assert_eq!(released[1].payload.as_ref(), b"packet2");
    }

    #[test]
    fn skip_index_wrong_index_does_nothing() {
        let mut channels = OrderingChannels::new(16);

        // Buffer packet 1
        let pkt1 = make_ordered_packet(0, 1, b"packet1");
        channels.handle_ordered(pkt1).unwrap();

        // Try to skip index 5 (not the expected index 0)
        let released = channels.skip_index(0, Sequence24::new(5));
        assert!(released.is_none());
    }

    #[test]
    fn skip_index_invalid_channel() {
        let mut channels = OrderingChannels::new(4);

        let released = channels.skip_index(10, Sequence24::new(0));
        assert!(released.is_none());
    }

    #[test]
    fn max_channels() {
        let channels = OrderingChannels::new(8);
        assert_eq!(channels.max_channels(), 8);
    }

    #[test]
    fn large_gap_buffers_correctly() {
        let mut channels = OrderingChannels::new(16);

        // Send packets with a large gap: 0, 5, 10
        let pkt0 = make_ordered_packet(0, 0, b"pkt0");
        let pkt5 = make_ordered_packet(0, 5, b"pkt5");
        let pkt10 = make_ordered_packet(0, 10, b"pkt10");

        // Packet 0 delivered immediately
        let ready = channels.handle_ordered(pkt0).unwrap();
        assert_eq!(ready.len(), 1);

        // Packets 5 and 10 buffered (waiting for 1-4 and 6-9)
        let ready = channels.handle_ordered(pkt5).unwrap();
        assert!(ready.is_empty());

        let ready = channels.handle_ordered(pkt10).unwrap();
        assert!(ready.is_empty());

        // Now send 1, 2, 3, 4 - only 1-4 released, 5 waits
        for i in 1..=4 {
            let pkt = make_ordered_packet(0, i, format!("pkt{}", i).as_bytes());
            let ready = channels.handle_ordered(pkt).unwrap();
            if i < 4 {
                assert_eq!(ready.len(), 1); // Just this packet
            } else {
                // Packet 4 releases 4 and 5
                assert_eq!(ready.len(), 2);
            }
        }
    }
}

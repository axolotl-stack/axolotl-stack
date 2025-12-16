//! RakNet per-peer session state and reliability/ordering logic.
//! Keeps the Session struct and shared helpers; per-area logic lives in
//! submodules (outbound, inbound, tick).
//!
//! The `Session` struct is the core of the RakNet protocol implementation.
//! It manages:
//! - Reliability (tracking sent packets, handling ACKs/NACKs)
//! - Ordering (ensuring ordered packets arrive in sequence)
//! - Fragmentation (splitting/reassembling large packets)
//! - Congestion Control (sliding window)

pub mod ack_queue;
mod inbound;
pub mod manager;
mod ordering_channels;
mod outbound;
mod reliable_tracker;
mod sliding_window;
pub mod split_assembler;
mod tick;

use std::{
    cmp::Ordering,
    collections::{BTreeMap, BinaryHeap, VecDeque},
    time::{Duration, Instant},
};

use crate::protocol::{
    constants::{self, MAX_ACK_SEQUENCES},
    datagram::Datagram,
    encapsulated_packet::EncapsulatedPacket,
    packet::{DecodeError, RaknetPacket},
    reliability::Reliability,
    types::Sequence24,
};

use crate::protocol::ack::SequenceRange;
use ack_queue::AckQueue;
use ordering_channels::OrderingChannels;
use reliable_tracker::ReliableTracker;
use sliding_window::SlidingWindow;
use split_assembler::SplitAssembler;

/// Packet decoded out of a session along with delivery metadata.
pub struct IncomingPacket {
    pub packet: RaknetPacket,
    pub reliability: Reliability,
    pub ordering_channel: Option<u8>,
}

/// Tunable low-level session parameters to mirror Cloudburst configurability.
#[derive(Debug, Clone)]
pub struct SessionTunables {
    pub max_ordering_channels: usize,
    pub ack_queue_capacity: usize,
    pub split_timeout: Duration,
    pub reliable_window: u32,
    pub max_split_parts: u32,
    pub max_concurrent_splits: usize,
}

impl Default for SessionTunables {
    fn default() -> Self {
        Self {
            max_ordering_channels: constants::MAXIMUM_ORDERING_CHANNELS as usize,
            ack_queue_capacity: 1024,
            // Reduced split timeout to clear dead buffers faster
            split_timeout: Duration::from_secs(30),
            reliable_window: constants::MAX_ACK_SEQUENCES as u32,
            max_split_parts: 8192,
            max_concurrent_splits: 4096,
        }
    }
}

struct TrackedDatagram {
    datagram: Datagram,
    send_time: Instant,
    next_send: Instant,
}

struct QueuedEncap {
    weight: u64,
    pkt: EncapsulatedPacket,
}
impl PartialEq for QueuedEncap {
    fn eq(&self, other: &Self) -> bool {
        self.weight == other.weight
    }
}
impl Eq for QueuedEncap {}
impl PartialOrd for QueuedEncap {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for QueuedEncap {
    fn cmp(&self, other: &Self) -> Ordering {
        other.weight.cmp(&self.weight)
    }
}

pub struct Session {
    mtu: usize,

    sliding: SlidingWindow,
    split_index: u16,
    datagram_read_index: Sequence24,
    datagram_write_index: Sequence24,
    sequence_write_index: Sequence24,
    reliability_write_index: Sequence24,
    split_assembler: SplitAssembler,
    ordering: OrderingChannels,
    reliable_tracker: ReliableTracker,
    outgoing_heap: BinaryHeap<QueuedEncap>,
    outgoing_packet_next_weights: [u64; 4],
    last_min_weight: u64,
    sent_datagrams: BTreeMap<Sequence24, TrackedDatagram>,
    incoming_acks: VecDeque<SequenceRange>,
    incoming_naks: VecDeque<SequenceRange>,
    outgoing_acks: AckQueue,
    outgoing_naks: AckQueue,
}

impl Session {
    pub fn new(mtu: usize) -> Self {
        Self::with_tunables(mtu, SessionTunables::default())
    }

    pub fn with_max_channels(mtu: usize, max_channels: usize) -> Self {
        let tunables = SessionTunables {
            max_ordering_channels: max_channels,
            ..Default::default()
        };
        Self::with_tunables(mtu, tunables)
    }

    pub fn with_tunables(mtu: usize, tunables: SessionTunables) -> Self {
        let mut s = Self {
            mtu,
            sliding: SlidingWindow::new(mtu),
            split_index: 0,
            datagram_read_index: Sequence24::new(0),
            datagram_write_index: Sequence24::new(0),
            sequence_write_index: Sequence24::new(0),
            reliability_write_index: Sequence24::new(0),
            split_assembler: SplitAssembler::new(
                tunables.split_timeout,
                tunables.max_split_parts,
                tunables.max_concurrent_splits,
            ),
            ordering: OrderingChannels::new(tunables.max_ordering_channels),
            reliable_tracker: ReliableTracker::new(tunables.reliable_window as usize),
            outgoing_heap: BinaryHeap::new(),
            outgoing_packet_next_weights: [0; 4],
            last_min_weight: 0,
            sent_datagrams: BTreeMap::new(),
            incoming_acks: VecDeque::new(),
            incoming_naks: VecDeque::new(),
            outgoing_acks: AckQueue::new(tunables.ack_queue_capacity),
            outgoing_naks: AckQueue::new(tunables.ack_queue_capacity),
        };

        for level in 0..4 {
            s.outgoing_packet_next_weights[level] = ((1u64 << level) * level as u64) + level as u64;
        }

        s
    }

    pub fn mtu(&self) -> usize {
        self.mtu
    }

    /// Process datagram sequence for ACK/NACK generation (Cloudburst-style).
    #[tracing::instrument(skip_all, level = "trace")]
    pub fn process_datagram_sequence(&mut self, seq: Sequence24) {
        let prev = self.datagram_read_index;

        if prev > seq {
            let range = SequenceRange {
                start: seq,
                end: seq,
            };
            tracing::trace!(event = "ack_out_of_order");
            self.outgoing_acks.push(range);
            return;
        }

        self.datagram_read_index = seq.next();

        if seq == prev {
            let range = SequenceRange {
                start: seq,
                end: seq,
            };
            tracing::trace!(event = "ack_in_order");
            self.outgoing_acks.push(range);
            return;
        }

        let mut nack_start = prev;
        let nack_end_inclusive = seq.prev();
        let missing = prev.distance_to(seq).saturating_sub(1);

        if missing > 0 {
            tracing::trace!("datagram_gap");
        }

        loop {
            let mut chunk_end = nack_start;
            let mut count = 0;

            while count < (MAX_ACK_SEQUENCES - 1) && chunk_end < nack_end_inclusive {
                chunk_end = chunk_end.next();
                count += 1;
            }

            let range = SequenceRange {
                start: nack_start,
                end: chunk_end,
            };
            tracing::trace!(event = "queue_nak");
            self.outgoing_naks.push(range);

            if chunk_end == nack_end_inclusive {
                break;
            }

            nack_start = chunk_end.next();
        }

        let ack_range = SequenceRange {
            start: seq,
            end: seq,
        };
        tracing::trace!(event = "ack_gap_end");
        self.outgoing_acks.push(ack_range);
    }

    /// Handle ordered delivery via per-channel heaps.
    fn handle_ordered(
        &mut self,
        enc: EncapsulatedPacket,
        out: &mut Vec<IncomingPacket>,
    ) -> Result<(), DecodeError> {
        if let Some(ready) = self.ordering.handle_ordered(enc) {
            for pkt in ready {
                self.decode_and_push(pkt, out)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn tunables_limit_ack_queue_capacity() {
        let tunables = SessionTunables {
            ack_queue_capacity: 1,
            ..Default::default()
        };
        let mut session = Session::with_tunables(1200, tunables);

        session.process_datagram_sequence(Sequence24::new(0));
        session.process_datagram_sequence(Sequence24::new(1));

        let out = session.on_tick(Instant::now());
        let ack = out
            .iter()
            .find(|d| {
                d.header
                    .flags
                    .contains(crate::protocol::constants::DatagramFlags::ACK)
            })
            .expect("ack datagram present");

        if let crate::protocol::datagram::DatagramPayload::Ack(payload) = &ack.payload {
            assert_eq!(payload.ranges.len(), 1);
        } else {
            panic!("expected ack datagram");
        }
    }
}

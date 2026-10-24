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
pub mod policy;
mod receive_window;
mod reliable_tracker;
mod sliding_window;
pub mod split_assembler;
mod tick;
mod vanilla;

use std::{
    cmp::Ordering,
    collections::{BTreeMap, BinaryHeap, VecDeque},
    time::{Duration, Instant},
};
use thiserror::Error;

use crate::protocol::{
    constants,
    datagram::Datagram,
    encapsulated_packet::EncapsulatedPacket,
    packet::{DecodeError, EncodeError, RaknetPacket},
    reliability::Reliability,
    types::Sequence24,
};

use crate::protocol::ack::SequenceRange;
use ack_queue::AckQueue;
use ordering_channels::OrderingChannels;
pub use policy::TransmissionPolicy;
use reliable_tracker::ReliableTracker;
use sliding_window::SlidingWindow;
use split_assembler::SplitAssembler;
use vanilla::VanillaState;

/// Packet decoded out of a session along with delivery metadata.
pub struct IncomingPacket {
    pub packet: RaknetPacket,
    pub reliability: Reliability,
    pub ordering_channel: Option<u8>,
}

#[derive(Error, Debug)]
pub enum QueuePacketError {
    #[error("ordering channel {channel} out of range (max channels: {max_channels})")]
    InvalidOrderingChannel { channel: u8, max_channels: usize },
    #[error("outgoing packet queue is full")]
    QueueFull,
    #[error(transparent)]
    Encode(#[from] EncodeError),
}

/// Low-level parameters bounded by the lunar-bedrock/go-raknet `87a6b7e` receive limits.
#[derive(Debug, Clone)]
pub struct SessionTunables {
    pub max_ordering_channels: usize,
    pub ack_queue_capacity: usize,
    pub split_timeout: Duration,
    pub reliable_window: u32,
    pub max_split_parts: u32,
    pub max_concurrent_splits: usize,
    /// Maximum number of incoming ACK/NACK ranges to queue before dropping oldest.
    /// Prevents OOM from malicious peers sending excessive ACK/NACK packets.
    pub max_incoming_ack_queue: usize,
    /// Transmission policy (framing, congestion control, resend, ACK cadence).
    pub transmission: TransmissionPolicy,
}

impl Default for SessionTunables {
    fn default() -> Self {
        Self {
            max_ordering_channels: constants::MAXIMUM_ORDERING_CHANNELS as usize,
            // Local bounded ACK staging; upstream conn.go:flushACKs flushes each tick.
            ack_queue_capacity: receive_window::RECEIVE_WINDOW_SIZE as usize,
            // Safety deviation: upstream Conn.receiveSplitPacket retains incomplete splits.
            split_timeout: Duration::from_secs(30),
            reliable_window: receive_window::RECEIVE_WINDOW_SIZE,
            // go-raknet `87a6b7e` conn.go:Conn.receiveSplitPacket.
            max_split_parts: 512,
            max_concurrent_splits: 16,
            max_incoming_ack_queue: 4096,
            transmission: TransmissionPolicy::Optimized,
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
    /// `Some` selects `TransmissionPolicy::Vanilla`. Boxed so Optimized
    /// sessions keep their pre-policy size and never allocate it.
    vanilla: Option<Box<VanillaState>>,

    sliding: SlidingWindow,
    split_index: u16,
    receive_window: receive_window::ReceiveWindow,
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
    pending_incoming: VecDeque<IncomingPacket>,
    outgoing_acks: AckQueue,
    outgoing_naks: AckQueue,
    /// Maximum number of incoming ACK/NACK ranges to queue.
    max_incoming_ack_queue: usize,
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
        // Preserve infallible low-level constructors while enforcing hard safety ceilings.
        // Transport constructors return typed configuration errors instead.
        let mtu = mtu.clamp(
            constants::MINIMUM_MTU_SIZE as usize,
            constants::MAXIMUM_MTU_SIZE as usize,
        );
        let mut s = Self {
            mtu,
            vanilla: tunables
                .transmission
                .is_vanilla()
                .then(|| Box::new(VanillaState::new(mtu))),
            sliding: SlidingWindow::new(mtu),
            split_index: 0,
            receive_window: receive_window::ReceiveWindow::default(),
            datagram_write_index: Sequence24::new(0),
            sequence_write_index: Sequence24::new(0),
            reliability_write_index: Sequence24::new(0),
            split_assembler: SplitAssembler::new(
                tunables.split_timeout,
                tunables.max_split_parts,
                tunables.max_concurrent_splits,
            ),
            ordering: OrderingChannels::new(tunables.max_ordering_channels.min(256)),
            reliable_tracker: ReliableTracker::new(
                tunables
                    .reliable_window
                    .min(receive_window::RECEIVE_WINDOW_SIZE) as usize,
            ),
            outgoing_heap: BinaryHeap::new(),
            outgoing_packet_next_weights: [0; 4],
            last_min_weight: 0,
            sent_datagrams: BTreeMap::new(),
            incoming_acks: VecDeque::new(),
            incoming_naks: VecDeque::new(),
            pending_incoming: VecDeque::new(),
            outgoing_acks: AckQueue::new(
                tunables
                    .ack_queue_capacity
                    .min(receive_window::RECEIVE_WINDOW_SIZE as usize),
            ),
            outgoing_naks: AckQueue::new(
                tunables
                    .ack_queue_capacity
                    .min(receive_window::RECEIVE_WINDOW_SIZE as usize),
            ),
            max_incoming_ack_queue: tunables
                .max_incoming_ack_queue
                .min(constants::MAX_ACK_SEQUENCES as usize),
        };

        for level in 0..4 {
            s.outgoing_packet_next_weights[level] = ((1u64 << level) * level as u64) + level as u64;
        }

        s
    }

    pub fn mtu(&self) -> usize {
        self.mtu
    }

    pub fn transmission_policy(&self) -> TransmissionPolicy {
        if self.is_vanilla() {
            TransmissionPolicy::Vanilla
        } else {
            TransmissionPolicy::Optimized
        }
    }

    /// Latest RTT estimate in microseconds (`u64::MAX` before the first
    /// sample): Optimized's smoothed ACK RTT, Vanilla's `estimatedRTT` of
    /// Mojang's sliding window.
    pub fn rtt_micros(&self) -> u64 {
        match &self.vanilla {
            Some(vanilla) => vanilla.cc.rtt_micros(),
            None => self.sliding.rtt_micros(),
        }
    }

    #[inline]
    fn is_vanilla(&self) -> bool {
        self.vanilla.is_some()
    }

    /// Compatibility entry point; invalid sequences are ignored without feedback.
    /// Managed sessions use the checked path to close on protocol violations.
    #[tracing::instrument(skip_all, level = "trace")]
    pub fn process_datagram_sequence(&mut self, seq: Sequence24) {
        let _ = self.accept_datagram_sequence(seq, Instant::now);
    }

    fn accept_datagram_sequence(
        &mut self,
        seq: Sequence24,
        now: impl FnOnce() -> Instant,
    ) -> Result<bool, DecodeError> {
        if self.is_vanilla() {
            return self.vanilla_accept_sequence(seq, now());
        }
        let delay = self.sliding.nak_delay();
        let naks = &mut self.outgoing_naks;
        if !self
            .receive_window
            .receive(seq, now, delay, |range| naks.push(range))?
        {
            return Ok(false);
        }
        self.outgoing_acks.push(SequenceRange {
            start: seq,
            end: seq,
        });
        Ok(true)
    }

    /// Handle ordered delivery via bounded per-channel queues.
    fn handle_ordered(
        &mut self,
        enc: EncapsulatedPacket,
        out: &mut Vec<IncomingPacket>,
    ) -> Result<(), DecodeError> {
        self.ordering.validate_ordered(&enc)?;
        if let Some(ready) = self.ordering.handle_ordered(enc) {
            for pkt in ready {
                self.decode_and_push(pkt, out)?;
            }
        }
        Ok(())
    }

    pub fn drain_pending_incoming(&mut self) -> Vec<IncomingPacket> {
        self.pending_incoming.drain(..).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ack::{AckNackPayload, SequenceRange};
    use std::time::Instant;

    #[test]
    fn hostile_datagram_gap_does_not_advance_or_queue_feedback() {
        let mut session = Session::new(1200);
        session.process_datagram_sequence(Sequence24::new(1 << 23));
        assert_eq!(session.receive_window.next(), Sequence24::new(0));
        assert_eq!(session.receive_window.len(), 0);
        let feedback = session.on_tick(Instant::now());
        assert!(feedback.is_empty());
    }

    #[test]
    fn nak_aging_tracks_rtt_updates_at_the_exact_boundary() {
        use crate::protocol::{
            constants::DatagramFlags, datagram::DatagramPayload, types::DatagramHeader,
        };

        let now = Instant::now();
        let mut session = Session::new(1200);
        let datagram = Datagram {
            header: DatagramHeader {
                flags: DatagramFlags::VALID,
                sequence: Sequence24::new(0),
            },
            payload: DatagramPayload::EncapsulatedPackets(Vec::new()),
        };
        session.sliding.on_ack(
            now,
            &datagram,
            Sequence24::new(0),
            now - Duration::from_millis(100),
        );
        session
            .accept_datagram_sequence(Sequence24::new(2), || now)
            .unwrap();
        let later = now + Duration::from_millis(149);
        session
            .accept_datagram_sequence(Sequence24::new(4), || later)
            .unwrap();
        session.sliding.on_ack(
            later,
            &datagram,
            Sequence24::new(1),
            later - Duration::from_millis(300),
        );
        // The smoothed RTT is now 110ms, moving the NAK boundary from 150 to 165ms.
        session
            .accept_datagram_sequence(Sequence24::new(6), || now + Duration::from_millis(150))
            .unwrap();
        assert!(
            session
                .on_tick(now + Duration::from_millis(150))
                .iter()
                .all(|datagram| { !matches!(datagram.payload, DatagramPayload::Nak(_)) })
        );
        session
            .accept_datagram_sequence(Sequence24::new(8), || now + Duration::from_millis(165))
            .unwrap();
        let feedback = session.on_tick(now + Duration::from_millis(165));
        let ranges: Vec<_> = feedback
            .iter()
            .filter_map(|datagram| match &datagram.payload {
                DatagramPayload::Nak(payload) => Some(payload.ranges.as_slice()),
                _ => None,
            })
            .flatten()
            .copied()
            .collect();
        assert_eq!(
            ranges,
            [SequenceRange {
                start: Sequence24::new(0),
                end: Sequence24::new(1),
            }]
        );
    }

    #[test]
    fn peer_triggered_responses_cannot_grow_outgoing_queue() {
        use crate::protocol::{packet::ConnectedPong, state::RakPriority, types::RaknetTime};
        let mut session = Session::new(1200);
        for _ in 0..receive_window::RECEIVE_WINDOW_SIZE {
            session
                .queue_packet(
                    RaknetPacket::ConnectedPong(ConnectedPong {
                        ping_time: RaknetTime(0),
                        pong_time: RaknetTime(0),
                    }),
                    Reliability::Unreliable,
                    0,
                    RakPriority::Immediate,
                )
                .unwrap();
        }
        assert!(
            session
                .queue_packet(
                    RaknetPacket::ConnectedPong(ConnectedPong {
                        ping_time: RaknetTime(0),
                        pong_time: RaknetTime(0)
                    }),
                    Reliability::Unreliable,
                    0,
                    RakPriority::Immediate,
                )
                .is_err()
        );
        assert_eq!(
            session.outgoing_heap.len(),
            receive_window::RECEIVE_WINDOW_SIZE as usize
        );
    }

    #[test]
    fn retransmitted_loss_recovers_ordered_delivery() {
        use crate::protocol::{datagram::DatagramPayload, state::RakPriority};
        let now = Instant::now();
        let mut sender = Session::new(1200);
        let mut receiver = Session::new(1200);
        let mut sent = Vec::new();
        for id in [0xfe, 0xff] {
            sender
                .queue_packet(
                    RaknetPacket::UserData {
                        id,
                        payload: bytes::Bytes::from_static(b"data"),
                    },
                    Reliability::ReliableOrdered,
                    0,
                    RakPriority::Normal,
                )
                .unwrap();
            sent.push(sender.build_data_datagram(now).unwrap());
        }
        let later = sent.pop().unwrap();
        assert!(
            receiver
                .accept_datagram_sequence(later.header.sequence, || now)
                .unwrap()
        );
        let DatagramPayload::EncapsulatedPackets(packets) = later.payload else {
            unreachable!()
        };
        assert!(
            receiver
                .handle_data_payload(packets, now)
                .unwrap()
                .is_empty()
        );
        for feedback in receiver.on_tick(now) {
            if let DatagramPayload::Nak(payload) = feedback.payload {
                sender.handle_nack_payload(payload);
            }
        }
        let retransmitted = sender
            .on_tick(now)
            .into_iter()
            .find(|d| matches!(d.payload, DatagramPayload::EncapsulatedPackets(_)))
            .unwrap();
        assert!(
            receiver
                .accept_datagram_sequence(retransmitted.header.sequence, || now)
                .unwrap()
        );
        let DatagramPayload::EncapsulatedPackets(packets) = retransmitted.payload else {
            unreachable!()
        };
        let received = receiver.handle_data_payload(packets, now).unwrap();
        assert_eq!(
            received.iter().map(|p| p.packet.id()).collect::<Vec<_>>(),
            vec![0xfe, 0xff]
        );
    }

    #[test]
    fn fragmented_message_does_not_stall_reliable_tracking() {
        use crate::protocol::{datagram::DatagramPayload, state::RakPriority};
        let now = Instant::now();
        let mut sender = Session::new(1200);
        let mut receiver = Session::new(1200);
        sender
            .queue_packet(
                RaknetPacket::UserData {
                    id: 0xfe,
                    payload: bytes::Bytes::from(vec![0; 1500]),
                },
                Reliability::Reliable,
                0,
                RakPriority::Normal,
            )
            .unwrap();
        let mut fragments = Vec::new();
        while let Some(datagram) = sender.build_data_datagram(now) {
            let seq = datagram.header.sequence;
            sender.handle_ack_payload(AckNackPayload {
                ranges: vec![SequenceRange {
                    start: seq,
                    end: seq,
                }],
            });
            sender.process_incoming_acks_naks(now);
            if let DatagramPayload::EncapsulatedPackets(packets) = datagram.payload {
                fragments.extend(packets);
            }
        }
        assert_eq!(
            receiver.handle_data_payload(fragments, now).unwrap().len(),
            1
        );
        for _ in 0..2050 {
            sender
                .queue_packet(
                    RaknetPacket::UserData {
                        id: 0xff,
                        payload: bytes::Bytes::new(),
                    },
                    Reliability::Reliable,
                    0,
                    RakPriority::Normal,
                )
                .unwrap();
            let datagram = sender.build_data_datagram(now).unwrap();
            let seq = datagram.header.sequence;
            let DatagramPayload::EncapsulatedPackets(packets) = datagram.payload else {
                unreachable!()
            };
            assert_eq!(receiver.handle_data_payload(packets, now).unwrap().len(), 1);
            sender.handle_ack_payload(AckNackPayload {
                ranges: vec![SequenceRange {
                    start: seq,
                    end: seq,
                }],
            });
            sender.process_incoming_acks_naks(now);
        }
    }

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

    #[test]
    fn incoming_ack_queue_respects_limit() {
        let tunables = SessionTunables {
            max_incoming_ack_queue: 3,
            ..Default::default()
        };
        let mut session = Session::with_tunables(1200, tunables);

        // Add 5 ACK ranges, but only 3 should be kept
        let payload = AckNackPayload {
            ranges: vec![
                SequenceRange {
                    start: Sequence24::new(0),
                    end: Sequence24::new(0),
                },
                SequenceRange {
                    start: Sequence24::new(1),
                    end: Sequence24::new(1),
                },
                SequenceRange {
                    start: Sequence24::new(2),
                    end: Sequence24::new(2),
                },
                SequenceRange {
                    start: Sequence24::new(3),
                    end: Sequence24::new(3),
                },
                SequenceRange {
                    start: Sequence24::new(4),
                    end: Sequence24::new(4),
                },
            ],
        };

        session.handle_ack_payload(payload);

        // Should only have 3 entries (oldest dropped)
        assert_eq!(session.incoming_acks.len(), 3);

        // The oldest entries (0, 1) should have been dropped
        let first = session.incoming_acks.front().unwrap();
        assert_eq!(first.start, Sequence24::new(2));
    }

    #[test]
    fn incoming_ack_queue_drops_overwide_ranges() {
        let mut session = Session::with_tunables(1200, SessionTunables::default());

        session.handle_ack_payload(AckNackPayload {
            ranges: vec![SequenceRange {
                start: Sequence24::new(0),
                end: Sequence24::new(constants::MAX_INBOUND_ACK_SEQUENCES),
            }],
        });

        assert!(session.incoming_acks.is_empty());
    }

    #[test]
    fn incoming_nack_queue_respects_limit() {
        let tunables = SessionTunables {
            max_incoming_ack_queue: 2,
            ..Default::default()
        };
        let mut session = Session::with_tunables(1200, tunables);

        // Add 4 NACK ranges, but only 2 should be kept
        let payload = AckNackPayload {
            ranges: vec![
                SequenceRange {
                    start: Sequence24::new(10),
                    end: Sequence24::new(10),
                },
                SequenceRange {
                    start: Sequence24::new(20),
                    end: Sequence24::new(20),
                },
                SequenceRange {
                    start: Sequence24::new(30),
                    end: Sequence24::new(30),
                },
                SequenceRange {
                    start: Sequence24::new(40),
                    end: Sequence24::new(40),
                },
            ],
        };

        session.handle_nack_payload(payload);

        // Should only have 2 entries
        assert_eq!(session.incoming_naks.len(), 2);

        // The oldest entries should have been dropped
        let first = session.incoming_naks.front().unwrap();
        assert_eq!(first.start, Sequence24::new(30));
    }
}

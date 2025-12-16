use std::time::Instant;

use bytes::BytesMut;

use crate::protocol::{
    ack::AckNackPayload,
    constants,
    datagram::{Datagram, DatagramPayload},
    encapsulated_packet::EncapsulatedPacket,
    packet::RaknetPacket,
    reliability::Reliability,
    state::RakPriority,
    types::{EncapsulatedPacketHeader, Sequence24},
};

use super::{QueuedEncap, Session, TrackedDatagram};

impl Session {
    pub fn queue_packet(
        &mut self,
        pkt: RaknetPacket,
        reliability: Reliability,
        channel: u8,
        priority: RakPriority,
    ) -> usize {
        if channel as usize >= self.ordering.max_channels() {
            return 0;
        }
        let mut payload_buf = BytesMut::new();
        if pkt.encode(&mut payload_buf).is_err() {
            return 0;
        }
        let payload = payload_buf.freeze();

        let max_len = self.max_encapsulated_payload_len(reliability, false).max(1);

        if payload.len() <= max_len {
            self.enqueue_single_encap(payload, reliability, channel, priority)
        } else {
            self.enqueue_fragmented_encaps(payload, reliability, channel, priority)
        }
    }

    /// Build the next DATA datagram to send, if any, respecting MTU and sliding window.
    pub fn build_data_datagram(&mut self, now: Instant) -> Option<Datagram> {
        if self.outgoing_heap.is_empty() {
            return None;
        }

        let mut transmission_bw = self.sliding.get_transmission_bandwidth();
        if transmission_bw == 0 {
            return None;
        }

        let mut packets = Vec::new();
        // Account for IP + UDP headers so the full on-wire packet stays within the
        // negotiated MTU and avoids downstream fragmentation.
        let mut current_size = constants::IPV4_HEADER_SIZE
            + constants::UDP_HEADER_SIZE
            + constants::RAKNET_DATAGRAM_HEADER_SIZE;

        self.fill_datagram(&mut packets, &mut current_size, &mut transmission_bw);

        if packets.is_empty() {
            return None;
        }

        let seq = self.datagram_write_index;
        self.datagram_write_index = self.datagram_write_index.next();

        let has_reliable = packets.iter().any(|p| p.header.reliability.is_reliable());
        let has_split = packets.iter().any(|p| p.header.is_split);

        let flags = if !self.outgoing_heap.is_empty() || has_split {
            // Burst/split transfer: signal continuous send.
            crate::protocol::constants::DatagramFlags::VALID
                | crate::protocol::constants::DatagramFlags::CONTINUOUS_SEND
        } else {
            // Default: VALID with Needs B&AS bit (Cloudburst/Bedrock use 0x84 here).
            crate::protocol::constants::DatagramFlags::VALID
                | crate::protocol::constants::DatagramFlags::HAS_B_AND_AS
        };
        let dgram = Datagram {
            header: crate::protocol::types::DatagramHeader {
                flags,
                sequence: seq,
            },
            payload: DatagramPayload::EncapsulatedPackets(packets),
        };

        if has_reliable {
            return Some(self.track_sent_datagram(dgram, seq, now));
        }

        Some(dgram)
    }

    fn fill_datagram(
        &mut self,
        packets: &mut Vec<EncapsulatedPacket>,
        current_size: &mut usize,
        transmission_bw: &mut usize,
    ) {
        while let Some(top) = self.outgoing_heap.peek() {
            let pkt_size = top.pkt.size();

            if *transmission_bw < pkt_size {
                break;
            }
            if *current_size + pkt_size > self.mtu {
                break;
            }

            let queued = self.outgoing_heap.pop().unwrap();
            *transmission_bw -= pkt_size;
            *current_size += pkt_size;
            packets.push(queued.pkt);
        }
    }

    pub(crate) fn build_ack_datagram(&mut self, _now: Instant) -> Option<Datagram> {
        let mut ranges = Vec::new(); // TODO: Cache this in Session if allocation is still hot
        self.outgoing_acks.pop_for_mtu(
            self.mtu,
            constants::IPV4_HEADER_SIZE + constants::UDP_HEADER_SIZE + 2 + 1,
            &mut ranges,
        );
        if ranges.is_empty() {
            return None;
        }
        ranges.sort_by_key(|r| r.start.value());

        let ack_payload = AckNackPayload { ranges };
        tracing::trace!("ack_payload");

        let dgram = Datagram {
            header: crate::protocol::types::DatagramHeader {
                flags: crate::protocol::constants::DatagramFlags::VALID
                    | crate::protocol::constants::DatagramFlags::ACK,
                sequence: Sequence24::new(0),
            },

            payload: DatagramPayload::Ack(ack_payload),
        };

        self.sliding.on_send_ack();
        Some(dgram)
    }

    pub(crate) fn build_nak_datagram(&mut self) -> Option<Datagram> {
        let mut ranges = Vec::new(); // TODO: Cache this in Session if allocation is still hot
        self.outgoing_naks.pop_for_mtu(
            self.mtu,
            constants::IPV4_HEADER_SIZE + constants::UDP_HEADER_SIZE + 2 + 1,
            &mut ranges,
        );
        if ranges.is_empty() {
            return None;
        }
        ranges.sort_by_key(|r| r.start.value());

        let nak_payload = AckNackPayload { ranges };
        tracing::trace!("nak_payload");

        let dgram = Datagram {
            header: crate::protocol::types::DatagramHeader {
                flags: crate::protocol::constants::DatagramFlags::VALID
                    | crate::protocol::constants::DatagramFlags::NACK,
                sequence: Sequence24::new(0),
            },
            payload: DatagramPayload::Nak(nak_payload),
        };

        Some(dgram)
    }

    /// Maximum payload size for an encapsulated packet given the current MTU.
    /// Uses the actual header footprint for the reliability/flags instead of worst-case.
    fn max_encapsulated_payload_len(&self, reliability: Reliability, is_split: bool) -> usize {
        let header = self.encapsulated_header_overhead(reliability, is_split);
        // Only subtract the encapsulated header; datagram/IP/UDP headers are accounted for
        // once when packing the datagram in `fill_datagram`.
        self.mtu.saturating_sub(
            header
                + constants::IPV4_HEADER_SIZE
                + constants::UDP_HEADER_SIZE
                + constants::RAKNET_DATAGRAM_HEADER_SIZE,
        )
    }

    fn next_reliable_index(&mut self) -> Sequence24 {
        let idx = self.reliability_write_index;
        self.reliability_write_index = self.reliability_write_index.next();
        idx
    }

    fn next_sequence_index(&mut self) -> Sequence24 {
        let idx = self.sequence_write_index;
        self.sequence_write_index = self.sequence_write_index.next();
        idx
    }

    fn next_ordering_index(&mut self, channel: u8) -> Option<Sequence24> {
        self.ordering.next_order_index(channel)
    }

    fn enqueue_single_encap(
        &mut self,
        payload: bytes::Bytes,
        reliability: Reliability,
        channel: u8,
        priority: RakPriority,
    ) -> usize {
        let header = EncapsulatedPacketHeader {
            reliability,
            is_split: false,
            needs_bas: false,
        };

        let ordering_index = if reliability.is_ordered() || reliability.is_sequenced() {
            self.next_ordering_index(channel)
        } else {
            None
        };

        let sequence_index = if reliability.is_sequenced() {
            Some(self.next_sequence_index())
        } else {
            None
        };

        let reliable_index = if reliability.is_reliable() {
            Some(self.next_reliable_index())
        } else {
            None
        };

        let encapsulated = EncapsulatedPacket {
            header,
            bit_length: (payload.len() as u16) << 3,
            reliable_index,
            sequence_index,
            ordering_index,
            ordering_channel: ordering_index.map(|_| channel),
            split: None,
            payload,
        };

        let size = encapsulated.size();
        self.push_outgoing_encap(encapsulated, priority);
        if reliability.is_reliable() { size } else { 0 }
    }

    fn normalize_reliability_for_split(&self, reliability: Reliability) -> Reliability {
        match reliability {
            Reliability::Unreliable => Reliability::Reliable,
            Reliability::UnreliableSequenced => Reliability::ReliableSequenced,
            Reliability::UnreliableWithAckReceipt => Reliability::ReliableWithAckReceipt,
            other => other,
        }
    }

    fn enqueue_fragmented_encaps(
        &mut self,
        mut payload: bytes::Bytes,
        reliability: Reliability,
        channel: u8,
        priority: RakPriority,
    ) -> usize {
        let reliability = self.normalize_reliability_for_split(reliability);
        let max_len = self.max_encapsulated_payload_len(reliability, true).max(1);

        let total = payload.len();
        let parts = ((total - 1) / max_len) + 1;

        let mut chunks = Vec::with_capacity(parts);
        for _ in 0..parts {
            let take = payload.len().min(max_len);
            let chunk = payload.split_to(take);
            chunks.push(chunk);
        }
        debug_assert!(payload.is_empty());

        let split_id = self.split_index;
        self.split_index = self.split_index.wrapping_add(1);

        let ordering_index = if reliability.is_ordered() || reliability.is_sequenced() {
            self.next_ordering_index(channel)
        } else {
            None
        };

        let sequence_index = if reliability.is_sequenced() {
            Some(self.next_sequence_index())
        } else {
            None
        };

        let mut reliable_bytes = 0usize;

        for (i, chunk) in chunks.into_iter().enumerate() {
            let header = EncapsulatedPacketHeader {
                reliability,
                is_split: true,
                needs_bas: false,
            };

            let reliable_index = if reliability.is_reliable() {
                Some(self.next_reliable_index())
            } else {
                None
            };

            let split = Some(crate::protocol::encapsulated_packet::SplitInfo {
                count: parts as u32,
                id: split_id,
                index: i as u32,
            });

            let encapsulated = EncapsulatedPacket {
                header,
                bit_length: (chunk.len() as u16) << 3,
                reliable_index,
                sequence_index,
                ordering_index,
                ordering_channel: ordering_index.map(|_| channel),
                split,
                payload: chunk,
            };

            let size = encapsulated.size();

            self.push_outgoing_encap(encapsulated, priority);
            if reliability.is_reliable() {
                reliable_bytes += size;
            }
        }
        reliable_bytes
    }

    fn push_outgoing_encap(&mut self, pkt: EncapsulatedPacket, priority: RakPriority) {
        let weight = self.get_next_weight(priority);
        self.outgoing_heap.push(QueuedEncap { weight, pkt });
    }

    /// Compute the header overhead (flags + indexes + split metadata) for an encapsulated packet.
    fn encapsulated_header_overhead(&self, reliability: Reliability, is_split: bool) -> usize {
        let mut size = 3; // flags + bit_length
        if reliability.is_reliable() {
            size += 3; // reliable_index
        }
        if reliability.is_sequenced() {
            size += 3; // sequence_index
        }
        if reliability.is_ordered() || reliability.is_sequenced() {
            size += 3; // ordering_index
            size += 1; // ordering_channel
        }
        if is_split {
            size += 4; // partCount
            size += 2; // partId
            size += 4; // partIndex
        }
        size
    }

    fn track_sent_datagram(&mut self, dgram: Datagram, seq: Sequence24, now: Instant) -> Datagram {
        let rto = self.sliding.get_rto_for_retransmission();
        let tracked = TrackedDatagram {
            datagram: dgram,
            send_time: now,
            next_send: now + rto,
        };
        if let DatagramPayload::EncapsulatedPackets(_) = &tracked.datagram.payload {
            self.sliding.on_reliable_send(&tracked.datagram);
        }
        let dgram = tracked.datagram.clone();
        self.sent_datagrams.insert(seq, tracked);
        dgram
    }

    fn get_next_weight(&mut self, priority: RakPriority) -> u64 {
        let level = priority.as_index();
        let mut next = self.outgoing_packet_next_weights[level];

        if !self.outgoing_heap.is_empty() {
            if next >= self.last_min_weight {
                next = self.last_min_weight + ((1u64 << level) * level as u64) + level as u64;
                self.outgoing_packet_next_weights[level] =
                    next + ((1u64 << level) * (level as u64 + 1)) + level as u64;
            }
        } else {
            for p in 0..4 {
                self.outgoing_packet_next_weights[p] = ((1u64 << p) * p as u64) + p as u64;
            }
        }

        self.last_min_weight = next - ((1u64 << level) * level as u64) + level as u64;
        next
    }

    pub(crate) fn collect_resendable_datagrams(
        &self,
        now: Instant,
        bw: &mut usize,
    ) -> Vec<Sequence24> {
        let mut to_resend = Vec::new();
        for (seq, tracked) in self.sent_datagrams.iter() {
            let dgram_size = tracked.datagram.size();
            if tracked.next_send <= now && *bw >= dgram_size {
                *bw -= dgram_size;
                to_resend.push(*seq);
            }
        }
        to_resend
    }

    pub(crate) fn resend_datagrams(
        &mut self,
        to_resend: Vec<Sequence24>,
        now: Instant,
        out: &mut Vec<Datagram>,
    ) {
        let mut resent_any = false;

        for seq in to_resend {
            if let Some(tracked) = self.sent_datagrams.get_mut(&seq) {
                let rto = self.sliding.get_rto_for_retransmission();
                tracked.send_time = now;
                tracked.next_send = now + rto;
                resent_any = true;
                out.push(tracked.datagram.clone());
            }
        }

        if resent_any {
            self.sliding.on_resend(self.datagram_write_index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::datagram::DatagramPayload;
    use crate::protocol::packet::RaknetPacket;
    use bytes::Bytes;
    use std::time::Instant;

    #[test]
    fn priorities_follow_cloudburst_weights() {
        let mut session = Session::new(1400);
        let now = Instant::now();

        // enqueue in reverse priority order to ensure heap ordering is respected
        session.queue_packet(
            RaknetPacket::UserData {
                id: 0x81,
                payload: Bytes::from_static(b"low"),
            },
            Reliability::Reliable,
            0,
            RakPriority::Low,
        );
        session.queue_packet(
            RaknetPacket::UserData {
                id: 0x82,
                payload: Bytes::from_static(b"high"),
            },
            Reliability::Reliable,
            0,
            RakPriority::High,
        );
        session.queue_packet(
            RaknetPacket::UserData {
                id: 0x83,
                payload: Bytes::from_static(b"imm"),
            },
            Reliability::Reliable,
            0,
            RakPriority::Immediate,
        );

        let dgram = session
            .build_data_datagram(now)
            .expect("should build a datagram");

        let DatagramPayload::EncapsulatedPackets(pkts) = dgram.payload else {
            panic!("expected encapsulated datagram");
        };

        let ids: Vec<u8> = pkts
            .iter()
            .map(|enc| {
                let mut buf = enc.payload.clone();
                match RaknetPacket::decode(&mut buf).expect("decode payload") {
                    RaknetPacket::UserData { id, .. } => id,
                    _ => panic!("unexpected non-user packet"),
                }
            })
            .collect();

        assert_eq!(ids, vec![0x83, 0x82, 0x81]);
    }

    #[test]
    fn priority_weights_reset_when_queue_drains() {
        let mut session = Session::new(1200);
        let now = Instant::now();

        session.queue_packet(
            RaknetPacket::UserData {
                id: 0x90,
                payload: Bytes::from_static(b"a"),
            },
            Reliability::Reliable,
            0,
            RakPriority::Normal,
        );

        let _ = session.build_data_datagram(now).unwrap();

        // Draining the heap should reset weights, so the next packet starts at base weight.
        let next_weight = session.get_next_weight(RakPriority::Normal);
        assert_eq!(
            next_weight,
            ((1u64 << RakPriority::Normal.as_index()) * RakPriority::Normal as u64)
                + RakPriority::Normal as u64
        );
    }

    #[test]
    fn golden_datagram_bytes_match_expectation() {
        let mut session = Session::new(1500);
        let now = Instant::now();

        session.queue_packet(
            RaknetPacket::UserData {
                id: 0x80,
                payload: Bytes::from_static(b"\x01"),
            },
            Reliability::Reliable,
            0,
            RakPriority::Normal,
        );

        let dgram = session.build_data_datagram(now).expect("datagram");
        let mut buf = BytesMut::new();
        dgram.encode(&mut buf).unwrap();
        let bytes = buf.freeze();

        // Flags: VALID|HAS_B_AND_AS (0x84), sequence 0, single reliable encapsulated payload.
        let expected: &[u8] = &[
            0x84, 0x00, 0x00, 0x00, // datagram header
            0x40, // encapsulated header (reliable)
            0x00, 0x10, // bit length = 16 bits
            0x00, 0x00, 0x00, // reliable index
            0x80, // user data id
            0x01, // payload byte
        ];

        assert_eq!(&bytes[..], expected);
    }

    #[test]
    fn sequenced_packets_get_sequence_index() {
        let mut session = Session::new(1500);
        let now = Instant::now();

        session.queue_packet(
            RaknetPacket::UserData {
                id: 0x90,
                payload: Bytes::from_static(b"\xAA\xBB"),
            },
            Reliability::UnreliableSequenced,
            0,
            RakPriority::Normal,
        );

        let dgram = session.build_data_datagram(now).expect("datagram");
        let DatagramPayload::EncapsulatedPackets(pkts) = dgram.payload else {
            panic!("expected encapsulated datagram");
        };

        assert_eq!(pkts.len(), 1);
        assert!(
            pkts[0].sequence_index.is_some(),
            "sequence index should be set"
        );
        assert!(
            pkts[0].ordering_index.is_some(),
            "ordering index should be set for sequenced reliabilities"
        );
        assert!(
            pkts[0].ordering_channel.is_some(),
            "ordering channel should be set for sequenced reliabilities"
        );
    }
}

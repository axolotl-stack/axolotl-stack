//! `TransmissionPolicy::Vanilla` session mechanics (lunar-bedrock/go-raknet
//! `87a6b7e`, see `PARITY.md` DG-*, ACK-*, CC-*).

use std::time::Instant;

use crate::protocol::{
    ack::{AckNackPayload, SequenceRange},
    constants::DatagramFlags,
    datagram::{Datagram, DatagramPayload},
    types::{DatagramHeader, Sequence24},
};

use super::{Session, TrackedDatagram, ack_queue};

/// Reliable frame bytes of a tracked datagram, in the units of
/// `queue_packet`'s return value.
fn reliable_frame_bytes(datagram: &Datagram) -> usize {
    match &datagram.payload {
        DatagramPayload::EncapsulatedPackets(frames) => frames
            .iter()
            .filter(|frame| frame.header.reliability.is_reliable())
            .map(|frame| frame.size())
            .sum(),
        _ => 0,
    }
}

/// go-raknet `conn.go:effectiveMTU`: IPv4 + UDP headers, for every family.
const UDP_IP_OVERHEAD: usize = 28;
/// go-raknet `packet.go:packetAdditionalSize`, independent of reliability.
const PACKET_ADDITIONAL_SIZE: usize = 1 + 3 + 1 + 2 + 3 + 3 + 1;
/// go-raknet `packet.go:splitAdditionalSize`.
const SPLIT_ADDITIONAL_SIZE: usize = 4 + 2 + 4;

/// go-raknet `packet.go:split`: the largest unsplit message, or the fragment
/// size once a message is split.
pub(super) fn max_fragment_len(mtu: usize, is_split: bool) -> usize {
    let split = if is_split { SPLIT_ADDITIONAL_SIZE } else { 0 };
    mtu.saturating_sub(UDP_IP_OVERHEAD + PACKET_ADDITIONAL_SIZE + split)
}

impl Session {
    /// go-raknet `conn.go:sendDatagram`: exactly one frame, flags `0x84`,
    /// recorded for resend with the current time when reliable.
    pub(super) fn build_vanilla_datagram(&mut self, now: Instant) -> Datagram {
        let queued = self
            .outgoing_heap
            .pop()
            .expect("caller checked the outgoing queue");
        let seq = self.datagram_write_index;
        self.datagram_write_index = seq.next();
        let reliable = queued.pkt.header.reliability.is_reliable();
        let datagram = Datagram {
            header: DatagramHeader {
                flags: DatagramFlags::VALID | DatagramFlags::HAS_B_AND_AS,
                sequence: seq,
            },
            payload: DatagramPayload::EncapsulatedPackets(vec![queued.pkt]),
        };
        if reliable {
            self.sent_datagrams.insert(
                seq,
                TrackedDatagram {
                    datagram: datagram.clone(),
                    send_time: now,
                    next_send: now,
                },
            );
        }
        datagram
    }

    /// Next datagram for an immediate flush: NAK-triggered resends, pending
    /// NAK feedback, then queued data. ACKs wait for the 100 ms tick.
    pub(crate) fn next_vanilla_datagram(&mut self, now: Instant) -> Option<Datagram> {
        self.process_incoming_acks_naks(now);
        if let Some(datagram) = self.vanilla_mut().resend_ready.pop_front() {
            return Some(datagram);
        }
        if let Some(datagram) = self.vanilla_mut().feedback_ready.pop_front() {
            return Some(datagram);
        }
        let mut feedback = Vec::new();
        self.vanilla_feedback(false, &mut feedback);
        if !feedback.is_empty() {
            self.vanilla_mut().feedback_ready.extend(feedback);
            return self.vanilla_mut().feedback_ready.pop_front();
        }
        self.build_data_datagram(now)
    }

    /// go-raknet `conn.go:startTicking` body before the closing/ping logic:
    /// `flushACKs`, then `checkResend` on every third tick.
    pub(crate) fn on_vanilla_tick(&mut self, now: Instant, resend_check: bool) -> Vec<Datagram> {
        let mut out = Vec::new();
        self.process_incoming_acks_naks(now);
        self.prune_expired_splits(now);
        self.vanilla_feedback(true, &mut out);
        if resend_check {
            // conn.go:checkResend stores the RTT before scanning.
            let rtt = self.vanilla_mut().rtt(now);
            self.vanilla_mut().store_rtt(rtt);
            let delay = rtt + rtt / 2;
            let due: Vec<Sequence24> = self
                .sent_datagrams
                .iter()
                .filter(|(_, tracked)| now.saturating_duration_since(tracked.send_time) > delay)
                .map(|(&seq, _)| seq)
                .collect();
            for seq in due {
                if let Some(tracked) = self.sent_datagrams.remove(&seq) {
                    out.push(self.vanilla_retransmit(tracked, now));
                }
            }
        }
        out.extend(self.vanilla_mut().resend_ready.drain(..));
        out.extend(self.vanilla_mut().feedback_ready.drain(..));
        self.vanilla_feedback(false, &mut out);
        out
    }

    /// go-raknet `conn.go:handleACK` / `resend_map.go:acknowledge`.
    pub(super) fn vanilla_process_acks(&mut self, now: Instant) {
        while let Some(range) = self.incoming_acks.pop_front() {
            while let Some((&seq, _)) = self.sent_datagrams.range(range.start..=range.end).next() {
                if let Some(tracked) = self.sent_datagrams.remove(&seq) {
                    let bytes = reliable_frame_bytes(&tracked.datagram);
                    let vanilla = self.vanilla_mut();
                    vanilla.record(now, tracked.send_time, 1);
                    vanilla.acked_reliable_bytes =
                        vanilla.acked_reliable_bytes.saturating_add(bytes);
                }
            }
        }
    }

    /// go-raknet `conn.go:handleNACK` / `resend`: every NAKed datagram still
    /// awaiting an ACK is resent immediately with a fresh sequence number.
    ///
    /// The datagrams are snapshotted before any resend (RAKNET-01): a resend
    /// is re-keyed to the next write index, which may itself fall inside a
    /// NAKed range and must not be resent again (go's `resend` would). The
    /// queued ranges are merged first, so each tracked datagram is resent at
    /// most once per pass and the work is bounded by the resend map.
    pub(super) fn vanilla_process_naks(&mut self, now: Instant) {
        if self.incoming_naks.is_empty() {
            return;
        }
        let mut ranges: Vec<SequenceRange> = self.incoming_naks.drain(..).collect();
        ranges.sort_unstable_by_key(|range| range.start.value());
        let mut due = Vec::new();
        // Highest sequence already covered by an earlier (merged) range.
        let mut covered: Option<u32> = None;
        for range in ranges {
            let end = range.end.value();
            let start = match covered {
                Some(covered) if covered >= end => continue,
                Some(covered) if covered >= range.start.value() => covered + 1,
                _ => range.start.value(),
            };
            covered = Some(end);
            due.extend(
                self.sent_datagrams
                    .range(Sequence24::new(start)..=range.end)
                    .map(|(&seq, _)| seq),
            );
        }
        for seq in due {
            if let Some(tracked) = self.sent_datagrams.remove(&seq) {
                let datagram = self.vanilla_retransmit(tracked, now);
                self.vanilla_mut().resend_ready.push_back(datagram);
            }
        }
    }

    /// Reliable frame bytes ACKed since the last call (manager queue cap).
    pub(crate) fn take_vanilla_acked_bytes(&mut self) -> usize {
        self.vanilla.as_mut().map_or(0, |vanilla| {
            std::mem::take(&mut vanilla.acked_reliable_bytes)
        })
    }

    /// `resend_map.go:retransmit` (sample `2 × age`) followed by
    /// `conn.go:sendDatagram` (fresh sequence, new timestamp).
    fn vanilla_retransmit(&mut self, mut tracked: TrackedDatagram, now: Instant) -> Datagram {
        self.vanilla_mut().record(now, tracked.send_time, 2);
        let seq = self.datagram_write_index;
        self.datagram_write_index = seq.next();
        tracked.datagram.header.sequence = seq;
        tracked.send_time = now;
        tracked.next_send = now;
        let datagram = tracked.datagram.clone();
        self.sent_datagrams.insert(seq, tracked);
        datagram
    }

    /// go-raknet `conn.go:sendAcknowledgement`: everything pending, sorted,
    /// merged and chunked exactly like `acknowledgement.write`.
    fn vanilla_feedback(&mut self, ack: bool, out: &mut Vec<Datagram>) {
        let queue = if ack {
            &mut self.outgoing_acks
        } else {
            &mut self.outgoing_naks
        };
        let mut ranges = Vec::new();
        queue.drain_into(&mut ranges);
        if ranges.is_empty() {
            return;
        }
        let flags = DatagramFlags::VALID
            | if ack {
                DatagramFlags::ACK
            } else {
                DatagramFlags::NACK
            };
        for chunk in ack_queue::go_ack_chunks(ranges, self.mtu) {
            let payload = AckNackPayload { ranges: chunk };
            out.push(Datagram {
                header: DatagramHeader {
                    flags,
                    sequence: Sequence24::new(0),
                },
                payload: if ack {
                    DatagramPayload::Ack(payload)
                } else {
                    DatagramPayload::Nak(payload)
                },
            });
        }
    }

    /// Vanilla RTT as `resend_map.go:rtt` would compute it now.
    pub(crate) fn vanilla_rtt(&mut self, now: Instant) -> std::time::Duration {
        self.vanilla_mut().rtt(now)
    }

    /// Unacknowledged datagrams (`len(conn.retransmission.unacknowledged)`).
    pub(crate) fn unacknowledged(&self) -> usize {
        self.sent_datagrams.len()
    }

    /// Drop the resend state (`conn.go:closeImmediately`).
    pub(crate) fn clear_unacknowledged(&mut self) {
        self.sent_datagrams.clear();
        self.vanilla_mut().resend_ready.clear();
    }

    /// `conn.go:startTicking` tick counter.
    pub(crate) fn vanilla_ticks(&self) -> u64 {
        self.vanilla.as_ref().map_or(0, |vanilla| vanilla.ticks)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bytes::Bytes;

    use super::*;
    use crate::protocol::{
        ack::SequenceRange, encapsulated_packet::EncapsulatedPacket, reliability::Reliability,
        state::RakPriority,
    };
    use crate::session::{SessionTunables, TransmissionPolicy};

    fn session(policy: TransmissionPolicy, mtu: usize) -> Session {
        Session::with_tunables(
            mtu,
            SessionTunables {
                transmission: policy,
                ..Default::default()
            },
        )
    }

    fn queue(session: &mut Session, len: usize, reliability: Reliability, priority: RakPriority) {
        let mut payload = vec![0xfe; len];
        payload[1..]
            .iter_mut()
            .enumerate()
            .for_each(|(i, b)| *b = i as u8);
        session
            .queue_encoded_packet(Bytes::from(payload), reliability, 0, priority)
            .unwrap();
    }

    fn drain(session: &mut Session, now: Instant) -> Vec<Datagram> {
        std::iter::from_fn(|| session.next_vanilla_datagram(now)).collect()
    }

    fn frames(datagram: &Datagram) -> &[EncapsulatedPacket] {
        match &datagram.payload {
            DatagramPayload::EncapsulatedPackets(frames) => frames,
            other => panic!("expected data datagram, got {other:?}"),
        }
    }

    fn ranges(datagram: &Datagram) -> Vec<(u32, u32)> {
        match &datagram.payload {
            DatagramPayload::Ack(payload) | DatagramPayload::Nak(payload) => payload
                .ranges
                .iter()
                .map(|range| (range.start.value(), range.end.value()))
                .collect(),
            other => panic!("expected feedback, got {other:?}"),
        }
    }

    fn ack(session: &mut Session, seq: u32) {
        session.handle_ack_payload(AckNackPayload {
            ranges: vec![SequenceRange {
                start: Sequence24::new(seq),
                end: Sequence24::new(seq),
            }],
        });
    }

    fn nak(session: &mut Session, seq: u32) {
        session.handle_nack_payload(AckNackPayload {
            ranges: vec![SequenceRange {
                start: Sequence24::new(seq),
                end: Sequence24::new(seq),
            }],
        });
    }

    /// DG-1/DG-2: go-raknet `conn.go:sendDatagram` writes one frame per
    /// datagram with flags 0x84, including split fragments and bursts.
    #[test]
    fn vanilla_one_frame_per_datagram_with_needs_bas_flag() {
        let now = Instant::now();
        let mut s = session(TransmissionPolicy::Vanilla, 1492);
        for _ in 0..3 {
            queue(
                &mut s,
                10,
                Reliability::ReliableOrdered,
                RakPriority::Normal,
            );
        }
        queue(
            &mut s,
            4000,
            Reliability::ReliableOrdered,
            RakPriority::Normal,
        );
        let datagrams = drain(&mut s, now);
        assert_eq!(datagrams.len(), 3 + 3);
        for (index, datagram) in datagrams.iter().enumerate() {
            assert_eq!(datagram.header.flags.bits(), 0x84);
            assert_eq!(datagram.header.sequence.value(), index as u32);
            assert_eq!(frames(datagram).len(), 1);
        }

        let mut optimized = session(TransmissionPolicy::Optimized, 1492);
        for _ in 0..3 {
            queue(
                &mut optimized,
                10,
                Reliability::ReliableOrdered,
                RakPriority::Normal,
            );
        }
        let packed = optimized.build_data_datagram(now).unwrap();
        assert_eq!(frames(&packed).len(), 3);
    }

    /// REL-2/REL-3/DG-5: frames leave in write order with consecutive message
    /// and order indices; RakNet priorities do not reorder them.
    #[test]
    fn vanilla_sends_in_write_order_ignoring_priority() {
        let now = Instant::now();
        let mut s = session(TransmissionPolicy::Vanilla, 1492);
        for priority in [RakPriority::Low, RakPriority::High, RakPriority::Immediate] {
            queue(&mut s, 10, Reliability::ReliableOrdered, priority);
        }
        let indices: Vec<_> = drain(&mut s, now)
            .iter()
            .map(|datagram| {
                let frame = &frames(datagram)[0];
                (
                    frame.reliable_index.unwrap().value(),
                    frame.ordering_index.unwrap().value(),
                    frame.ordering_channel.unwrap(),
                )
            })
            .collect();
        assert_eq!(indices, [(0, 0, 0), (1, 1, 0), (2, 2, 0)]);
    }

    /// REL-11/MTU-10: go-raknet `packet.go:split` budgets
    /// `mtu - 28 - 14` per frame (`- 10` more once split), for every reliability.
    #[test]
    fn vanilla_split_threshold_uses_go_constant_overhead() {
        let now = Instant::now();
        for reliability in [Reliability::Unreliable, Reliability::ReliableOrdered] {
            let mut s = session(TransmissionPolicy::Vanilla, 1492);
            queue(&mut s, 1492 - 42, reliability, RakPriority::Normal);
            let single = drain(&mut s, now);
            assert_eq!(single.len(), 1);
            assert!(frames(&single[0])[0].split.is_none());

            queue(&mut s, 1492 - 41, reliability, RakPriority::Normal);
            let split = drain(&mut s, now);
            let lengths: Vec<_> = split.iter().map(|d| frames(d)[0].payload.len()).collect();
            assert_eq!(lengths, [1492 - 52, 11]);
            let encoded: Vec<_> = split
                .iter()
                .map(|d| {
                    let mut out = bytes::BytesMut::new();
                    d.encode(&mut out).unwrap();
                    out.len()
                })
                .collect();
            // Every datagram fits the UDP payload of the MTU.
            assert!(encoded.iter().all(|&len| len <= 1492 - 28), "{encoded:?}");
        }
        // Optimized budgets the actual (smaller) Unreliable header.
        let mut optimized = session(TransmissionPolicy::Optimized, 1492);
        queue(
            &mut optimized,
            1492 - 35,
            Reliability::Unreliable,
            RakPriority::Normal,
        );
        let datagram = optimized.build_data_datagram(now).unwrap();
        assert!(frames(&datagram)[0].split.is_none());
    }

    /// REL-5: go-raknet keeps the message reliability for fragments.
    #[test]
    fn vanilla_split_keeps_message_reliability() {
        let now = Instant::now();
        let mut s = session(TransmissionPolicy::Vanilla, 1492);
        queue(&mut s, 3000, Reliability::Unreliable, RakPriority::Normal);
        let datagrams = drain(&mut s, now);
        assert_eq!(datagrams.len(), 3);
        for datagram in &datagrams {
            let frame = &frames(datagram)[0];
            assert_eq!(frame.header.reliability, Reliability::Unreliable);
            assert!(frame.reliable_index.is_none());
            assert!(frame.split.is_some());
        }
        // Unreliable datagrams are never tracked for resend.
        assert_eq!(s.unacknowledged(), 0);
    }

    /// REL-10: split IDs advance only for split messages.
    #[test]
    fn split_ids_advance_only_for_split_messages() {
        let now = Instant::now();
        for policy in [TransmissionPolicy::Vanilla, TransmissionPolicy::Optimized] {
            let mut s = session(policy, 1492);
            for len in [10, 3000, 10, 3000] {
                queue(
                    &mut s,
                    len,
                    Reliability::ReliableOrdered,
                    RakPriority::Normal,
                );
            }
            let mut ids = Vec::new();
            while let Some(datagram) = s.build_data_datagram(now) {
                for frame in frames(&datagram) {
                    if let Some(split) = &frame.split
                        && !ids.contains(&split.id)
                    {
                        ids.push(split.id);
                    }
                }
                // Keep Optimized's window open.
                if !policy.is_vanilla() {
                    s.handle_ack_payload(AckNackPayload {
                        ranges: vec![SequenceRange {
                            start: datagram.header.sequence,
                            end: datagram.header.sequence,
                        }],
                    });
                    s.process_incoming_acks_naks(now);
                }
            }
            assert_eq!(ids, [0, 1], "{policy:?}");
        }
    }

    /// CC-1: no congestion window; everything queued is sent at once.
    #[test]
    fn vanilla_sends_without_a_congestion_window() {
        let now = Instant::now();
        let mut vanilla = session(TransmissionPolicy::Vanilla, 1492);
        let mut optimized = session(TransmissionPolicy::Optimized, 1492);
        for s in [&mut vanilla, &mut optimized] {
            for _ in 0..100 {
                queue(s, 1000, Reliability::ReliableOrdered, RakPriority::Normal);
            }
        }
        assert_eq!(drain(&mut vanilla, now).len(), 100);
        assert_eq!(vanilla.unacknowledged(), 100);
        let mut sent = 0;
        while optimized.build_data_datagram(now).is_some() {
            sent += 1;
        }
        assert!(sent < 100, "Optimized keeps its congestion window");
    }

    /// ACK-5/ACK-6/CC-3: ACK samples count once, NAK resends twice their age;
    /// the RTT is their mean.
    #[test]
    fn vanilla_rtt_is_the_mean_of_recent_samples() {
        let start = Instant::now();
        let mut s = session(TransmissionPolicy::Vanilla, 1492);
        queue(
            &mut s,
            10,
            Reliability::ReliableOrdered,
            RakPriority::Normal,
        );
        queue(
            &mut s,
            10,
            Reliability::ReliableOrdered,
            RakPriority::Normal,
        );
        drain(&mut s, start);
        ack(&mut s, 0);
        drain(&mut s, start + Duration::from_millis(40));
        nak(&mut s, 1);
        drain(&mut s, start + Duration::from_millis(60));
        assert_eq!(
            s.vanilla_rtt(start + Duration::from_millis(60)),
            Duration::from_millis(80)
        );
        // Samples older than five seconds are forgotten: back to 50 ms.
        assert_eq!(
            s.vanilla_rtt(start + Duration::from_millis(5061)),
            Duration::from_millis(50)
        );
    }

    /// ACK-6: a NAK resends immediately with a fresh datagram sequence while
    /// keeping the frame's indices.
    #[test]
    fn vanilla_nak_resends_immediately_with_fresh_sequences() {
        let now = Instant::now();
        let mut s = session(TransmissionPolicy::Vanilla, 1492);
        for _ in 0..3 {
            queue(
                &mut s,
                10,
                Reliability::ReliableOrdered,
                RakPriority::Normal,
            );
        }
        drain(&mut s, now);
        nak(&mut s, 1);
        let resent = drain(&mut s, now);
        assert_eq!(resent.len(), 1);
        assert_eq!(resent[0].header.sequence.value(), 3);
        assert_eq!(resent[0].header.flags.bits(), 0x84);
        assert_eq!(frames(&resent[0])[0].reliable_index.unwrap().value(), 1);
        // The old sequence is gone; the new one awaits its ACK.
        nak(&mut s, 1);
        assert!(drain(&mut s, now).is_empty());
        assert_eq!(s.unacknowledged(), 3);
    }

    /// ACK-6/RAKNET-01: one NAK covering 8192 sequences past the write index
    /// resends only the datagrams that were awaiting an ACK, once each, even
    /// though their fresh sequences fall inside the NAKed range (also when a
    /// second, overlapping NAK is queued before processing).
    #[test]
    fn vanilla_nak_past_the_write_index_resends_only_existing_datagrams() {
        let now = Instant::now();
        let mut s = session(TransmissionPolicy::Vanilla, 1492);
        for _ in 0..3 {
            queue(
                &mut s,
                10,
                Reliability::ReliableOrdered,
                RakPriority::Normal,
            );
        }
        drain(&mut s, now);
        for _ in 0..2 {
            s.handle_nack_payload(AckNackPayload {
                ranges: vec![SequenceRange {
                    start: Sequence24::new(0),
                    end: Sequence24::new(8191),
                }],
            });
        }
        let resent = drain(&mut s, now);
        let sequences: Vec<_> = resent.iter().map(|d| d.header.sequence.value()).collect();
        assert_eq!(sequences, [3, 4, 5]);
        let indices: Vec<_> = resent
            .iter()
            .map(|d| frames(d)[0].reliable_index.unwrap().value())
            .collect();
        assert_eq!(indices, [0, 1, 2]);
        assert_eq!(s.unacknowledged(), 3);
    }

    /// CC-2: unacked datagrams older than 1.5 × RTT are resent on every third
    /// 100 ms tick only.
    #[test]
    fn vanilla_timeout_resends_on_every_third_tick_after_one_and_a_half_rtt() {
        let start = Instant::now();
        let tick = |n: u32| start + Duration::from_millis(100) * n;
        let mut s = session(TransmissionPolicy::Vanilla, 1492);
        queue(
            &mut s,
            10,
            Reliability::ReliableOrdered,
            RakPriority::Normal,
        );
        drain(&mut s, start);
        assert!(s.on_tick(tick(1)).is_empty());
        assert!(s.on_tick(tick(2)).is_empty());
        queue(
            &mut s,
            10,
            Reliability::ReliableOrdered,
            RakPriority::Normal,
        );
        drain(&mut s, start + Duration::from_millis(250));
        // RTT has no samples (50 ms): delay 75 ms. Only the first datagram
        // (age 300 ms) is due; the second (age 50 ms) is not.
        let resent = s.on_tick(tick(3));
        assert_eq!(resent.len(), 1);
        assert_eq!(resent[0].header.sequence.value(), 2);
        assert_eq!(frames(&resent[0])[0].reliable_index.unwrap().value(), 0);
        // The resend recorded 2 × 300 ms.
        assert_eq!(s.vanilla_rtt(tick(3)), Duration::from_millis(600));
        // Ticks 4 and 5 never resend, however old the datagrams are.
        assert!(s.on_tick(tick(4) + Duration::from_secs(2)).is_empty());
        assert!(s.on_tick(tick(5) + Duration::from_secs(2)).is_empty());
    }

    /// ACK-3: the NAK delay is 1.5 × the RTT stored by the last resend check
    /// (zero before the first check).
    #[test]
    fn vanilla_nak_delay_uses_rtt_stored_by_resend_checks() {
        let start = Instant::now();
        let mut s = session(TransmissionPolicy::Vanilla, 1492);
        let nak_ranges = |s: &mut Session, now: Instant| -> Vec<(u32, u32)> {
            drain(s, now).iter().flat_map(ranges).collect()
        };
        s.accept_datagram_sequence(Sequence24::new(0), || start)
            .unwrap();
        s.accept_datagram_sequence(Sequence24::new(2), || start)
            .unwrap();
        assert_eq!(nak_ranges(&mut s, start), [(1, 1)]);

        for n in 1..=3 {
            s.on_tick(start + Duration::from_millis(100) * n);
        }
        // Stored RTT is now 50 ms: gaps wait for a datagram 75 ms old.
        let t1 = start + Duration::from_secs(1);
        s.accept_datagram_sequence(Sequence24::new(4), || t1)
            .unwrap();
        assert!(nak_ranges(&mut s, t1).is_empty());
        let t2 = t1 + Duration::from_millis(74);
        s.accept_datagram_sequence(Sequence24::new(6), || t2)
            .unwrap();
        assert!(nak_ranges(&mut s, t2).is_empty());
        let t3 = t1 + Duration::from_millis(75);
        s.accept_datagram_sequence(Sequence24::new(8), || t3)
            .unwrap();
        assert_eq!(nak_ranges(&mut s, t3), [(3, 3)]);
    }

    /// ACK-1: ACKs wait for the tick and cover everything received since.
    #[test]
    fn vanilla_acks_only_on_ticks() {
        let now = Instant::now();
        let mut s = session(TransmissionPolicy::Vanilla, 1492);
        for seq in [0, 1, 3, 2] {
            s.accept_datagram_sequence(Sequence24::new(seq), || now)
                .unwrap();
        }
        // Sequence 3 arrived out of order: the zero NAK delay flushes the
        // hole for 2 at once; the late 2 is then a duplicate (go marks NAKed
        // gaps received).
        let immediate = drain(&mut s, now);
        assert!(
            immediate
                .iter()
                .all(|d| matches!(d.payload, DatagramPayload::Nak(_)))
        );
        let tick = s.on_tick(now + Duration::from_millis(100));
        assert_eq!(tick.len(), 1);
        assert_eq!(tick[0].header.flags.bits(), 0xc0);
        assert_eq!(ranges(&tick[0]), [(0, 1), (3, 3)]);
        assert!(s.on_tick(now + Duration::from_millis(200)).is_empty());
    }
}

//! `TransmissionPolicy::Vanilla` reliability layer: Mojang RakNet's
//! `ReliabilityLayer` (the Jenkins RakNet 4.081 fork in the Bedrock client and
//! BDS; see `PARITY.md` DG-*, ACK-*, CC-*, REL-*).
//!
//! Unlike Optimized, the unit of retransmission is the reliable *message*:
//! a datagram history maps datagram numbers to the message numbers they
//! carried, and resends are re-packed (resends first, then new messages) into
//! fresh datagrams by the next update. One update (`vanilla_update`) does what
//! `ReliabilityLayer::Update` does: ACKs, at most one NAK datagram, then
//! resends and new data within the sliding window.

use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use crate::protocol::{
    ack::{AckNackPayload, SequenceRange},
    constants::{DatagramFlags, MAX_ACK_SEQUENCES},
    datagram::{Datagram, DatagramPayload},
    encapsulated_packet::EncapsulatedPacket,
    packet::DecodeError,
    types::{DatagramHeader, Sequence24},
};

use super::Session;
use super::policy::{
    MojangCongestion, VANILLA_ACK_DELAY, VANILLA_HISTORY_LEN, VANILLA_MAX_NAKS_PER_GAP,
    VANILLA_MAX_SKIPPED_DATAGRAMS, VANILLA_RESEND_SLOTS, VANILLA_UDP_HEADER,
    vanilla_datagram_capacity,
};

/// A reliable message awaiting its ACK (`resendBuffer` entry).
#[derive(Debug)]
struct ResendEntry {
    packet: EncapsulatedPacket,
    next_action: Instant,
    /// Position token in `resend_order`; older positions are stale.
    generation: u64,
}

/// Per-session state of the Vanilla policy.
#[derive(Debug)]
pub(crate) struct VanillaState {
    pub(crate) cc: MojangCongestion,
    /// Reliable messages in flight by message number (at most 512).
    resend: HashMap<u32, ResendEntry>,
    /// `resendLinkedList`: (message number, generation), oldest (re)send
    /// first. Entries whose generation no longer matches are skipped.
    resend_order: VecDeque<(u32, u64)>,
    generation: u64,
    /// `unacknowledgedBytes`: header + payload of new reliable sends until
    /// their ACK (resends do not add).
    unacked_bytes: usize,
    /// Datagram history from `history_base`: send time and the reliable
    /// message numbers carried; `None` once ACKed.
    history: VecDeque<Option<(Instant, Vec<u32>)>>,
    history_base: Sequence24,
    /// `expectedNextSequenceNumber` of received datagrams (NAK gaps).
    expected_next: Sequence24,
    /// `oldestUnsentAck`.
    oldest_unsent_ack: Option<Instant>,
    /// A split message completed: ACK in the next update.
    ack_now: bool,
    /// `bandwidthExceededStatistic`: the send queue was non-empty at the
    /// start of the previous update.
    bandwidth_exceeded: bool,
    /// Datagrams built by the last update, not yet handed to the transport.
    ready: VecDeque<Datagram>,
    last_update: Option<Instant>,
    /// Reliable bytes retired by ACKs since the manager last credited them
    /// against `max_queued_reliable_bytes`.
    pub(crate) acked_reliable_bytes: usize,
}

impl VanillaState {
    pub(crate) fn new(mtu: usize) -> Self {
        Self {
            cc: MojangCongestion::new(mtu),
            resend: HashMap::new(),
            resend_order: VecDeque::new(),
            generation: 0,
            unacked_bytes: 0,
            history: VecDeque::new(),
            history_base: Sequence24::new(0),
            expected_next: Sequence24::new(0),
            oldest_unsent_ack: None,
            ack_now: false,
            bandwidth_exceeded: false,
            ready: VecDeque::new(),
            last_update: None,
            acked_reliable_bytes: 0,
        }
    }

    fn push_resend(&mut self, number: u32, packet: EncapsulatedPacket, next_action: Instant) {
        self.generation += 1;
        self.resend_order.push_back((number, self.generation));
        self.resend.insert(
            number,
            ResendEntry {
                packet,
                next_action,
                generation: self.generation,
            },
        );
        // Stale positions accumulate when messages are ACKed or moved to the
        // tail; compact before they outnumber the live entries.
        if self.resend_order.len() > 2 * self.resend.len() + 64 {
            let resend = &self.resend;
            self.resend_order.retain(|&(number, generation)| {
                resend
                    .get(&number)
                    .is_some_and(|entry| entry.generation == generation)
            });
        }
    }

    /// History node of datagram `seq`, if still tracked.
    fn history_slot(&mut self, seq: Sequence24) -> Option<&mut Option<(Instant, Vec<u32>)>> {
        let offset = self.history_base.distance_to(seq);
        self.history.get_mut(offset as usize)
    }

    fn remove_message(&mut self, number: u32) {
        if let Some(entry) = self.resend.remove(&number) {
            let bytes = entry.packet.size();
            self.unacked_bytes = self.unacked_bytes.saturating_sub(bytes);
            self.acked_reliable_bytes = self.acked_reliable_bytes.saturating_add(bytes);
        }
    }
}

/// Mojang `RangeList::Serialize` budget per ACK/NAK datagram:
/// `(MTU - 28) * 8 - 72` bits; a range is added while `16 + written + 65`
/// bits fit. Ranges are sorted and merged; a datagram also closes at the
/// 8192-sequence decoder limit of RAKNET-01.
pub(crate) fn mojang_ack_chunks(
    mut ranges: Vec<SequenceRange>,
    mtu: usize,
) -> Vec<Vec<SequenceRange>> {
    ranges.sort_unstable_by_key(|range| range.start.value());
    let mut merged: Vec<(u32, u32)> = Vec::with_capacity(ranges.len());
    for range in ranges {
        let (start, end) = (range.start.value(), range.end.value());
        match merged.last_mut() {
            Some(last) if start <= last.1.saturating_add(1) => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    let max_bits = (mtu.saturating_sub(VANILLA_UDP_HEADER) * 8).saturating_sub(72);
    let max_sequences = u32::from(MAX_ACK_SEQUENCES);
    let mut chunks = Vec::new();
    let mut records = Vec::new();
    let (mut bits, mut sequences) = (0usize, 0u32);
    let mut pending = merged.into_iter().peekable();
    while let Some(&(start, end)) = pending.peek() {
        let room = max_sequences - sequences;
        if 16 + bits + 65 > max_bits || room == 0 {
            chunks.push(std::mem::take(&mut records));
            (bits, sequences) = (0, 0);
            continue;
        }
        let take_end = end.min(start + (room - 1));
        records.push(SequenceRange {
            start: Sequence24::new(start),
            end: Sequence24::new(take_end),
        });
        bits += if start == take_end { 32 } else { 56 };
        sequences += take_end - start + 1;
        pending.next();
        if take_end < end {
            // The rest of an oversized range starts the next datagram.
            chunks.push(std::mem::take(&mut records));
            (bits, sequences) = (0, 0);
            let rest = (take_end + 1, end);
            let remaining: Vec<(u32, u32)> = std::iter::once(rest).chain(pending).collect();
            pending = remaining.into_iter().peekable();
        }
    }
    if !records.is_empty() {
        chunks.push(records);
    }
    chunks
}

fn feedback_datagram(ranges: Vec<SequenceRange>, ack: bool) -> Datagram {
    let payload = AckNackPayload { ranges };
    Datagram {
        header: DatagramHeader {
            flags: DatagramFlags::VALID
                | if ack {
                    DatagramFlags::ACK
                } else {
                    DatagramFlags::NACK
                },
            sequence: Sequence24::new(0),
        },
        payload: if ack {
            DatagramPayload::Ack(payload)
        } else {
            DatagramPayload::Nak(payload)
        },
    }
}

impl Session {
    /// Vanilla state; only called on Vanilla code paths.
    fn vanilla_mut(&mut self) -> &mut VanillaState {
        self.vanilla
            .as_deref_mut()
            .expect("Vanilla code path on an Optimized session")
    }

    fn vanilla_ref(&self) -> &VanillaState {
        self.vanilla
            .as_deref()
            .expect("Vanilla code path on an Optimized session")
    }

    /// `OnGotPacket` + `acknowlegements.Insert`: every data datagram is ACKed
    /// (duplicates included; messages are de-duplicated by message number),
    /// and a forward gap NAKs each skipped number at once (at most 1000; a
    /// gap above 50000 rejects the datagram without an ACK).
    pub(super) fn vanilla_accept_sequence(
        &mut self,
        seq: Sequence24,
        now: Instant,
    ) -> Result<bool, DecodeError> {
        let vanilla = self.vanilla_mut();
        let expected = vanilla.expected_next;
        if seq == expected {
            vanilla.expected_next = seq.next();
        } else if seq > expected {
            let skipped = expected.distance_to(seq);
            if skipped > VANILLA_MAX_SKIPPED_DATAGRAMS {
                return Ok(false);
            }
            vanilla.expected_next = seq.next();
            let naked = skipped.min(VANILLA_MAX_NAKS_PER_GAP);
            let start = Sequence24::new(seq.value().wrapping_sub(naked) & 0x00ff_ffff);
            self.outgoing_naks.push(SequenceRange {
                start,
                end: seq.prev(),
            });
        }
        self.vanilla_mut().oldest_unsent_ack.get_or_insert(now);
        self.outgoing_acks.push(SequenceRange {
            start: seq,
            end: seq,
        });
        Ok(true)
    }

    /// A split message completed: `SendACKs` runs immediately (lens
    /// RL_006:1438-1441).
    pub(super) fn vanilla_split_completed(&mut self) {
        self.vanilla_mut().ack_now = true;
    }

    /// ACK processing of `HandleSocketReceiveFromConnectedPlayer`: an RTT
    /// sample per tracked datagram, window growth, then its messages leave
    /// the resend buffer.
    pub(super) fn vanilla_process_acks(&mut self, now: Instant) {
        let next = self.datagram_write_index;
        while let Some(range) = self.incoming_acks.pop_front() {
            let vanilla = self.vanilla.as_deref_mut().expect("Vanilla session");
            let mut seq = range.start;
            loop {
                if let Some(slot) = vanilla.history_slot(seq)
                    && let Some((sent, messages)) = slot.take()
                {
                    vanilla.cc.on_rtt(now.saturating_duration_since(sent));
                    vanilla.cc.is_continuous_send = vanilla.bandwidth_exceeded;
                    vanilla.cc.on_ack(seq, next);
                    for number in messages {
                        vanilla.remove_message(number);
                    }
                }
                if seq == range.end {
                    break;
                }
                seq = seq.next();
            }
        }
    }

    /// NAK processing: `OnNAK` per NAKed datagram, then its messages become
    /// due, so the next update resends them (lens RL_006:337-353).
    pub(super) fn vanilla_process_naks(&mut self, now: Instant) {
        let next = self.datagram_write_index;
        while let Some(range) = self.incoming_naks.pop_front() {
            let vanilla = self.vanilla.as_deref_mut().expect("Vanilla session");
            let mut seq = range.start;
            loop {
                let messages = vanilla
                    .history_slot(seq)
                    .and_then(|slot| slot.as_ref())
                    .map(|(_, messages)| messages.clone());
                if let Some(messages) = messages {
                    vanilla.cc.on_nak(next);
                    for number in messages {
                        if let Some(entry) = vanilla.resend.get_mut(&number) {
                            entry.next_action = now;
                        }
                    }
                }
                if seq == range.end {
                    break;
                }
                seq = seq.next();
            }
        }
    }

    /// Next datagram for the transport. The first call for a given `now`
    /// runs one update; later calls drain what it built.
    pub(crate) fn next_vanilla_datagram(&mut self, now: Instant) -> Option<Datagram> {
        if self.vanilla_ref().ready.is_empty() && self.vanilla_ref().last_update != Some(now) {
            self.vanilla_update(now);
        }
        self.vanilla_mut().ready.pop_front()
    }

    /// Runs one update unless one already ran at `now`, and returns every
    /// datagram it built.
    pub(crate) fn vanilla_tick(&mut self, now: Instant) -> Vec<Datagram> {
        if self.vanilla_ref().last_update != Some(now) {
            self.vanilla_update(now);
        }
        self.vanilla_mut().ready.drain(..).collect()
    }

    /// When pending ACKs become due: at once before the first RTT sample or
    /// after a split completed, else 10 ms after the oldest unacknowledged
    /// datagram.
    pub(crate) fn vanilla_ack_deadline(&self) -> Option<Instant> {
        let vanilla = self.vanilla_ref();
        let oldest = vanilla.oldest_unsent_ack?;
        if vanilla.ack_now || !vanilla.cc.rtt_known() {
            Some(oldest)
        } else {
            Some(oldest + VANILLA_ACK_DELAY)
        }
    }

    /// Reliable messages awaiting an ACK (`messagesInResendBuffer`).
    pub(crate) fn vanilla_messages_in_flight(&self) -> usize {
        self.vanilla_ref().resend.len()
    }

    /// No pending ACKs (`DISCONNECT_ON_NO_ACK` close condition).
    pub(crate) fn vanilla_acks_pending(&self) -> bool {
        self.vanilla_ref().oldest_unsent_ack.is_some()
    }

    /// Reliable frame bytes ACKed since the last call (manager queue cap).
    pub(crate) fn take_vanilla_acked_bytes(&mut self) -> usize {
        self.vanilla.as_mut().map_or(0, |vanilla| {
            std::mem::take(&mut vanilla.acked_reliable_bytes)
        })
    }

    /// `ReliabilityLayer::Update` (lens RL_010): ACKs if due, one NAK
    /// datagram, then resends and new messages packed into datagrams.
    fn vanilla_update(&mut self, now: Instant) {
        self.process_incoming_acks_naks(now);
        self.prune_expired_splits(now);
        let mtu = self.mtu;
        let vanilla = self.vanilla.as_deref_mut().expect("Vanilla session");
        vanilla.last_update = Some(now);

        if let Some(oldest) = vanilla.oldest_unsent_ack
            && (vanilla.ack_now || !vanilla.cc.rtt_known() || oldest + VANILLA_ACK_DELAY <= now)
        {
            let mut ranges = Vec::new();
            self.outgoing_acks.drain_into(&mut ranges);
            for chunk in mojang_ack_chunks(ranges, mtu) {
                vanilla.ready.push_back(feedback_datagram(chunk, true));
            }
            vanilla.oldest_unsent_ack = None;
            vanilla.ack_now = false;
        }
        let mut naks = Vec::new();
        self.outgoing_naks.drain_into(&mut naks);
        if let Some(chunk) = mojang_ack_chunks(naks, mtu).into_iter().next() {
            // One NAK datagram per update; the rest of the list is cleared.
            vanilla.ready.push_back(feedback_datagram(chunk, false));
        }

        let slow_start = vanilla.cc.in_slow_start();
        let continuous = vanilla.bandwidth_exceeded;
        vanilla.bandwidth_exceeded = !self.outgoing_heap.is_empty();
        vanilla.cc.is_continuous_send = continuous;
        let retransmission_budget = vanilla.unacked_bytes;
        let transmission_budget = vanilla.cc.transmission_bandwidth(vanilla.unacked_bytes);
        let capacity = vanilla_datagram_capacity(mtu);

        let mut datagrams: Vec<(Vec<EncapsulatedPacket>, Vec<u32>)> = Vec::new();
        let mut current: (Vec<EncapsulatedPacket>, Vec<u32>) = (Vec::new(), Vec::new());
        let mut current_size = 0usize;
        let mut total = 0usize;
        let mut add =
            |packet: EncapsulatedPacket,
             number: Option<u32>,
             datagrams: &mut Vec<(Vec<EncapsulatedPacket>, Vec<u32>)>| {
                let size = packet.size();
                if current_size + size > capacity && !current.0.is_empty() {
                    datagrams.push(std::mem::take(&mut current));
                    current_size = 0;
                }
                current_size += size;
                total += size;
                current.0.push(packet);
                current.1.extend(number);
                total
            };

        // Resends: the head of the resend list while due.
        let mut sent = 0usize;
        while sent < retransmission_budget {
            let Some(&(number, generation)) = vanilla.resend_order.front() else {
                break;
            };
            let live = vanilla
                .resend
                .get(&number)
                .filter(|entry| entry.generation == generation);
            let Some(entry) = live else {
                vanilla.resend_order.pop_front();
                continue;
            };
            if entry.next_action > now {
                break;
            }
            vanilla.resend_order.pop_front();
            let mut entry = vanilla.resend.remove(&number).expect("live entry");
            vanilla.cc.on_resend(self.datagram_write_index);
            entry.next_action = now + vanilla.cc.rto();
            let packet = entry.packet.clone();
            vanilla.push_resend(number, entry.packet, entry.next_action);
            sent = add(packet, Some(number), &mut datagrams);
        }

        // New messages within the window while a resend slot is free.
        while sent < transmission_budget {
            let slot_owner = (self.reliability_write_index.value())
                .wrapping_sub(VANILLA_RESEND_SLOTS)
                & 0x00ff_ffff;
            if vanilla.resend.contains_key(&slot_owner) {
                break;
            }
            let Some(queued) = self.outgoing_heap.pop() else {
                break;
            };
            let mut packet = queued.pkt;
            let number = if packet.header.reliability.is_reliable() {
                let index = self.reliability_write_index;
                self.reliability_write_index = index.next();
                packet.reliable_index = Some(index);
                vanilla.unacked_bytes += packet.size();
                vanilla.push_resend(index.value(), packet.clone(), now + vanilla.cc.rto());
                Some(index.value())
            } else {
                None
            };
            sent = add(packet, number, &mut datagrams);
        }
        if !current.0.is_empty() {
            datagrams.push(current);
        }

        for (index, (frames, numbers)) in datagrams.into_iter().enumerate() {
            let seq = self.datagram_write_index;
            self.datagram_write_index = seq.next();
            let mut flags = DatagramFlags::VALID;
            if slow_start {
                flags |= DatagramFlags::HAS_B_AND_AS;
            }
            if continuous || index > 0 {
                flags |= DatagramFlags::CONTINUOUS_SEND;
            }
            if vanilla.history.len() >= VANILLA_HISTORY_LEN {
                vanilla.history.pop_front();
                vanilla.history_base = vanilla.history_base.next();
            }
            vanilla.history.push_back(Some((now, numbers)));
            vanilla.ready.push_back(Datagram {
                header: DatagramHeader {
                    flags,
                    sequence: seq,
                },
                payload: DatagramPayload::EncapsulatedPackets(frames),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bytes::Bytes;

    use super::*;
    use crate::protocol::{reliability::Reliability, state::RakPriority};
    use crate::session::{SessionTunables, TransmissionPolicy};

    fn session(mtu: usize) -> Session {
        Session::with_tunables(
            mtu,
            SessionTunables {
                transmission: TransmissionPolicy::Vanilla,
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

    fn data(datagrams: &[Datagram]) -> Vec<&Datagram> {
        datagrams
            .iter()
            .filter(|d| matches!(d.payload, DatagramPayload::EncapsulatedPackets(_)))
            .collect()
    }

    fn frames(datagram: &Datagram) -> &[EncapsulatedPacket] {
        match &datagram.payload {
            DatagramPayload::EncapsulatedPackets(frames) => frames,
            other => panic!("expected data datagram, got {other:?}"),
        }
    }

    fn feedback(datagrams: &[Datagram], ack: bool) -> Vec<Vec<(u32, u32)>> {
        datagrams
            .iter()
            .filter_map(|d| match (&d.payload, ack) {
                (DatagramPayload::Ack(p), true) | (DatagramPayload::Nak(p), false) => Some(
                    p.ranges
                        .iter()
                        .map(|r| (r.start.value(), r.end.value()))
                        .collect(),
                ),
                _ => None,
            })
            .collect()
    }

    fn ack(session: &mut Session, first: u32, last: u32) {
        session.handle_ack_payload(AckNackPayload {
            ranges: vec![SequenceRange {
                start: Sequence24::new(first),
                end: Sequence24::new(last),
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

    /// DG-1/DG-2: messages are packed up to `MTU - 37` bytes; the first
    /// datagram of an update is 0x84 (slow start, not continuous), later
    /// ones in the same update 0x8c.
    #[test]
    fn packs_messages_and_flags_continuous_send_after_the_first_datagram() {
        let now = Instant::now();
        let mut s = session(1400);
        for _ in 0..40 {
            queue(
                &mut s,
                60,
                Reliability::ReliableOrdered,
                RakPriority::Normal,
            );
        }
        let out = drain(&mut s, now);
        let flags: Vec<u8> = out.iter().map(|d| d.header.flags.bits()).collect();
        assert_eq!(flags, vec![0x84, 0x8c]);
        // 60-byte payload + 10-byte header: 19 per datagram (19·70 = 1330 ≤ 1363).
        assert_eq!(frames(&out[0]).len(), 19);
        assert!(out[0].size() <= 1400 - 28);
        let numbers: Vec<u32> = out
            .iter()
            .flat_map(frames)
            .map(|f| f.reliable_index.unwrap().value())
            .collect();
        assert_eq!(numbers, (0..numbers.len() as u32).collect::<Vec<_>>());
    }

    /// Lets the window, not cwnd, bound a test: slow start as if 2000
    /// datagrams had been ACKed while continuously sending.
    fn open_window(s: &mut Session) {
        let cc = &mut s.vanilla_mut().cc;
        cc.is_continuous_send = true;
        for seq in 0..2000 {
            cc.on_ack(Sequence24::new(seq), Sequence24::new(seq + 1));
        }
        cc.is_continuous_send = false;
    }

    /// CC-1: the BDS capture's slow start: two datagrams, then four, eight…
    /// as each round of ACKs arrives while the queue stays backlogged.
    #[test]
    fn slow_start_doubles_the_burst_per_acked_round() {
        let mut now = Instant::now();
        let mut s = session(1400);
        queue(
            &mut s,
            200_000,
            Reliability::ReliableOrdered,
            RakPriority::Normal,
        );
        let mut bursts = Vec::new();
        let mut next = 0;
        for _ in 0..5 {
            let sent = data(&drain(&mut s, now)).len() as u32;
            bursts.push(sent);
            now += Duration::from_millis(20);
            ack(&mut s, next, next + sent - 1);
            next += sent;
        }
        // One window (1372 bytes) plus the datagram that crossed it, then
        // +1372 bytes of window per ACKed datagram.
        assert_eq!(bursts, vec![2, 4, 8, 16, 32]);
    }

    /// REL-2: message numbers are assigned when a message is sent, so a
    /// higher-priority message queued later gets the lower number.
    #[test]
    fn message_numbers_follow_send_order_not_queue_order() {
        let now = Instant::now();
        let mut s = session(1400);
        queue(&mut s, 10, Reliability::ReliableOrdered, RakPriority::Low);
        queue(&mut s, 10, Reliability::Reliable, RakPriority::Immediate);
        let out = drain(&mut s, now);
        let sent: Vec<(Reliability, u32)> = frames(&out[0])
            .iter()
            .map(|f| (f.header.reliability, f.reliable_index.unwrap().value()))
            .collect();
        assert_eq!(
            sent,
            vec![
                (Reliability::Reliable, 0),
                (Reliability::ReliableOrdered, 1)
            ]
        );
    }

    /// REL-5/REL-11: fragments carry at most `MTU - 60` bytes and an
    /// unreliable message is upgraded to reliable once split.
    #[test]
    fn split_fragments_are_mtu_minus_60_and_reliable() {
        let now = Instant::now();
        let mut s = session(1400);
        open_window(&mut s);
        queue(&mut s, 1340, Reliability::Unreliable, RakPriority::Normal);
        queue(&mut s, 1341, Reliability::Unreliable, RakPriority::Normal);
        let out = drain(&mut s, now);
        let all: Vec<&EncapsulatedPacket> = out.iter().flat_map(frames).collect();
        assert_eq!(all[0].split, None);
        assert_eq!(all[0].header.reliability, Reliability::Unreliable);
        let fragments: Vec<(usize, Reliability)> = all[1..]
            .iter()
            .map(|f| (f.payload.len(), f.header.reliability))
            .collect();
        assert_eq!(
            fragments,
            vec![(1340, Reliability::Reliable), (1, Reliability::Reliable)]
        );
    }

    /// ACK-1: ACKs leave at once before the first RTT sample; afterwards
    /// only when the oldest unacknowledged datagram is 10 ms old.
    #[test]
    fn acks_are_immediate_until_rtt_is_known_then_wait_10ms() {
        let start = Instant::now();
        let mut s = session(1400);
        s.vanilla_accept_sequence(Sequence24::new(0), start)
            .unwrap();
        assert_eq!(s.vanilla_ack_deadline(), Some(start));
        assert_eq!(feedback(&drain(&mut s, start), true), vec![vec![(0, 0)]]);

        // An RTT sample: send one message and have it ACKed.
        queue(&mut s, 10, Reliability::Reliable, RakPriority::Normal);
        let t1 = start + Duration::from_millis(1);
        drain(&mut s, t1);
        ack(&mut s, 0, 0);
        let t2 = start + Duration::from_millis(2);
        drain(&mut s, t2);

        s.vanilla_accept_sequence(Sequence24::new(1), t2).unwrap();
        s.vanilla_accept_sequence(Sequence24::new(2), t2 + Duration::from_millis(4))
            .unwrap();
        assert_eq!(
            s.vanilla_ack_deadline(),
            Some(t2 + Duration::from_millis(10))
        );
        assert!(drain(&mut s, t2 + Duration::from_millis(9)).is_empty());
        let out = drain(&mut s, t2 + Duration::from_millis(10));
        assert_eq!(feedback(&out, true), vec![vec![(1, 2)]]);
        assert_eq!(s.vanilla_ack_deadline(), None);
    }

    /// ACK-1: a completed split message is ACKed without waiting.
    #[test]
    fn split_completion_acks_at_once() {
        let start = Instant::now();
        let mut sender = session(576);
        let mut receiver = session(576);
        // Give the receiver an RTT sample so ACKs would otherwise wait.
        queue(
            &mut receiver,
            10,
            Reliability::Reliable,
            RakPriority::Normal,
        );
        drain(&mut receiver, start);
        ack(&mut receiver, 0, 0);
        drain(&mut receiver, start + Duration::from_millis(1));

        queue(
            &mut sender,
            600,
            Reliability::ReliableOrdered,
            RakPriority::Normal,
        );
        let now = start + Duration::from_millis(2);
        for datagram in data(&drain(&mut sender, now)) {
            receiver
                .vanilla_accept_sequence(datagram.header.sequence, now)
                .unwrap();
            receiver
                .handle_data_payload(frames(datagram).to_vec(), now)
                .unwrap();
        }
        assert_eq!(receiver.vanilla_ack_deadline(), Some(now));
        assert_eq!(
            feedback(&drain(&mut receiver, now), true),
            vec![vec![(0, 1)]]
        );
    }

    /// ACK-3/REL-8: a gap is NAKed by the next update (number by number,
    /// at most 1000), duplicates are ACKed again, and a gap above 50000
    /// rejects the datagram.
    #[test]
    fn gaps_are_naked_immediately_and_duplicates_are_reacked() {
        let now = Instant::now();
        let mut s = session(1400);
        for seq in [0, 3, 3, 1] {
            assert!(
                s.vanilla_accept_sequence(Sequence24::new(seq), now)
                    .unwrap()
            );
        }
        let out = drain(&mut s, now);
        assert_eq!(feedback(&out, false), vec![vec![(1, 2)]]);
        assert_eq!(feedback(&out, true), vec![vec![(0, 1), (3, 3)]]);

        assert!(
            s.vanilla_accept_sequence(Sequence24::new(2004), now)
                .unwrap()
        );
        let later = now + Duration::from_millis(1);
        assert_eq!(
            feedback(&drain(&mut s, later), false),
            vec![vec![(1004, 2003)]]
        );
        assert!(
            !s.vanilla_accept_sequence(Sequence24::new(2005 + 50_001), later)
                .unwrap()
        );
    }

    /// ACK-6: a NAK makes the datagram's messages due; the next update
    /// resends them in a fresh datagram, packed with new data, keeping their
    /// message numbers.
    #[test]
    fn nak_resends_messages_repacked_with_new_data() {
        let now = Instant::now();
        let mut s = session(1400);
        queue(
            &mut s,
            10,
            Reliability::ReliableOrdered,
            RakPriority::Normal,
        );
        let first = drain(&mut s, now);
        assert_eq!(first[0].header.sequence.value(), 0);
        queue(
            &mut s,
            20,
            Reliability::ReliableOrdered,
            RakPriority::Normal,
        );
        nak(&mut s, 0);
        let later = now + Duration::from_millis(5);
        let out = drain(&mut s, later);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].header.sequence.value(), 1);
        let numbers: Vec<u32> = frames(&out[0])
            .iter()
            .map(|f| f.reliable_index.unwrap().value())
            .collect();
        assert_eq!(numbers, vec![0, 1]);
        assert_eq!(s.vanilla_messages_in_flight(), 2);
        ack(&mut s, 1, 1);
        drain(&mut s, later + Duration::from_millis(1));
        assert_eq!(s.vanilla_messages_in_flight(), 0);
    }

    /// CC-2: an unacknowledged message is resent every RTO without
    /// exponential backoff (2 s before the first RTT sample).
    #[test]
    fn timeout_resends_repeat_every_rto_without_backoff() {
        let start = Instant::now();
        let mut s = session(1400);
        queue(&mut s, 10, Reliability::Reliable, RakPriority::Normal);
        drain(&mut s, start);
        let mut resends = Vec::new();
        let mut now = start;
        for _ in 0..799 {
            now += Duration::from_millis(10);
            if !data(&drain(&mut s, now)).is_empty() {
                resends.push(now.duration_since(start).as_millis());
            }
        }
        assert_eq!(resends, vec![2000, 4000, 6000]);
    }

    /// CC-1/DG-2: a NAK while continuously sending halves the slow-start
    /// threshold below the window, so the next datagrams drop the 0x04 bit
    /// (0x88 instead of 0x8c).
    #[test]
    fn needs_bas_flag_clears_once_the_window_exceeds_the_threshold() {
        let mut now = Instant::now();
        let mut s = session(1400);
        queue(
            &mut s,
            100_000,
            Reliability::ReliableOrdered,
            RakPriority::Normal,
        );
        let mut next = 0;
        for _ in 0..3 {
            let sent = data(&drain(&mut s, now)).len() as u32;
            now += Duration::from_millis(5);
            ack(&mut s, next, next + sent - 1);
            next += sent;
        }
        let burst = data(&drain(&mut s, now)).len() as u32;
        assert!(burst > 1);
        nak(&mut s, next);
        now += Duration::from_millis(5);
        let out = drain(&mut s, now);
        let flags: Vec<u8> = data(&out).iter().map(|d| d.header.flags.bits()).collect();
        assert!(!flags.is_empty());
        assert!(flags.iter().all(|&f| f == 0x88), "{flags:x?}");
    }

    /// CC-1/REL-12: at most 512 reliable messages are in flight.
    #[test]
    fn new_sends_stop_at_512_messages_in_flight() {
        let now = Instant::now();
        let mut s = session(1400);
        open_window(&mut s);
        for _ in 0..600 {
            queue(&mut s, 1, Reliability::Reliable, RakPriority::Normal);
        }
        drain(&mut s, now);
        assert_eq!(s.vanilla_messages_in_flight(), 512);
        assert_eq!(s.outgoing_heap.len(), 88);
    }

    /// REL-10: split IDs advance only for split messages.
    #[test]
    fn split_ids_advance_only_for_split_messages() {
        let now = Instant::now();
        let mut s = session(576);
        queue(
            &mut s,
            600,
            Reliability::ReliableOrdered,
            RakPriority::Normal,
        );
        queue(
            &mut s,
            10,
            Reliability::ReliableOrdered,
            RakPriority::Normal,
        );
        queue(
            &mut s,
            600,
            Reliability::ReliableOrdered,
            RakPriority::Normal,
        );
        s.vanilla_mut().cc.is_continuous_send = true;
        for seq in 0..10 {
            s.vanilla_mut()
                .cc
                .on_ack(Sequence24::new(seq), Sequence24::new(seq + 1));
        }
        let ids: Vec<Option<u16>> = drain(&mut s, now)
            .iter()
            .flat_map(frames)
            .map(|f| f.split.as_ref().map(|split| split.id))
            .collect();
        assert_eq!(
            ids,
            vec![Some(0), Some(0), None, Some(1), Some(1)],
            "{ids:?}"
        );
    }

    /// ACK-2: ranges are merged and each datagram holds what fits the
    /// `(MTU - 28) * 8 - 72` bit budget with Jenkins' 65-bit estimate.
    #[test]
    fn ack_chunks_merge_and_respect_the_range_list_budget() {
        let singles: Vec<SequenceRange> = (0..1000u32)
            .map(|seq| SequenceRange {
                start: Sequence24::new(seq * 2),
                end: Sequence24::new(seq * 2),
            })
            .collect();
        let chunks = mojang_ack_chunks(singles, 576);
        // (576 - 28)·8 - 72 = 4312 bits; the k-th single (32 bits) is added
        // while 16 + 32·(k - 1) + 65 ≤ 4312, so 133 fit.
        assert_eq!(chunks[0].len(), 133);
        assert_eq!(chunks.iter().map(Vec::len).sum::<usize>(), 1000);
        let merged = mojang_ack_chunks(
            vec![
                SequenceRange {
                    start: Sequence24::new(5),
                    end: Sequence24::new(9),
                },
                SequenceRange {
                    start: Sequence24::new(0),
                    end: Sequence24::new(4),
                },
            ],
            1400,
        );
        assert_eq!(merged.len(), 1);
        assert_eq!(
            (merged[0][0].start.value(), merged[0][0].end.value()),
            (0, 9)
        );
    }
}

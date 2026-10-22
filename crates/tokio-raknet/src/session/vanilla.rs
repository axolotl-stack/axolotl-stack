//! `TransmissionPolicy::Vanilla` session mechanics (lunar-bedrock/go-raknet
//! `87a6b7e`, see `PARITY.md` DG-*, ACK-*, CC-*).

use std::time::Instant;

use crate::protocol::{
    ack::AckNackPayload,
    constants::DatagramFlags,
    datagram::{Datagram, DatagramPayload},
    types::{DatagramHeader, Sequence24},
};

use super::{Session, TrackedDatagram, ack_queue};

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
        if let Some(datagram) = self.vanilla.resend_ready.pop_front() {
            return Some(datagram);
        }
        if let Some(datagram) = self.vanilla.feedback_ready.pop_front() {
            return Some(datagram);
        }
        let mut feedback = Vec::new();
        self.vanilla_feedback(false, &mut feedback);
        if !feedback.is_empty() {
            self.vanilla.feedback_ready.extend(feedback);
            return self.vanilla.feedback_ready.pop_front();
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
            let rtt = self.vanilla.rtt(now);
            self.vanilla.store_rtt(rtt);
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
        out.extend(self.vanilla.resend_ready.drain(..));
        out.extend(self.vanilla.feedback_ready.drain(..));
        self.vanilla_feedback(false, &mut out);
        out
    }

    /// go-raknet `conn.go:handleACK` / `resend_map.go:acknowledge`.
    pub(super) fn vanilla_process_acks(&mut self, now: Instant) {
        while let Some(range) = self.incoming_acks.pop_front() {
            while let Some((&seq, _)) = self.sent_datagrams.range(range.start..=range.end).next() {
                if let Some(tracked) = self.sent_datagrams.remove(&seq) {
                    self.vanilla.record(now, tracked.send_time, 1);
                }
            }
        }
    }

    /// go-raknet `conn.go:handleNACK` / `resend`: every NAKed datagram still
    /// awaiting an ACK is resent immediately with a fresh sequence number.
    pub(super) fn vanilla_process_naks(&mut self, now: Instant) {
        while let Some(range) = self.incoming_naks.pop_front() {
            while let Some((&seq, _)) = self.sent_datagrams.range(range.start..=range.end).next() {
                if let Some(tracked) = self.sent_datagrams.remove(&seq) {
                    let datagram = self.vanilla_retransmit(tracked, now);
                    self.vanilla.resend_ready.push_back(datagram);
                }
            }
        }
    }

    /// `resend_map.go:retransmit` (sample `2 × age`) followed by
    /// `conn.go:sendDatagram` (fresh sequence, new timestamp).
    fn vanilla_retransmit(&mut self, mut tracked: TrackedDatagram, now: Instant) -> Datagram {
        self.vanilla.record(now, tracked.send_time, 2);
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
        self.vanilla.rtt(now)
    }

    /// Unacknowledged datagrams (`len(conn.retransmission.unacknowledged)`).
    pub(crate) fn unacknowledged(&self) -> usize {
        self.sent_datagrams.len()
    }

    /// Drop the resend state (`conn.go:closeImmediately`).
    pub(crate) fn clear_unacknowledged(&mut self) {
        self.sent_datagrams.clear();
        self.vanilla.resend_ready.clear();
    }

    /// `conn.go:startTicking` tick counter.
    pub(crate) fn vanilla_ticks(&self) -> u64 {
        self.vanilla.ticks
    }
}

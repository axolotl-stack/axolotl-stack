use std::collections::HashMap;
use std::time::{Duration, Instant};

use bytes::BytesMut;

use crate::protocol::encapsulated_packet::EncapsulatedPacket;
use crate::protocol::packet::DecodeError;
use crate::protocol::reliability::Reliability;
use crate::protocol::types::Sequence24;

struct SplitEntry {
    reliability: Reliability,
    reliable_index: Option<Sequence24>,
    sequence_index: Option<Sequence24>,
    ordering_index: Option<Sequence24>,
    ordering_channel: Option<u8>,
    needs_bas: bool,

    parts: Vec<Option<bytes::Bytes>>,
    received: usize,
    last_update: Instant,
}

pub struct SplitAssembler {
    entries: HashMap<u16, SplitEntry>,
    ttl: Duration,
    max_parts: u32,
    max_concurrent: usize,
}

impl SplitAssembler {
    pub fn new(ttl: Duration, max_parts: u32, max_concurrent: usize) -> Self {
        let ttl = if ttl.is_zero() {
            Duration::from_secs(30)
        } else {
            ttl
        };
        Self {
            entries: HashMap::new(),
            ttl,
            max_parts,
            max_concurrent,
        }
    }

    pub fn add(
        &mut self,
        pkt: EncapsulatedPacket,
        now: Instant,
    ) -> Result<Option<EncapsulatedPacket>, DecodeError> {
        if !pkt.header.is_split {
            return Ok(Some(pkt));
        }

        // If header says it's split, we MUST have split info.
        // If not, it's a malformed packet logic error or decode error.
        let split = pkt.split.as_ref().ok_or(DecodeError::MissingSplitInfo)?;

        if split.count > self.max_parts {
            return Err(DecodeError::SplitTooLarge);
        }

        if self.entries.len() >= self.max_concurrent && !self.entries.contains_key(&split.id) {
            return Err(DecodeError::SplitBufferFull);
        }

        let entry = self.entries.entry(split.id).or_insert_with(|| SplitEntry {
            reliability: pkt.header.reliability,
            reliable_index: pkt.reliable_index,
            sequence_index: pkt.sequence_index,
            ordering_index: pkt.ordering_index,
            ordering_channel: pkt.ordering_channel,
            needs_bas: pkt.header.needs_bas,
            parts: vec![None; split.count as usize],
            received: 0,
            last_update: now,
        });

        if entry.parts.len() != split.count as usize {
            return Err(DecodeError::SplitCountMismatch);
        }
        let idx = split.index as usize;
        if idx >= entry.parts.len() {
            return Err(DecodeError::SplitIndexOutOfRange);
        }
        if entry.parts[idx].is_some() {
            // Duplicate part, just ignore it.
            // Returning an error here causes connection drops/lag in some implementations
            // if the sender aggressively retransmits parts.
            tracing::warn!(
                id = split.id,
                index = split.index,
                count = split.count,
                "duplicate_split_part"
            );
            return Ok(None);
        }

        entry.parts[idx] = Some(pkt.payload.clone());
        entry.received += 1;
        entry.last_update = now;

        if entry.received != entry.parts.len() {
            return Ok(None);
        }

        // All parts present: reassemble
        let mut buf = BytesMut::new();
        for part in &entry.parts {
            let part = part.as_ref().ok_or(DecodeError::SplitCountMismatch)?; // Should not happen given entry.received check
            buf.extend_from_slice(part);
        }
        let payload = buf.freeze();
        let bit_length = (payload.len() as u16) << 3;

        tracing::trace!("reassembled_split_packet");

        let header = crate::protocol::types::EncapsulatedPacketHeader::new(
            entry.reliability,
            false,
            entry.needs_bas,
        );

        let assembled = EncapsulatedPacket {
            header,
            bit_length,
            reliable_index: entry.reliable_index,
            sequence_index: entry.sequence_index,
            ordering_index: entry.ordering_index,
            ordering_channel: entry.ordering_channel,
            split: None,
            payload,
        };

        self.entries.remove(&split.id);
        Ok(Some(assembled))
    }

    pub fn prune(&mut self, now: Instant) -> Vec<(Option<u8>, Option<Sequence24>)> {
        let mut dropped = Vec::new();
        self.entries.retain(|id, entry| {
            if now.duration_since(entry.last_update) >= self.ttl {
                tracing::warn!(
                    id = id,
                    age = ?now.duration_since(entry.last_update),
                    "dropping_expired_split_packet"
                );
                dropped.push((entry.ordering_channel, entry.ordering_index));
                false
            } else {
                true
            }
        });
        dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{
        encapsulated_packet::SplitInfo,
        reliability::Reliability,
        types::{EncapsulatedPacketHeader, Sequence24},
    };
    use bytes::Bytes;

    fn make_split_encap(count: u32, index: u32) -> EncapsulatedPacket {
        EncapsulatedPacket {
            header: EncapsulatedPacketHeader::new(Reliability::ReliableOrdered, true, true),
            bit_length: 16,
            reliable_index: Some(Sequence24::new(index)),
            sequence_index: None,
            ordering_index: Some(Sequence24::new(0)),
            ordering_channel: Some(0),
            split: Some(SplitInfo {
                count,
                id: 1,
                index,
            }),
            payload: Bytes::from_static(b"abcd"),
        }
    }

    #[test]
    fn rejects_too_many_parts() {
        let mut assembler = SplitAssembler::new(Duration::from_secs(30), 128, 256);
        let pkt = make_split_encap(129, 0);
        let now = Instant::now();
        let res = assembler.add(pkt, now);
        assert!(matches!(res, Err(DecodeError::SplitTooLarge)));
    }

    #[test]
    fn rejects_when_buffer_full() {
        let mut assembler = SplitAssembler::new(Duration::from_secs(30), 128, 4);
        let now = Instant::now();

        // Fill the buffer with max entries
        for id in 0..4 {
            let mut pkt = make_split_encap(2, 0);
            if let Some(split) = pkt.split.as_mut() {
                split.id = id as u16;
            }
            let _ = assembler.add(pkt, now);
        }

        let mut overflow = make_split_encap(2, 0);
        if let Some(split) = overflow.split.as_mut() {
            split.id = 999;
        }
        let res = assembler.add(overflow, now);
        assert!(matches!(res, Err(DecodeError::SplitBufferFull)));
    }
}

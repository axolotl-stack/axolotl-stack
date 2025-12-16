use crate::protocol::{
    packet::{DecodeError, RaknetEncodable},
    types::{EncapsulatedPacketHeader, Sequence24},
};
use bytes::{Buf, BufMut, Bytes};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SplitInfo {
    pub count: u32,
    pub id: u16,
    pub index: u32,
}

/// Mirrors Cloudburst EncapsulatedPacket / Go Frame at a high level.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EncapsulatedPacket {
    pub header: EncapsulatedPacketHeader, // reliability + split + needs_bas bits
    pub bit_length: u16,                  // bitSize from the wire
    pub reliable_index: Option<Sequence24>,
    pub sequence_index: Option<Sequence24>,
    pub ordering_index: Option<Sequence24>,
    pub ordering_channel: Option<u8>,
    pub split: Option<SplitInfo>,
    pub payload: Bytes,
}

impl EncapsulatedPacket {
    /// Convenience: payload length in bytes (derived from bit_length).
    pub fn payload_len(&self) -> usize {
        ((self.bit_length as usize) + 7) >> 3
    }

    /// Total on-wire size (header + payload) of this encapsulated packet.
    pub fn size(&self) -> usize {
        // Base header: 1 byte flags + 2 bytes bit_length.
        let mut size = 3usize;

        let rel = self.header.reliability;
        if rel.is_reliable() {
            size += 3; // reliable_index
        }
        if rel.is_sequenced() {
            size += 3; // sequence_index
        }
        if rel.is_ordered() || rel.is_sequenced() {
            size += 3; // ordering_index
            size += 1; // ordering_channel
        }
        if self.header.is_split {
            size += 4; // partCount (u32)
            size += 2; // partId (u16)
            size += 4; // partIndex (u32)
        }

        size + self.payload_len()
    }
}

impl RaknetEncodable for EncapsulatedPacket {
    fn encode_raknet(
        &self,
        dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        use crate::protocol::packet::EncodeError;

        // 1) flags byte
        self.header.encode_raknet(dst)?;

        // 2) bit length
        self.bit_length.encode_raknet(dst)?;

        // 3) reliability‑dependent indexes
        let rel = self.header.reliability;

        if rel.is_reliable() {
            let idx = self
                .reliable_index
                .ok_or(EncodeError::MissingReliableIndex)?;
            idx.encode_raknet(dst)?;
        }

        if rel.is_sequenced() {
            let idx = self
                .sequence_index
                .ok_or(EncodeError::MissingSequenceIndex)?;
            idx.encode_raknet(dst)?;
        }

        if rel.is_ordered() || rel.is_sequenced() {
            let idx = self
                .ordering_index
                .ok_or(EncodeError::MissingOrderingIndex)?;
            idx.encode_raknet(dst)?;

            let ch = self
                .ordering_channel
                .ok_or(EncodeError::MissingOrderingChannel)?;
            ch.encode_raknet(dst)?;
        }

        // 4) split metadata
        if self.header.is_split {
            let split = self.split.as_ref().ok_or(EncodeError::MissingSplitInfo)?;
            split.count.encode_raknet(dst)?;
            split.id.encode_raknet(dst)?;
            split.index.encode_raknet(dst)?;
        }

        // 5) payload (assumed already correct length)
        dst.put_slice(&self.payload);
        Ok(())
    }

    fn decode_raknet(src: &mut impl Buf) -> Result<Self, DecodeError> {
        // 1) flags / header byte
        let header = EncapsulatedPacketHeader::decode_raknet(src)?;

        // 2) bit length
        let bit_length = u16::decode_raknet(src)?;
        let payload_len = ((bit_length as usize) + 7) >> 3;

        let rel = header.reliability;

        // 3) reliability‑dependent indexes
        let reliable_index = if rel.is_reliable() {
            Some(Sequence24::decode_raknet(src)?)
        } else {
            None
        };

        let sequence_index = if rel.is_sequenced() {
            Some(Sequence24::decode_raknet(src)?)
        } else {
            None
        };

        let (ordering_index, ordering_channel) = if rel.is_ordered() || rel.is_sequenced() {
            let idx = Sequence24::decode_raknet(src)?;
            let ch = u8::decode_raknet(src)?;
            (Some(idx), Some(ch))
        } else {
            (None, None)
        };

        // 4) split metadata
        let split = if header.is_split {
            let count = u32::decode_raknet(src)?;
            let id = u16::decode_raknet(src)?;
            let index = u32::decode_raknet(src)?;
            Some(SplitInfo { count, id, index })
        } else {
            None
        };

        // 5) payload
        if src.remaining() < payload_len {
            return Err(DecodeError::UnexpectedEof);
        }
        let payload = src.copy_to_bytes(payload_len);

        Ok(EncapsulatedPacket {
            header,
            bit_length,
            reliable_index,
            sequence_index,
            ordering_index,
            ordering_channel,
            split,
            payload,
        })
    }
}

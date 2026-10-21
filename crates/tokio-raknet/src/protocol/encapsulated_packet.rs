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

        // Coalesce adjacent fixed fields without staging or copying the payload.
        let bit_length = self.bit_length.to_be_bytes();
        dst.put_slice(&[self.header.to_byte(), bit_length[0], bit_length[1]]);

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
            let ch = self
                .ordering_channel
                .ok_or(EncodeError::MissingOrderingChannel)?;
            dst.put_u32_le(idx.value() | (u32::from(ch) << 24));
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
        // packet.go:packet.read rejects empty content before handling indexes.
        if payload_len == 0 {
            return Err(DecodeError::InvalidPacketLength);
        }

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
            // conn.go:Conn.receiveSplitPacket checks the index before storage.
            // Reject zero counts too, rather than allowing an empty assembly.
            if count == 0 || index >= count {
                return Err(DecodeError::SplitIndexOutOfRange);
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    #[test]
    fn sequenced_split_encoding_preserves_mixed_endianness_and_high_channel() {
        use crate::protocol::reliability::Reliability;
        let packet = EncapsulatedPacket {
            header: EncapsulatedPacketHeader::new(Reliability::ReliableSequenced, true, true),
            bit_length: 8,
            reliable_index: Some(Sequence24::new(0xabcdef)),
            sequence_index: Some(Sequence24::new(0x123456)),
            ordering_index: Some(Sequence24::new(0xfedcba)),
            ordering_channel: Some(255),
            split: Some(SplitInfo {
                count: 2,
                id: 0x1234,
                index: 1,
            }),
            payload: Bytes::from_static(b"\xff"),
        };
        let mut encoded = BytesMut::new();
        packet.encode_raknet(&mut encoded).unwrap();
        assert_eq!(
            encoded.as_ref(),
            &[
                0x94, 0, 8, 0xef, 0xcd, 0xab, 0x56, 0x34, 0x12, 0xba, 0xdc, 0xfe, 0xff, 0, 0, 0, 2,
                0x12, 0x34, 0, 0, 0, 1, 0xff,
            ]
        );
    }

    #[test]
    fn rejects_empty_payload() {
        let mut src = &[0, 0, 0][..];
        assert!(matches!(
            EncapsulatedPacket::decode_raknet(&mut src),
            Err(DecodeError::InvalidPacketLength)
        ));
    }

    #[test]
    fn rejects_invalid_split_metadata() {
        for (count, index) in [(0u32, 0u32), (1, 1), (2, u32::MAX)] {
            let mut src = BytesMut::new();
            src.put_u8(0x10);
            src.put_u16(8);
            src.put_u32(count);
            src.put_u16(7);
            src.put_u32(index);
            src.put_u8(0xff);
            assert!(matches!(
                EncapsulatedPacket::decode_raknet(&mut src.freeze()),
                Err(DecodeError::SplitIndexOutOfRange)
            ));
        }
    }

    #[test]
    fn rejects_every_truncated_split_header() {
        // Reliable ordered split packet, with one payload byte.
        let wire = [
            0x70, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 7, 0, 0, 0, 0, 0xff,
        ];
        for len in 0..wire.len() {
            assert!(EncapsulatedPacket::decode_raknet(&mut &wire[..len]).is_err());
        }
        let packet = EncapsulatedPacket::decode_raknet(&mut &wire[..]).unwrap();
        assert_eq!(packet.payload.as_ref(), &[0xff]);
    }
}

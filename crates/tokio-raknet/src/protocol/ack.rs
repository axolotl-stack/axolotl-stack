use bytes::{Buf, BufMut};

use crate::protocol::{
    constants::{MAX_ACK_SEQUENCES, MAX_INBOUND_ACK_SEQUENCES},
    packet::{DecodeError, RaknetEncodable},
    types::Sequence24,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SequenceRange {
    pub start: Sequence24,
    pub end: Sequence24,
}

impl RaknetEncodable for SequenceRange {
    fn encode_raknet(
        &self,
        dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        let singleton = self.start == self.end;
        // One fixed-width store for the tag and little-endian start sequence.
        dst.put_u32_le((self.start.value() << 8) | u32::from(singleton));
        if !singleton {
            self.end.encode_raknet(dst)?;
        }
        Ok(())
    }

    fn decode_raknet(src: &mut impl Buf) -> Result<Self, DecodeError> {
        decode_record(src).map(|(range, _)| range)
    }
}

/// One ACK/NAK record and whether it used the single-sequence tag.
fn decode_record(src: &mut impl Buf) -> Result<(SequenceRange, bool), DecodeError> {
    let singleton = match u8::decode_raknet(src)? {
        0 => false,
        1 => true,
        _ => return Err(DecodeError::InvalidAckPacket),
    };
    let start = Sequence24::decode_raknet(src)?;
    let end = if singleton {
        start
    } else {
        Sequence24::decode_raknet(src)?
    };
    let range = SequenceRange { start, end };

    // Local wrapping ranges are split before encoding. Reject wrapped or
    // over-wide inbound ranges so a tiny ACK/NACK record cannot expand into
    // millions of per-sequence operations during session processing
    // (go-raknet rejects both too: `end - start` underflows or exceeds 8192).
    if !singleton && range.wraps() {
        return Err(DecodeError::InvalidAckPacket);
    }
    if range.len() > MAX_INBOUND_ACK_SEQUENCES {
        return Err(DecodeError::InvalidAckPacket);
    }

    Ok((range, singleton))
}

impl SequenceRange {
    /// Returns the number of inclusive sequence IDs covered by this range.
    pub fn len(&self) -> u32 {
        self.start.distance_to(self.end).saturating_add(1)
    }

    /// Sequence ranges are inclusive and validation rejects wrapped ranges, so
    /// decoded ranges always contain at least one sequence.
    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn size(&self) -> usize {
        let mut size = 4;
        if self.start != self.end {
            size += 3;
        }
        size
    }

    pub fn wraps(&self) -> bool {
        self.start != self.end && self.start.value() > self.end.value()
    }

    pub fn split_wrapping(&self) -> Option<(SequenceRange, SequenceRange)> {
        if !self.wraps() {
            return None;
        }

        let tail = SequenceRange {
            start: self.start,
            end: Sequence24::new(0x00FF_FFFF),
        };
        let head = SequenceRange {
            start: Sequence24::new(0),
            end: self.end,
        };

        Some((tail, head))
    }

    pub fn record_count(&self) -> usize {
        if self.wraps() { 2 } else { 1 }
    }

    pub fn encoded_size(&self) -> usize {
        if let Some((tail, head)) = self.split_wrapping() {
            tail.size() + head.size()
        } else {
            self.size()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AckNackPayload {
    pub ranges: Vec<SequenceRange>,
}

impl RaknetEncodable for AckNackPayload {
    fn encode_raknet(
        &self,
        dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        let total_records: usize = self.ranges.iter().map(|r| r.record_count()).sum();
        dst.put_u16(total_records as u16);

        for r in &self.ranges {
            if let Some((tail, head)) = r.split_wrapping() {
                tail.encode_raknet(dst)?;
                head.encode_raknet(dst)?;
            } else {
                r.encode_raknet(dst)?;
            }
        }
        Ok(())
    }

    fn decode_raknet(src: &mut impl Buf) -> Result<Self, DecodeError> {
        let len = u16::decode_raknet(src)?;

        if len > MAX_ACK_SEQUENCES {
            return Err(DecodeError::InvalidAckPacket);
        }

        // Every record needs at least a tag and a 24-bit sequence. Check before
        // reserving memory from an untrusted count (acknowledge.go:acknowledgement.read).
        if src.remaining() < usize::from(len) * 4 {
            return Err(DecodeError::UnexpectedEof);
        }

        let mut ranges = Vec::with_capacity(len as usize);
        // go-raknet acknowledge.go:acknowledgement.read: a range record is
        // accepted while `total + end - start <= 8192` (one range may cover
        // 8193 sequences), a single while `total + 1 <= 8192`.
        let max = u32::from(MAX_ACK_SEQUENCES);
        let mut total_sequences = 0u32;
        for _ in 0..len {
            let (range, singleton) = decode_record(src)?;
            let added = if singleton { 1 } else { range.len() - 1 };
            if total_sequences + added > max {
                return Err(DecodeError::InvalidAckPacket);
            }
            total_sequences += range.len();
            ranges.push(range);
        }

        Ok(Self { ranges })
    }
}

impl AckNackPayload {
    pub fn size(&self) -> usize {
        let mut size = 2;
        for r in &self.ranges {
            size += r.encoded_size();
        }
        size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    #[test]
    fn wrapped_singletons_encode_across_a_segmented_destination() {
        let payload = AckNackPayload {
            ranges: vec![
                SequenceRange {
                    start: Sequence24::new(0xffffff),
                    end: Sequence24::new(0),
                },
                SequenceRange {
                    start: Sequence24::new(0xabcdef),
                    end: Sequence24::new(0xabcdef),
                },
            ],
        };
        let mut first = [0u8; 3];
        let mut second = [0u8; 11];
        payload
            .encode_raknet(&mut (&mut first[..]).chain_mut(&mut second[..]))
            .unwrap();
        assert_eq!(
            [first.as_slice(), second.as_slice()].concat(),
            [0, 3, 1, 0xff, 0xff, 0xff, 1, 0, 0, 0, 1, 0xef, 0xcd, 0xab,]
        );
    }

    #[test]
    fn rejects_unknown_record_type() {
        let mut src = &[0, 1, 2, 0, 0, 0, 0, 0, 0][..];
        assert!(matches!(
            AckNackPayload::decode_raknet(&mut src),
            Err(DecodeError::InvalidAckPacket)
        ));
    }

    #[test]
    fn accepts_exact_sequence_limit() {
        let mut src = &[0, 1, 0, 0, 0, 0, 0xff, 0x1f, 0][..];
        let decoded = AckNackPayload::decode_raknet(&mut src).unwrap();
        assert_eq!(decoded.ranges[0].len(), 8192);
    }

    #[test]
    fn rejects_truncated_record_count() {
        let mut src = &[0x20, 0][..];
        assert!(matches!(
            AckNackPayload::decode_raknet(&mut src),
            Err(DecodeError::UnexpectedEof)
        ));
    }

    #[test]
    fn encodes_expected_layout() -> Result<(), DecodeError> {
        let payload = AckNackPayload {
            ranges: vec![
                SequenceRange {
                    start: Sequence24::new(1),
                    end: Sequence24::new(1),
                },
                SequenceRange {
                    start: Sequence24::new(5),
                    end: Sequence24::new(8),
                },
            ],
        };

        let mut buf = BytesMut::new();
        payload.encode_raknet(&mut buf).unwrap();

        let expected = [
            0x00, 0x02, // record count
            0x01, 0x01, 0x00, 0x00, // single packet 1
            0x00, 0x05, 0x00, 0x00, 0x08, 0x00, 0x00, // range 5-8
        ];

        assert_eq!(buf.as_ref(), expected);
        Ok(())
    }

    #[test]
    fn splits_wrap_ranges() -> Result<(), DecodeError> {
        let payload = AckNackPayload {
            ranges: vec![SequenceRange {
                start: Sequence24::new(0x00FF_FFFE),
                end: Sequence24::new(2),
            }],
        };

        assert_eq!(payload.size(), 16);

        let mut buf = BytesMut::new();
        payload.encode_raknet(&mut buf).unwrap();

        let expected = [
            0x00, 0x02, // record count after splitting
            0x00, 0xFE, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, // range [0xFFFE, 0xFFFFFF]
            0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, // range [0, 2]
        ];

        assert_eq!(buf.as_ref(), expected);
        Ok(())
    }

    #[test]
    fn rejects_wrapping_inbound_range() {
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&[
            0x00, 0x01, // one record
            0x00, // range, not singleton
            0x64, 0x00, 0x00, // start = 100
            0x32, 0x00, 0x00, // end = 50
        ]);

        let mut slice = buf.freeze();
        let err = AckNackPayload::decode_raknet(&mut slice).unwrap_err();
        assert!(matches!(err, DecodeError::InvalidAckPacket));
    }

    /// ACK-7: go-raknet rejects a range record once `end - start > 8192`.
    #[test]
    fn rejects_overwide_inbound_range() {
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&[
            0x00, 0x01, // one record
            0x00, // range, not singleton
            0x00, 0x00, 0x00, // start = 0
            0x01, 0x20, 0x00, // end = 8193, len = 8194
        ]);

        let mut slice = buf.freeze();
        let err = AckNackPayload::decode_raknet(&mut slice).unwrap_err();
        assert!(matches!(err, DecodeError::InvalidAckPacket));
    }

    /// ACK-7: go-raknet accepts a range record while `total + end - start
    /// <= 8192`, so a range may close a payload at 8193 sequences; a single
    /// record needs `total + 1 <= 8192`.
    #[test]
    fn accepts_go_range_boundary_of_8193_sequences() {
        // One range with `end - start == 8192`.
        let mut src = &[0, 1, 0, 0, 0, 0, 0x00, 0x20, 0x00][..];
        let decoded = AckNackPayload::decode_raknet(&mut src).unwrap();
        assert_eq!(decoded.ranges[0].len(), 8193);
        // A single, then a range of 8192 sequences: `1 + 8191 <= 8192`.
        let mut src = &[0, 2, 1, 0xaa, 0xbb, 0x00, 0, 0, 0, 0, 0xff, 0x1f, 0][..];
        let decoded = AckNackPayload::decode_raknet(&mut src).unwrap();
        let total: u32 = decoded.ranges.iter().map(SequenceRange::len).sum();
        assert_eq!(total, 8193);
        // A range of 8192 sequences, then a range record covering one more
        // sequence: `8192 + 0 <= 8192`.
        let mut src = &[0, 2, 0, 0, 0, 0, 0xff, 0x1f, 0, 0, 0, 0x30, 0, 0, 0x30, 0][..];
        assert!(AckNackPayload::decode_raknet(&mut src).is_ok());
    }

    #[test]
    fn rejects_payload_with_too_many_total_sequences() {
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&[
            0x00, 0x02, // two records
            0x00, // range, not singleton
            0x00, 0x00, 0x00, // start = 0
            0xff, 0x1f, 0x00, // end = 8191, len = 8192
            0x01, // singleton
            0x00, 0x20, 0x00, // sequence = 8192, total = 8193
        ]);

        let mut slice = buf.freeze();
        let err = AckNackPayload::decode_raknet(&mut slice).unwrap_err();
        assert!(matches!(err, DecodeError::InvalidAckPacket));
    }
}

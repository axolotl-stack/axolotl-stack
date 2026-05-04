use bytes::{Buf, BufMut};

use crate::protocol::{
    constants::MAX_ACK_SEQUENCES,
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
        singleton.encode_raknet(dst)?;
        self.start.encode_raknet(dst)?;
        if !singleton {
            self.end.encode_raknet(dst)?;
        }
        Ok(())
    }

    fn decode_raknet(src: &mut impl Buf) -> Result<Self, DecodeError> {
        let singleton = bool::decode_raknet(src)?;
        let start = Sequence24::decode_raknet(src)?;
        let end = if singleton {
            start
        } else {
            Sequence24::decode_raknet(src)?
        };
        let range = SequenceRange { start, end };

        // Local wrapping ranges are split before encoding. Reject wrapped or
        // over-wide inbound ranges so a tiny ACK/NACK record cannot expand into
        // millions of per-sequence operations during session processing.
        if !singleton && range.wraps() {
            return Err(DecodeError::InvalidAckPacket);
        }
        if range.len() > MAX_ACK_SEQUENCES as u32 {
            return Err(DecodeError::InvalidAckPacket);
        }

        Ok(range)
    }
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

        let mut ranges = Vec::with_capacity(len as usize);
        let mut total_sequences = 0u32;
        for _ in 0..len {
            let range = SequenceRange::decode_raknet(src)?;
            total_sequences = total_sequences.saturating_add(range.len());
            if total_sequences > MAX_ACK_SEQUENCES as u32 {
                return Err(DecodeError::InvalidAckPacket);
            }
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

    #[test]
    fn rejects_overwide_inbound_range() {
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&[
            0x00, 0x01, // one record
            0x00, // range, not singleton
            0x00, 0x00, 0x00, // start = 0
            0x00, 0x20, 0x00, // end = 8192, len = 8193
        ]);

        let mut slice = buf.freeze();
        let err = AckNackPayload::decode_raknet(&mut slice).unwrap_err();
        assert!(matches!(err, DecodeError::InvalidAckPacket));
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

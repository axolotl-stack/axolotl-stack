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
        Ok(SequenceRange { start, end })
    }
}

impl SequenceRange {
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

        Ok(Self {
            ranges: std::iter::repeat_with(|| SequenceRange::decode_raknet(src))
                .take(len as usize)
                .collect::<Result<Vec<SequenceRange>, DecodeError>>()?,
        })
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
}

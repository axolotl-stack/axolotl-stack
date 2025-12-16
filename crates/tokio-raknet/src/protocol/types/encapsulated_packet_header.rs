use bytes::{Buf, BufMut};

use crate::protocol::{
    constants::{FRAME_FLAG_NEEDS_BAS, FRAME_FLAG_SPLIT},
    packet::{DecodeError, RaknetEncodable},
    reliability::Reliability,
};

/// Header byte of a RakNet encapsulated packet (frame).
///
/// Layout on the wire (first byte of the frame):
///   bits 7..5: reliability (0..7)
///   bit 4:     split flag
///   bit 3:     needs B&As flag
///   bits 2..0: reserved (currently unused, must be zero)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncapsulatedPacketHeader {
    pub reliability: Reliability,
    pub is_split: bool,
    pub needs_bas: bool,
}

impl EncapsulatedPacketHeader {
    /// Construct a header with the given reliability and flags.
    pub fn new(reliability: Reliability, is_split: bool, needs_bas: bool) -> Self {
        Self {
            reliability,
            is_split,
            needs_bas,
        }
    }

    /// Same as `new` but with no flags set.
    pub fn with_reliability(reliability: Reliability) -> Self {
        Self {
            reliability,
            is_split: false,
            needs_bas: false,
        }
    }

    /// Convert this header into the raw header byte.
    #[inline]
    pub fn to_byte(self) -> u8 {
        let mut b = (self.reliability as u8) << 5;

        if self.is_split {
            b |= FRAME_FLAG_SPLIT;
        }
        if self.needs_bas {
            b |= FRAME_FLAG_NEEDS_BAS;
        }

        b
    }

    /// Construct a header from the raw header byte.
    #[inline]
    pub fn from_byte(b: u8) -> Result<Self, DecodeError> {
        let reliability_bits = b >> 5;
        let reliability = Reliability::try_from(reliability_bits)?;

        let is_split = (b & FRAME_FLAG_SPLIT) != 0;
        let needs_bas = (b & FRAME_FLAG_NEEDS_BAS) != 0;

        Ok(Self {
            reliability,
            is_split,
            needs_bas,
        })
    }
}

impl RaknetEncodable for EncapsulatedPacketHeader {
    fn encode_raknet(
        &self,
        dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        dst.put_u8(self.to_byte());
        Ok(())
    }

    fn decode_raknet(src: &mut impl Buf) -> Result<Self, DecodeError> {
        if !src.has_remaining() {
            return Err(DecodeError::UnexpectedEof);
        }
        let b = src.get_u8();
        EncapsulatedPacketHeader::from_byte(b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    #[test]
    fn roundtrip_header_fields() {
        for &reliability in &[
            Reliability::Unreliable,
            Reliability::Reliable,
            Reliability::ReliableOrderedWithAckReceipt,
        ] {
            for &is_split in &[false, true] {
                for &needs_bas in &[false, true] {
                    let header = EncapsulatedPacketHeader::new(reliability, is_split, needs_bas);
                    let b = header.to_byte();
                    let decoded = EncapsulatedPacketHeader::from_byte(b).unwrap();
                    assert_eq!(decoded.reliability, reliability);
                    assert_eq!(decoded.is_split, is_split);
                    assert_eq!(decoded.needs_bas, needs_bas);
                }
            }
        }
    }

    #[test]
    fn roundtrip_via_raknetencodable() {
        let header = EncapsulatedPacketHeader::new(Reliability::ReliableOrdered, true, true);
        let mut buf = BytesMut::new();
        header.encode_raknet(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = EncapsulatedPacketHeader::decode_raknet(&mut slice).unwrap();
        assert_eq!(decoded, header);
    }
}

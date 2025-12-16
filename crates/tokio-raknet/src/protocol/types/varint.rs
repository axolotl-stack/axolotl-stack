use bytes::{Buf, BufMut};

use crate::protocol::packet::{DecodeError, EncodeError, RaknetEncodable};

/// Unsigned variable-length integer encoding used by RakNet.
pub struct VarUInt(pub u64);

/// Signed variable-length integer encoding (zig-zag over `VarUInt`).
pub struct VarInt(pub i64);

impl RaknetEncodable for VarUInt {
    fn encode_raknet(&self, dst: &mut impl BufMut) -> Result<(), EncodeError> {
        // clone it to mut it.
        let mut v = self.0;
        while v >= 0x80 {
            dst.put_u8(((v & 0x7f) | 0x80) as u8);
            v >>= 7
        }
        dst.put_u8((v & 0x7f) as u8);
        Ok(())
    }

    fn decode_raknet(src: &mut impl Buf) -> Result<Self, DecodeError> {
        let mut result = 0;
        let mut shift = 0;
        loop {
            if shift >= 64 {
                return Err(DecodeError::VarIntExceedsLimit);
            }
            if !src.has_remaining() {
                return Err(DecodeError::UnexpectedEof);
            }
            let v = src.get_u8();
            result |= ((v & 0x7f) as u64) << shift;
            if v & 0x80 == 0 {
                break;
            }
            shift += 7
        }
        Ok(VarUInt(result))
    }
}

impl RaknetEncodable for VarInt {
    fn encode_raknet(&self, dst: &mut impl BufMut) -> Result<(), EncodeError> {
        let ux = ((self.0 << 1) ^ (self.0 >> 63)) as u64;
        VarUInt(ux).encode_raknet(dst)
    }

    fn decode_raknet(src: &mut impl Buf) -> Result<Self, DecodeError> {
        let ux = VarUInt::decode_raknet(src)?.0;
        let x = ((ux >> 1) as i64) ^ (-((ux & 1) as i64));
        Ok(VarInt(x))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    #[test]
    fn varuint_roundtrip_basic_values() -> Result<(), DecodeError> {
        for &v in &[0u64, 1, 127, 128, 255, 300, u32::MAX as u64] {
            let original = VarUInt(v);
            let mut buf = BytesMut::new();
            original.encode_raknet(&mut buf).unwrap();
            let mut slice = buf.freeze();
            let decoded = VarUInt::decode_raknet(&mut slice)?;
            assert_eq!(decoded.0, v);
        }
        Ok(())
    }

    #[test]
    fn varint_roundtrip_basic_values() -> Result<(), DecodeError> {
        for &v in &[0i64, 1, -1, 127, -127, 300, -300] {
            let original = VarInt(v);
            let mut buf = BytesMut::new();
            original.encode_raknet(&mut buf).unwrap();
            let mut slice = buf.freeze();
            let decoded = VarInt::decode_raknet(&mut slice)?;
            assert_eq!(decoded.0, v);
        }
        Ok(())
    }
}

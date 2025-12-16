use bytes::{Buf, BufMut};
use std::mem;

use crate::protocol::packet::{DecodeError, EncodeError, RaknetEncodable};

/// Internal helper macro implementing big-endian integer encoding/decoding
/// for a concrete integer type.
macro_rules! impl_raknet_int {
    ($ty:ty, $put:ident, $get:ident) => {
        impl RaknetEncodable for $ty {
            fn encode_raknet(&self, dst: &mut impl BufMut) -> Result<(), EncodeError> {
                dst.$put(*self as _);
                Ok(())
            }

            fn decode_raknet(src: &mut impl Buf) -> Result<Self, DecodeError> {
                let size = mem::size_of::<$ty>();
                if src.remaining() < size {
                    return Err(DecodeError::UnexpectedEof);
                }
                Ok(src.$get() as $ty)
            }
        }
    };
}

// Unsigned big-endian ints:
impl_raknet_int!(u16, put_u16, get_u16);
impl_raknet_int!(u32, put_u32, get_u32);
impl_raknet_int!(u64, put_u64, get_u64);

// Signed big-endian ints (cast through the unsigned read/write):
impl_raknet_int!(i16, put_i16, get_i16);
impl_raknet_int!(i32, put_i32, get_i32);
impl_raknet_int!(i64, put_i64, get_i64);

/// Little-endian 16-bit unsigned integer wrapper used by some fields.
pub struct U16LE(pub u16);

impl RaknetEncodable for U16LE {
    fn encode_raknet(&self, dst: &mut impl BufMut) -> Result<(), EncodeError> {
        dst.put_u16_le(self.0);
        Ok(())
    }

    fn decode_raknet(src: &mut impl Buf) -> Result<Self, DecodeError> {
        if src.remaining() < 2 {
            return Err(DecodeError::UnexpectedEof);
        }
        Ok(U16LE(src.get_u16_le()))
    }
}

/// Little-endian 24-bit unsigned integer wrapper (3-byte encoding).
pub struct U24LE(pub u32);

impl RaknetEncodable for U24LE {
    fn encode_raknet(&self, dst: &mut impl BufMut) -> Result<(), EncodeError> {
        let v = self.0;
        // 3-byte little-endian
        dst.put_u8((v & 0xFF) as u8);
        dst.put_u8(((v >> 8) & 0xFF) as u8);
        dst.put_u8(((v >> 16) & 0xFF) as u8);
        Ok(())
    }

    fn decode_raknet(src: &mut impl Buf) -> Result<Self, DecodeError> {
        if src.remaining() < 3 {
            return Err(DecodeError::UnexpectedEof);
        }
        let b0 = src.get_u8() as u32;
        let b1 = src.get_u8() as u32;
        let b2 = src.get_u8() as u32;
        Ok(U24LE(b0 | (b1 << 8) | (b2 << 16)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    #[test]
    fn u16le_roundtrip() {
        let value = U16LE(0xABCD);
        let mut buf = BytesMut::new();
        value.encode_raknet(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = U16LE::decode_raknet(&mut slice).unwrap();
        assert_eq!(decoded.0, value.0);
    }

    #[test]
    fn u24le_roundtrip() {
        let value = U24LE(0x00FFEE);
        let mut buf = BytesMut::new();
        value.encode_raknet(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = U24LE::decode_raknet(&mut slice).unwrap();
        assert_eq!(decoded.0, value.0);
    }
}

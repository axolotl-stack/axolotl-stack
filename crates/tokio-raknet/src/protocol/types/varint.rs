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

    // --- Additional comprehensive tests ---

    #[test]
    fn varuint_boundary_encoding_sizes() {
        // Test encoding produces correct byte counts at boundaries
        let test_cases: &[(u64, usize)] = &[
            (0, 1),         // 1 byte: 0-127
            (127, 1),       // max 1 byte
            (128, 2),       // min 2 bytes
            (16383, 2),     // max 2 bytes (2^14 - 1)
            (16384, 3),     // min 3 bytes
            (2097151, 3),   // max 3 bytes (2^21 - 1)
            (2097152, 4),   // min 4 bytes
            (268435455, 4), // max 4 bytes (2^28 - 1)
            (268435456, 5), // min 5 bytes
        ];

        for &(value, expected_bytes) in test_cases {
            let mut buf = BytesMut::new();
            VarUInt(value).encode_raknet(&mut buf).unwrap();
            assert_eq!(
                buf.len(),
                expected_bytes,
                "VarUInt({}) should be {} bytes, got {}",
                value,
                expected_bytes,
                buf.len()
            );

            // Verify roundtrip
            let mut slice = buf.freeze();
            let decoded = VarUInt::decode_raknet(&mut slice).unwrap();
            assert_eq!(decoded.0, value);
        }
    }

    #[test]
    fn varint_signed_boundary_values() {
        // Test zig-zag encoding at boundaries
        let test_cases: &[i64] = &[
            0,
            1,
            -1,
            63,
            -64,
            64,
            -65,
            8191,
            -8192,
            i32::MAX as i64,
            i32::MIN as i64,
            i64::MAX,
            i64::MIN,
        ];

        for &value in test_cases {
            let mut buf = BytesMut::new();
            VarInt(value).encode_raknet(&mut buf).unwrap();

            let mut slice = buf.freeze();
            let decoded = VarInt::decode_raknet(&mut slice).unwrap();
            assert_eq!(decoded.0, value, "VarInt roundtrip failed for {}", value);
        }
    }

    #[test]
    fn varuint_zero_is_single_byte() {
        let mut buf = BytesMut::new();
        VarUInt(0).encode_raknet(&mut buf).unwrap();
        assert_eq!(buf.len(), 1);
        assert_eq!(buf[0], 0);
    }

    #[test]
    fn varint_zero_is_single_byte() {
        let mut buf = BytesMut::new();
        VarInt(0).encode_raknet(&mut buf).unwrap();
        assert_eq!(buf.len(), 1);
        assert_eq!(buf[0], 0);
    }

    #[test]
    fn varint_negative_one_is_single_byte() {
        // -1 in zig-zag is 1, which fits in a single byte
        let mut buf = BytesMut::new();
        VarInt(-1).encode_raknet(&mut buf).unwrap();
        assert_eq!(buf.len(), 1);
        assert_eq!(buf[0], 1); // -1 -> zig-zag -> 1
    }

    #[test]
    fn varuint_large_values() {
        let large_values: &[u64] = &[u64::MAX, u64::MAX - 1, u64::MAX / 2, 1 << 62, 1 << 56];

        for &value in large_values {
            let mut buf = BytesMut::new();
            VarUInt(value).encode_raknet(&mut buf).unwrap();

            let mut slice = buf.freeze();
            let decoded = VarUInt::decode_raknet(&mut slice).unwrap();
            assert_eq!(decoded.0, value);
        }
    }

    #[test]
    fn varuint_decode_rejects_too_long() {
        // Create a malformed varint that would exceed 64 bits
        // 10 bytes of continuation bits (would need 70 bits)
        let malformed: &[u8] = &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80];
        let mut slice = bytes::Bytes::copy_from_slice(malformed);
        let result = VarUInt::decode_raknet(&mut slice);
        assert!(matches!(result, Err(DecodeError::VarIntExceedsLimit)));
    }

    #[test]
    fn varuint_decode_handles_eof() {
        // Incomplete varint (continuation bit set but no more bytes)
        let incomplete: &[u8] = &[0x80]; // Has continuation bit
        let mut slice = bytes::Bytes::copy_from_slice(incomplete);
        let result = VarUInt::decode_raknet(&mut slice);
        assert!(matches!(result, Err(DecodeError::UnexpectedEof)));
    }

    #[test]
    fn varuint_decode_empty_buffer() {
        let mut slice = bytes::Bytes::new();
        let result = VarUInt::decode_raknet(&mut slice);
        assert!(matches!(result, Err(DecodeError::UnexpectedEof)));
    }

    #[test]
    fn zigzag_encoding_pattern() {
        // Verify zig-zag encoding: 0 -> 0, -1 -> 1, 1 -> 2, -2 -> 3, etc.
        let cases: &[(i64, u64)] = &[(0, 0), (-1, 1), (1, 2), (-2, 3), (2, 4), (-3, 5), (3, 6)];

        for &(signed, expected_unsigned) in cases {
            // Encode the signed value
            let ux = ((signed << 1) ^ (signed >> 63)) as u64;
            assert_eq!(
                ux, expected_unsigned,
                "Zig-zag of {} should be {}",
                signed, expected_unsigned
            );

            // Decode back
            let x = ((ux >> 1) as i64) ^ (-((ux & 1) as i64));
            assert_eq!(x, signed);
        }
    }
}

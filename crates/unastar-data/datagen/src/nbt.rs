//! Network NBT decoding with correct signed ZigZag integers.
//!
//! zuri_nbt 0.4.0 decodes odd ZigZag values as `-x` rather than `!x`,
//! shifting every negative integer by one. Delegate other primitives to its
//! network encoding, but decode signed varints from their original bytes.

use std::io::Read;
use zuri_nbt::encoding::NetworkLittleEndian as ZuriNetworkLittleEndian;
use zuri_nbt::err::{NBTError, ReadError};
use zuri_nbt::reader::{Reader, Res};

pub struct NetworkLittleEndian;

impl NetworkLittleEndian {
    fn unsigned<R: Read>(&mut self, reader: &mut R, bits: u32) -> Res<u64> {
        let mut value = 0u64;
        for shift in (0..bits).step_by(7) {
            let byte = self.u8(reader)?;
            let payload = u64::from(byte & 0x7f);
            if payload > (u64::MAX >> (64 - (bits - shift).min(7))) {
                return Err(NBTError::new(ReadError::Custom(
                    "varint overflows integer".to_string(),
                )));
            }
            value |= payload << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(NBTError::new(ReadError::Custom(
            "varint overflows integer".to_string(),
        )))
    }
}

impl Reader for NetworkLittleEndian {
    fn i16<R: Read>(&mut self, reader: &mut R) -> Res<i16> {
        ZuriNetworkLittleEndian.i16(reader)
    }

    fn i32<R: Read>(&mut self, reader: &mut R) -> Res<i32> {
        let value = self.unsigned(reader, 32)? as u32;
        Ok(((value >> 1) as i32) ^ -((value & 1) as i32))
    }

    fn i64<R: Read>(&mut self, reader: &mut R) -> Res<i64> {
        let value = self.unsigned(reader, 64)?;
        Ok(((value >> 1) as i64) ^ -((value & 1) as i64))
    }

    fn f32<R: Read>(&mut self, reader: &mut R) -> Res<f32> {
        ZuriNetworkLittleEndian.f32(reader)
    }

    fn f64<R: Read>(&mut self, reader: &mut R) -> Res<f64> {
        ZuriNetworkLittleEndian.f64(reader)
    }

    fn string<R: Read>(&mut self, reader: &mut R) -> Res<String> {
        ZuriNetworkLittleEndian.string(reader)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_zigzag_preserves_negative_values_and_bounds() {
        for (bytes, expected) in [
            (&[0x00][..], 0),
            (&[0x01][..], -1),
            (&[0x02][..], 1),
            (&[0x97, 0x02][..], -140),
            (&[0xff, 0xff, 0xff, 0xff, 0x0f][..], i32::MIN),
            (&[0xfe, 0xff, 0xff, 0xff, 0x0f][..], i32::MAX),
        ] {
            assert_eq!(NetworkLittleEndian.i32(&mut &bytes[..]).unwrap(), expected);
        }
        for (bytes, expected) in [
            (&[0x01][..], -1),
            (&[0x97, 0x02][..], -140),
            (
                &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01][..],
                i64::MIN,
            ),
            (
                &[0xfe, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01][..],
                i64::MAX,
            ),
        ] {
            assert_eq!(NetworkLittleEndian.i64(&mut &bytes[..]).unwrap(), expected);
        }
    }

    #[test]
    fn rejects_overflowing_and_truncated_signed_varints() {
        for bytes in [
            &[0xff, 0xff, 0xff, 0xff, 0x10][..],
            &[0xff, 0xff, 0xff, 0xff, 0x80][..],
            &[0x80][..],
        ] {
            assert!(NetworkLittleEndian.i32(&mut &bytes[..]).is_err());
        }
        for bytes in [
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02][..],
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x80][..],
            &[0x80][..],
        ] {
            assert!(NetworkLittleEndian.i64(&mut &bytes[..]).is_err());
        }
    }
}

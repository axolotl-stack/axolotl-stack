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

/// Extracts each item's `data` compound from protocolgen's `items.nbt` as
/// verbatim network NBT, re-rooted as an unnamed compound.
///
/// The bytes are sliced, never re-encoded, so key order and every value stay
/// exactly as captured from BDS; `zuri_nbt` maps would neither keep the order
/// nor encode negative varints correctly (see the module docs).
pub fn registry_component_data(
    bytes: &[u8],
) -> Result<std::collections::BTreeMap<String, Vec<u8>>, String> {
    let mut cursor = SpanCursor { bytes, pos: 0 };
    if cursor.u8()? != TAG_COMPOUND {
        return Err("items.nbt root must be a compound".into());
    }
    cursor.string()?;
    let mut items = std::collections::BTreeMap::new();
    loop {
        let tag = cursor.u8()?;
        if tag == TAG_END {
            break;
        }
        let name = cursor.string()?.to_owned();
        if tag != TAG_COMPOUND {
            return Err(format!("items.nbt entry {name} is not a compound"));
        }
        let mut data = None;
        loop {
            let field = cursor.u8()?;
            if field == TAG_END {
                break;
            }
            let key = cursor.string()?;
            let start = cursor.pos;
            cursor.skip_payload(field)?;
            if key == "data" {
                if field != TAG_COMPOUND {
                    return Err(format!("items.nbt {name} data is not a compound"));
                }
                let mut rooted = vec![TAG_COMPOUND, 0];
                rooted.extend_from_slice(&bytes[start..cursor.pos]);
                data = Some(rooted);
            }
        }
        let data = data.ok_or_else(|| format!("items.nbt {name} has no data compound"))?;
        if items.insert(name.clone(), data).is_some() {
            return Err(format!("items.nbt repeats entry {name}"));
        }
    }
    if cursor.pos != bytes.len() {
        return Err("items.nbt contains trailing bytes".into());
    }
    Ok(items)
}

const TAG_END: u8 = 0;
const TAG_COMPOUND: u8 = 10;

/// Walks network little-endian NBT without decoding values.
struct SpanCursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> SpanCursor<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self
            .pos
            .checked_add(len)
            .filter(|&end| end <= self.bytes.len())
            .ok_or_else(|| format!("NBT truncated at byte {}", self.pos))?;
        let slice = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn varint(&mut self, max_bytes: usize) -> Result<u64, String> {
        let mut value = 0u64;
        for index in 0..max_bytes {
            let byte = self.u8()?;
            value |= u64::from(byte & 0x7f) << (7 * index);
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(format!("NBT varint too long at byte {}", self.pos))
    }

    /// List and array lengths are zig-zag encoded signed `int`s.
    fn length(&mut self) -> Result<usize, String> {
        let raw = self.varint(5)?;
        let value = ((raw >> 1) as i64) ^ -((raw & 1) as i64);
        usize::try_from(value).map_err(|_| format!("negative NBT length {value}"))
    }

    fn string(&mut self) -> Result<&'a str, String> {
        let len = self.varint(5)? as usize;
        std::str::from_utf8(self.take(len)?).map_err(|e| format!("NBT string: {e}"))
    }

    fn skip_payload(&mut self, tag: u8) -> Result<(), String> {
        match tag {
            1 => self.take(1).map(drop),
            2 => self.take(2).map(drop),
            3 => self.varint(5).map(drop),
            4 => self.varint(10).map(drop),
            5 => self.take(4).map(drop),
            6 => self.take(8).map(drop),
            7 => {
                let len = self.length()?;
                self.take(len).map(drop)
            }
            8 => self.string().map(drop),
            9 => {
                let element = self.u8()?;
                for _ in 0..self.length()? {
                    self.skip_payload(element)?;
                }
                Ok(())
            }
            10 => loop {
                let field = self.u8()?;
                if field == TAG_END {
                    return Ok(());
                }
                self.string()?;
                self.skip_payload(field)?;
            },
            11 => {
                for _ in 0..self.length()? {
                    self.varint(5)?;
                }
                Ok(())
            }
            12 => {
                for _ in 0..self.length()? {
                    self.varint(10)?;
                }
                Ok(())
            }
            other => Err(format!("unknown NBT tag {other} at byte {}", self.pos)),
        }
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

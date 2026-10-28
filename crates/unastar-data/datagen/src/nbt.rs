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

/// Extracts each block's definition from protocolgen's `data_driven_blocks.nbt`
/// (a little-endian root compound holding a `blocks` list of `{name, components}`),
/// transcoded to the network NBT that `StartGame` block definitions carry.
///
/// These are the vanilla data-driven blocks (behavior pack `blocks/`). The
/// client numbers sequential block ids over its built-in blocks plus the blocks
/// a server defines, so the definitions are required for canonical-palette ids.
/// Transcoding keeps key order and every value; only integer, string and length
/// encodings change between the two NBT flavours.
pub fn data_driven_block_definitions(
    bytes: &[u8],
) -> Result<std::collections::BTreeMap<String, Vec<u8>>, String> {
    let mut cursor = LeCursor { bytes, pos: 0 };
    if cursor.u8()? != TAG_COMPOUND {
        return Err("data_driven_blocks.nbt root must be a compound".into());
    }
    cursor.string()?;
    let mut blocks = std::collections::BTreeMap::new();
    loop {
        let tag = cursor.u8()?;
        if tag == TAG_END {
            break;
        }
        let key = cursor.string()?;
        if key != "blocks" || tag != TAG_LIST || cursor.u8()? != TAG_COMPOUND {
            return Err(format!(
                "data_driven_blocks.nbt has unexpected root field {key}"
            ));
        }
        for _ in 0..cursor.length()? {
            let (mut name, mut definition) = (None, None);
            loop {
                let field = cursor.u8()?;
                if field == TAG_END {
                    break;
                }
                match (cursor.string()?, field) {
                    ("name", 8) => name = Some(cursor.string()?.to_owned()),
                    ("components", TAG_COMPOUND) => {
                        let mut network = vec![TAG_COMPOUND, 0];
                        cursor.transcode_payload(TAG_COMPOUND, &mut network)?;
                        definition = Some(network);
                    }
                    (other, _) => {
                        return Err(format!("data-driven block has unexpected field {other}"));
                    }
                }
            }
            let name = name.ok_or("data-driven block without a name")?;
            let definition =
                definition.ok_or_else(|| format!("data-driven block {name} has no components"))?;
            if blocks.insert(name.clone(), definition).is_some() {
                return Err(format!("data_driven_blocks.nbt repeats {name}"));
            }
        }
    }
    if cursor.pos != bytes.len() {
        return Err("data_driven_blocks.nbt contains trailing bytes".into());
    }
    Ok(blocks)
}

const TAG_LIST: u8 = 9;

/// Reads little-endian (disk) NBT and re-emits it as network little-endian NBT.
struct LeCursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> LeCursor<'a> {
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

    fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn i64(&mut self) -> Result<i64, String> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn length(&mut self) -> Result<usize, String> {
        let value = self.i32()?;
        usize::try_from(value).map_err(|_| format!("negative NBT length {value}"))
    }

    fn string(&mut self) -> Result<&'a str, String> {
        let len = u16::from_le_bytes(self.take(2)?.try_into().unwrap());
        std::str::from_utf8(self.take(len.into())?).map_err(|e| format!("NBT string: {e}"))
    }

    fn transcode_payload(&mut self, tag: u8, out: &mut Vec<u8>) -> Result<(), String> {
        match tag {
            1 | 2 | 5 | 6 => {
                let width = [0, 1, 2, 0, 0, 4, 8][tag as usize];
                out.extend_from_slice(self.take(width)?);
            }
            3 => put_varint(out, zigzag(self.i32()?.into())),
            4 => put_varint(out, zigzag(self.i64()?)),
            7 => {
                let len = self.length()?;
                put_length(out, len);
                out.extend_from_slice(self.take(len)?);
            }
            8 => put_string(out, self.string()?),
            TAG_LIST => {
                let element = self.u8()?;
                out.push(element);
                let len = self.length()?;
                put_length(out, len);
                for _ in 0..len {
                    self.transcode_payload(element, out)?;
                }
            }
            TAG_COMPOUND => loop {
                let field = self.u8()?;
                out.push(field);
                if field == TAG_END {
                    return Ok(());
                }
                put_string(out, self.string()?);
                self.transcode_payload(field, out)?;
            },
            11 => {
                let len = self.length()?;
                put_length(out, len);
                for _ in 0..len {
                    put_varint(out, zigzag(self.i32()?.into()));
                }
            }
            12 => {
                let len = self.length()?;
                put_length(out, len);
                for _ in 0..len {
                    put_varint(out, zigzag(self.i64()?));
                }
            }
            other => return Err(format!("unknown NBT tag {other} at byte {}", self.pos)),
        }
        Ok(())
    }
}

fn zigzag(value: i64) -> u64 {
    ((value << 1) ^ (value >> 63)) as u64
}

fn put_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push(value as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

/// Network list/array lengths are zig-zag encoded signed `int`s.
fn put_length(out: &mut Vec<u8>, len: usize) {
    put_varint(out, zigzag(len as i64));
}

fn put_string(out: &mut Vec<u8>, value: &str) {
    put_varint(out, value.len() as u64);
    out.extend_from_slice(value.as_bytes());
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

    #[test]
    fn data_driven_definitions_transcode_every_value_to_network_nbt() {
        use zuri_nbt::encoding::LittleEndian;
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../valentine_gen/protocolgen/generated/data/registry/data_driven_blocks.nbt");
        let bytes = std::fs::read(&path).unwrap();
        let definitions = data_driven_block_definitions(&bytes).unwrap();
        let zuri_nbt::NBTTag::Compound(root) =
            zuri_nbt::NBTTag::read(&mut bytes.as_slice(), LittleEndian).unwrap()
        else {
            panic!("root compound");
        };
        let Some(zuri_nbt::NBTTag::List(blocks)) = root.0.get("blocks") else {
            panic!("blocks list");
        };
        assert_eq!(blocks.0.len(), definitions.len());
        for block in &blocks.0 {
            let zuri_nbt::NBTTag::Compound(block) = block else {
                panic!("block compound");
            };
            let Some(zuri_nbt::NBTTag::String(name)) = block.0.get("name") else {
                panic!("block name");
            };
            let network = &definitions[&name.0];
            let mut input = network.as_slice();
            let decoded = zuri_nbt::NBTTag::read(&mut input, NetworkLittleEndian).unwrap();
            assert!(input.is_empty(), "{} has trailing bytes", name.0);
            assert_eq!(Some(&decoded), block.0.get("components"), "{}", name.0);
        }
    }
}

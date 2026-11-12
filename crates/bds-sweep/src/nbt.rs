//! Minimal network-NBT (little-endian, varint) reader and writer for the
//! block-actor data a client edits and sends back (sign text). Keeps every
//! tag and the order of compound entries, so an edited compound differs from
//! the server's only in the edited values.

use anyhow::{Context, Result, bail, ensure};

#[derive(Debug, Clone, PartialEq)]
pub enum Tag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<i8>),
    String(String),
    /// Element tag id and elements.
    List(u8, Vec<Tag>),
    Compound(Vec<(String, Tag)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Tag {
    fn id(&self) -> u8 {
        match self {
            Self::Byte(_) => 1,
            Self::Short(_) => 2,
            Self::Int(_) => 3,
            Self::Long(_) => 4,
            Self::Float(_) => 5,
            Self::Double(_) => 6,
            Self::ByteArray(_) => 7,
            Self::String(_) => 8,
            Self::List(..) => 9,
            Self::Compound(_) => 10,
            Self::IntArray(_) => 11,
            Self::LongArray(_) => 12,
        }
    }

    /// The entry `name` of a compound.
    pub fn get_mut(&mut self, name: &str) -> Option<&mut Tag> {
        match self {
            Self::Compound(entries) => entries
                .iter_mut()
                .find(|(key, _)| key == name)
                .map(|(_, tag)| tag),
            _ => None,
        }
    }
}

struct Input<'a>(&'a [u8]);

impl Input<'_> {
    fn byte(&mut self) -> Result<u8> {
        let (&first, rest) = self.0.split_first().context("NBT ends early")?;
        self.0 = rest;
        Ok(first)
    }

    fn bytes(&mut self, n: usize) -> Result<&[u8]> {
        ensure!(self.0.len() >= n, "NBT ends early");
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }

    fn varint(&mut self, bits: u32) -> Result<u64> {
        let mut value = 0u64;
        let mut shift = 0;
        loop {
            let byte = self.byte()?;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
            shift += 7;
            ensure!(shift < bits + 7, "NBT varint overflow");
        }
    }

    fn zigzag32(&mut self) -> Result<i32> {
        let value = self.varint(32)? as u32;
        Ok(((value >> 1) as i32) ^ -((value & 1) as i32))
    }

    fn zigzag64(&mut self) -> Result<i64> {
        let value = self.varint(64)?;
        Ok(((value >> 1) as i64) ^ -((value & 1) as i64))
    }

    fn string(&mut self) -> Result<String> {
        let len = self.varint(32)? as usize;
        Ok(String::from_utf8(self.bytes(len)?.to_vec())?)
    }

    fn payload(&mut self, id: u8) -> Result<Tag> {
        Ok(match id {
            1 => Tag::Byte(self.byte()? as i8),
            2 => Tag::Short(i16::from_le_bytes(self.bytes(2)?.try_into()?)),
            3 => Tag::Int(self.zigzag32()?),
            4 => Tag::Long(self.zigzag64()?),
            5 => Tag::Float(f32::from_le_bytes(self.bytes(4)?.try_into()?)),
            6 => Tag::Double(f64::from_le_bytes(self.bytes(8)?.try_into()?)),
            7 => {
                let len = self.zigzag32()? as usize;
                Tag::ByteArray(self.bytes(len)?.iter().map(|&b| b as i8).collect())
            }
            8 => Tag::String(self.string()?),
            9 => {
                let element = self.byte()?;
                let len = self.zigzag32()?.max(0) as usize;
                let items = (0..len)
                    .map(|_| self.payload(element))
                    .collect::<Result<_>>()?;
                Tag::List(element, items)
            }
            10 => {
                let mut entries = Vec::new();
                loop {
                    let id = self.byte()?;
                    if id == 0 {
                        break;
                    }
                    let name = self.string()?;
                    entries.push((name, self.payload(id)?));
                }
                Tag::Compound(entries)
            }
            11 => {
                let len = self.zigzag32()? as usize;
                Tag::IntArray((0..len).map(|_| self.zigzag32()).collect::<Result<_>>()?)
            }
            12 => {
                let len = self.zigzag32()? as usize;
                Tag::LongArray((0..len).map(|_| self.zigzag64()).collect::<Result<_>>()?)
            }
            other => bail!("unknown NBT tag {other}"),
        })
    }
}

/// Read a rooted network-NBT compound; returns its root name and tag.
pub fn read(bytes: &[u8]) -> Result<(String, Tag)> {
    let mut input = Input(bytes);
    let id = input.byte()?;
    ensure!(id == 10, "root tag {id} is not a compound");
    let name = input.string()?;
    let tag = input.payload(10)?;
    ensure!(input.0.is_empty(), "trailing NBT bytes");
    Ok((name, tag))
}

fn varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn zigzag32(out: &mut Vec<u8>, value: i32) {
    varint(out, u64::from(((value << 1) ^ (value >> 31)) as u32));
}

fn zigzag64(out: &mut Vec<u8>, value: i64) {
    varint(out, ((value << 1) ^ (value >> 63)) as u64);
}

fn string(out: &mut Vec<u8>, value: &str) {
    varint(out, value.len() as u64);
    out.extend_from_slice(value.as_bytes());
}

fn payload(out: &mut Vec<u8>, tag: &Tag) {
    match tag {
        Tag::Byte(v) => out.push(*v as u8),
        Tag::Short(v) => out.extend_from_slice(&v.to_le_bytes()),
        Tag::Int(v) => zigzag32(out, *v),
        Tag::Long(v) => zigzag64(out, *v),
        Tag::Float(v) => out.extend_from_slice(&v.to_le_bytes()),
        Tag::Double(v) => out.extend_from_slice(&v.to_le_bytes()),
        Tag::ByteArray(v) => {
            zigzag32(out, v.len() as i32);
            out.extend(v.iter().map(|&b| b as u8));
        }
        Tag::String(v) => string(out, v),
        Tag::List(element, items) => {
            out.push(*element);
            zigzag32(out, items.len() as i32);
            for item in items {
                payload(out, item);
            }
        }
        Tag::Compound(entries) => {
            for (name, tag) in entries {
                out.push(tag.id());
                string(out, name);
                payload(out, tag);
            }
            out.push(0);
        }
        Tag::IntArray(v) => {
            zigzag32(out, v.len() as i32);
            for item in v {
                zigzag32(out, *item);
            }
        }
        Tag::LongArray(v) => {
            zigzag32(out, v.len() as i32);
            for item in v {
                zigzag64(out, *item);
            }
        }
    }
}

/// Write a rooted network-NBT compound.
pub fn write(name: &str, tag: &Tag) -> Vec<u8> {
    let mut out = vec![tag.id()];
    string(&mut out, name);
    payload(&mut out, tag);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sign actor BDS 1.26.51 sends right after a sign is placed
    /// (`crates/unastar/src/server/game/testdata/bds-placements.json`).
    const SIGN: &[u8] = b"\n\0\n\x08BackText\x08\x0cFilteredText\0\x01\x0fHideGlowOutline\0\x01\x0eIgnoreLighting\0\x01\x11PersistFormatting\x01\x03\rSignTextColor\xff\xff\xff\x0f\x08\x04Text\0\x08\tTextOwner\0\0\x03\x12BlockEntityVersion\0\n\tFrontText\x08\x0cFilteredText\0\x01\x0fHideGlowOutline\0\x01\x0eIgnoreLighting\0\x01\x11PersistFormatting\x01\x03\rSignTextColor\xff\xff\xff\x0f\x08\x04Text\0\x08\tTextOwner\0\0\x01\x07IsWaxed\0\x04\x12LockedForEditingBy\xfd\xff\xff\xff\x1f\x08\x02id\x04Sign\x03\x01x\x08\x03\x01yu\x03\x01z\x08\0";

    #[test]
    fn sign_actor_round_trips_with_negative_varints() {
        let (name, mut tag) = read(SIGN).unwrap();
        assert_eq!(write(&name, &tag), SIGN);
        let front = tag.get_mut("FrontText").unwrap();
        assert_eq!(
            front.get_mut("SignTextColor"),
            Some(&mut Tag::Int(-16_777_216))
        );
        assert_eq!(
            tag.get_mut("LockedForEditingBy"),
            Some(&mut Tag::Long(-4_294_967_295))
        );
        assert_eq!(tag.get_mut("y"), Some(&mut Tag::Int(-59)));
    }
}

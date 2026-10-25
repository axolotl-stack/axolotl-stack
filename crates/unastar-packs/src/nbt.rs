//! Minimal NBT reader for the three encodings the generator inputs use.
//!
//! - [`Encoding::NetworkLittleEndian`]: protocolgen's `canonical_block_states.nbt`
//!   (zig-zag varint ints/longs, varint string and list lengths).
//! - [`Encoding::LittleEndian`]: Bedrock `.mcstructure` templates.
//! - [`Encoding::BigEndian`]: Java-format `.nbt` structure templates that BDS
//!   ships loose in `behavior_packs/*/structures` (gzip-compressed).
//!
//! The plain enum is shared by build-time template compilers. Writers emit
//! network compound children for block actors and raw Java roots for entities.

use crate::error::{Error, Result};
use std::collections::BTreeMap;

/// Byte layout of an NBT stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    NetworkLittleEndian,
    LittleEndian,
    BigEndian,
}

/// A decoded NBT value.
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
    List(Vec<Tag>),
    Compound(BTreeMap<String, Tag>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Tag {
    /// Look up a compound child.
    pub fn get(&self, key: &str) -> Option<&Tag> {
        match self {
            Tag::Compound(map) => map.get(key),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Tag::String(s) => Some(s),
            _ => None,
        }
    }

    /// Integer value of any integral tag.
    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Tag::Byte(v) => Some(v.into()),
            Tag::Short(v) => Some(v.into()),
            Tag::Int(v) => Some(v.into()),
            Tag::Long(v) => Some(v),
            _ => None,
        }
    }

    /// Byte, Short or Int value (the types used by template geometry).
    pub fn as_int(&self) -> Option<i32> {
        match *self {
            Tag::Byte(value) => Some(value as i32),
            Tag::Short(value) => Some(value as i32),
            Tag::Int(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Tag]> {
        match self {
            Tag::List(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_compound(&self) -> Option<&BTreeMap<String, Tag>> {
        match self {
            Tag::Compound(v) => Some(v),
            _ => None,
        }
    }
}

/// Cursor over an NBT byte stream.
pub struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
    encoding: Encoding,
    origin: &'a str,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8], encoding: Encoding, origin: &'a str) -> Self {
        Self {
            bytes,
            pos: 0,
            encoding,
            origin,
        }
    }

    /// `true` once every byte was consumed.
    pub fn is_empty(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    /// Read one named root tag and return `(name, value)`.
    pub fn read_root(&mut self) -> Result<(String, Tag)> {
        let id = self.u8()?;
        let name = self.string()?;
        let value = self.payload(id, 0)?;
        Ok((name, value))
    }

    fn err(&self, reason: impl Into<String>) -> Error {
        Error::Nbt {
            origin: self.origin.to_owned(),
            reason: format!("at byte {}: {}", self.pos, reason.into()),
        }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.bytes.len());
        let Some(end) = end else {
            return Err(self.err(format!("truncated, wanted {n} bytes")));
        };
        let slice = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn fixed<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut out: [u8; N] = self.take(N)?.try_into().expect("length checked");
        if self.encoding == Encoding::BigEndian {
            out.reverse();
        }
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn varint(&mut self, bits: u32) -> Result<u64> {
        let mut value = 0u64;
        let mut shift = 0;
        loop {
            let byte = self.u8()?;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
            shift += 7;
            if shift >= bits + 7 {
                return Err(self.err("varint too long"));
            }
        }
    }

    fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_le_bytes(self.fixed()?))
    }

    fn i32(&mut self) -> Result<i32> {
        if self.encoding == Encoding::NetworkLittleEndian {
            let v = self.varint(32)? as u32;
            return Ok(((v >> 1) as i32) ^ -((v & 1) as i32));
        }
        Ok(i32::from_le_bytes(self.fixed()?))
    }

    fn i64(&mut self) -> Result<i64> {
        if self.encoding == Encoding::NetworkLittleEndian {
            let v = self.varint(64)?;
            return Ok(((v >> 1) as i64) ^ -((v & 1) as i64));
        }
        Ok(i64::from_le_bytes(self.fixed()?))
    }

    fn len(&mut self) -> Result<usize> {
        let n = self.i32()?;
        usize::try_from(n).map_err(|_| self.err(format!("negative length {n}")))
    }

    fn string(&mut self) -> Result<String> {
        let n = match self.encoding {
            Encoding::NetworkLittleEndian => self.varint(32)? as usize,
            _ => u16::from_le_bytes(self.fixed()?) as usize,
        };
        let bytes = self.take(n)?;
        // Java NBT uses modified UTF-8; vanilla names are ASCII, so plain UTF-8
        // decoding is exact for every input the generator reads.
        String::from_utf8(bytes.to_vec()).map_err(|e| self.err(format!("string not UTF-8: {e}")))
    }

    fn payload(&mut self, id: u8, depth: u32) -> Result<Tag> {
        if depth > 512 {
            return Err(self.err("nesting too deep"));
        }
        Ok(match id {
            1 => Tag::Byte(self.u8()? as i8),
            2 => Tag::Short(self.i16()?),
            3 => Tag::Int(self.i32()?),
            4 => Tag::Long(self.i64()?),
            5 => Tag::Float(f32::from_le_bytes(self.fixed()?)),
            6 => Tag::Double(f64::from_le_bytes(self.fixed()?)),
            7 => {
                let n = self.len()?;
                Tag::ByteArray(self.take(n)?.iter().map(|&b| b as i8).collect())
            }
            8 => Tag::String(self.string()?),
            9 => {
                let element = self.u8()?;
                let n = self.len()?;
                let mut items = Vec::with_capacity(n.min(1 << 16));
                for _ in 0..n {
                    items.push(self.payload(element, depth + 1)?);
                }
                Tag::List(items)
            }
            10 => {
                let mut map = BTreeMap::new();
                loop {
                    let child = self.u8()?;
                    if child == 0 {
                        break;
                    }
                    let name = self.string()?;
                    let value = self.payload(child, depth + 1)?;
                    map.insert(name, value);
                }
                Tag::Compound(map)
            }
            11 => {
                let n = self.len()?;
                let mut items = Vec::with_capacity(n.min(1 << 20));
                for _ in 0..n {
                    items.push(self.i32()?);
                }
                Tag::IntArray(items)
            }
            12 => {
                let n = self.len()?;
                let mut items = Vec::with_capacity(n.min(1 << 20));
                for _ in 0..n {
                    items.push(self.i64()?);
                }
                Tag::LongArray(items)
            }
            other => return Err(self.err(format!("unknown tag id {other}"))),
        })
    }
}

/// Read one entire rooted compound, rejecting trailing or unsupported content.
pub fn read_compound(origin: &str, bytes: &[u8], encoding: Encoding) -> Result<Tag> {
    let mut reader = Reader::new(bytes, encoding, origin);
    let (_, root) = reader.read_root()?;
    if !matches!(root, Tag::Compound(_)) {
        return Err(Error::data(origin, "NBT root is not a compound"));
    }
    if !reader.is_empty() {
        return Err(Error::data(
            origin,
            "trailing bytes after NBT root compound",
        ));
    }
    Ok(root)
}

/// Decode a gzip-compressed big-endian (Java format) NBT file.
pub fn read_gzip_java(origin: &str, bytes: &[u8]) -> Result<Tag> {
    use std::io::Read;
    let mut raw = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_end(&mut raw)
        .map_err(|e| Error::Nbt {
            origin: origin.to_owned(),
            reason: format!("gzip: {e}"),
        })?;
    read_compound(origin, &raw, Encoding::BigEndian)
}

/// Re-encode a compound as uncompressed big-endian NBT (unnamed root).
pub fn encode_java(tag: &Tag) -> Vec<u8> {
    fn id(t: &Tag) -> u8 {
        match t {
            Tag::Byte(_) => 1,
            Tag::Short(_) => 2,
            Tag::Int(_) => 3,
            Tag::Long(_) => 4,
            Tag::Float(_) => 5,
            Tag::Double(_) => 6,
            Tag::ByteArray(_) => 7,
            Tag::String(_) => 8,
            Tag::List(_) => 9,
            Tag::Compound(_) => 10,
            Tag::IntArray(_) => 11,
            Tag::LongArray(_) => 12,
        }
    }
    fn s(out: &mut Vec<u8>, v: &str) {
        out.extend_from_slice(&(v.len() as u16).to_be_bytes());
        out.extend_from_slice(v.as_bytes());
    }
    fn p(out: &mut Vec<u8>, t: &Tag) {
        match t {
            Tag::Byte(v) => out.push(*v as u8),
            Tag::Short(v) => out.extend_from_slice(&v.to_be_bytes()),
            Tag::Int(v) => out.extend_from_slice(&v.to_be_bytes()),
            Tag::Long(v) => out.extend_from_slice(&v.to_be_bytes()),
            Tag::Float(v) => out.extend_from_slice(&v.to_bits().to_be_bytes()),
            Tag::Double(v) => out.extend_from_slice(&v.to_bits().to_be_bytes()),
            Tag::ByteArray(v) => {
                out.extend_from_slice(&(v.len() as i32).to_be_bytes());
                out.extend(v.iter().map(|b| *b as u8));
            }
            Tag::String(v) => s(out, v),
            Tag::List(v) => {
                out.push(v.first().map(id).unwrap_or(0));
                out.extend_from_slice(&(v.len() as i32).to_be_bytes());
                for x in v {
                    p(out, x);
                }
            }
            Tag::Compound(m) => {
                for (k, v) in m {
                    out.push(id(v));
                    s(out, k);
                    p(out, v);
                }
                out.push(0);
            }
            Tag::IntArray(v) => {
                out.extend_from_slice(&(v.len() as i32).to_be_bytes());
                for x in v {
                    out.extend_from_slice(&x.to_be_bytes());
                }
            }
            Tag::LongArray(v) => {
                out.extend_from_slice(&(v.len() as i32).to_be_bytes());
                for x in v {
                    out.extend_from_slice(&x.to_be_bytes());
                }
            }
        }
    }
    let mut out = vec![10, 0, 0];
    p(&mut out, tag);
    out
}

/// Encode named NETWORK-endian child tags, without a root header or end tag.
pub fn encode_network_fields(fields: &BTreeMap<String, Tag>) -> Result<Vec<u8>> {
    fn var(out: &mut Vec<u8>, mut value: u64) {
        while value >= 0x80 {
            out.push(value as u8 | 0x80);
            value >>= 7;
        }
        out.push(value as u8);
    }
    fn int(out: &mut Vec<u8>, value: i32) {
        var(out, ((value as u32) << 1 ^ (value >> 31) as u32) as u64);
    }
    fn long(out: &mut Vec<u8>, value: i64) {
        var(out, (value as u64) << 1 ^ (value >> 63) as u64);
    }
    fn string(out: &mut Vec<u8>, value: &str) -> Result<()> {
        let len = u32::try_from(value.len())
            .map_err(|error| Error::data("network NBT encoder", error.to_string()))?;
        var(out, len as u64);
        out.extend_from_slice(value.as_bytes());
        Ok(())
    }
    fn id(tag: &Tag) -> u8 {
        match tag {
            Tag::Byte(_) => 1,
            Tag::Short(_) => 2,
            Tag::Int(_) => 3,
            Tag::Long(_) => 4,
            Tag::Float(_) => 5,
            Tag::Double(_) => 6,
            Tag::ByteArray(_) => 7,
            Tag::String(_) => 8,
            Tag::List(_) => 9,
            Tag::Compound(_) => 10,
            Tag::IntArray(_) => 11,
            Tag::LongArray(_) => 12,
        }
    }
    fn children(out: &mut Vec<u8>, fields: &BTreeMap<String, Tag>) -> Result<()> {
        for (name, tag) in fields {
            out.push(id(tag));
            string(out, name)?;
            payload(out, tag)?;
        }
        Ok(())
    }
    fn payload(out: &mut Vec<u8>, tag: &Tag) -> Result<()> {
        match tag {
            Tag::Byte(value) => out.push(*value as u8),
            Tag::Short(value) => out.extend_from_slice(&value.to_le_bytes()),
            Tag::Int(value) => int(out, *value),
            Tag::Long(value) => long(out, *value),
            Tag::Float(value) => out.extend_from_slice(&value.to_le_bytes()),
            Tag::Double(value) => out.extend_from_slice(&value.to_le_bytes()),
            Tag::String(value) => string(out, value)?,
            Tag::Compound(fields) => {
                children(out, fields)?;
                out.push(0);
            }
            Tag::List(values) => {
                let kind = values.first().map(id).unwrap_or(0);
                if values.iter().any(|tag| id(tag) != kind) {
                    return Err(Error::data(
                        "network NBT encoder",
                        "NBT list contains mixed tag types",
                    ));
                }
                out.push(kind);
                int(
                    out,
                    i32::try_from(values.len())
                        .map_err(|error| Error::data("network NBT encoder", error.to_string()))?,
                );
                for value in values {
                    payload(out, value)?;
                }
            }
            Tag::ByteArray(values) => {
                int(
                    out,
                    i32::try_from(values.len())
                        .map_err(|error| Error::data("network NBT encoder", error.to_string()))?,
                );
                out.extend(values.iter().map(|value| *value as u8));
            }
            Tag::IntArray(values) => {
                int(
                    out,
                    i32::try_from(values.len())
                        .map_err(|error| Error::data("network NBT encoder", error.to_string()))?,
                );
                for value in values {
                    int(out, *value);
                }
            }
            Tag::LongArray(values) => {
                int(
                    out,
                    i32::try_from(values.len())
                        .map_err(|error| Error::data("network NBT encoder", error.to_string()))?,
                );
                for value in values {
                    long(out, *value);
                }
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    children(&mut out, fields)?;
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_varints_decode_negative_zigzag_exactly() {
        // compound "" { int "a" = -1 (zigzag 1), int "b" = 2 (zigzag 4) }
        let bytes = [10, 0, 3, 1, b'a', 1, 3, 1, b'b', 4, 0];
        let (_, tag) = Reader::new(&bytes, Encoding::NetworkLittleEndian, "t")
            .read_root()
            .unwrap();
        assert_eq!(tag.get("a"), Some(&Tag::Int(-1)));
        assert_eq!(tag.get("b"), Some(&Tag::Int(2)));
    }

    #[test]
    fn big_and_little_endian_fixed_width() {
        let be = [10, 0, 0, 3, 0, 1, b'x', 0, 0, 1, 2, 0];
        let le = [10, 0, 0, 3, 1, 0, b'x', 2, 1, 0, 0, 0];
        let a = Reader::new(&be, Encoding::BigEndian, "t")
            .read_root()
            .unwrap()
            .1;
        let b = Reader::new(&le, Encoding::LittleEndian, "t")
            .read_root()
            .unwrap()
            .1;
        assert_eq!(a.get("x"), Some(&Tag::Int(258)));
        assert_eq!(b.get("x"), Some(&Tag::Int(258)));
    }

    #[test]
    fn truncation_is_reported_with_origin() {
        let err = Reader::new(&[10, 0, 0, 3, 0, 1], Encoding::BigEndian, "f.nbt")
            .read_root()
            .unwrap_err()
            .to_string();
        assert!(err.contains("f.nbt") && err.contains("truncated"), "{err}");
    }
}

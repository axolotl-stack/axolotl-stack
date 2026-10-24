//! Java Edition (big-endian) NBT reader for structure templates (build time).

use std::collections::BTreeMap;

pub type Result<T, E = Box<dyn std::error::Error>> = std::result::Result<T, E>;

/// An NBT value.
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
    pub fn get(&self, key: &str) -> Option<&Tag> {
        match self {
            Tag::Compound(m) => m.get(key),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Tag::String(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_int(&self) -> Option<i32> {
        match *self {
            Tag::Byte(v) => Some(v as i32),
            Tag::Short(v) => Some(v as i32),
            Tag::Int(v) => Some(v),
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
            Tag::Compound(m) => Some(m),
            _ => None,
        }
    }
}

struct Reader<'a> {
    b: &'a [u8],
    o: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let s = self.b.get(self.o..self.o + n).ok_or("nbt: truncated")?;
        self.o += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_be_bytes(self.take(2)?.try_into()?))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into()?))
    }
    fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_be_bytes(self.take(8)?.try_into()?))
    }
    fn string(&mut self) -> Result<String> {
        let n = u16::from_be_bytes(self.take(2)?.try_into()?) as usize;
        // Java modified UTF-8; vanilla template strings are plain ASCII/UTF-8.
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    fn len(&mut self) -> Result<usize> {
        let n = self.i32()?;
        usize::try_from(n).map_err(|_| "nbt: negative length".into())
    }
    fn payload(&mut self, t: u8) -> Result<Tag> {
        Ok(match t {
            1 => Tag::Byte(self.u8()? as i8),
            2 => Tag::Short(self.i16()?),
            3 => Tag::Int(self.i32()?),
            4 => Tag::Long(self.i64()?),
            5 => Tag::Float(f32::from_bits(self.i32()? as u32)),
            6 => Tag::Double(f64::from_bits(self.i64()? as u64)),
            7 => {
                let n = self.len()?;
                Tag::ByteArray(self.take(n)?.iter().map(|b| *b as i8).collect())
            }
            8 => Tag::String(self.string()?),
            9 => {
                let et = self.u8()?;
                let n = self.len()?;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(self.payload(et)?);
                }
                Tag::List(v)
            }
            10 => {
                let mut m = BTreeMap::new();
                loop {
                    let tt = self.u8()?;
                    if tt == 0 {
                        break;
                    }
                    let name = self.string()?;
                    m.insert(name, self.payload(tt)?);
                }
                Tag::Compound(m)
            }
            11 => {
                let n = self.len()?;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(self.i32()?);
                }
                Tag::IntArray(v)
            }
            12 => {
                let n = self.len()?;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(self.i64()?);
                }
                Tag::LongArray(v)
            }
            t => return Err(format!("nbt: unknown tag type {t}").into()),
        })
    }
}

/// Parse an uncompressed NBT document (root compound).
pub fn parse(bytes: &[u8]) -> Result<Tag> {
    let mut r = Reader { b: bytes, o: 0 };
    if r.u8()? != 10 {
        return Err("nbt: root is not a compound".into());
    }
    let _name = r.string()?;
    r.payload(10)
}

/// Re-encode a compound as uncompressed big-endian NBT (unnamed root).
pub fn encode(tag: &Tag) -> Vec<u8> {
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
